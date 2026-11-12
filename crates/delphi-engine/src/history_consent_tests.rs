use super::*;
use crate::history_record::{append_in, Appended, Draft, Observed, Outcome, OutcomeValue, PromptIn, Provenance, Settings, Trigger};
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
fn nothing_is_shown_today_because_the_s8_switch_is_off() {
    // The constant S8 flips. Until then neither text is ever returned to a caller that passes the real switch.
    assert!(!SHOW_AT_INSTALL, "the opt-out default is held until S7 passes and the egress check is live (OD-8)");
    let ws = tempfile::tempdir().unwrap();
    assert_eq!(question_to_show(ws.path(), SHOW_AT_INSTALL), None);
    assert_eq!(first_run_notice(ws.path(), SHOW_AT_INSTALL, true), None);
    assert!(!blocks_writes(ws.path(), SHOW_AT_INSTALL), "the question gates nothing while it is not shown");
}

#[test]
fn the_texts_are_written_and_say_what_the_spec_says() {
    assert!(QUESTION.starts_with("Keep a history of your prompts and what came of them on this machine?"));
    assert!(QUESTION.contains("Yes by default") && QUESTION.contains("turn it off or erase it at any time"));
    assert!(NOTICE.contains("delphi history off") && NOTICE.contains("delphi history purge") && NOTICE.contains("delphi history status"));
}

#[test]
fn with_the_switch_on_the_question_is_asked_once_and_the_notice_follows_the_answer() {
    let ws = tempfile::tempdir().unwrap();
    assert_eq!(question_to_show(ws.path(), true), Some(QUESTION));
    assert_eq!(first_run_notice(ws.path(), true, true), Some(NOTICE));
    assert_eq!(first_run_notice(ws.path(), true, false), None, "no notice that the history is on when it is not");
    record_answer(ws.path(), &human(), Answer::Yes).unwrap();
    assert_eq!(answered(ws.path()), Some(Answer::Yes));
    assert_eq!(question_to_show(ws.path(), true), None, "asked once");
    assert_eq!(first_run_notice(ws.path(), true, true), Some(NOTICE));
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
