//! The terminal approver for the admin operations (spec 011 sections 3.2, 4.1, 8; plan milestone X9a): the only thing
//! in the command line that can answer "yes" for a roster signature, a freshness statement or a genesis. The member
//! commands have their own (`xenia_cmd.rs`); this one is the same idea over the admin preview.
//!
//! What this module is allowed to touch is deliberately small, and a test reads this file's own source to keep it so:
//! it holds a [`Terminal`] (somewhere to print and one place to read a line from) and the immutable preview it is
//! given. It has no way to reach the engine, a daemon, a socket, the network, a process, the environment or the file
//! system, so nothing that could answer in a person's place is in reach of the code that decides.
//!
//! * **A person at a terminal, or nothing.** [`StdTerminal`] answers `interactive()` only when stdin and stderr are
//!   both terminals and the process is not running inside an agent seat. A piped or redirected stdin, a script and a
//!   harness all get a refusal that names why. There is no flag, environment variable, file or pipe that answers.
//! * **The operator types it.** Every security flag is shown with its words and must be acknowledged by typing its
//!   name; the final step is typing the first eight characters of the digest of the exact bytes plus a code that is
//!   new for each prompt. Anything else, an empty line or the end of input cancels.
//! * **The bytes shown are the bytes digested.** The preview is printed from the same fields the library digests;
//!   control characters other than the line break are printed escaped, so a record cannot repaint the screen.

use delphi_core::xenia_admin::{AdminApprover, AdminDecision, AdminPreview};

/// Somewhere to print and read, and the answer to "is a person here". Tests give their own; the real one is
/// [`StdTerminal`].
pub trait Terminal {
    /// `Ok(())` only for a person at a terminal; otherwise the reason it is not.
    fn interactive(&self) -> Result<(), String>;
    fn show(&mut self, text: &str);
    /// One line typed by the person; `None` at the end of input or on a read error.
    fn ask(&mut self, prompt: &str) -> Option<String>;
    /// A short code that is new for every call.
    fn fresh_code(&mut self) -> String;
}

/// The real terminal: prompts go to stderr (stdout may carry `--json`), answers come from stdin.
pub struct StdTerminal {
    agent_seat: bool,
    code: fn() -> String,
}

impl StdTerminal {
    /// `agent_seat` is true when the process was started by an agent seat (the caller looks at its environment);
    /// `code` gives a fresh unguessable string per call.
    pub fn new(agent_seat: bool, code: fn() -> String) -> StdTerminal {
        StdTerminal { agent_seat, code }
    }
}

impl Terminal for StdTerminal {
    fn interactive(&self) -> Result<(), String> {
        if self.agent_seat {
            return Err("this process was started by an agent seat; approving is a human act and has no non-interactive mode".into());
        }
        // SAFETY: isatty only inspects a file descriptor.
        let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(2) == 1 };
        if !on_terminal {
            return Err("approving is a human act: run this in a terminal (it has no flag, environment variable, file or pipe that answers for you)".into());
        }
        Ok(())
    }

    fn show(&mut self, text: &str) {
        use std::io::Write;
        let mut e = std::io::stderr().lock();
        let _ = e.write_all(text.as_bytes());
        let _ = e.flush();
    }

    fn ask(&mut self, prompt: &str) -> Option<String> {
        use std::io::{BufRead, Write};
        self.show(prompt);
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim().to_string()),
        }
    }

    fn fresh_code(&mut self) -> String {
        (self.code)()
    }
}

/// Print `text` with every control character except the line break written as `\xNN`, and a line break only where
/// the text has one. What the screen shows is then exactly the printable bytes, never an escape sequence.
pub fn tame(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => out.push('\n'),
            c if c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}') => {
                out.push_str(&format!("\\u{{{:04x}}}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn bytes_text(b: &[u8]) -> String {
    tame(&String::from_utf8_lossy(b))
}

fn short_digest(digest: &str) -> String {
    digest.trim_start_matches("blake3:").chars().take(8).collect()
}

/// Why the last decision was a refusal, for the caller to report instead of the library's plain "declined".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    NotInteractive(String),
    FlagNotAcknowledged(String),
    WrongChallenge,
    Cancelled,
}

impl Why {
    pub fn reason(&self) -> &'static str {
        match self {
            Why::NotInteractive(_) => "not-interactive",
            Why::FlagNotAcknowledged(_) => "flag-not-acknowledged",
            Why::WrongChallenge => "challenge-failed",
            Why::Cancelled => "cancelled",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Why::NotInteractive(m) => m.clone(),
            Why::FlagNotAcknowledged(f) => format!("the flag `{f}` was not acknowledged by typing its name; nothing was signed"),
            Why::WrongChallenge => "what was typed does not match the challenge; nothing was signed".into(),
            Why::Cancelled => "cancelled at the prompt (no answer); nothing was signed".into(),
        }
    }
}

/// The interactive approver for both traits. After a refusal, [`TermApprover::why`] says why.
pub struct TermApprover<'t> {
    term: &'t mut dyn Terminal,
    why: Option<Why>,
}

impl<'t> TermApprover<'t> {
    pub fn new(term: &'t mut dyn Terminal) -> TermApprover<'t> {
        TermApprover { term, why: None }
    }

    pub fn why(&self) -> Option<&Why> {
        self.why.as_ref()
    }

    /// Flags first (each by name), then the digest challenge. `true` only when everything was typed right.
    fn challenge(&mut self, flags: &[(String, String)], digest: &str, what: &str) -> bool {
        for (slug, text) in flags {
            self.term.show(&format!("\nFLAG {slug}: {}\n", tame(text)));
            let ans = self.term.ask(&format!("  type `{slug}` to acknowledge it, anything else cancels: "));
            match ans {
                Some(a) if a == *slug => {}
                Some(_) => {
                    self.why = Some(Why::FlagNotAcknowledged(slug.clone()));
                    return false;
                }
                None => {
                    self.why = Some(Why::Cancelled);
                    return false;
                }
            }
        }
        let code = self.term.fresh_code();
        let want = format!("{}-{}", short_digest(digest), code);
        self.term.show(&format!("\ndigest of everything above: {digest}\n"));
        let ans = self.term.ask(&format!("to {what}, type {want} (the first 8 characters of the digest, a dash, this prompt's code); anything else cancels: "));
        match ans {
            Some(a) if a == want => true,
            Some(_) => {
                self.why = Some(Why::WrongChallenge);
                false
            }
            None => {
                self.why = Some(Why::Cancelled);
                false
            }
        }
    }

    fn gate(&mut self) -> bool {
        match self.term.interactive() {
            Ok(()) => true,
            Err(m) => {
                self.why = Some(Why::NotInteractive(m));
                false
            }
        }
    }
}

impl AdminApprover for TermApprover<'_> {
    fn decide(&mut self, p: &AdminPreview) -> AdminDecision {
        self.why = None;
        if !self.gate() {
            return AdminDecision::Deny;
        }
        let mut s = String::new();
        s.push_str(&format!("\n=== admin {:?}: {}/{} epoch {} as {} ===\n", p.action, tame(&p.org), tame(&p.store), p.epoch, p.signer));
        if let Some(h) = &p.store_head {
            s.push_str(&format!("store head this is built on: {h}\n"));
        }
        if let Some(e) = &p.expires_at {
            s.push_str(&format!("statement expires: {e}\n"));
        }
        for c in &p.changes {
            s.push_str(&format!("change: {} {} ({}, {}) {}\n", c.kind.slug(), c.did, tame(&c.person), tame(&c.handle), tame(&c.detail)));
        }
        if let Some(q) = &p.quorum {
            s.push_str(&format!("quorum: {} of {} people have signed ({}){}\n", q.have, q.need, tame(&q.people.join(", ")), if q.met { ", met" } else { "" }));
        }
        for (base, n) in &p.walked {
            s.push_str(&format!("history walked from {base}: {n} commit(s), no findings\n"));
        }
        for f in &p.files {
            s.push_str(&format!("\n--- {} (exact bytes)\n", tame(&f.path)));
            s.push_str(&bytes_text(&f.bytes));
            if !f.bytes.ends_with(b"\n") {
                s.push('\n');
            }
        }
        s.push_str("\nthe key will sign exactly these domains:\n");
        for d in &p.signs {
            s.push_str(&format!("  {}\n", tame(d).replace('\n', "\\n")));
        }
        for r in &p.reminders {
            s.push_str(&format!("\nreminder {}: {}\n", r.slug(), r.text()));
        }
        self.term.show(&s);
        let flags: Vec<(String, String)> = p.flags.iter().map(|f| (f.slug().to_string(), f.text().to_string())).collect();
        if self.challenge(&flags, &p.digest, "sign this") {
            AdminDecision::Approve(p.approve(true))
        } else {
            AdminDecision::Deny
        }
    }
}

/// A plain typed confirmation for acts that have no preview type (a rewrite recovery): the lines are shown, and the
/// person must type exactly `expect`. Refuses, before showing anything, when no person is at a terminal.
pub fn confirm_typed(term: &mut dyn Terminal, lines: &str, expect: &str, what: &str) -> Result<(), Why> {
    if let Err(m) = term.interactive() {
        return Err(Why::NotInteractive(m));
    }
    term.show(lines);
    match term.ask(&format!("\nto {what}, type {expect}; anything else cancels: ")) {
        Some(a) if a == expect => Ok(()),
        Some(_) => Err(Why::WrongChallenge),
        None => Err(Why::Cancelled),
    }
}
