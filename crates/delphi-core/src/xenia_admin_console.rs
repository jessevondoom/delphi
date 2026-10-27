//! Limited admin signing from the console (operator decision, plan milestone X9a): the rules that decide whether an
//! admin operation may be answered at a console card or only in a terminal, the card itself, and the parked-card
//! machinery. The daemon's RPC (`delphi-engine/src/service_xenia.rs`) is a thin shell over this module.
//!
//! **What the console may sign:** `sign <n>` and `publish <n>` of a roster proposal, `refresh`, and writing an unsigned
//! proposal, but only when the proposal's **typed diff** (the epoch it would make against the verified head epoch,
//! [`classify`]) touches no admin: no admin entry added, removed, retired, re-keyed or changed, no new key for a person
//! who is an admin, no rotation, no compromise (`voids_tombstones`), no fewer admins than the policy minimum. The
//! classification reads the parsed epochs the verifier judged, never a command name and never text from the store.
//! **What it may not:** genesis, any rotation, anything that touches an admin, and a rewrite recovery. Those come back
//! as a refusal that names the terminal command to run.
//!
//! **Safeguards beyond a member's attestation** (the daemon enforces the ones that need daemon state):
//! the card shows the typed diff, flags, proposal number, base epoch, epoch hash, quorum state and a digest; the answer
//! needs the code *and* the proposal number retyped *and* each flag named; one admin card at a time; at most
//! [`MAX_PER_HOUR`] console signatures per hour (counted in the store journals, so a restart does not reset it);
//! the proposal is re-fetched and re-verified when the answer arrives and a change refuses ([`ConsoleError::Stale`]);
//! every action is journaled and told to the Narrator; the switch `[xenia] console_admin` in
//! `$DELPHI_HOME/operator/xenia.toml` (default off).

use crate::xenia_admin::{Admin, AdminApprover, AdminDecision, AdminError, AdminPreview};
use crate::xenia_store::Store;
use crate::xenia_sync::VerifyConfig;
pub use delphi_xenia::admin::{NewMember, Operation, Rotation, DEFAULT_VALIDITY_SECS};
pub use delphi_xenia::identity::Identity;
use delphi_xenia::roster::{Epoch, Status};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// How long a card waits for its answer before it counts as a denial.
pub const WAIT: Duration = Duration::from_secs(15 * 60);
/// Console admin signatures allowed per install per hour.
pub const MAX_PER_HOUR: usize = 5;

// ---------------------------------------------------------------------------------------------------------
// The switch

/// `[xenia] console_admin = true` in `$DELPHI_HOME/operator/xenia.toml`. The operator zone is not a file an agent can
/// read, write or propose a change to. Missing, unreadable, a symlink or anything but the literal `true`: off.
pub fn console_admin_enabled(home: &Path) -> bool {
    let p = home.join("operator").join("xenia.toml");
    match std::fs::symlink_metadata(&p) {
        Ok(m) if m.file_type().is_file() && m.len() < 16 * 1024 => {}
        _ => return false,
    }
    let Ok(text) = std::fs::read_to_string(&p) else { return false };
    toml::from_str::<toml::Value>(&text).ok().and_then(|v| v.get("xenia")?.get("console_admin")?.as_bool()).unwrap_or(false)
}

// ---------------------------------------------------------------------------------------------------------
// Classification

/// Why an operation is terminal-only. The slug keys [`message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalOnly {
    /// An admin entry is added, removed, retired, re-keyed, or gains or loses the admin role.
    AdminTouched,
    /// A new key for a person who is an admin.
    KeyForAdminPerson,
    /// A key is retired with a successor.
    Rotation,
    /// Something stops having effect (`voids_tombstones` turns on).
    Compromise,
    /// Fewer admin people than the policy minimum.
    FewerAdmins,
}

impl TerminalOnly {
    pub fn slug(self) -> &'static str {
        match self {
            TerminalOnly::AdminTouched => "terminal-admin",
            TerminalOnly::KeyForAdminPerson => "terminal-admin-key",
            TerminalOnly::Rotation => "terminal-rotation",
            TerminalOnly::Compromise => "terminal-compromise",
            TerminalOnly::FewerAdmins => "terminal-few-admins",
        }
    }
}

fn admin_rows(e: &Epoch) -> Vec<String> {
    let mut v: Vec<String> = e.members.iter().filter(|m| m.is_admin()).map(|m| format!("{}|{:?}|{:?}|{}|{}", m.did, m.status, m.successor, m.voids_tombstones, m.person)).collect();
    v.sort();
    v
}

/// Whether the step from `base` (the verified head epoch) to `cand` may be signed at a console. Pure.
pub fn classify(base: &Epoch, cand: &Epoch, min_admins: usize) -> Result<(), TerminalOnly> {
    if admin_rows(base) != admin_rows(cand) {
        return Err(TerminalOnly::AdminTouched);
    }
    let admin_people: BTreeSet<&str> = base.members.iter().filter(|m| m.is_admin()).map(|m| m.person.as_str()).collect();
    for m in &cand.members {
        let is_new = !base.members.iter().any(|b| b.did == m.did);
        if is_new && admin_people.contains(m.person.as_str()) {
            return Err(TerminalOnly::KeyForAdminPerson);
        }
    }
    for m in &cand.members {
        let Some(b) = base.members.iter().find(|b| b.did == m.did) else { continue };
        if (b.status == Status::Active && m.status == Status::Retired) || (b.successor.is_none() && m.successor.is_some()) {
            return Err(TerminalOnly::Rotation);
        }
        if !b.voids_tombstones && m.voids_tombstones {
            return Err(TerminalOnly::Compromise);
        }
    }
    let people = |e: &Epoch| e.active_admins().values().copied().collect::<BTreeSet<&str>>().len();
    if people(cand) < people(base) && people(cand) < min_admins {
        return Err(TerminalOnly::FewerAdmins);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------
// What the operator is told (one place; slug-keyed so a shared catalog can take them over)

/// Plain-language text for a refusal or a card note. `{command}` and `{n}` are filled by [`fill`]. Every entry says
/// what to do next.
pub fn message(slug: &str) -> &'static str {
    match slug {
        "switch-off" => "Console admin signing is switched off. To allow it, create $DELPHI_HOME/operator/xenia.toml containing `[xenia]` and `console_admin = true` (only you can edit that file). Until then run this in a terminal: {command}",
        "terminal-admin" => "This change adds, removes or changes an admin, so it can only be signed in a terminal, where you type the code yourself. Run: {command}",
        "terminal-admin-key" => "This adds a key for a person who is already an admin, which gives that person a second signing key. That can only be signed in a terminal. Run: {command}",
        "terminal-rotation" => "A key rotation can only be signed in a terminal. Run: {command}",
        "terminal-compromise" => "This makes an entry's withdrawals stop counting (a compromise or a removal for cause). It can only be signed in a terminal. Run: {command}",
        "terminal-few-admins" => "This would leave fewer admins than the policy allows, so it can only be done in a terminal. Run: {command}",
        "terminal-genesis" => "Creating a store (the genesis ceremony) only happens in a terminal, with every founder present. Run: {command}",
        "terminal-rewrite" => "Accepting a rewritten store history only happens in a terminal, where every affected file is listed and you type the commit. Run: {command}",
        "pending-card" => "An admin card is already waiting. Finish or deny the pending card first (answer it, or wait for it to expire), then ask again.",
        "rate-limit" => "Five admin signatures were made from the console in the last hour, which is the limit. Wait an hour, or sign in a terminal: {command}",
        "stale" => "The proposal changed or is no longer the next roster epoch, so nothing was signed. Run `delphi xenia admin show {n}` to see where it stands, and ask again if it is still wanted.",
        "no-card" => "No admin card with that code is waiting (it was answered, it expired, or the code was mistyped). Ask for the card again.",
        "wrong-confirm" => "Retype the proposal number shown on the card (or `refresh` for a refresh) along with the code.",
        "flags-unacknowledged" => "This card has flags. Name each one (the list of flag names on the card) to sign; they do not block, but signing means you read them.",
        "no-key" => "This install has no Xenia key yet. Run `delphi xenia whoami` in a terminal to create it, then ask an admin to add you if you are not in the roster.",
        "denied" => "Cancelled: nothing was signed or sent.",
        "not-admin" => "This install's key is not an active admin of the current roster, so it cannot sign this. Ask an admin to run it.",
        _ => "Something was refused and nothing was signed. Run the same command in a terminal for the full reason.",
    }
}

/// [`message`] with `{command}` and `{n}` filled in.
pub fn fill(slug: &str, command: &str, n: &str) -> String {
    message(slug).replace("{command}", command).replace("{n}", n)
}

// ---------------------------------------------------------------------------------------------------------
// The work, and the answers

/// What a console action did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminDone {
    pub commit: String,
    pub paths: Vec<String>,
    pub already_there: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleError {
    /// The diff touches an admin (or is a rotation, ...): terminal only. Nothing was shown or signed.
    Terminal(TerminalOnly),
    /// The proposal changed between the card and the answer, or is no longer the next epoch.
    Stale,
    Admin(AdminError),
    NoKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Sign(u64),
    Publish(u64, i64),
    Refresh(i64),
}

impl Kind {
    pub fn proposal(&self) -> Option<u64> {
        match self {
            Kind::Sign(n) | Kind::Publish(n, _) => Some(*n),
            Kind::Refresh(_) => None,
        }
    }

    pub fn word(&self) -> &'static str {
        match self {
            Kind::Sign(_) => "sign",
            Kind::Publish(..) => "publish",
            Kind::Refresh(_) => "refresh",
        }
    }
}

/// Wraps the channel approver: after a yes, re-fetch the proposal and refuse if it is not exactly what was classified.
struct Guarded<'a> {
    inner: &'a mut dyn AdminApprover,
    admin: &'a Admin<'a>,
    classified: Option<(u64, Epoch, Epoch)>,
    stale: bool,
}

impl AdminApprover for Guarded<'_> {
    fn decide(&mut self, p: &AdminPreview) -> AdminDecision {
        let d = self.inner.decide(p);
        if let (AdminDecision::Approve(_), Some((n, base, cand))) = (&d, &self.classified) {
            match self.admin.proposal_epochs(*n) {
                Ok((b, c)) if &b == base && &c == cand => {}
                _ => {
                    self.stale = true;
                    return AdminDecision::Deny;
                }
            }
        }
        d
    }
}

/// Run one console admin action up to its card and through the signature. `approver` is asked once. A diff that is
/// terminal-only returns before any card exists.
pub fn run(store: &Store, key: &Identity, cfg: VerifyConfig, kind: &Kind, approver: &mut dyn AdminApprover) -> Result<AdminDone, ConsoleError> {
    let admin = Admin::new(store, cfg);
    let classified = match kind.proposal() {
        Some(n) => {
            let (base, cand) = admin.proposal_epochs(n).map_err(|e| match e {
                AdminError::StaleProposal { .. } | AdminError::ProposalNotFound(_) => ConsoleError::Stale,
                other => ConsoleError::Admin(other),
            })?;
            classify(&base, &cand, admin.policy.min_admins).map_err(ConsoleError::Terminal)?;
            Some((n, base, cand))
        }
        None => None,
    };
    let mut g = Guarded { inner: approver, admin: &admin, classified, stale: false };
    let r = match kind {
        Kind::Sign(n) => admin.sign(key, *n, &mut g).map(|d| AdminDone { commit: d.commit, paths: Vec::new(), already_there: d.already_there }),
        Kind::Publish(n, v) => admin.publish(key, *n, *v, &mut g).map(|d| AdminDone { commit: d.commit, paths: d.paths, already_there: d.already_there }),
        Kind::Refresh(v) => admin.refresh(key, *v, &mut g).map(|d| AdminDone { commit: d.commit, paths: d.paths, already_there: d.already_there }),
    };
    match r {
        Err(AdminError::Denied) if g.stale => Err(ConsoleError::Stale),
        Err(e) => Err(ConsoleError::Admin(e)),
        Ok(d) => Ok(d),
    }
}

/// What the human said. The gate turns it into an approval of exactly the preview it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answer {
    pub approve: bool,
    pub flags_acknowledged: bool,
}

enum Msg {
    Preview(Box<AdminPreview>),
    Done(Box<Result<AdminDone, ConsoleError>>),
}

struct ChannelApprover {
    out: mpsc::Sender<Msg>,
    answers: mpsc::Receiver<Answer>,
    wait: Duration,
}

impl AdminApprover for ChannelApprover {
    fn decide(&mut self, preview: &AdminPreview) -> AdminDecision {
        if self.out.send(Msg::Preview(Box::new(preview.clone()))).is_err() {
            return AdminDecision::Deny;
        }
        match self.answers.recv_timeout(self.wait) {
            Ok(a) if a.approve => AdminDecision::Approve(preview.approve(a.flags_acknowledged)),
            _ => AdminDecision::Deny,
        }
    }
}

/// An admin action parked at its card.
pub struct Pending {
    pub preview: AdminPreview,
    answers: mpsc::Sender<Answer>,
    done: mpsc::Receiver<Msg>,
}

impl Pending {
    /// Answer the card and wait for the rest of the gate. Consumes the card.
    pub fn decide(self, a: Answer) -> Result<AdminDone, ConsoleError> {
        let _ = self.answers.send(a);
        loop {
            match self.done.recv() {
                Ok(Msg::Done(r)) => return *r,
                Ok(Msg::Preview(_)) => continue,
                Err(_) => return Err(ConsoleError::Admin(AdminError::Io("the signing thread ended without a result".into()))),
            }
        }
    }
}

/// Run `job` on its own thread up to the card. `Err` is a refusal that happened before there was anything to show.
pub fn begin<F>(job: F, wait: Duration) -> Result<Pending, ConsoleError>
where
    F: FnOnce(&mut dyn AdminApprover) -> Result<AdminDone, ConsoleError> + Send + 'static,
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
            Ok(_) => Err(ConsoleError::Admin(AdminError::Io("the action finished without asking".into()))),
        },
        Err(_) => Err(ConsoleError::Admin(AdminError::Io("the signing thread ended without a result".into()))),
    }
}

// ---------------------------------------------------------------------------------------------------------
// The card

/// The short code covering the digest, as for a member's card.
pub fn code(p: &AdminPreview) -> String {
    p.digest.trim_start_matches("blake3:").chars().take(12).collect()
}

fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { '?' } else { c }).collect()
}

/// What the operator must retype besides the code: the proposal number, or `refresh`.
pub fn confirm_word(kind: &Kind) -> String {
    kind.proposal().map(|n| n.to_string()).unwrap_or_else(|| "refresh".into())
}

/// The card, as text: the typed diff, flags, proposal number, base epoch, epoch hash, quorum state, the exact bytes
/// and the digest.
pub fn card(kind: &Kind, p: &AdminPreview) -> String {
    let mut o = String::new();
    o.push_str(&format!("XENIA ADMIN  {} {}  (store {}/{})\n", kind.word(), kind.proposal().map(|n| format!("proposal {n}")).unwrap_or_default(), one_line(&p.org), one_line(&p.store)));
    o.push_str(&format!("base epoch {}  ->  epoch {}   signer {}\n", p.epoch.saturating_sub(1), p.epoch, one_line(&p.signer)));
    if let Some(f) = p.files.first() {
        o.push_str(&format!("epoch hash {}\n", delphi_xenia::hash::of(&f.bytes)));
    }
    if let Some(h) = &p.store_head {
        o.push_str(&format!("built on store head {}\n", one_line(h)));
    }
    for c in &p.changes {
        o.push_str(&format!("change: {} {} ({}, {}) {}\n", c.kind.slug(), c.did, one_line(&c.person), one_line(&c.handle), one_line(&c.detail)));
    }
    if let Some(q) = &p.quorum {
        o.push_str(&format!("quorum: {} of {} people have signed ({}){}\n", q.have, q.need, one_line(&q.people.join(", ")), if q.met { ", met" } else { ", still needed" }));
    }
    for (base, n) in &p.walked {
        o.push_str(&format!("history walked from {base}: {n} commit(s), no findings\n"));
    }
    for f in &p.files {
        o.push_str(&format!("\n=== {} (exact bytes) ===\n", one_line(&f.path)));
        o.push_str(&crate::xenia_action::escape(&f.bytes));
        if !f.bytes.ends_with(b"\n") {
            o.push('\n');
        }
    }
    o.push_str("\nthe key will sign exactly these domains:\n");
    for d in &p.signs {
        o.push_str(&format!("  {}\n", one_line(d)));
    }
    if !p.flags.is_empty() {
        o.push_str("\nFlags (they do not block; name each one to sign):\n");
        for f in &p.flags {
            o.push_str(&format!("  {}: {}\n", f.slug(), f.text()));
        }
    }
    for r in &p.reminders {
        o.push_str(&format!("\nNote {}: {}\n", r.slug(), r.text()));
    }
    o.push_str(&format!("\nApproval code: {}   (covers every byte above)\nTo sign: the code, the number `{}`, and {}\n", code(p), confirm_word(kind), if p.flags.is_empty() { "no flags to name".to_string() } else { format!("the flag names: {}", p.flags.iter().map(|f| f.slug()).collect::<Vec<_>>().join(", ")) }));
    o
}

// ---------------------------------------------------------------------------------------------------------
// Rate limit

/// Console signatures among journal entries given as `(kind, outcome, via, at)`, in the hour before `now`.
pub fn recent_console_signatures<'a>(entries: impl IntoIterator<Item = (&'a str, &'a str, &'a str, i64)>, now: i64) -> usize {
    entries.into_iter().filter(|(kind, outcome, via, at)| *kind == "admin" && *via == "console" && matches!(*outcome, "signed" | "published" | "refreshed") && now - *at < 3600 && *at <= now + 60).count()
}

#[cfg(test)]
#[path = "xenia_admin_console_tests.rs"]
mod tests;
