//! The terminal under the typed confirmation of `sync --accept-rewritten-head` (spec 011 section 8.4; plan milestone X9a).
//!
//! What this module may touch is deliberately small: a [`Terminal`] (somewhere to print and one place to read a line
//! from). It has no way to reach the engine, a daemon, a socket, the network, a process or the environment, so nothing
//! that could answer in a person's place is in reach of the code that decides. [`StdTerminal`] answers `interactive()`
//! only when stdin and stderr are both terminals and the process is not running inside an agent seat; a piped or
//! redirected stdin, a script and a harness all get a refusal that names why. There is no flag, environment variable,
//! file or pipe that answers.

/// Somewhere to print and read, and the answer to "is a person here". Tests give their own; the real one is
/// [`StdTerminal`].
pub trait Terminal {
    /// `Ok(())` only for a person at a terminal; otherwise the reason it is not.
    fn interactive(&self) -> Result<(), String>;
    fn show(&mut self, text: &str);
    /// One line typed by the person; `None` at the end of input or on a read error.
    fn ask(&mut self, prompt: &str) -> Option<String>;
}

/// The real terminal: prompts go to stderr (stdout may carry `--json`), answers come from stdin.
pub struct StdTerminal {
    agent_seat: bool,
}

impl StdTerminal {
    /// `agent_seat` is true when the process was started by an agent seat (the caller looks at its environment).
    pub fn new(agent_seat: bool) -> StdTerminal {
        StdTerminal { agent_seat }
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

}

/// Print `text` with every control character except the line break written as `\xNN`, and a line break only where
/// the text has one. What the screen shows is then exactly the printable bytes, never an escape sequence.
pub fn tame(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => out.push('\n'),
            c if c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}' | '\u{e0000}'..='\u{e007f}') => {
                out.push_str(&format!("\\u{{{:04x}}}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Why the last decision was a refusal, for the caller to report instead of the library's plain "declined".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    NotInteractive(String),
    WrongChallenge,
    Cancelled,
}

impl Why {
    pub fn reason(&self) -> &'static str {
        match self {
            Why::NotInteractive(_) => "not-interactive",
            Why::WrongChallenge => "challenge-failed",
            Why::Cancelled => "cancelled",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Why::NotInteractive(m) => m.clone(),
            Why::WrongChallenge => "what was typed does not match the challenge; nothing was signed".into(),
            Why::Cancelled => "cancelled at the prompt (no answer); nothing was signed".into(),
        }
    }
}

/// A plain typed confirmation for acts that have no preview type (a rewrite recovery): the lines are shown, and the
/// person must type exactly `expect`. Refuses, before showing anything, when no person is at a terminal.
pub fn confirm_typed(term: &mut dyn Terminal, lines: &str, expect: &str, what: &str) -> Result<(), Why> {
    if let Err(m) = term.interactive() {
        return Err(Why::NotInteractive(m));
    }
    // Whatever the caller built, what reaches the screen is printable text: no escape sequence, no hidden reordering.
    term.show(&tame(lines));
    match term.ask(&format!("\nto {what}, type {expect}; anything else cancels: ")) {
        Some(a) if a == expect => Ok(()),
        Some(_) => Err(Why::WrongChallenge),
        None => Err(Why::Cancelled),
    }
}

/// An agent seat starts its tools with these set; a signature never comes from inside one.
pub(crate) fn inside_an_agent_seat() -> bool {
    std::env::var_os("DELPHI_BRIDGE_SOCKET").is_some() || std::env::var_os("DELPHI_BRIDGE_TOKEN").is_some()
}
