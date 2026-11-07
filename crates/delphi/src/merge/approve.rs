//! How a merge is approved (spec 094 §3.2): `tty`, or a `chat-quote`.
//!
//! * `tty`: the operator is at a terminal and types the PR number back (for a batch, the word `all`).
//! * `chat-quote`: an agent runs the command because the operator's turn said to, and passes the operator's own words. The
//!   quote must **name the action**: the PR number for a single merge, or `all`/`every`/`each` for a batch. A vague "yes" or a
//!   quote about another pull request is refused. It is still self-attested by the caller, which is why it is recorded as
//!   `chat-quote` everywhere and never accepted for a gate-changing pull request.

use super::record::Attestation;
use regex::Regex;

pub trait Terminal {
    /// Both input and output are a terminal, so a person can be reading and answering.
    fn is_interactive(&self) -> bool;
    fn show(&mut self, text: &str);
    /// One line typed by the person, or `None` when there is no input.
    fn ask(&mut self, prompt: &str) -> Option<String>;
}

pub struct StdTerminal;

impl Terminal for StdTerminal {
    fn is_interactive(&self) -> bool {
        // SAFETY: isatty only inspects a file descriptor.
        unsafe { libc::isatty(0) == 1 && libc::isatty(2) == 1 }
    }
    fn show(&mut self, text: &str) {
        eprintln!("{text}");
    }
    fn ask(&mut self, prompt: &str) -> Option<String> {
        use std::io::{BufRead, Write};
        eprint!("{prompt}");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(n) if n > 0 => Some(line.trim().to_string()),
            _ => None,
        }
    }
}

/// What the approval is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// One pull request: a quote must contain its number.
    Pr(u64),
    /// A batch (`--all`): a quote must say `all`, `every` or `each`.
    Batch,
    /// A bounded `--auto` session: a quote must ask for automatic merging (`auto`, `automatically`, `as they come in`).
    /// Neither a single pull request's number nor "all" authorizes a session that merges pull requests which do not exist yet.
    Auto,
}

/// A quote is words from the operator, not a payload: bounded, printable, one paragraph at most.
fn quote_ok(q: &str) -> Result<&str, String> {
    let q = q.trim();
    if q.is_empty() {
        return Err("--operator-said needs the operator's own words".into());
    }
    if q.chars().count() > 2000 {
        return Err("--operator-said is longer than 2000 characters; pass the operator's request, not a transcript".into());
    }
    if q.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err("--operator-said has control characters in it".into());
    }
    Ok(q)
}

/// Does the quote name the action? A PR number must appear as its own number (`#552`, `552`, not `1552`).
pub fn quote_binds(quote: &str, target: Target) -> bool {
    match target {
        Target::Pr(n) => Regex::new(&format!(r"(^|[^0-9]){n}([^0-9]|$)")).map(|re| re.is_match(quote)).unwrap_or(false),
        Target::Batch => Regex::new(r"(?i)\b(all|every|each)\b").map(|re| re.is_match(quote)).unwrap_or(false),
        Target::Auto => Regex::new(r"(?i)\b(auto|automatic|automatically|as they (come|are queued|arrive|land))\b").map(|re| re.is_match(quote)).unwrap_or(false),
    }
}

/// Obtain the approval for `target`. `gate_changing` demands a person at a terminal who has been shown the diff.
pub fn attest(term: &mut dyn Terminal, quote: Option<&str>, target: Target, gate_changing: bool) -> Result<Attestation, String> {
    let (what, word) = match target {
        Target::Pr(n) => (format!("pull request #{n}"), n.to_string()),
        Target::Batch => ("every queued pull request listed above".to_string(), "all".to_string()),
        Target::Auto => ("a bounded --auto session (it merges queued pull requests as they arrive, within the caps shown above)".to_string(), "auto".to_string()),
    };
    if gate_changing {
        if quote.is_some() {
            return Err(format!("{what} changes a gate, so a chat quote is not accepted: run `delphi merge` yourself in a terminal"));
        }
        if !term.is_interactive() {
            return Err(format!("{what} changes a gate and needs a person at a terminal, who is shown the whole diff first"));
        }
        return typed(term, &what, &word);
    }
    if let Some(q) = quote {
        let q = quote_ok(q)?;
        if !quote_binds(q, target) {
            return Err(match target {
                Target::Pr(n) => format!("the quote does not name pull request #{n}; the operator's request must mention it (or run the command in a terminal)"),
                Target::Batch => "the quote does not say `all`, `every` or `each`; the operator's request must ask for the whole queue (or run the command in a terminal)".to_string(),
                Target::Auto => "the quote does not ask for automatic merging (`auto`, `automatically`, `as they come in`); a request to merge one pull request or `all` does not authorize a session (or run the command in a terminal)".to_string(),
            });
        }
        return Ok(Attestation::ChatQuote(q.to_string()));
    }
    if term.is_interactive() {
        return typed(term, &what, &word);
    }
    Err("there is no terminal to ask at and no --operator-said: the operator must run this at a terminal, or an agent must pass the operator's own words".into())
}

fn typed(term: &mut dyn Terminal, what: &str, word: &str) -> Result<Attestation, String> {
    match term.ask(&format!("To merge {what}, type {word}; anything else cancels: ")) {
        Some(a) if a == word => Ok(Attestation::Tty),
        Some(_) => Err("cancelled: that was not the confirmation word".into()),
        None => Err("cancelled: no answer".into()),
    }
}

#[cfg(test)]
pub struct FakeTerminal {
    pub interactive: bool,
    pub answers: std::collections::VecDeque<String>,
    pub shown: Vec<String>,
}

#[cfg(test)]
impl FakeTerminal {
    pub fn new(interactive: bool, answers: &[&str]) -> FakeTerminal {
        FakeTerminal { interactive, answers: answers.iter().map(|s| s.to_string()).collect(), shown: vec![] }
    }
}

#[cfg(test)]
impl Terminal for FakeTerminal {
    fn is_interactive(&self) -> bool {
        self.interactive
    }
    fn show(&mut self, text: &str) {
        self.shown.push(text.to_string());
    }
    fn ask(&mut self, _prompt: &str) -> Option<String> {
        self.answers.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quote_must_name_the_pull_request_as_its_own_number() {
        assert!(quote_binds("merge #552 please", Target::Pr(552)));
        assert!(quote_binds("merge 552", Target::Pr(552)));
        assert!(quote_binds("552", Target::Pr(552)));
        assert!(!quote_binds("merge #1552", Target::Pr(552)));
        assert!(!quote_binds("merge #5521", Target::Pr(552)));
        assert!(!quote_binds("yes, go ahead", Target::Pr(552)));
        assert!(quote_binds("merge all the ready ones", Target::Batch));
        assert!(quote_binds("Merge EVERY queued PR", Target::Batch));
        assert!(!quote_binds("merge small ones", Target::Batch));
        assert!(!quote_binds("merge #552", Target::Batch));
    }

    #[test]
    fn a_chat_quote_is_accepted_for_an_ordinary_pull_request_and_labelled() {
        let mut t = FakeTerminal::new(false, &[]);
        let a = attest(&mut t, Some("merge #552"), Target::Pr(552), false).unwrap();
        assert_eq!(a, Attestation::ChatQuote("merge #552".into()));
        assert_eq!(a.label(), "chat-quote");
    }

    #[test]
    fn a_vague_or_wrong_quote_is_refused_and_so_is_an_oversized_or_empty_one() {
        let mut t = FakeTerminal::new(false, &[]);
        assert!(attest(&mut t, Some("yes"), Target::Pr(552), false).unwrap_err().contains("does not name"));
        assert!(attest(&mut t, Some("merge #553"), Target::Pr(552), false).is_err());
        assert!(attest(&mut t, Some("   "), Target::Pr(552), false).unwrap_err().contains("own words"));
        assert!(attest(&mut t, Some(&format!("552 {}", "x".repeat(2001))), Target::Pr(552), false).unwrap_err().contains("2000"));
        assert!(attest(&mut t, Some("552 \u{1b}[31m"), Target::Pr(552), false).unwrap_err().contains("control"));
    }

    #[test]
    fn with_no_quote_a_terminal_asks_for_the_number_and_no_terminal_refuses() {
        let mut ok = FakeTerminal::new(true, &["552"]);
        assert_eq!(attest(&mut ok, None, Target::Pr(552), false), Ok(Attestation::Tty));
        let mut wrong = FakeTerminal::new(true, &["yes"]);
        assert!(attest(&mut wrong, None, Target::Pr(552), false).unwrap_err().contains("cancelled"));
        let mut none = FakeTerminal::new(true, &[]);
        assert!(attest(&mut none, None, Target::Pr(552), false).unwrap_err().contains("no answer"));
        let mut no_tty = FakeTerminal::new(false, &[]);
        assert!(attest(&mut no_tty, None, Target::Pr(552), false).unwrap_err().contains("no terminal"));
    }

    #[test]
    fn a_gate_changing_pull_request_never_takes_a_quote_and_needs_a_person() {
        let mut t = FakeTerminal::new(true, &["552"]);
        assert!(attest(&mut t, Some("merge #552"), Target::Pr(552), true).unwrap_err().contains("not accepted"));
        let mut no_tty = FakeTerminal::new(false, &[]);
        assert!(attest(&mut no_tty, None, Target::Pr(552), true).unwrap_err().contains("needs a person"));
        let mut ok = FakeTerminal::new(true, &["552"]);
        assert_eq!(attest(&mut ok, None, Target::Pr(552), true), Ok(Attestation::Tty));
    }

    #[test]
    fn a_session_needs_its_own_words_and_neither_a_number_nor_all_will_do() {
        assert!(quote_binds("go into auto mode for the evening", Target::Auto));
        assert!(quote_binds("merge them automatically", Target::Auto));
        assert!(quote_binds("merge PRs as they come in", Target::Auto));
        assert!(!quote_binds("merge #552", Target::Auto));
        assert!(!quote_binds("merge all of them", Target::Auto));
        assert!(!quote_binds("author of the change", Target::Auto), "a word that merely contains `auto` is not the word");
        let mut q = FakeTerminal::new(false, &[]);
        assert!(matches!(attest(&mut q, Some("run auto for two hours"), Target::Auto, false), Ok(Attestation::ChatQuote(_))));
        assert!(attest(&mut q, Some("merge all"), Target::Auto, false).unwrap_err().contains("does not authorize a session"));
        let mut tty = FakeTerminal::new(true, &["auto"]);
        assert_eq!(attest(&mut tty, None, Target::Auto, false), Ok(Attestation::Tty));
    }

    #[test]
    fn a_batch_is_confirmed_with_the_word_all() {
        let mut ok = FakeTerminal::new(true, &["all"]);
        assert_eq!(attest(&mut ok, None, Target::Batch, false), Ok(Attestation::Tty));
        let mut q = FakeTerminal::new(false, &[]);
        assert!(matches!(attest(&mut q, Some("merge all of them"), Target::Batch, false), Ok(Attestation::ChatQuote(_))));
    }
}
