use super::*;
use crate::history_record::{append, append_in, Appended, Draft, Observed, Outcome, OutcomeValue, PromptIn, Provenance, Settings, Trigger};
use crate::history_redact::Typing;

fn draft() -> Draft {
    Draft {
        trigger: Trigger::ResearchRun,
        mission: "m1".into(),
        project: "home".into(),
        at: "2026-11-12T00:00:00Z".into(),
        prompt: PromptIn::Observed { text: "hello".into(), marked: vec![], typing: Typing::Typed },
        reply: None,
        outcome: Outcome { value: OutcomeValue::Done, provenance: Provenance::Observed },
        observed: Observed::default(),
    }
}

fn on() -> Settings {
    Settings { enabled: true, ..Settings::default() }
}

fn human() -> Human {
    Human::at_terminal(true, false, false).unwrap()
}

#[test]
fn the_question_is_on_since_s8_and_a_fresh_or_upgraded_install_is_asked_and_records_nothing_until_answered() {
    // The constant slice S8 flipped (the operator's go, 2026-11-12, after S7 passed and the egress check covered every channel).
    const { assert!(SHOW_AT_INSTALL, "slice S8 turned the question on") };
    // A fresh workspace directory and an upgraded one look the same: no answer file. Both are asked, and neither records.
    let ws = tempfile::tempdir().unwrap();
    assert_eq!(question_to_show(ws.path(), SHOW_AT_INSTALL), Some(QUESTION));
    assert!(blocks_writes(ws.path(), SHOW_AT_INSTALL), "nothing is recorded until a person has answered");
    assert_eq!(first_run_notice(ws.path(), SHOW_AT_INSTALL, true), None, "the notice follows a yes, not the question");
    // The default setting is on (opt-out), and still nothing is written or keyed.
    assert!(Settings::default().enabled, "S8 made the default on");
    assert_eq!(append(ws.path(), &Settings::default(), draft()), Appended::Dormant);
    assert!(!ws.path().join("local").exists(), "no file and no key were made before the answer");
}

#[test]
fn the_texts_are_written_and_say_what_the_spec_says() {
    assert!(QUESTION.starts_with("Keep a history of your prompts and what came of them on this machine?"));
    assert!(QUESTION.contains("Yes by default") && QUESTION.contains("turn it off or erase it at any time"));
    assert!(NOTICE.contains("delphi history off") && NOTICE.contains("delphi history purge") && NOTICE.contains("delphi history status"));
    // S8: the facts printed above the question say what is kept, where, how to stop it, and what redaction cannot promise.
    for must in ["Kept:", "Where:", "Control:", "Limits:", "on this machine only", "fingerprint and a length", "never as text", "delphi history off", "delphi history purge", "best effort"] {
        assert!(FACTS.contains(must), "the facts must say `{must}`");
    }
    // The notice carries the same residual risks the S7 audit found, in plain words.
    for must in ["best effort", "short pasted text is kept as written", "mission logs keep prompts after a purge", "never sent anywhere"] {
        assert!(NOTICE.contains(must), "the notice must say `{must}`");
    }
}

#[test]
fn with_the_switch_on_the_question_is_asked_once_and_the_notice_follows_the_answer() {
    let ws = tempfile::tempdir().unwrap();
    assert_eq!(question_to_show(ws.path(), true), Some(QUESTION));
    assert_eq!(first_run_notice(ws.path(), true, true), None, "unanswered: the question comes first, not the notice");
    record_answer(ws.path(), &human(), Answer::Yes).unwrap();
    assert_eq!(answered(ws.path()), Some(Answer::Yes));
    assert_eq!(question_to_show(ws.path(), true), None, "asked once");
    assert_eq!(first_run_notice(ws.path(), true, true), Some(NOTICE));
    assert_eq!(first_run_notice(ws.path(), true, false), None, "no notice that the history is on when it is not");
}

#[test]
fn the_console_notice_is_said_once_after_a_yes_and_never_after_a_no_or_while_unanswered() {
    let ws = tempfile::tempdir().unwrap();
    assert_eq!(take_notice(ws.path(), true), None, "unanswered: nothing to say, and nothing written");
    assert!(!ws.path().join("local").exists());
    record_answer(ws.path(), &human(), Answer::Yes).unwrap();
    assert_eq!(take_notice(ws.path(), false), None, "history off: no notice that it is on");
    assert!(!noticed(ws.path()), "a call that said nothing leaves no marker");
    assert_eq!(take_notice(ws.path(), true), Some(NOTICE));
    assert!(noticed(ws.path()));
    assert_eq!(take_notice(ws.path(), true), None, "said once");
    // A no says nothing, whatever the marker.
    let no = tempfile::tempdir().unwrap();
    record_answer(no.path(), &human(), Answer::No).unwrap();
    assert_eq!(take_notice(no.path(), true), None);
}

#[test]
fn until_a_human_answers_no_record_is_written_and_a_no_stops_them_for_good() {
    let ws = tempfile::tempdir().unwrap();
    // The switch on and nobody has answered: an upgraded install writes nothing (§7 item 4, §11 item 6).
    assert_eq!(append_in(ws.path(), &on(), draft(), true), Appended::Dormant);
    assert!(!ws.path().join("local").exists(), "no file and no key were made");
    record_answer(ws.path(), &human(), Answer::Yes).unwrap();
    assert!(matches!(append_in(ws.path(), &on(), draft(), true), Appended::Written(_)));
    // A no is the same marker `delphi history off` writes: nothing new is recorded, nothing is deleted.
    record_answer(ws.path(), &human(), Answer::No).unwrap();
    assert!(crate::history_record::global_off(ws.path()));
    assert_eq!(append_in(ws.path(), &on(), draft(), true), Appended::SwitchedOff);
    assert_eq!(first_run_notice(ws.path(), true, true), None, "no notice after a no");
    assert_eq!(crate::history_record::read(ws.path(), "home").unwrap().counted().count(), 1);
    // A later yes turns the switch back.
    record_answer(ws.path(), &human(), Answer::Yes).unwrap();
    assert!(!crate::history_record::global_off(ws.path()));
}

#[test]
fn only_a_human_at_a_terminal_can_answer() {
    assert!(Human::at_terminal(true, false, false).is_ok());
    assert!(Human::at_terminal(true, true, false).unwrap_err().contains("agent seat"), "an agent seat is refused");
    assert!(Human::at_terminal(true, false, true).unwrap_err().contains("host socket"), "a call over the host socket is refused");
    assert!(Human::at_terminal(false, false, false).unwrap_err().contains("terminal"), "no terminal is refused");
    // Refusal comes first even when everything else is wrong too.
    assert!(Human::at_terminal(false, true, true).is_err());
}

#[test]
fn an_unreadable_answer_reads_as_not_answered() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(ws.path().join("local")).unwrap();
    std::fs::write(ws.path().join("local/history.consent"), "maybe\n").unwrap();
    assert_eq!(answered(ws.path()), None);
    assert_eq!(question_to_show(ws.path(), true), Some(QUESTION));
}

#[test]
fn a_tap_holds_and_writes_nothing_until_the_question_is_answered_and_a_prompt_typed_before_the_answer_is_never_written_after_it() {
    use crate::history_record::Tap;
    let ws = tempfile::tempdir().unwrap();
    let tap = Tap::new(ws.path().to_path_buf(), Settings::default(), "home");
    assert!(!tap.recording(), "on by default, not recording: nobody has answered");
    tap.note_operator_prompt("t_0001", "the words typed before any answer CANARY-BEFORE-CONSENT");
    assert_eq!(tap.on_merge_row("m1", &serde_json::json!({"event": "merged", "pr": 1})), None);
    assert!(!ws.path().join("local").exists(), "nothing was written or keyed");
    // The operator answers yes. The earlier prompt was not held, so no later trigger can write it.
    record_answer(ws.path(), &human(), Answer::Yes).unwrap();
    assert!(tap.recording());
    let _ = tap.on_merge_row("m1", &serde_json::json!({"event": "merged", "pr": 2}));
    let stored = std::fs::read_dir(ws.path().join("local")).map(|d| d.flatten().count()).unwrap_or(0);
    let all = walk(ws.path());
    assert!(!all.contains("CANARY-BEFORE-CONSENT"), "a prompt typed before the answer was held and written after it (stored files: {stored})");
}

fn walk(dir: &std::path::Path) -> String {
    let mut s = String::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            s.push_str(&walk(&p));
        } else if let Ok(t) = std::fs::read_to_string(&p) {
            s.push_str(&t);
        }
    }
    s
}

#[test]
fn a_host_report_is_told_the_history_is_waiting_for_the_operators_answer_and_nothing_is_held() {
    use crate::history_record::{HostReport, Tap};
    let ws = tempfile::tempdir().unwrap();
    let tap = Tap::new(ws.path().to_path_buf(), Settings::default(), "home");
    let r = tap.on_host_exchange("m1", &serde_json::json!({"prompt": "hello CANARY-HOST", "did": "x"}), 0);
    assert_eq!(r, HostReport::AwaitingAnswer);
    assert_eq!(r.word(), "waiting for the operator's answer");
    assert!(!ws.path().join("local").exists());
    let off = Tap::new(ws.path().to_path_buf(), Settings { enabled: false, ..Settings::default() }, "home");
    assert_eq!(off.on_host_exchange("m1", &serde_json::json!({"prompt": "hello", "did": "x"}), 0), HostReport::Off, "off still says off");
}
