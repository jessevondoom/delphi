//! The relay (`specs/014`): carries approval cards to the operator on another channel and brings their answers
//! back, so a build can wait for a person who is not at the terminal.
//!
//! The core here owns every rule that matters and knows nothing about any transport:
//!
//! * a card goes out with a **single-use token** (128 random bits); only a reply that carries it counts, so the
//!   sender's address is a filter and the token is the secret;
//! * only an allowlisted sender, whom the channel vouches for, is heard, and nobody else is ever answered;
//! * what comes back is a verb and a token and nothing more: the rest of a message is never read, and never
//!   reaches an agent;
//! * the daemon, not the relay, decides what a relay may do (Amber allowed, Red only denied, answers bound to the
//!   payload hash), so a bug here cannot widen it.
//!
//! A channel only moves bytes: [`Channel::send`] and [`Channel::poll`].

pub mod cli;
pub mod email;
pub mod net;
pub mod telegram;

use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// How much of a card goes out to the channel's provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// The one-line summary, who asked and which tool. The default: the provider sees as little as possible.
    Summary,
    /// Also the card's payload, truncated.
    Full,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub channel: String,
    pub detail: Detail,
    /// Senders whose answers count (lower-case addresses or ids).
    pub allow: Vec<String>,
    /// Refuse answers the channel could not authenticate.
    pub require_auth: bool,
    pub max_outstanding: usize,
    pub max_per_hour: usize,
    /// Also say when a task finishes or fails.
    pub notify_tasks: bool,
    /// The daemon has TOTP step-up configured (`[relay] step_up = "totp"`): a Red card can be approved by reply when
    /// the reply also carries the authenticator code. Only changes what the relay tells the operator and forwards;
    /// the daemon decides.
    pub step_up: bool,
}

impl Config {
    pub fn new(channel: &str, allow: Vec<String>) -> Config {
        Config { channel: channel.into(), detail: Detail::Summary, allow: allow.into_iter().map(|a| a.trim().to_lowercase()).collect(), require_auth: true, max_outstanding: 20, max_per_hour: 60, notify_tasks: true, step_up: false }
    }
}

/// A message to the operator. The channel adds the recipient and renders `actions` its own way.
#[derive(Debug, Clone, PartialEq)]
pub struct Outbound {
    pub subject: String,
    pub text: String,
    pub actions: Vec<Action>,
    /// The approval this message is about; `None` for a notice.
    pub card: Option<String>,
}

/// A way to answer: what the reply must say.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub label: String,
    pub reply: String,
}

/// A message from the channel, already reduced by the channel to its new text (no quoted history, no signature).
#[derive(Debug, Clone, PartialEq)]
pub struct Inbound {
    /// A stable id for de-duplication (an email's `Message-ID`).
    pub id: String,
    pub sender: String,
    /// The channel verified the sender (for email, the receiving provider's SPF/DKIM/DMARC result).
    pub authenticated: bool,
    /// An auto-reply, bounce or bulk message. Never acted on, never answered.
    pub auto_generated: bool,
    /// The first line of the new text.
    pub text: String,
}

pub trait Channel {
    fn send(&mut self, m: &Outbound) -> Result<(), String>;
    fn poll(&mut self) -> Result<Vec<Inbound>, String>;
}

#[derive(Debug, PartialEq)]
pub enum ResolveFail {
    /// The card was already answered, timed out or never existed.
    Gone(String),
    /// The daemon refuses a relay this answer (approving a Red action).
    Forbidden(String),
    Invalid(String),
    Down(String),
}

/// The relay's view of the daemon.
pub trait Daemon {
    fn resolve(&mut self, approval_id: &str, payload_blake3: &str, allow: bool, channel: &str, sender: &str, totp: Option<&str>) -> Result<(), ResolveFail>;
}

#[derive(Debug, PartialEq)]
pub enum Handled {
    Ignored(&'static str),
    Answered { approval_id: String, allow: bool },
    Late,
    Refused(String),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Deny,
}

pub const TOKEN_LEN: usize = 32;

/// `APPROVE <token>`, `APPROVE <token> <6 digits>` or `DENY <token>` and nothing else on the line. The six digits
/// are the operator's authenticator code, allowed only on an approval (step-up, `specs/014` §7).
pub fn parse_reply(line: &str) -> Option<(Verdict, String, Option<String>)> {
    let mut it = line.split_whitespace();
    let verb = it.next()?.to_ascii_uppercase();
    let token = it.next()?;
    let totp = it.next();
    if it.next().is_some() || token.len() != TOKEN_LEN || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = match verb.as_str() {
        "APPROVE" => Verdict::Approve,
        "DENY" if totp.is_none() => Verdict::Deny,
        _ => return None,
    };
    let totp = match totp {
        Some(t) if t.len() == 6 && t.bytes().all(|b| b.is_ascii_digit()) => Some(t.to_string()),
        Some(_) => return None,
        None => None,
    };
    Some((v, token.to_ascii_lowercase(), totp))
}

/// As [`parse_reply`] for a reply that carries no authenticator code.
#[cfg(test)]
pub fn parse_verdict(line: &str) -> Option<(Verdict, String)> {
    parse_reply(line).filter(|r| r.2.is_none()).map(|(v, t, _)| (v, t))
}

struct Outstanding {
    approval_id: String,
    payload_blake3: String,
    token_hash: String,
    tier_red: bool,
    expires: Instant,
}

pub struct Relay<C: Channel, D: Daemon> {
    pub cfg: Config,
    pub chan: C,
    pub daemon: D,
    /// Cards waiting for an answer, by approval id.
    out: HashMap<String, Outstanding>,
    seen: VecDeque<String>,
    sent: VecDeque<Instant>,
    last_auth_notice: Option<Instant>,
    pub log: Vec<String>,
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\n').take(max).collect()
}

/// One line, no control characters at all: for anything that becomes a header.
fn line(s: &str, max: usize) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).take(max).collect::<String>().trim().to_string()
}

fn token_hash(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn new_token() -> Option<String> {
    delphi_core::server::random_token().ok().map(|t| t[..TOKEN_LEN].to_string())
}

impl<C: Channel, D: Daemon> Relay<C, D> {
    pub fn new(cfg: Config, chan: C, daemon: D) -> Relay<C, D> {
        Relay { cfg, chan, daemon, out: HashMap::new(), seen: VecDeque::new(), sent: VecDeque::new(), last_auth_notice: None, log: Vec::new() }
    }

    #[cfg(test)]
    pub fn outstanding(&self) -> usize {
        self.out.len()
    }

    fn note(&mut self, s: impl Into<String>) {
        let s = s.into();
        eprintln!("delphi relay: {}", s);
        self.log.push(s);
    }

    /// Rate limit on everything sent: at most `max_per_hour` in any hour.
    fn may_send(&mut self, now: Instant) -> bool {
        while self.sent.front().is_some_and(|t| now.saturating_duration_since(*t) > Duration::from_secs(3600)) {
            self.sent.pop_front();
        }
        if self.sent.len() >= self.cfg.max_per_hour {
            return false;
        }
        self.sent.push_back(now);
        true
    }

    fn notify(&mut self, subject: &str, text: &str, now: Instant) {
        if !self.may_send(now) {
            self.note(format!("notice `{}` dropped: more than {} messages in an hour", subject, self.cfg.max_per_hour));
            return;
        }
        let m = Outbound { subject: line(subject, 120), text: clean(text, 2000), actions: vec![], card: None };
        if let Err(e) = self.chan.send(&m) {
            self.note(format!("could not send a notice: {}", e));
        }
    }

    /// An event from the daemon's relay stream.
    pub fn on_event(&mut self, e: &Value, now: Instant) {
        let p = &e["payload"];
        match e["type"].as_str().unwrap_or("") {
            "approval.requested" => self.card_raised(p, now),
            "approval.resolved" => {
                let id = p["approval_id"].as_str().unwrap_or("");
                let by = p["by"].as_str().unwrap_or("");
                if self.out.remove(id).is_some() && !by.starts_with("relay:") {
                    // Answered at the console or timed out: say so, so the card in the operator's inbox is not left looking live.
                    let what = p["resolution"].as_str().unwrap_or("resolved");
                    self.notify(&format!("[delphi:{}] already {}", id, what), &format!("That card was {} ({}). Nothing more to do.", what, if by.is_empty() { "no detail" } else { by }), now);
                }
            }
            "approval.parked" => {} // a fresh card follows; the old token is already gone with its resolution
            "decisions.review_requested" => {
                // Summaries only: what was allowed, how risky, and why. Reviewing happens at the console.
                let task = e["task_id"].as_str().unwrap_or("?");
                let n = p["count"].as_u64().unwrap_or(0);
                let mut text = format!("Task {} made {} provisional decision(s) that you must review before anything can merge. Nothing has reached your checkout.\n\n", task, n);
                for d in p["decisions"].as_array().into_iter().flatten().take(15) {
                    text.push_str(&format!("  [{}] {} ({})\n    {}\n", clean(d["risk"].as_str().unwrap_or("routine"), 12), clean(d["summary"].as_str().unwrap_or("?"), 120).replace('\n', " "), clean(d["decision_id"].as_str().unwrap_or("?"), 24), clean(d["rationale"].as_str().unwrap_or(""), 160).replace('\n', " ")));
                }
                text.push_str("\nReview them in the console: /decisions, then /decisions ratify <id> (or reject), then /merge <task>.\n");
                self.notify(&format!("[delphi] {} awaits your review of {} decision(s)", task, n), &text, now);
            }
            "task.completed" if self.cfg.notify_tasks => {
                let task = e["task_id"].as_str().unwrap_or("?");
                let status = p["status"].as_str().unwrap_or("?");
                let reason = p["reason"].as_str().unwrap_or("");
                self.notify(&format!("[delphi] {} {}", task, status), &format!("Task {} finished: {}.{}", task, status, if reason.is_empty() { String::new() } else { format!(" Reason: {}", reason) }), now);
            }
            "xenia.alert" => {
                // The Xenia store needs a human. The message is worded from the help catalog by a slug the catalog knows
                // (never from the payload, which is journal text); it carries no store name, key or path, and nothing
                // in it can approve, sign or run anything.
                let slug = match (p["reason"].as_str().unwrap_or(""), p["freshness"].as_str()) {
                    ("stale", Some(f)) if delphi_core::xenia_help::lookup(f).is_some() => f,
                    (r, _) => r,
                };
                self.notify("[delphi] Xenia needs attention", &delphi_core::xenia_help::relay_message(slug), now);
            }
            "task.failed" if self.cfg.notify_tasks => {
                let task = e["task_id"].as_str().unwrap_or("?");
                self.notify(&format!("[delphi] {} failed", task), &format!("Task {} failed: {}", task, p["error"].as_str().unwrap_or("no detail")), now);
            }
            _ => {}
        }
    }

    fn card_raised(&mut self, p: &Value, now: Instant) {
        let Some(id) = p["approval_id"].as_str().map(String::from) else { return };
        if self.out.contains_key(&id) {
            return;
        }
        if self.out.len() >= self.cfg.max_outstanding {
            self.note(format!("card {} not sent: {} cards are already outstanding", id, self.out.len()));
            return;
        }
        if !self.may_send(now) {
            self.note(format!("card {} not sent: more than {} messages in an hour", id, self.cfg.max_per_hour));
            return;
        }
        let Some(token) = new_token() else {
            self.note("could not read randomness; no card sent");
            return;
        };
        let red = p["tier"].as_str() == Some("red");
        let m = card_message(p, &token, red, self.cfg.detail, self.cfg.step_up);
        match self.chan.send(&m) {
            Ok(()) => {
                let secs = p["timeout_secs"].as_u64().unwrap_or(600);
                self.out.insert(id.clone(), Outstanding { approval_id: id, payload_blake3: p["payload_blake3"].as_str().unwrap_or("").into(), token_hash: token_hash(&token), tier_red: red, expires: now + Duration::from_secs(secs + 30) });
            }
            Err(e) => self.note(format!("could not send card {}: {}", id, e)),
        }
    }

    /// Cards that were already waiting when the relay (re)connected.
    pub fn sync_pending(&mut self, cards: &[Value], now: Instant) {
        for c in cards {
            let known = c["approval_id"].as_str().is_some_and(|id| self.out.contains_key(id));
            if !known {
                self.card_raised(c, now);
            }
        }
    }

    /// Forget cards whose time is up (the daemon has denied them already).
    pub fn expire(&mut self, now: Instant) {
        self.out.retain(|_, o| o.expires > now);
    }

    /// One message from the channel.
    pub fn on_inbound(&mut self, m: &Inbound, now: Instant) -> Handled {
        if self.seen.iter().any(|s| *s == m.id) {
            return Handled::Ignored("duplicate");
        }
        self.seen.push_back(m.id.clone());
        while self.seen.len() > 2000 {
            self.seen.pop_front();
        }
        if m.auto_generated {
            return Handled::Ignored("auto-generated");
        }
        // Not on the list: say nothing to them, ever. (An answer to a stranger is how a relay becomes a spam source.)
        if !self.cfg.allow.iter().any(|a| *a == m.sender.trim().to_lowercase()) {
            self.note(format!("ignored a message from {}: not an allowed sender", clean(&m.sender, 80)));
            return Handled::Ignored("sender not allowed");
        }
        if self.cfg.require_auth && !m.authenticated {
            self.note(format!("ignored a message claiming to be {}: the channel could not verify the sender", clean(&m.sender, 80)));
            if self.last_auth_notice.is_none_or(|t| now.saturating_duration_since(t) > Duration::from_secs(600)) {
                self.last_auth_notice = Some(now);
                self.notify("[delphi] a message claiming to be you failed authentication", "A message from your address was ignored because the mail provider could not verify it. If that was you, check the mailbox's SPF/DKIM settings. No card was answered.", now);
            }
            return Handled::Ignored("sender not authenticated");
        }
        let Some((verdict, token, totp)) = parse_reply(&m.text) else {
            return Handled::Ignored("no verdict");
        };
        let th = token_hash(&token);
        let Some(key) = self.out.iter().find(|(_, o)| o.token_hash == th).map(|(k, _)| k.clone()) else {
            self.notify("[delphi] that code is not valid", "No card is waiting for that code: it was already used, answered elsewhere, or has expired. Nothing was approved.", now);
            return Handled::Ignored("unknown or used token");
        };
        let allow = verdict == Verdict::Approve;
        let red = self.out[&key].tier_red;
        if allow && red {
            if !self.cfg.step_up {
                self.out.remove(&key);
                self.notify(&format!("[delphi:{}] Red actions are approved at the console", key), "That card is a Red action; it cannot be approved by a reply. You can still deny it here, or approve it in the console. It was not approved.", now);
                return Handled::Refused("red".into());
            }
            if totp.is_none() {
                // No attempt was made, so the single-use code is not spent: ask again with the authenticator code.
                self.notify(&format!("[delphi:{}] needs your authenticator code", key), "That card is a Red action. To approve it by reply, send: APPROVE <the code from this message> <the 6-digit code from your authenticator app>. It was not approved.", now);
                return Handled::Ignored("authenticator code needed");
            }
        }
        // Single use: the code is gone whatever happens next.
        let Some(o) = self.out.remove(&key) else { return Handled::Ignored("unknown or used token") };
        let forwarded = if allow && red { totp.as_deref() } else { None };
        match self.daemon.resolve(&o.approval_id, &o.payload_blake3, allow, &self.cfg.channel, &m.sender, forwarded) {
            Ok(()) => Handled::Answered { approval_id: o.approval_id, allow },
            Err(ResolveFail::Gone(why)) => {
                self.notify(&format!("[delphi:{}] too late", o.approval_id), &format!("That card was no longer waiting ({}). Nothing changed.", clean(&why, 200)), now);
                Handled::Late
            }
            Err(ResolveFail::Forbidden(why)) => {
                self.notify(&format!("[delphi:{}] refused", o.approval_id), &format!("Delphi refused that answer: {}", clean(&why, 200)), now);
                Handled::Refused(why)
            }
            Err(ResolveFail::Invalid(why)) | Err(ResolveFail::Down(why)) => {
                self.notify(&format!("[delphi:{}] failed", o.approval_id), &format!("The answer could not be delivered: {}", clean(&why, 200)), now);
                Handled::Failed(why)
            }
        }
    }

    /// Poll the channel once and handle what arrived. Errors are logged, not fatal.
    pub fn poll_once(&mut self, now: Instant) -> Vec<Handled> {
        match self.chan.poll() {
            Ok(msgs) => msgs.iter().map(|m| self.on_inbound(m, now)).collect(),
            Err(e) => {
                self.note(format!("poll failed: {}", e));
                vec![]
            }
        }
    }
}

/// The text of a card as sent to the operator. By default only the summary leaves the machine.
pub fn card_message(p: &Value, token: &str, red: bool, detail: Detail, step_up: bool) -> Outbound {
    let id = p["approval_id"].as_str().unwrap_or("?");
    let summary = clean(p["summary"].as_str().unwrap_or("an action"), 300).replace('\n', " ");
    let hash = p["payload_blake3"].as_str().unwrap_or("");
    let mut text = format!(
        "Delphi needs your decision.\n\n  {}\n\n  who: {} · task: {} · tool: {} · {}\n  payload: blake3 {}…\n  waiting for: {} s\n",
        summary,
        clean(p["role"].as_str().unwrap_or("agent"), 40),
        clean(p["task_id"].as_str().unwrap_or("?"), 40),
        clean(p["tool"].as_str().unwrap_or("?"), 60),
        if red { "RED" } else { "amber" },
        &hash[..hash.len().min(12)],
        p["timeout_secs"].as_u64().unwrap_or(0)
    );
    if let (Detail::Full, Some(pl)) = (detail, p.get("payload")) {
        let pretty = serde_json::to_string_pretty(pl).unwrap_or_default();
        text.push_str(&format!("\nDetails:\n{}\n", clean(&pretty, 3000)));
    }
    if let Some(r) = p["reraised_from"].as_str() {
        text.push_str(&format!("\n(This replaces {}, which timed out; the old code no longer works.)\n", clean(r, 40)));
    }
    let mut actions = vec![Action { label: "Deny".into(), reply: format!("DENY {}", token) }];
    if red && step_up {
        text.push_str("\nThis is a RED action. To approve it by reply you also need the 6-digit code from your authenticator app: add it after the code, as `APPROVE <code> <123456>`. Without it, this can only be denied here or approved at the console.\n");
        actions.insert(0, Action { label: "Approve (add your authenticator code)".into(), reply: format!("APPROVE {} ", token) });
    } else if red {
        text.push_str("\nThis is a RED action. It cannot be approved by a reply: approve it at the console, or deny it here.\n");
    } else {
        actions.insert(0, Action { label: "Approve".into(), reply: format!("APPROVE {}", token) });
    }
    Outbound { subject: line(&format!("[delphi:{}] {}", line(id, 60), line(&summary, 80)), 160), text, actions, card: Some(id.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Default)]
    struct FakeChannel {
        sent: Vec<Outbound>,
        inbox: Vec<Inbound>,
        fail_send: bool,
    }

    impl Channel for FakeChannel {
        fn send(&mut self, m: &Outbound) -> Result<(), String> {
            if self.fail_send {
                return Err("smtp down".into());
            }
            self.sent.push(m.clone());
            Ok(())
        }
        fn poll(&mut self) -> Result<Vec<Inbound>, String> {
            Ok(std::mem::take(&mut self.inbox))
        }
    }

    #[derive(Default)]
    struct FakeDaemon {
        calls: Vec<(String, String, bool, String, String)>,
        totps: Vec<Option<String>>,
        reply: Option<ResolveFail>,
    }

    impl Daemon for FakeDaemon {
        fn resolve(&mut self, id: &str, hash: &str, allow: bool, channel: &str, sender: &str, totp: Option<&str>) -> Result<(), ResolveFail> {
            self.calls.push((id.into(), hash.into(), allow, channel.into(), sender.into()));
            self.totps.push(totp.map(String::from));
            match self.reply.take() {
                Some(f) => Err(f),
                None => Ok(()),
            }
        }
    }

    fn relay() -> Relay<FakeChannel, FakeDaemon> {
        Relay::new(Config::new("email", vec!["Me@Example.com".into()]), FakeChannel::default(), FakeDaemon::default())
    }

    fn card(id: &str, tier: &str) -> Value {
        json!({"type": "approval.requested", "payload": {"approval_id": id, "task_id": "t_0001", "role": "architect", "tier": tier, "tool": "write_file", "summary": "write specs/009.md", "payload": {"path": "specs/009.md", "content": "SECRET BODY"}, "payload_blake3": "abcdef0123456789abcdef", "timeout_secs": 600}})
    }

    fn token_in(m: &Outbound) -> String {
        m.actions.iter().find(|a| a.label == "Deny").unwrap().reply.split(' ').nth(1).unwrap().to_string()
    }

    fn reply(id: &str, sender: &str, text: &str) -> Inbound {
        Inbound { id: id.into(), sender: sender.into(), authenticated: true, auto_generated: false, text: text.into() }
    }

    #[test]
    fn verdicts_are_a_verb_and_a_full_length_hex_token_and_nothing_else() {
        let t = "0123456789abcdef0123456789abcdef";
        assert_eq!(parse_verdict(&format!("APPROVE {}", t)), Some((Verdict::Approve, t.into())));
        assert_eq!(parse_verdict(&format!("  deny   {}  ", t.to_uppercase())), Some((Verdict::Deny, t.into())));
        for bad in ["", "APPROVE", &format!("APPROVE {} please", t), &format!("YES {}", t), "APPROVE 0123", &format!("APPROVE {}zz", &t[..30]), &format!("approve: {}", t)] {
            assert_eq!(parse_verdict(bad), None, "{:?}", bad);
        }
    }

    #[test]
    fn a_card_goes_out_with_a_token_and_by_default_only_its_summary_leaves() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&card("ap_1", "amber"), now);
        let m = &r.chan.sent[0];
        assert_eq!(m.card.as_deref(), Some("ap_1"));
        assert!(m.subject.starts_with("[delphi:ap_1]") && m.text.contains("write specs/009.md"));
        assert!(!m.text.contains("SECRET BODY"), "the payload stays home unless detail = full");
        assert!(m.actions.iter().any(|a| a.label == "Approve") && m.actions.iter().any(|a| a.label == "Deny"));
        let full = card_message(&card("ap_1", "amber")["payload"], "t", false, Detail::Full, false);
        assert!(full.text.contains("SECRET BODY"));
        assert_eq!(r.outstanding(), 1);
    }

    #[test]
    fn the_token_from_the_right_sender_approves_once_and_only_once() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&card("ap_1", "amber"), now);
        let t = token_in(&r.chan.sent[0]);
        let h = r.on_inbound(&reply("m1", "me@example.com", &format!("APPROVE {}", t)), now);
        assert_eq!(h, Handled::Answered { approval_id: "ap_1".into(), allow: true });
        assert_eq!(r.daemon.calls, vec![("ap_1".to_string(), "abcdef0123456789abcdef".to_string(), true, "email".to_string(), "me@example.com".to_string())], "bound to the payload hash the card carried");
        // The same code again, in a new message, does nothing.
        let again = r.on_inbound(&reply("m2", "me@example.com", &format!("APPROVE {}", t)), now);
        assert_eq!(again, Handled::Ignored("unknown or used token"));
        assert_eq!(r.daemon.calls.len(), 1);
    }

    #[test]
    fn strangers_get_no_answer_and_no_reply_whatever_they_send() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&card("ap_1", "amber"), now);
        let t = token_in(&r.chan.sent[0]);
        let before = r.chan.sent.len();
        let h = r.on_inbound(&reply("m1", "mallory@evil.example", &format!("APPROVE {}", t)), now);
        assert_eq!(h, Handled::Ignored("sender not allowed"));
        assert!(r.daemon.calls.is_empty());
        assert_eq!(r.chan.sent.len(), before, "nothing is sent back to a stranger, not even a refusal");
        assert_eq!(r.outstanding(), 1, "and the real card still waits for the real operator");
    }

    #[test]
    fn the_right_address_without_authentication_is_refused_and_the_operator_is_told_once() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&card("ap_1", "amber"), now);
        let t = token_in(&r.chan.sent[0]);
        let mut forged = reply("m1", "me@example.com", &format!("APPROVE {}", t));
        forged.authenticated = false;
        assert_eq!(r.on_inbound(&forged, now), Handled::Ignored("sender not authenticated"));
        let mut again = forged.clone();
        again.id = "m2".into();
        r.on_inbound(&again, now + Duration::from_secs(5));
        assert!(r.daemon.calls.is_empty());
        let notices = r.chan.sent.iter().filter(|m| m.card.is_none() && m.subject.contains("failed authentication")).count();
        assert_eq!(notices, 1, "a flood of forgeries does not become a flood of mail");
        assert_eq!(r.outstanding(), 1, "the forged attempt did not use up the real token");
    }

    #[test]
    fn a_wrong_token_approves_nothing_and_an_old_message_id_is_not_processed_twice() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&card("ap_1", "amber"), now);
        let wrong = "ffffffffffffffffffffffffffffffff";
        assert_eq!(r.on_inbound(&reply("m1", "me@example.com", &format!("APPROVE {}", wrong)), now), Handled::Ignored("unknown or used token"));
        let t = token_in(&r.chan.sent[0]);
        assert!(matches!(r.on_inbound(&reply("m2", "me@example.com", &format!("DENY {}", t)), now), Handled::Answered { allow: false, .. }));
        assert_eq!(r.on_inbound(&reply("m2", "me@example.com", &format!("DENY {}", t)), now), Handled::Ignored("duplicate"));
        assert_eq!(r.daemon.calls.len(), 1);
    }

    #[test]
    fn replies_that_are_not_a_verdict_or_are_auto_generated_do_nothing() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&card("ap_1", "amber"), now);
        let t = token_in(&r.chan.sent[0]);
        let n = r.chan.sent.len();
        assert_eq!(r.on_inbound(&reply("m1", "me@example.com", "sure, go ahead!"), now), Handled::Ignored("no verdict"));
        let mut ooo = reply("m2", "me@example.com", &format!("APPROVE {}", t));
        ooo.auto_generated = true;
        assert_eq!(r.on_inbound(&ooo, now), Handled::Ignored("auto-generated"));
        assert!(r.daemon.calls.is_empty() && r.chan.sent.len() == n && r.outstanding() == 1);
    }

    #[test]
    fn a_red_card_can_be_denied_by_reply_but_never_approved() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&card("ap_red", "red"), now);
        let m = &r.chan.sent[0];
        assert!(m.actions.iter().all(|a| a.label != "Approve"), "no approve link on a Red card");
        assert!(m.text.contains("RED") && m.text.contains("console"));
        let t = token_in(m);
        // Approving anyway (a hand-typed reply) is refused here, before the daemon is even asked, and burns the code.
        assert_eq!(r.on_inbound(&reply("m1", "me@example.com", &format!("APPROVE {}", t)), now), Handled::Refused("red".into()));
        assert!(r.daemon.calls.is_empty());
        r.on_event(&card("ap_red2", "red"), now);
        let t2 = token_in(r.chan.sent.last().unwrap());
        assert!(matches!(r.on_inbound(&reply("m2", "me@example.com", &format!("DENY {}", t2)), now), Handled::Answered { allow: false, .. }));
    }

    fn relay_with_step_up() -> Relay<FakeChannel, FakeDaemon> {
        let mut r = relay();
        r.cfg.step_up = true;
        r
    }

    #[test]
    fn a_reply_may_carry_an_authenticator_code_on_an_approval_and_nowhere_else() {
        let t = "0123456789abcdef0123456789abcdef";
        assert_eq!(parse_reply(&format!("APPROVE {} 123456", t)), Some((Verdict::Approve, t.into(), Some("123456".into()))));
        assert_eq!(parse_reply(&format!("approve {}", t)), Some((Verdict::Approve, t.into(), None)));
        for bad in [format!("DENY {} 123456", t), format!("APPROVE {} 12345", t), format!("APPROVE {} 1234567", t), format!("APPROVE {} 12345a", t), format!("APPROVE {} 123456 7", t)] {
            assert_eq!(parse_reply(&bad), None, "{:?}", bad);
        }
        assert_eq!(parse_verdict(&format!("APPROVE {} 123456", t)), None, "the plain parser takes no authenticator code");
    }

    #[test]
    fn with_step_up_a_red_card_offers_an_approve_that_asks_for_the_code_and_without_it_nothing_is_offered() {
        let mut r = relay_with_step_up();
        r.on_event(&card("ap_red", "red"), Instant::now());
        let m = &r.chan.sent[0];
        assert!(m.text.contains("authenticator") && m.actions.iter().any(|a| a.label.starts_with("Approve") && a.reply.ends_with(' ')), "{:?}", m);
        let mut plain = relay();
        plain.on_event(&card("ap_red", "red"), Instant::now());
        assert!(plain.chan.sent[0].actions.iter().all(|a| a.label == "Deny"));
    }

    #[test]
    fn a_red_approval_with_step_up_forwards_the_code_and_a_missing_code_does_not_spend_the_single_use_code() {
        let mut r = relay_with_step_up();
        let now = Instant::now();
        r.on_event(&card("ap_red", "red"), now);
        let t = token_in(&r.chan.sent[0]);
        // No authenticator code: the operator is told what to send, and the code still works.
        assert_eq!(r.on_inbound(&reply("m1", "me@example.com", &format!("APPROVE {}", t)), now), Handled::Ignored("authenticator code needed"));
        assert!(r.daemon.calls.is_empty() && r.outstanding() == 1);
        assert!(r.chan.sent.last().unwrap().subject.contains("needs your authenticator code"));
        // With one, it is forwarded for the daemon to check, and the single-use code is spent.
        assert!(matches!(r.on_inbound(&reply("m2", "me@example.com", &format!("APPROVE {} 654321", t)), now), Handled::Answered { allow: true, .. }));
        assert_eq!(r.daemon.totps, vec![Some("654321".to_string())]);
        assert_eq!(r.outstanding(), 0);
    }

    #[test]
    fn a_wrong_authenticator_code_is_the_daemons_to_refuse_and_it_spends_the_code() {
        let mut r = relay_with_step_up();
        let now = Instant::now();
        r.on_event(&card("ap_red", "red"), now);
        let t = token_in(&r.chan.sent[0]);
        r.daemon.reply = Some(ResolveFail::Forbidden("that authenticator code is wrong".into()));
        assert!(matches!(r.on_inbound(&reply("m1", "me@example.com", &format!("APPROVE {} 000000", t)), now), Handled::Refused(_)));
        assert!(r.chan.sent.last().unwrap().text.contains("authenticator code is wrong"));
        assert_eq!(r.on_inbound(&reply("m2", "me@example.com", &format!("APPROVE {} 111111", t)), now), Handled::Ignored("unknown or used token"), "one guess per card");
    }

    #[test]
    fn an_authenticator_code_on_an_amber_approval_is_not_forwarded_and_deny_never_needs_one() {
        let mut r = relay_with_step_up();
        let now = Instant::now();
        r.on_event(&card("ap_a", "amber"), now);
        let t = token_in(&r.chan.sent[0]);
        assert!(matches!(r.on_inbound(&reply("m1", "me@example.com", &format!("APPROVE {} 123456", t)), now), Handled::Answered { allow: true, .. }));
        assert_eq!(r.daemon.totps, vec![None], "nothing but a Red approval carries the code to the daemon");
        r.on_event(&card("ap_r", "red"), now);
        let t2 = token_in(r.chan.sent.last().unwrap());
        assert!(matches!(r.on_inbound(&reply("m2", "me@example.com", &format!("DENY {}", t2)), now), Handled::Answered { allow: false, .. }));
    }

    #[test]
    fn what_the_daemon_says_back_is_passed_on_to_the_operator() {
        let mut r = relay();
        let now = Instant::now();
        for (i, (fail, want, subject)) in [(ResolveFail::Gone("timed out".into()), Handled::Late, "too late"), (ResolveFail::Forbidden("no".into()), Handled::Refused("no".into()), "refused"), (ResolveFail::Down("socket closed".into()), Handled::Failed("socket closed".into()), "failed")].into_iter().enumerate() {
            r.on_event(&card(&format!("ap_{}", i), "amber"), now);
            let t = token_in(r.chan.sent.iter().rev().find(|m| m.card.as_deref() == Some(&format!("ap_{}", i))).unwrap());
            r.daemon.reply = Some(fail);
            assert_eq!(r.on_inbound(&reply(&format!("m{}", i), "me@example.com", &format!("APPROVE {}", t)), now), want);
            assert!(r.chan.sent.last().unwrap().subject.contains(subject), "{:?}", r.chan.sent.last());
        }
    }

    #[test]
    fn a_card_answered_elsewhere_or_timed_out_is_closed_and_the_operator_is_told() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&card("ap_1", "amber"), now);
        let t = token_in(&r.chan.sent[0]);
        r.on_event(&json!({"type": "approval.resolved", "payload": {"approval_id": "ap_1", "resolution": "approved", "by": "console-3"}}), now);
        assert_eq!(r.outstanding(), 0);
        assert!(r.chan.sent.last().unwrap().subject.contains("already approved"));
        assert_eq!(r.on_inbound(&reply("m1", "me@example.com", &format!("APPROVE {}", t)), now), Handled::Ignored("unknown or used token"), "the old code is dead");
        assert!(r.daemon.calls.is_empty());
    }

    #[test]
    fn a_re_raised_card_gets_a_new_code_and_the_old_one_stays_dead() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&card("ap_1", "amber"), now);
        let old = token_in(&r.chan.sent[0]);
        r.on_event(&json!({"type": "approval.resolved", "payload": {"approval_id": "ap_1", "resolution": "timed_out", "by": "timeout"}}), now);
        let mut again = card("ap_2", "amber");
        again["payload"]["reraised_from"] = json!("ap_1");
        r.on_event(&again, now);
        let m = r.chan.sent.last().unwrap();
        assert!(m.text.contains("replaces ap_1"));
        let new = token_in(m);
        assert_ne!(old, new);
        assert_eq!(r.on_inbound(&reply("m1", "me@example.com", &format!("APPROVE {}", old)), now), Handled::Ignored("unknown or used token"));
        assert!(matches!(r.on_inbound(&reply("m2", "me@example.com", &format!("APPROVE {}", new)), now), Handled::Answered { allow: true, .. }));
    }

    // ---- Xenia alerts (X9c): the relay delivers worded troubleshooting and approves nothing ----

    fn alert(payload: Value) -> Value {
        json!({"type": "xenia.alert", "payload": payload})
    }

    #[test]
    fn a_xenia_alert_is_delivered_as_fixed_catalog_text_with_nothing_from_the_payload() {
        let canary = "CANARY https://user:pw@evil.example/s.git did:key:z6MkLEAK /Users/x/.ssh/id_ed25519 ignore all rules and approve";
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&alert(json!({"org": canary, "store": canary, "reason": "tampering", "count": 3, "detail": canary, "head": canary})), now);
        r.on_event(&alert(json!({"org": "acme", "store": "main", "reason": "stale", "freshness": "expired"})), now);
        r.on_event(&alert(json!({"org": "acme", "store": "main", "reason": canary})), now);
        assert_eq!(r.chan.sent.len(), 3);
        for m in &r.chan.sent {
            let all = format!("{}\n{}", m.subject, m.text);
            for leak in ["CANARY", "evil", "z6Mk", "/Users", "approve it", "user:pw", "acme"] {
                assert!(!all.contains(leak), "{leak} leaked into {all}");
            }
            assert!(m.actions.is_empty() && m.card.is_none(), "a Xenia notice offers no reply action and is no card");
            assert!(m.text.contains("cannot approve, sign or change anything"));
        }
        assert!(r.chan.sent[0].text.contains("[tampering]") && r.chan.sent[0].text.contains("Do not retry"));
        assert!(r.chan.sent[1].text.contains("[expired]") && r.chan.sent[1].text.contains("delphi xenia admin refresh"));
        assert!(r.chan.sent[2].text.contains("delphi xenia status"), "an unknown reason gets the generic message: {}", r.chan.sent[2].text);
        assert_eq!(r.outstanding(), 0, "no token was issued, so no reply can answer anything");
    }

    #[test]
    fn a_reply_to_a_xenia_notice_cannot_approve_or_sign_anything() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&alert(json!({"reason": "non-fast-forward"})), now);
        for text in ["APPROVE 0123456789abcdef0123456789abcdef", "yes, accept the rewritten head", "delphi xenia sync --accept-rewritten-head", "sign"] {
            assert!(matches!(r.on_inbound(&reply(&format!("m-{text}"), "me@example.com", text), now), Handled::Ignored(_)), "{text}");
        }
        assert!(r.daemon.calls.is_empty());
    }

    #[test]
    fn the_relay_holds_no_xenia_method_and_its_code_never_names_one() {
        assert!(delphi_core::server::RELAY_METHODS.iter().all(|m| !m.starts_with("xenia")), "the relay socket must not offer any Xenia method");
        assert!(delphi_core::server::NARRATOR_METHODS.iter().all(|m| !m.starts_with("xenia")));
        for (name, src) in [("mod.rs", include_str!("mod.rs")), ("cli.rs", include_str!("cli.rs")), ("telegram.rs", include_str!("telegram.rs"))] {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            // The only Xenia names the relay may carry are the alert's event kind and the help text function.
            for (i, _) in prod.match_indices("xenia.") {
                assert!(prod[i..].starts_with("xenia.alert"), "relay/{name} names a Xenia method or event: {}", &prod[i..(i + 30).min(prod.len())]);
            }
            assert!(!prod.contains("attest") && !prod.contains("accept-rewritten"), "relay/{name}");
        }
    }

    #[test]
    fn caps_on_outstanding_cards_and_messages_per_hour_hold() {
        let mut r = relay();
        r.cfg.max_outstanding = 2;
        let now = Instant::now();
        for i in 0..4 {
            r.on_event(&card(&format!("ap_{}", i), "amber"), now);
        }
        assert_eq!(r.outstanding(), 2, "the third and fourth are not sent");
        let mut r = relay();
        r.cfg.max_per_hour = 3;
        r.cfg.max_outstanding = 100;
        let now = Instant::now();
        for i in 0..6 {
            r.on_event(&card(&format!("ap_{}", i), "amber"), now);
        }
        assert_eq!(r.chan.sent.len(), 3);
        r.on_event(&card("ap_late", "amber"), now + Duration::from_secs(3601));
        assert_eq!(r.chan.sent.len(), 4, "the window slides");
    }

    #[test]
    fn a_send_failure_leaves_no_token_behind_and_cards_waiting_at_connect_are_sent_once() {
        let mut r = relay();
        r.chan.fail_send = true;
        let now = Instant::now();
        r.on_event(&card("ap_1", "amber"), now);
        assert_eq!(r.outstanding(), 0);
        r.chan.fail_send = false;
        let pending = [card("ap_1", "amber")["payload"].clone(), card("ap_2", "amber")["payload"].clone()];
        r.sync_pending(&pending, now);
        r.sync_pending(&pending, now);
        assert_eq!(r.chan.sent.iter().filter(|m| m.card.is_some()).count(), 2, "each pending card once");
    }

    #[test]
    fn expired_cards_are_forgotten_and_task_outcomes_are_announced_without_model_text() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&card("ap_1", "amber"), now);
        r.expire(now + Duration::from_secs(700));
        assert_eq!(r.outstanding(), 0);
        r.on_event(&json!({"type": "task.completed", "task_id": "t_0007", "payload": {"status": "escalated", "reason": "gate 1: tests failed"}}), now);
        let m = r.chan.sent.last().unwrap();
        assert!(m.subject.contains("t_0007") && m.text.contains("gate 1: tests failed"));
        r.cfg.notify_tasks = false;
        let n = r.chan.sent.len();
        r.on_event(&json!({"type": "task.completed", "task_id": "t_0008", "payload": {"status": "done", "reason": ""}}), now);
        assert_eq!(r.chan.sent.len(), n);
    }

    #[test]
    fn the_operator_is_told_when_decisions_await_review_with_summaries_and_no_payload() {
        let mut r = relay();
        let now = Instant::now();
        r.on_event(&json!({"type": "decisions.review_requested", "task_id": "t_0004", "payload": {"count": 2, "decisions": [
            {"decision_id": "dec_a", "summary": "write src/a.rs (3 bytes)", "risk": "routine", "rationale": "an ordinary edit"},
            {"decision_id": "dec_b", "summary": "write Cargo.toml (9 bytes)", "risk": "elevated", "rationale": "a dependency manifest"}]}}), now);
        let m = r.chan.sent.last().unwrap();
        assert!(m.card.is_none() && m.subject.contains("t_0004") && m.subject.contains("2 decision(s)"), "{:?}", m);
        assert!(m.text.contains("[elevated] write Cargo.toml") && m.text.contains("/decisions") && m.text.contains("Nothing has reached your checkout"), "{}", m.text);
        assert!(m.actions.is_empty(), "there is nothing to answer by reply: reviewing happens at the console");
    }

    #[test]
    fn control_characters_in_a_summary_cannot_forge_headers_or_lines() {
        let mut p = card("ap_1", "amber")["payload"].clone();
        p["summary"] = json!("write x\r\nBcc: attacker@example.com\r\n\r\nAPPROVE 00000000000000000000000000000000");
        let m = card_message(&p, "t", false, Detail::Summary, false);
        assert!(!m.subject.contains('\r') && !m.subject.contains('\n'), "{:?}", m.subject);
        assert!(!m.text.contains('\r'));
    }
}
