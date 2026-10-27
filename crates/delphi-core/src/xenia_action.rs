//! The operator-facing side of the publish gate (plan milestone X9): what a command word means, what the approval
//! card says, and the two ways a human answers it.
//!
//! * [`Action`] is `publish`, `endorse`, `dispute`, `retract`, `withdraw`, `restore` or `republish`, built from plain
//!   key/value options so the CLI's flags and the console's JSON object go through one parser ([`Action::build`]).
//! * [`card`] renders a [`Preview`]: the exact bytes that will be signed (control characters shown as escapes, so the
//!   card cannot hide or redraw anything), the flags, the similar records, the reminders and a short approval code
//!   taken from the digest.
//! * [`begin`] runs an action on its own thread up to the preview and parks it there until [`Pending::decide`]: the
//!   daemon's two-step (`xenia.attest.prepare`, then `xenia.attest.decide`). The thread's only contact with the
//!   outside while parked is the `Approver` the gate already defines, so nothing about the gate changes: no model
//!   runs between the preview and the signature, and the key is used by the thread that holds it and by nothing else.
//!   An unanswered card is a denial after [`WAIT`].

use crate::xenia_publish::{ContentOrigin, Decision, MemoryDraft, Preview, PublishError, Published, Publisher, Scope, WatchedFile};
pub use delphi_xenia::identity::Identity;
use delphi_xenia::record::{AppliesTo, AuthorStatus, Check, Confidence, MemoryType, Role, SourceCeiling, TombstoneReason, VerifiedBy};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// How long a card waits for its answer before it counts as a denial.
pub const WAIT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone)]
pub enum Action {
    Publish(Box<MemoryDraft>),
    Endorse(crate::xenia_member::EndorseSpec),
    Dispute { target: String, note: String },
    Retract { target: String },
    Withdraw { target: String, reason: TombstoneReason, note: Option<String> },
    Restore { target: String, note: Option<String> },
    /// After a routine rotation: re-sign the old key's records with the new key.
    Republish { old_did: String },
}

/// Where the files an operator names live, to tell the gate which scope a file belongs to.
#[derive(Debug, Clone)]
pub struct ScopeCtx {
    pub home: PathBuf,
    pub workspace: Option<PathBuf>,
    /// Today as `YYYY-MM-DD`, for the default review date.
    pub today: String,
}

impl ScopeCtx {
    /// A file under the operator's own area, or outside every zone, is operator scope (never published). Under the
    /// workspace's `team/` it is team scope; elsewhere in the workspace, workspace scope.
    pub fn scope_of(&self, file: &Path) -> Scope {
        let f = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        let under = |base: &Path| std::fs::canonicalize(base).map(|b| f.starts_with(b)).unwrap_or(false);
        match &self.workspace {
            Some(ws) if under(&ws.join("team")) => Scope::Team,
            Some(ws) if under(ws) => Scope::Workspace,
            _ => Scope::Operator,
        }
    }
}

type Opts = BTreeMap<String, String>;

fn csv(v: Option<&String>) -> Vec<String> {
    v.map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default()
}

fn pick<T: Copy>(table: &[(&str, T)], key: &str, v: Option<&String>, default: T) -> Result<T, String> {
    match v {
        None => Ok(default),
        Some(s) => table.iter().find(|(n, _)| n == s).map(|(_, t)| *t).ok_or_else(|| format!("--{key} is one of: {}", table.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "))),
    }
}

fn plus_days(today: &str, days: i64) -> String {
    let secs = delphi_xenia::time::parse_utc(&format!("{today}T00:00:00Z")).unwrap_or(0) + days * 86400;
    delphi_xenia::time::format_utc(secs)[..10].to_string()
}

impl Action {
    /// The words that name an action on the command line and in the console.
    pub const WORDS: [&'static str; 7] = ["publish", "endorse", "dispute", "retract", "withdraw", "restore", "republish"];

    /// Build an action from its word, the record it is about (`target`; the file for `publish`) and options.
    pub fn build(word: &str, target: Option<&str>, o: &Opts, ctx: &ScopeCtx) -> Result<Action, String> {
        let need = |what: &str| target.map(str::to_string).filter(|t| !t.is_empty()).ok_or_else(|| format!("{word} needs {what}"));
        let note = o.get("note").cloned().filter(|n| !n.is_empty());
        match word {
            "publish" => {
                let file = PathBuf::from(need("the file to publish")?);
                let (watched, body) = WatchedFile::read(&file).map_err(|e| e.to_string())?;
                let title = o.get("title").cloned().ok_or("publish needs --title")?;
                let summary = o.get("summary").cloned().ok_or("publish needs --summary")?;
                let ty = pick(MemoryType::TABLE, "type", o.get("type"), MemoryType::Finding)?;
                let verified_by = pick(VerifiedBy::TABLE, "verified-by", o.get("verified-by"), VerifiedBy::None)?;
                let confidence = pick(Confidence::TABLE, "confidence", o.get("confidence"), Confidence::Medium)?;
                let role = pick(Role::TABLE, "role", o.get("role"), Role::Operator)?;
                let draft = MemoryDraft {
                    origin: ContentOrigin { scope: ctx.scope_of(&file), promoted: o.get("promoted").is_some_and(|v| v == "true"), file: Some(watched) },
                    id: o.get("id").cloned(),
                    title,
                    summary,
                    tags: csv(o.get("tags")),
                    memory_type: ty,
                    applies_to: AppliesTo { repos: csv(o.get("repos")), tech: csv(o.get("tech")), paths: vec![] },
                    verified_by,
                    confidence,
                    body,
                    pointers: vec![],
                    evidence: vec![],
                    related: vec![],
                    review_by: o.get("review-by").cloned().unwrap_or_else(|| plus_days(&ctx.today, 180)),
                    expires: None,
                    supersedes: None,
                    // What a human writing by hand claims about where the text came from; the receiver clamps it anyway.
                    source_ceiling: SourceCeiling::InternalGenerated,
                    author_role: role,
                    status: AuthorStatus::Active,
                };
                Ok(Action::Publish(Box::new(draft)))
            }
            "endorse" => {
                let checked = match o.get("checked") {
                    None => vec![Check::TargetOpen, Check::SummaryMatches],
                    Some(s) => s.split(',').map(|c| Check::TABLE.iter().find(|(n, _)| *n == c.trim()).map(|(_, v)| *v).ok_or_else(|| format!("--checked is a list of: {}", Check::TABLE.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")))).collect::<Result<_, _>>()?,
                };
                Ok(Action::Endorse(crate::xenia_member::EndorseSpec { target: need("a record id")?, scope: csv(o.get("scope")), checked, note }))
            }
            "dispute" => Ok(Action::Dispute { target: need("a record id")?, note: note.ok_or("dispute needs --note: say what is wrong")? }),
            "retract" => Ok(Action::Retract { target: need("a record id")? }),
            "withdraw" => Ok(Action::Withdraw { target: need("a record id")?, reason: pick(TombstoneReason::TABLE, "reason", o.get("reason"), TombstoneReason::Other).and_then(|r| if o.contains_key("reason") { Ok(r) } else { Err("withdraw needs --reason".to_string()) })?, note }),
            "restore" => Ok(Action::Restore { target: need("a record id")?, note }),
            "republish" => Ok(Action::Republish { old_did: o.get("old").cloned().ok_or("republish needs --old <did of the retired key> (after a routine rotation, from the new key)")? }),
            other => Err(format!("unknown action `{other}` (one of: {})", Action::WORDS.join(", "))),
        }
    }

    pub fn word(&self) -> &'static str {
        match self {
            Action::Publish(_) => "publish",
            Action::Endorse(_) => "endorse",
            Action::Dispute { .. } => "dispute",
            Action::Retract { .. } => "retract",
            Action::Withdraw { .. } => "withdraw",
            Action::Restore { .. } => "restore",
            Action::Republish { .. } => "republish",
        }
    }

    /// One line for the top of the card.
    pub fn title(&self) -> String {
        match self {
            Action::Publish(d) => format!("Publish a memory to the org store: {}", one_line(&d.title)),
            Action::Endorse(s) => format!("Endorse {} (you attest that you opened it and its pointers and that the summary matches)", one_line(&s.target)),
            Action::Dispute { target, .. } => format!("Dispute {} (flags it to the stewards; changes no tier)", one_line(target)),
            Action::Retract { target } => format!("Withdraw your endorsement of {}", one_line(target)),
            Action::Withdraw { target, reason, .. } => format!("Withdraw {} from the org store ({})", one_line(target), reason.as_str()),
            Action::Restore { target, .. } => format!("Restore {}", one_line(target)),
            Action::Republish { old_did } => format!("Re-sign the records of the retired key {} with this key", one_line(old_did)),
        }
    }

    /// Run through the gate. The one call between the preview and the signature is `approver.decide`.
    pub fn run(self, p: &Publisher<'_>, signer: &Identity, approver: &mut dyn crate::xenia_publish::Approver) -> Result<Published, PublishError> {
        match self {
            Action::Publish(d) => p.publish(signer, vec![d.into_item()], approver),
            Action::Endorse(s) => p.endorse(signer, s, approver),
            Action::Dispute { target, note } => p.dispute(signer, &target, note, approver),
            Action::Retract { target } => p.retract_endorsement(signer, &target, approver),
            Action::Withdraw { target, reason, note } => p.withdraw(signer, &target, reason, note, approver),
            Action::Restore { target, note } => p.restore(signer, &target, note, approver),
            Action::Republish { old_did } => p.resign(signer, &old_did, approver),
        }
    }
}

/// A character that changes how text reads without showing up as text: controls, bidirectional overrides and
/// isolates, zero-width characters and tag characters. One definition for every line of a card.
pub(crate) fn is_hidden(c: char) -> bool {
    c.is_control() || ('\u{202a}'..='\u{202e}').contains(&c) || ('\u{2066}'..='\u{2069}').contains(&c) || ('\u{200b}'..='\u{200f}').contains(&c) || c == '\u{feff}' || ('\u{e0000}'..='\u{e007f}').contains(&c)
}

fn one_line(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars().take(200) {
        if c.is_control() {
            o.push(' ');
        } else if is_hidden(c) {
            o.push_str(&format!("\\u{{{:x}}}", c as u32));
        } else {
            o.push(c);
        }
    }
    o
}

/// Text made safe to print on a terminal or into a console note: every control character except the newline is
/// shown as an escape, so a record cannot move the cursor, recolor the card or hide a line.
pub fn escape(bytes: &[u8]) -> String {
    let mut out = String::new();
    for c in String::from_utf8_lossy(bytes).chars() {
        match c {
            '\n' => out.push('\n'),
            '\t' => out.push_str("    "),
            c if is_hidden(c) => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// The short code the operator types (CLI) or confirms (console): the first 12 hex digits of the digest.
pub fn code(preview: &Preview) -> String {
    preview.digest.trim_start_matches("blake3:").chars().take(12).collect()
}

fn reminder(slug: &str) -> &'static str {
    match slug {
        "rotate-secrets" => "rotate-secrets: a record was withdrawn as sensitive; the text stays in the store's git history, so rotate whatever it exposed",
        _ => "reminder",
    }
}

/// The approval card, as text. It shows everything that will be signed; there is nothing signed that it does not show.
pub fn card(title: &str, p: &Preview) -> String {
    let mut o = String::new();
    o.push_str(&format!("XENIA  {}\n", one_line(title)));
    o.push_str(&format!("built on store head {}\n", &p.head[..p.head.len().min(12)]));
    for i in &p.items {
        o.push_str(&format!("\n=== {} ({} rev {}, signed by {}) ===\n", one_line(&i.path), i.kind.as_str(), i.rev, one_line(&i.signer)));
        o.push_str(&escape(&i.bytes));
        if !i.bytes.ends_with(b"\n") {
            o.push('\n');
        }
        o.push_str(&format!("=== sidecar: signed_at {}, roster epoch {}, file hash {} ===\n", i.signed_at, i.roster_epoch, i.hash));
    }
    if !p.flags.is_empty() {
        o.push_str("\nFlags (these do not block; signing means you read them):\n");
        for f in &p.flags {
            o.push_str(&format!("  {} in {} line {}: {}\n", f.class, one_line(&f.field), f.line, one_line(&f.found)));
        }
    }
    if !p.similar.is_empty() {
        o.push_str("\nSimilar records already in the store:\n");
        for s in &p.similar {
            o.push_str(&format!("  {} [{}] {} (score {:.2})\n", one_line(&s.id), one_line(&s.tier), one_line(&s.title), s.score));
        }
    }
    for r in &p.reminders {
        o.push_str(&format!("\nNote: {}\n", reminder(r)));
    }
    o.push_str(&format!("\nApproval code: {}   (covers every byte above and the sidecar fields)\n", code(p)));
    o
}

// ---------------------------------------------------------------------------------------------------------
// Answering the card from another thread

/// What the human said. The gate turns it into an [`crate::xenia_publish::Approval`] of exactly the preview it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answer {
    pub approve: bool,
    pub flags_acknowledged: bool,
}

enum Msg {
    Preview(Box<Preview>),
    Done(Box<Result<Published, PublishError>>),
}

struct ChannelApprover {
    out: mpsc::Sender<Msg>,
    answers: mpsc::Receiver<Answer>,
    wait: Duration,
}

impl crate::xenia_publish::Approver for ChannelApprover {
    fn decide(&mut self, preview: &Preview) -> Decision {
        if self.out.send(Msg::Preview(Box::new(preview.clone()))).is_err() {
            return Decision::Deny;
        }
        match self.answers.recv_timeout(self.wait) {
            Ok(a) if a.approve => Decision::Approve(preview.approve(a.flags_acknowledged)),
            _ => Decision::Deny,
        }
    }
}

/// An action parked at its card.
pub struct Pending {
    pub preview: Preview,
    answers: mpsc::Sender<Answer>,
    done: mpsc::Receiver<Msg>,
}

impl Pending {
    /// Answer the card and wait for the rest of the gate (sign, verify, push). Consumes the card: it is answered once.
    pub fn decide(self, a: Answer) -> Result<Published, PublishError> {
        let _ = self.answers.send(a);
        loop {
            match self.done.recv() {
                Ok(Msg::Done(r)) => return *r,
                Ok(Msg::Preview(_)) => continue,
                Err(_) => return Err(PublishError::Io("the publishing thread ended without a result".into())),
            }
        }
    }
}

/// Run `job` (which builds a store, a publisher and the key itself, on the new thread) up to its card. `Err` is a
/// refusal that happened before there was anything to show (a rule, a scan block, an unreachable store).
pub fn begin<F>(job: F, wait: Duration) -> Result<Pending, PublishError>
where
    F: FnOnce(&mut dyn crate::xenia_publish::Approver) -> Result<Published, PublishError> + Send + 'static,
{
    let (tx, rx) = mpsc::channel::<Msg>();
    let (atx, arx) = mpsc::channel::<Answer>();
    let done_tx = tx.clone();
    std::thread::spawn(move || {
        let mut ap = ChannelApprover { out: tx, answers: arx, wait };
        let r = job(&mut ap);
        let _ = done_tx.send(Msg::Done(Box::new(r)));
    });
    match rx.recv() {
        Ok(Msg::Preview(preview)) => Ok(Pending { preview: *preview, answers: atx, done: rx }),
        Ok(Msg::Done(r)) => match *r {
            Err(e) => Err(e),
            Ok(_) => Err(PublishError::Io("the action finished without asking".into())),
        },
        Err(_) => Err(PublishError::Io("the publishing thread ended without a result".into())),
    }
}

#[cfg(test)]
#[path = "xenia_action_tests.rs"]
mod tests;
