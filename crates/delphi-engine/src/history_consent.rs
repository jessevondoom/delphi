//! The install question and the first-run notice of the project history (`specs/102` §7 items 3, 4, 7 and 10, §11 item 15), slices S4 and S8.
//!
//! **Shown since slice S8.** [`SHOW_AT_INSTALL`] is `true` (the operator's go, 2026-11-12, after the red-team audit of slice S7 passed and
//! the egress check of spec 098 §12 item 16 was wired for all four channels by spec 103). The default of `[history] enabled` is on
//! (opt-out, the operator's decision of 2026-11-11), **but nothing is recorded, or even held in memory, until a person has answered the
//! question** (§7 item 4, §11 item 6): a fresh install and an upgraded one are the same, the answer file `local/history.consent` is the
//! switch. An answer of no switches the history off (the same marker `delphi history off` writes).
//!
//! **Only a human answers** (§11 item 15). [`record_answer`] needs a [`Human`], and the only way to make one is [`Human::at_terminal`],
//! which refuses an agent seat, a call that came in over the host socket, and any process without a terminal on both ends. There is no
//! daemon method that answers, so nothing a host or a seat can send reaches it.
//!
//! **The notice is shown once** in the console ([`take_notice`]): after a yes, the first time the console asks. The marker
//! `local/history.noticed` records that it was shown.

use std::path::{Path, PathBuf};

/// The S8 switch. `true` since slice S8: the question is asked (once, of a person at a terminal) and nothing is recorded until it is answered.
pub const SHOW_AT_INSTALL: bool = true;

/// The install question (`delphi init`, `delphi connect`), word for word from §7 item 3.
pub const QUESTION: &str = "Keep a history of your prompts and what came of them on this machine? Yes by default; you can turn it off or erase it at any time. [Y/n]";

/// What the question is about, printed above it (§7 item 3: the operator is told plainly what is stored, where, and how to stop it).
pub const FACTS: &str = "Delphi can keep a history of your prompts and what came of them, so a project's story is not lost between sessions.
  Kept:    the words you type to Delphi (secrets are masked, best effort; a pasted block is kept as a fingerprint and a length, never as text)
           and a short summary of what came of each.
  Where:   on this machine only, under your Delphi home. Nothing sends it anywhere: not a model (unless you allow it), a relay, an email,
           a connector, a bundle or an export.
  Control: `delphi history off` stops it at once; `delphi history purge` erases it; `delphi history status` shows what is kept.
  Limits:  masking can miss a secret in an unusual shape; short pasted text is kept as written; the mission logs keep prompts after a purge.";

/// The first-run notice in the console (§7 item 7), said once: the history is on, how to turn it off, and what redaction cannot promise.
pub const NOTICE: &str = "Delphi is keeping a history of your prompts and what came of them on this machine only (redacted, never sent anywhere). Turn it off with `delphi history off`; erase it with `delphi history purge`; see what is kept with `delphi history status`. Redaction is best effort: some secret shapes may not be masked, short pasted text is kept as written, and the mission logs keep prompts after a purge.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
}

fn consent_path(ws_dir: &Path) -> PathBuf {
    // Beside `history.off`, not inside the store directory: a project named `consent` has a directory there.
    ws_dir.join("local/history.consent")
}

/// What the operator answered, if they did. A missing or unreadable file means not answered.
pub fn answered(ws_dir: &Path) -> Option<Answer> {
    match std::fs::read_to_string(consent_path(ws_dir)).ok()?.split_whitespace().next()? {
        "yes" => Some(Answer::Yes),
        "no" => Some(Answer::No),
        _ => None,
    }
}

/// Proof that a person at a terminal is answering. Made only by [`Human::at_terminal`].
#[derive(Debug)]
pub struct Human(());

impl Human {
    /// `terminal` is "stdin and stdout are a terminal", `agent_seat` is "this process was started by an agent seat" (the bridge
    /// variables the seat sets), and `host_socket` is "this call arrived over the host socket". Each of the last two refuses.
    pub fn at_terminal(terminal: bool, agent_seat: bool, host_socket: bool) -> Result<Human, String> {
        if agent_seat {
            return Err("the history question is a human's to answer and is refused inside an agent seat".into());
        }
        if host_socket {
            return Err("the history question cannot be answered over the host socket".into());
        }
        if !terminal {
            return Err("the history question needs a terminal on both stdin and stdout".into());
        }
        Ok(Human(()))
    }
}

/// Record the answer. Yes removes the global switch-off marker; No writes it, which stops new records at once and deletes nothing.
/// `[history] enabled` is not edited: the default stays what slice S8 makes it, and an answer of yes only means asked-and-allowed.
pub fn record_answer(ws_dir: &Path, _human: &Human, answer: Answer) -> Result<(), String> {
    let path = consent_path(ws_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|_| "the answer could not be recorded".to_string())?;
    }
    let word = if answer == Answer::Yes { "yes" } else { "no" };
    std::fs::write(&path, format!("{word}\n")).map_err(|_| "the answer could not be recorded".to_string())?;
    match answer {
        Answer::Yes => {
            let _ = std::fs::remove_file(crate::history_record::global_off_path(ws_dir));
        }
        Answer::No => {
            crate::history_manage::off(ws_dir, None)?;
        }
    }
    Ok(())
}

/// The question to put to the operator now, or `None`: while the S8 switch is off (`show` is the switch, passed in so a test can
/// turn it on), or once it has been answered.
pub fn question_to_show(ws_dir: &Path, show: bool) -> Option<&'static str> {
    (show && answered(ws_dir).is_none()).then_some(QUESTION)
}

fn noticed_path(ws_dir: &Path) -> PathBuf {
    ws_dir.join("local/history.noticed")
}

/// Has the console already shown the notice? A missing marker means not yet.
pub fn noticed(ws_dir: &Path) -> bool {
    noticed_path(ws_dir).exists()
}

/// The first-run notice, or `None`: while the S8 switch is off, when the history is not on, when nobody said yes (a no says nothing; an
/// unanswered question is asked first), or once it has been shown.
pub fn first_run_notice(ws_dir: &Path, show: bool, history_on: bool) -> Option<&'static str> {
    (show && history_on && answered(ws_dir) == Some(Answer::Yes) && !noticed(ws_dir)).then_some(NOTICE)
}

/// The notice for the console to say now, **once**: returns it and records that it was shown. `None` when there is nothing to say.
pub fn take_notice(ws_dir: &Path, history_on: bool) -> Option<&'static str> {
    let text = first_run_notice(ws_dir, SHOW_AT_INSTALL, history_on)?;
    let path = noticed_path(ws_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    // If the marker cannot be written the notice is not said: it would repeat on every start otherwise.
    std::fs::write(&path, "shown\n").ok()?;
    Some(text)
}

/// Does the question still gate writes? True only when the S8 switch is on and nobody has answered (§7 item 4: "never switched on
/// silently"). With the switch off it never blocks.
pub fn blocks_writes(ws_dir: &Path, show: bool) -> bool {
    show && answered(ws_dir).is_none()
}

/// A scratch workspace directory in which the install question has been **answered yes**, the state of a real install after its first
/// run (`specs/102` S8: a record is written only after that). The history tests that are about writing, reading or erasing records use
/// it; the tests about the unanswered state use a bare `tempfile::tempdir()` on purpose.
#[cfg(test)]
pub(crate) fn answered_ws() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    answer_yes_in(d.path());
    d
}

/// Answer yes in an existing workspace directory (a test fixture that already has its own).
#[cfg(test)]
pub(crate) fn answer_yes_in(ws_dir: &Path) {
    std::fs::create_dir_all(ws_dir.join("local")).unwrap();
    std::fs::write(ws_dir.join("local/history.consent"), "yes\n").unwrap();
}

#[cfg(test)]
#[path = "history_consent_tests.rs"]
mod tests;
