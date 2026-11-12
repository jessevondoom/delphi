//! The install question and the first-run notice of the project history (`specs/102` §7 items 3, 4, 7 and 10, §11 item 15), slice S4.
//!
//! **Written, wired, and not shown.** [`SHOW_AT_INSTALL`] is `false` until slice S8 flips it (the operator's decision, OD-8: the whole
//! feature ships in one release, after the red-team audit of slice S7 has passed and the egress check is live). While it is `false`:
//! * [`question_to_show`] and [`first_run_notice`] return `None`, so nothing prints anywhere;
//! * [`blocks_writes`] is `false`, so the question gates nothing (an operator who sets `[history] enabled = true` by hand is not
//!   locked out by a question nobody can answer).
//!
//! When S8 flips it: a fresh install or an existing one is asked once; **until a human has answered, no record is written** (§7 item 4,
//! §11 item 6); an answer of no switches the history off (the same marker `delphi history off` writes).
//!
//! **Only a human answers** (§11 item 15). [`record_answer`] needs a [`Human`], and the only way to make one is [`Human::at_terminal`],
//! which refuses an agent seat, a call that came in over the host socket, and any process without a terminal on both ends. There is no
//! daemon method, so nothing a host or a seat can send reaches it.

use std::path::{Path, PathBuf};

/// The S8 switch. `false` until S8: the question and the notice are written but never shown.
pub const SHOW_AT_INSTALL: bool = false;

/// The install question (`delphi init`, `delphi connect`), word for word from §7 item 3.
pub const QUESTION: &str = "Keep a history of your prompts and what came of them on this machine? Yes by default; you can turn it off or erase it at any time. [Y/n]";

/// The first-run notice in the console (§7 item 7).
pub const NOTICE: &str = "Delphi keeps a history of your prompts and what came of them on this machine only (redacted, never sent anywhere). Turn it off with `delphi history off`; erase it with `delphi history purge`; see what is kept with `delphi history status`.";

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

/// The first-run notice, or `None`: while the S8 switch is off, when the history is not on, or when the operator answered no.
pub fn first_run_notice(ws_dir: &Path, show: bool, history_on: bool) -> Option<&'static str> {
    (show && history_on && answered(ws_dir) != Some(Answer::No)).then_some(NOTICE)
}

/// Does the question still gate writes? True only when the S8 switch is on and nobody has answered (§7 item 4: "never switched on
/// silently"). With the switch off it never blocks.
pub fn blocks_writes(ws_dir: &Path, show: bool) -> bool {
    show && answered(ws_dir).is_none()
}

#[cfg(test)]
#[path = "history_consent_tests.rs"]
mod tests;
