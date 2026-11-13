use super::*;
use serde_json::json;

const MISSION: &str = "m1";
const SID: &str = "6f2c1a90-aaaa-4bbb-8ccc-1234567890ab";

/// The settings of an install that turned the history off in config.toml. The default is on since S8 (nothing is recorded until the
/// install question is answered), so "off" has to be said.
fn off() -> Settings {
    Settings { enabled: false, ..Settings::default() }
}

/// In a workspace where the question was answered, the answer is the only thing under `local/`: no store, no key, nothing else.
fn only_the_answer_is_there(ws: &Path) -> bool {
    let mut v: Vec<String> = std::fs::read_dir(ws.join("local")).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    v.sort();
    v == ["history.consent"]
}

fn on() -> Settings {
    Settings { enabled: true, ..Settings::default() }
}

fn ev(kind: &str, payload: Value) -> EventEnvelope {
    EventEnvelope::new(MISSION, kind, payload)
}

fn research() -> EventEnvelope {
    ev("research.finished", json!({"topic": "queues", "new": 3, "updated": 1, "held": 0, "outcome": "done"}))
}

fn tap(ws: &Path, settings: Settings) -> Tap {
    Tap::new(ws.to_path_buf(), settings, "home")
}

fn only(a: Vec<Appended>) -> Record {
    assert_eq!(a.len(), 1, "{a:?}");
    match a.into_iter().next().unwrap() {
        Appended::Written(r) => *r,
        other => panic!("expected a record, got {other:?}"),
    }
}

fn store_text(ws: &Path) -> String {
    std::fs::read_to_string(store_path(ws, "home")).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------------------------------
// The default is dormant (§7 item 10, acceptance 9)

#[test]
fn the_default_is_on_but_records_nothing_until_answered_and_the_egress_check_is_registered_for_the_store() {
    let s = Settings::default();
    assert!(s.enabled, "since S8 the default is on (opt-out); the install question is what holds the first record back");
    assert_eq!(s.to_models, ToModels::Deny);
    assert_eq!((s.paste_chars, s.budget_mb), (400, 50));
    // Acceptance 9: a test fails if the default is on while the history store is not a registered, wired egress path.
    let registered = crate::egress::PATHS.iter().any(|(c, f, w)| *c == Channel::History && *f == "history_record.rs" && *w == crate::egress::Wired::Yes);
    assert!(registered || !s.enabled, "the default is on but the history store is not registered with the egress check");
    assert!(registered);
    // And on, with nobody having answered: the tap holds and writes nothing.
    let ws = tempfile::tempdir().unwrap();
    let t = tap(ws.path(), s);
    assert!(!t.recording());
    t.note_operator_prompt("t_1", "never held");
    assert!(t.on_events(MISSION, &[research()]).is_empty());
    assert!(!ws.path().join("local").exists(), "no file, no key and nothing held");
}

#[test]
fn a_dormant_tap_writes_nothing_and_makes_no_key() {
    // `enabled = false` in config.toml: nothing is held, written or keyed, whatever the install question says.
    let ws = tempfile::tempdir().unwrap();
    let t = tap(ws.path(), off());
    t.note_operator_prompt("t_1", "please do the thing");
    assert!(t.on_events(MISSION, &[research()]).is_empty());
    assert!(t.on_artifacts(MISSION, "t_1", vec![aref("a.md")]).is_none());
    assert!(t.on_merge_row(MISSION, &json!({"kind": "merged", "pr": 7})).is_none());
    assert!(!ws.path().join("local").exists(), "nothing is made under the home while history is off");
    // And the lower function answers dormant before it touches the disk.
    let d = Draft { trigger: Trigger::ResearchRun, mission: MISSION.into(), project: "home".into(), at: "2026-11-12T00:00:00Z".into(), prompt: PromptIn::Artifacts(vec![]), reply: None, outcome: unknown(), observed: Observed::default() };
    assert_eq!(append(ws.path(), &off(), d), Appended::Dormant);
    assert!(!ws.path().join("local").exists());
}

fn unknown() -> Outcome {
    Outcome { value: OutcomeValue::Unknown, provenance: Provenance::Observed }
}

fn aref(name: &str) -> ArtifactRef {
    ArtifactRef { log: "local/artifact-log/m1/artifact-prompts.jsonl".into(), entry: format!("hash-of-{name}"), artifact: name.into(), artifact_hash: Some(format!("blake-{name}")) }
}

#[test]
fn settings_are_read_clamped_and_a_wrong_value_never_turns_history_on() {
    let (s, p) = Settings::parse(&toml::from_str("").unwrap());
    assert_eq!((s, p), (Settings::default(), vec![]));
    let (s, p) = Settings::parse(&toml::from_str("[history]\nenabled = true\npaste_chars = 100\nbudget_mb = 5\nto_models = \"allow\"\n").unwrap());
    assert_eq!((s.enabled, s.paste_chars, s.budget_mb, s.to_models), (true, 100, 5, ToModels::Allow));
    assert!(p.is_empty(), "{p:?}");
    let (s, p) = Settings::parse(&toml::from_str("[history]\npaste_chars = 5\nbudget_mb = 99999\n").unwrap());
    assert_eq!((s.paste_chars, s.budget_mb), (50, 1024));
    assert_eq!(p.len(), 2, "{p:?}");
    let (s, p) = Settings::parse(&toml::from_str("[history]\nenabled = \"yes\"\nto_models = \"maybe\"\nverbose = true\n").unwrap());
    assert!(!s.enabled && s.to_models == ToModels::Deny, "a wrong type or value falls back to the safe default");
    assert_eq!(p.len(), 3, "{p:?}");
}

// ---------------------------------------------------------------------------------------------------------------------------
// The five triggers

#[test]
fn trigger_functions_read_the_event_kinds_that_exist() {
    assert_eq!(trigger_of(&research()), Some(Trigger::ResearchRun));
    assert_eq!(trigger_of(&ev("council.verdict", json!({"venue": "panel", "n": 2}))), Some(Trigger::Panel));
    assert_eq!(trigger_of(&ev("council.verdict", json!({"venue": "daemon", "n": 2}))), Some(Trigger::Council));
    assert_eq!(trigger_of_ledger_row(&json!({"kind": "merged", "pr": 7})), Some(Trigger::Merge));
    for k in ["research.refused", "council.started", "task.created", "agent.turn.complete", "merge.approved", "decision.resolved"] {
        assert_eq!(trigger_of(&ev(k, json!({"venue": "panel"}))), None, "{k} is not a trigger");
    }
    assert_eq!(trigger_of_ledger_row(&json!({"kind": "approved"})), None);
}

#[test]
fn each_trigger_writes_exactly_one_record_with_the_fixed_fields() {
    let cases: Vec<(&str, Trigger)> = vec![("research", Trigger::ResearchRun), ("panel", Trigger::Panel), ("council", Trigger::Council), ("merge", Trigger::Merge), ("artifact", Trigger::Artifact)];
    for (name, want) in cases {
        let ws = crate::history_consent::answered_ws();
        let t = tap(ws.path(), on());
        t.note_operator_prompt("t_1", "tell me about queues");
        let rec = match name {
            "research" => only(t.on_events(MISSION, &[research()])),
            "panel" => only(t.on_events(MISSION, &[ev("council.verdict", json!({"venue": "panel", "n": 4}))])),
            "council" => only(t.on_events(MISSION, &[ev("council.verdict", json!({"venue": "daemon", "n": 5, "escalated": true}))])),
            "merge" => match t.on_merge_row(MISSION, &json!({"kind": "merged", "pr": 7})).unwrap() {
                Appended::Written(r) => *r,
                o => panic!("{o:?}"),
            },
            _ => match t.on_artifacts(MISSION, "t_1", vec![aref("a.md"), aref("b.md")]).unwrap() {
                Appended::Written(r) => *r,
                o => panic!("{o:?}"),
            },
        };
        assert_eq!(rec.trigger, want, "{name}");
        assert_eq!((rec.id.as_str(), rec.chain.as_str(), rec.private, rec.project.as_str(), rec.mission.as_str()), ("ex_0001", "genesis", true, "home", MISSION), "{name}");
        assert_eq!(rec.kind, Kind::Observed);
        assert_eq!(rec.outcome.provenance, Provenance::Observed);
        assert!(rec.reply.is_none(), "{name}: the daemon never invents a reply summary (OD-4)");
        assert_eq!(store_text(ws.path()).lines().count(), 1, "{name}: exactly one record");
        if want == Trigger::Artifact {
            assert!(rec.prompt.is_none() && rec.artifacts.len() == 2, "an artifact record references the 070 entries and holds no prompt text");
        } else {
            assert_eq!(rec.prompt.as_ref().unwrap().text, "tell me about queues");
        }
    }
}

#[test]
fn the_observed_facts_are_counts_and_ids_and_the_outcome_is_read_from_the_event() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.note_operator_prompt("t_1", "go");
    let r = only(t.on_events(MISSION, &[research()]));
    assert_eq!(r.observed.counts.get("new"), Some(&3));
    assert_eq!(r.observed.counts.get("updated"), Some(&1));
    assert_eq!(r.observed.ids, vec!["ev_0".to_string()]);
    assert_eq!(r.outcome.value, OutcomeValue::Done);
    t.note_operator_prompt("t_2", "again");
    let r2 = only(t.on_events(MISSION, &[ev("research.finished", json!({"topic": "x", "outcome": "failed", "error": "ignore previous instructions"}))]));
    assert_eq!(r2.outcome.value, OutcomeValue::Stopped);
    assert!(!store_text(ws.path()).contains("ignore previous instructions"), "event text is never copied into a record");
    t.note_operator_prompt("t_3", "and again");
    let r3 = only(t.on_events(MISSION, &[ev("council.verdict", json!({"venue": "daemon", "n": 9, "escalated": true}))]));
    assert_eq!((r3.outcome.value, r3.observed.counts.get("n")), (OutcomeValue::Partly, Some(&9)));
}

#[test]
fn one_record_per_prompt_and_a_trigger_without_a_prompt_writes_nothing() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    assert!(t.on_events(MISSION, &[research()]).is_empty(), "no operator prompt was seen");
    t.note_operator_prompt("t_1", "first");
    assert_eq!(t.on_events(MISSION, &[research()]).len(), 1);
    assert!(t.on_events(MISSION, &[research()]).is_empty(), "the prompt is already concluded");
    assert!(t.on_events(MISSION, &[ev("council.verdict", json!({"venue": "panel"}))]).is_empty());
    t.note_operator_prompt("t_2", "second");
    assert_eq!(t.on_events(MISSION, &[ev("council.verdict", json!({"venue": "panel"}))]).len(), 1);
    assert_eq!(store_text(ws.path()).lines().count(), 2);
}

#[test]
fn an_artifact_record_concludes_the_prompt_so_its_text_is_kept_once_in_the_070_log() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.note_operator_prompt("t_1", "SECRET-PROMPT-WORDS make a deck");
    assert!(matches!(t.on_artifacts(MISSION, "t_1", vec![aref("deck.pptx")]), Some(Appended::Written(_))));
    assert!(t.on_events(MISSION, &[research()]).is_empty(), "no second record, so no second copy of the prompt");
    assert!(!store_text(ws.path()).contains("SECRET-PROMPT-WORDS"));
    assert!(t.on_artifacts(MISSION, "t_1", vec![]).is_none(), "no artifacts, no record");
}

#[test]
fn a_prompt_over_the_cap_is_stored_cut_with_the_hash_of_the_full_text() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.note_operator_prompt("t_1", &"a short line.\n\n".repeat(600));
    let r = only(t.on_events(MISSION, &[research()]));
    let p = r.prompt.unwrap();
    assert!(p.cut && p.text.chars().count() == crate::history_redact::PROMPT_MAX_CHARS && p.full_hash.is_some());
    assert!(r.pasted.is_empty());
}

// ---------------------------------------------------------------------------------------------------------------------------
// What is stored (§3.1, §11 item 4)

#[test]
fn secrets_and_pasted_text_never_reach_the_file() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    let pasted = "x9Qz".repeat(150); // 600 characters, one block
    let prompt = format!("Use sk-ant-FAKEFAKEFAKEFAKEFAKE00 for it, password is hunter2hunter2.\n\n{pasted}\n\nthen summarise");
    t.note_operator_prompt("t_1", &prompt);
    let r = only(t.on_events(MISSION, &[research()]));
    let file = store_text(ws.path());
    for s in ["sk-ant-FAKEFAKE", "hunter2hunter2", &pasted[..40]] {
        assert!(!file.contains(s), "{s} reached the store");
    }
    assert_eq!(r.pasted.len(), 1);
    assert_eq!(r.pasted[0].chars, 600);
    assert!(r.prompt.unwrap().text.contains("then summarise"));
}

#[test]
fn a_record_has_no_field_for_tool_results_thinking_dialogue_file_contents_or_pasted_text() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.note_operator_prompt("t_1", "ok");
    only(t.on_events(MISSION, &[research()]));
    let v: Value = serde_json::from_str(store_text(ws.path()).lines().next().unwrap()).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    keys.sort();
    assert_eq!(keys, ["at", "chain", "hash", "id", "kind", "mission", "observed", "outcome", "pasted", "private", "project", "prompt", "trigger"]);
    let record_fields: Vec<&str> = include_str!("history_record.rs").split("pub struct Record {").nth(1).unwrap().split("\n}\n").next().unwrap().lines().filter_map(|l| l.trim().strip_prefix("pub ")).filter_map(|l| l.split(':').next()).collect();
    for banned in ["tool_result", "tool_results", "thinking", "assistant", "dialogue", "content", "contents", "file", "files", "text", "pasted_text", "transcript"] {
        assert!(!record_fields.contains(&banned), "a record must have no `{banned}` field: {record_fields:?}");
    }
}

#[test]
fn relayed_text_is_never_observed_and_a_host_cannot_make_it_so() {
    for transcript in [None, Some(String::new()), Some("not a transcript".to_string())] {
        let d = Draft { trigger: Trigger::ResearchRun, mission: MISSION.into(), project: "home".into(), at: "t".into(), prompt: PromptIn::Reported { text: "text a tool returned".into(), marked: vec![], session_id: SID.into(), transcript }, reply: None, outcome: unknown(), observed: Observed::default() };
        let r = build(&[7u8; 32], &on(), d);
        assert_eq!(r.kind, Kind::Reported);
        assert!(r.pointer.is_none());
    }
}

fn user(text: &str) -> String {
    json!({"type": "user", "version": "2.0.1", "sessionId": SID, "message": {"role": "user", "content": text}}).to_string()
}

fn tool_result(text: &str) -> String {
    json!({"type": "user", "version": "2.0.1", "sessionId": SID, "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": text}]}}).to_string()
}

#[test]
fn a_reported_prompt_that_matches_the_transcript_reads_resolved_and_one_that_does_not_reads_reported() {
    let transcript = [user("please merge 12"), tool_result("text only a tool returned")].join("\n") + "\n";
    let mk = |text: &str| {
        let d = Draft { trigger: Trigger::Merge, mission: MISSION.into(), project: "home".into(), at: "t".into(), prompt: PromptIn::Reported { text: text.into(), marked: vec![], session_id: SID.into(), transcript: Some(transcript.clone()) }, reply: None, outcome: unknown(), observed: Observed::default() };
        build(&[7u8; 32], &on(), d)
    };
    let hit = mk("please merge 12");
    assert_eq!(hit.kind, Kind::Resolved);
    assert!(hit.pointer.is_some());
    assert_eq!(mk("please merge 13").kind, Kind::Reported);
    assert_eq!(mk("text only a tool returned").kind, Kind::Reported, "a span that is only in a tool result never reads resolved");
}

// ---------------------------------------------------------------------------------------------------------------------------
// What a host may say

#[test]
fn a_host_cannot_set_what_the_daemon_computes() {
    for k in ["kind", "observed", "private", "chain", "hash", "pointer", "id", "at", "project", "mission", "trigger"] {
        let err = parse_host_exchange(&json!({"asked": "x", k: "observed"})).unwrap_err();
        assert!(err.contains(k) && err.contains("daemon"), "{k}: {err}");
    }
    assert!(parse_host_exchange(&json!("text")).is_err());
}

#[test]
fn a_host_exchange_has_one_outcome_capped_redacted_fields_and_a_checked_session_id() {
    let ok = parse_host_exchange(&json!({"asked": "x".repeat(500), "did": "used Bearer abcDEF123456789xyz", "open": "", "outcome": "partly", "prompt": "the words", "session_id": SID})).unwrap();
    assert_eq!(ok.reply.asked.chars().count(), 200);
    assert!(!ok.reply.did.contains("abcDEF123456789xyz"));
    assert_eq!((ok.outcome, ok.prompt.as_deref(), ok.session_id.as_deref()), (Some(OutcomeValue::Partly), Some("the words"), Some(SID)));
    assert!(parse_host_exchange(&json!({"outcome": ["done", "stopped"]})).is_err(), "a second outcome is refused");
    assert!(parse_host_exchange(&json!({"outcome": "great"})).is_err());
    assert!(parse_host_exchange(&json!({"asked": 4})).is_err());
    assert_eq!(parse_host_exchange(&json!({"session_id": "../etc"})).unwrap().session_id, None);
}

#[test]
fn nothing_outside_the_tap_writes_a_record_and_parsing_a_host_exchange_makes_none() {
    let ws = crate::history_consent::answered_ws();
    parse_host_exchange(&json!({"asked": "x", "prompt": "y", "outcome": "done"})).unwrap();
    assert!(only_the_answer_is_there(ws.path()));
    // `append` is crate-private: the only callers are the tap's trigger paths.
    let src = include_str!("history_record.rs");
    assert!(src.contains("pub(crate) fn append("));
    let prod = src.split("\n#[cfg(test)]").next().unwrap();
    assert_eq!(prod.matches("append(&self.ws_dir").count(), 3, "only the tap's conclude, on_artifacts and on_host_exchange append");
}

// ---------------------------------------------------------------------------------------------------------------------------
// The chain, verified on every read (§11 item 14)

fn three(ws: &Path) {
    let t = tap(ws, on());
    for (i, p) in ["first prompt", "second prompt", "third prompt"].iter().enumerate() {
        t.note_operator_prompt(&format!("t_{i}"), p);
        only(t.on_events(MISSION, &[research()]));
    }
}

fn rewrite(ws: &Path, f: impl Fn(Vec<String>) -> Vec<String>) {
    let lines: Vec<String> = store_text(ws).lines().map(String::from).collect();
    std::fs::write(store_path(ws, "home"), f(lines).join("\n") + "\n").unwrap();
}

#[test]
fn an_untouched_chain_verifies_and_links_each_record_to_the_one_before() {
    let ws = crate::history_consent::answered_ws();
    three(ws.path());
    let r = read(ws.path(), "home").unwrap();
    assert_eq!(r.records.len(), 3);
    assert!(r.broken_at.is_none() && r.records.iter().all(|c| c.status == Status::Verified));
    let recs: Vec<&Record> = r.counted().collect();
    assert_eq!((recs[0].chain.as_str(), recs[1].chain.as_str(), recs[2].chain.as_str()), ("genesis", recs[0].hash.as_str(), recs[1].hash.as_str()));
    assert_eq!(recs.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["ex_0001", "ex_0002", "ex_0003"]);
    assert_eq!(read(ws.path(), "other").unwrap(), Read::default());
}

#[test]
fn an_edited_record_breaks_the_chain_there_and_after_and_is_never_shown_as_valid() {
    let ws = crate::history_consent::answered_ws();
    three(ws.path());
    rewrite(ws.path(), |l| l.into_iter().map(|x| x.replace("second prompt", "second EDITED")).collect());
    let r = read(ws.path(), "home").unwrap();
    assert_eq!(r.broken_at, Some(1));
    assert_eq!(r.records.iter().map(|c| c.status).collect::<Vec<_>>(), [Status::Verified, Status::ChainBroken, Status::ChainBroken]);
    assert_eq!(r.counted().count(), 1, "records from the break on are left out of every count");
    assert!(store_text(ws.path()).contains("second EDITED"), "nothing is repaired or deleted");
}

#[test]
fn a_tampered_last_record_is_caught_by_its_own_hash() {
    let ws = crate::history_consent::answered_ws();
    three(ws.path());
    rewrite(ws.path(), |l| l.into_iter().map(|x| x.replace("third prompt", "third EDITED")).collect());
    let r = read(ws.path(), "home").unwrap();
    assert_eq!((r.broken_at, r.counted().count()), (Some(2), 2));
}

#[test]
fn a_removed_or_reordered_record_and_an_unreadable_line_break_the_chain() {
    let ws = crate::history_consent::answered_ws();
    three(ws.path());
    rewrite(ws.path(), |mut l| {
        l.remove(1);
        l
    });
    assert_eq!(read(ws.path(), "home").unwrap().broken_at, Some(1));
    let ws2 = crate::history_consent::answered_ws();
    three(ws2.path());
    rewrite(ws2.path(), |mut l| {
        l.swap(0, 1);
        l
    });
    assert_eq!(read(ws2.path(), "home").unwrap().broken_at, Some(0));
    let ws3 = crate::history_consent::answered_ws();
    three(ws3.path());
    rewrite(ws3.path(), |mut l| {
        l[1] = "{not json".into();
        l
    });
    let r = read(ws3.path(), "home").unwrap();
    assert_eq!((r.broken_at, r.counted().count()), (Some(1), 1));
    assert_eq!(r.records[1].status, Status::ChainBroken, "what follows an unreadable line is flagged");
}

#[test]
fn a_record_rewritten_as_not_private_does_not_verify() {
    let ws = crate::history_consent::answered_ws();
    three(ws.path());
    rewrite(ws.path(), |l| l.into_iter().map(|x| x.replace("\"private\":true", "\"private\":false")).collect());
    assert_eq!(read(ws.path(), "home").unwrap().counted().count(), 0);
}

#[test]
fn without_the_key_nothing_is_verifiable_and_nothing_is_counted() {
    let ws = crate::history_consent::answered_ws();
    three(ws.path());
    std::fs::remove_file(crate::history_pointer::key_path(ws.path())).unwrap();
    let r = read(ws.path(), "home").unwrap();
    assert_eq!(r.records.len(), 3);
    assert!(r.records.iter().all(|c| c.status == Status::Unverifiable));
    assert_eq!(r.counted().count(), 0);
}

#[test]
fn appending_after_a_broken_store_never_repairs_it() {
    let ws = crate::history_consent::answered_ws();
    three(ws.path());
    rewrite(ws.path(), |l| l.into_iter().map(|x| x.replace("first prompt", "first EDITED")).collect());
    let t = tap(ws.path(), on());
    t.note_operator_prompt("t_9", "later");
    only(t.on_events(MISSION, &[research()]));
    let r = read(ws.path(), "home").unwrap();
    assert_eq!((r.records.len(), r.broken_at, r.counted().count()), (4, Some(0), 0));
}

// ---------------------------------------------------------------------------------------------------------------------------
// Switches, budget and files

#[cfg(unix)]
#[test]
fn the_store_and_the_key_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let ws = crate::history_consent::answered_ws();
    three(ws.path());
    let mode = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(store_path(ws.path(), "home")), 0o600);
    assert_eq!(mode(store_dir(ws.path(), "home")), 0o700);
    assert_eq!(mode(crate::history_pointer::key_path(ws.path())), 0o600);
}

#[test]
fn a_project_marked_off_records_nothing() {
    let ws = crate::history_consent::answered_ws();
    std::fs::create_dir_all(ws.path().join("local/projects/home")).unwrap();
    std::fs::write(ws.path().join("local/projects/home/history.off"), "").unwrap();
    let t = tap(ws.path(), on());
    t.note_operator_prompt("t_1", "hello");
    assert_eq!(t.on_events(MISSION, &[research()]), vec![Appended::ProjectOff]);
    assert!(!ws.path().join("local/history").exists());
}

#[test]
fn at_the_budget_the_daemon_stops_adding_and_evicts_nothing() {
    let ws = crate::history_consent::answered_ws();
    let s = Settings { budget_mb: 1, ..on() };
    std::fs::create_dir_all(store_dir(ws.path(), "home")).unwrap();
    let filler = vec![b'x'; 1_048_576];
    std::fs::write(store_path(ws.path(), "home"), &filler).unwrap();
    let t = Tap::new(ws.path().to_path_buf(), s, "home");
    t.note_operator_prompt("t_1", "hello");
    assert_eq!(t.on_events(MISSION, &[research()]), vec![Appended::Budget]);
    assert_eq!(std::fs::read(store_path(ws.path(), "home")).unwrap(), filler, "the store is untouched");
}

#[test]
fn the_project_comes_from_the_missions_scope_file_and_only_safe_names_are_used() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(Tap::project_of(d.path()), "home");
    std::fs::write(d.path().join("scope"), "my-project\n").unwrap();
    assert_eq!(Tap::project_of(d.path()), "my-project");
    std::fs::write(d.path().join("scope"), "../../escape").unwrap();
    assert_eq!(Tap::project_of(d.path()), "home");
    assert_eq!(Tap::new(PathBuf::from("/x"), on(), "a/b").project, "home");
}

// ---------------------------------------------------------------------------------------------------------------------------
// Egress (§7 item 2, §11 item 3)

fn written() -> Record {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.note_operator_prompt("t_1", "private words of the operator");
    only(t.on_events(MISSION, &[research()]))
}

#[test]
fn no_record_leaves_the_install_by_any_channel() {
    assert!(release(&written()).is_err());
    assert!(crate::egress::check(&meta_of(&written()), Channel::History).is_err());
}

#[test]
fn with_the_default_no_record_text_enters_any_model_prompt() {
    let r = written();
    for seat in [Seat::SameFamily, Seat::Foreign, Seat::Unknown] {
        assert!(for_prompt(&Settings::default(), seat, &r).is_err(), "{seat:?}");
    }
}

#[test]
fn even_with_allow_only_the_hosts_own_family_gets_a_record_and_an_unknown_family_never() {
    let r = written();
    let allow = Settings { to_models: ToModels::Allow, ..on() };
    assert_eq!(for_prompt(&allow, Seat::SameFamily, &r).unwrap(), "private words of the operator");
    assert!(for_prompt(&allow, Seat::Foreign, &r).unwrap_err().contains("foreign-family"));
    assert!(for_prompt(&allow, Seat::Unknown, &r).is_err(), "an unknown family means deny");
}

#[test]
fn a_record_that_names_nothing_but_the_private_flag_is_private_by_construction() {
    assert!(written().private);
    let mut r = written();
    r.private = false;
    assert!(release(&r).is_ok(), "the check reads the flag; a record is written private and the chain rejects any other value");
}

#[test]
fn control_characters_in_a_prompt_are_not_stored() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.note_operator_prompt("t_1", "hello \u{1b}[2J\u{7} there");
    let r = only(t.on_events(MISSION, &[research()]));
    assert!(!r.prompt.unwrap().text.chars().any(|c| c.is_control()));
    assert!(!store_text(ws.path()).contains('\u{1b}'));
}

#[test]
fn the_same_inputs_give_the_same_record_apart_from_the_time() {
    let mk = || {
        let ws = crate::history_consent::answered_ws();
        std::fs::create_dir_all(ws.path().join("local")).unwrap();
        std::fs::write(crate::history_pointer::key_path(ws.path()), "07".repeat(32)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(crate::history_pointer::key_path(ws.path()), std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let d = Draft { trigger: Trigger::Council, mission: MISSION.into(), project: "home".into(), at: "2026-11-12T00:00:00Z".into(), prompt: PromptIn::Observed { text: "same words".into(), marked: vec![], typing: Typing::NotSeen }, reply: None, outcome: unknown(), observed: Observed::default() };
        match append(ws.path(), &on(), d) {
            Appended::Written(r) => *r,
            o => panic!("{o:?}"),
        }
    };
    assert_eq!(mk(), mk());
}

// ---------------------------------------------------------------------------------------------------------------------------
// The host's exchange fields (§3.3, §10 item 1): a host completes a trigger the daemon saw; it never makes a record

fn exchange() -> Value {
    json!({"prompt": "please research queue options", "asked": "queue options", "did": "ran the scout", "open": "pick one"})
}

fn records(ws: &Path) -> Vec<Record> {
    read(ws, "home").map(|r| r.counted().cloned().collect()).unwrap_or_default()
}

#[test]
fn a_host_summary_completes_the_trigger_the_daemon_saw_and_the_reply_is_labelled_self_reported() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    // The daemon saw a research run finish, but held no prompt of its own (the operator typed into the host): nothing is written.
    assert!(t.on_events(MISSION, &[research()]).is_empty());
    assert!(!store_path(ws.path(), "home").exists(), "a trigger alone writes nothing when the daemon has no prompt");
    let r = t.on_host_exchange(MISSION, &exchange(), 0);
    assert_eq!(r.word(), "recorded");
    let rec = records(ws.path()).pop().expect("one record");
    assert_eq!((rec.trigger, rec.kind), (Trigger::ResearchRun, Kind::Reported), "no session id: the words are the host's report, nothing matched");
    assert_eq!(rec.prompt.as_ref().unwrap().text, "please research queue options");
    assert_eq!(rec.reply, Some(Reply { asked: "queue options".into(), did: "ran the scout".into(), open: "pick one".into() }));
    // The daemon's own observation stands: the outcome is the daemon's, the facts are the daemon's.
    assert_eq!(rec.outcome, Outcome { value: OutcomeValue::Done, provenance: Provenance::Observed });
    assert!(rec.observed.ids.iter().any(|i| i.starts_with("ev_")) && rec.observed.counts.get("new") == Some(&3));
    assert!(rec.private);
}

#[test]
fn a_host_report_alone_never_makes_a_record() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    assert_eq!(t.on_host_exchange(MISSION, &exchange(), 0), HostReport::NoTrigger);
    assert!(only_the_answer_is_there(ws.path()), "no record, no store, and no key");
}

#[test]
fn a_report_with_no_prompt_writes_nothing_and_keeps_the_trigger_for_one_that_has_it() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.on_events(MISSION, &[research()]);
    assert_eq!(t.on_host_exchange(MISSION, &json!({"asked": "x", "did": "y"}), 0), HostReport::NoPrompt);
    assert_eq!(t.on_host_exchange(MISSION, &json!({"prompt": "   ", "did": "y"}), 0), HostReport::NoPrompt);
    assert!(records(ws.path()).is_empty());
    assert_eq!(t.on_host_exchange(MISSION, &exchange(), 0).word(), "recorded", "the trigger was not used up");
}

#[test]
fn one_report_completes_the_latest_trigger_once_and_the_next_report_finds_none() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.on_events(MISSION, &[research()]);
    t.on_events(MISSION, &[ev("council.verdict", json!({"venue": "daemon", "escalated": false}))]);
    assert_eq!(t.on_host_exchange(MISSION, &exchange(), 0).word(), "recorded");
    assert_eq!(records(ws.path())[0].trigger, Trigger::Council, "the latest trigger is the one the host just saw finish");
    assert_eq!(t.on_host_exchange(MISSION, &exchange(), 0).word(), "recorded", "the earlier trigger is still waiting");
    assert_eq!(t.on_host_exchange(MISSION, &exchange(), 0), HostReport::NoTrigger);
    assert_eq!(records(ws.path()).len(), 2);
}

#[test]
fn a_trigger_the_daemon_attributed_to_its_own_prompt_is_not_open_to_a_host_report() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.note_operator_prompt("t_1", "the operator typed this into the daemon");
    let rec = only(t.on_events(MISSION, &[research()]));
    assert_eq!(rec.kind, Kind::Observed);
    assert_eq!(t.on_host_exchange(MISSION, &exchange(), 0), HostReport::NoTrigger, "one record per prompt; the daemon's own is final");
    assert_eq!(records(ws.path()).len(), 1);
}

#[test]
fn a_trigger_is_forgotten_after_the_window_and_the_list_is_bounded() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.on_events(MISSION, &[research()]);
    let far = delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap() + AWAITING_SECS + 1000;
    assert_eq!(t.on_host_exchange(MISSION, &exchange(), far), HostReport::NoTrigger, "a late summary is not bound to a trigger of another exchange");
    for _ in 0..AWAITING_MAX + 3 {
        t.on_events(MISSION, &[research()]);
    }
    let done = (0..AWAITING_MAX + 3).filter(|_| t.on_host_exchange(MISSION, &exchange(), 0).word() == "recorded").count();
    assert_eq!(done, AWAITING_MAX, "the oldest are dropped");
}

#[test]
fn a_host_cannot_set_what_the_daemon_computes_nor_the_outcome_through_a_summary() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    // A research run that reports nothing the daemon can read as a result: the daemon's outcome is `unknown`.
    t.on_events(MISSION, &[ev("research.finished", json!({"topic": "x"}))]);
    let mut p = exchange();
    for (k, v) in [("kind", "observed"), ("observed", "x"), ("private", "false"), ("outcome", "done"), ("trigger", "merge"), ("id", "ex_9999"), ("chain", "genesis")] {
        p[k] = json!(v);
    }
    assert_eq!(t.on_host_exchange(MISSION, &p, 0).word(), "recorded");
    let rec = records(ws.path()).pop().unwrap();
    assert_eq!(rec.kind, Kind::Reported, "never observed for relayed text");
    assert_eq!(rec.outcome, Outcome { value: OutcomeValue::Unknown, provenance: Provenance::Observed });
    assert_eq!((rec.trigger, rec.private, rec.id.as_str()), (Trigger::ResearchRun, true, "ex_0001"));
}

#[test]
fn the_reply_fields_are_capped_and_redacted_by_the_same_pipeline_and_a_secret_in_the_prompt_is_not_stored() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.on_events(MISSION, &[research()]);
    let p = json!({"prompt": "use Bearer abcDEF123456789xyzABC please", "asked": "queue options ".repeat(40), "did": "sent Bearer abcDEF123456789xyzABC", "open": "\u{1b}[2Jnothing"});
    assert_eq!(t.on_host_exchange(MISSION, &p, 0).word(), "recorded");
    let text = store_text(ws.path());
    assert!(!text.contains("abcDEF123456789xyzABC") && !text.contains('\u{1b}'), "{text}");
    let rec = records(ws.path()).pop().unwrap();
    assert_eq!(rec.reply.unwrap().asked.chars().count(), 200);
}

#[test]
fn a_reported_prompt_found_in_the_hosts_transcript_as_a_user_message_reads_resolved_and_a_tool_result_does_not() {
    let ws = crate::history_consent::answered_ws();
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("-proj");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(format!("{SID}.jsonl")), [user("please research queue options"), tool_result("text only a tool returned")].join("\n") + "\n").unwrap();
    let t = tap(ws.path(), on()).with_claude_root(root.path().to_path_buf());
    t.on_events(MISSION, &[research()]);
    let mut p = exchange();
    p["session_id"] = json!(SID);
    assert_eq!(t.on_host_exchange(MISSION, &p, 0).word(), "recorded");
    let hit = records(ws.path()).pop().unwrap();
    assert_eq!(hit.kind, Kind::Resolved);
    assert!(hit.pointer.is_some());
    t.on_events(MISSION, &[research()]);
    p["prompt"] = json!("text only a tool returned");
    assert_eq!(t.on_host_exchange(MISSION, &p, 0).word(), "recorded");
    assert_eq!(records(ws.path()).pop().unwrap().kind, Kind::Reported, "a span only in a tool result never reads resolved");
    // A session that is not found, or an id that could name a path, is just `reported`.
    t.on_events(MISSION, &[research()]);
    p["prompt"] = json!("please research queue options");
    p["session_id"] = json!("../../etc/passwd");
    assert_eq!(t.on_host_exchange(MISSION, &p, 0).word(), "recorded");
    assert_eq!(records(ws.path()).pop().unwrap().kind, Kind::Reported);
}

#[test]
fn a_dormant_tap_ignores_the_exchange_fields_and_a_call_without_them_is_unchanged() {
    let ws = tempfile::tempdir().unwrap();
    let t = tap(ws.path(), off());
    t.on_events(MISSION, &[research()]);
    assert_eq!(t.on_host_exchange(MISSION, &exchange(), 0), HostReport::Off);
    assert_eq!(t.on_host_exchange(MISSION, &json!({"summary": "s", "decisions": ["a"]}), 0), HostReport::Absent);
    assert!(!ws.path().join("local").exists(), "nothing is held, written or made while history is off");
    // On, and the question answered: a call without the fields is unchanged too.
    let answered = crate::history_consent::answered_ws();
    let on_tap = tap(answered.path(), on());
    assert_eq!(on_tap.on_host_exchange(MISSION, &json!({"summary": "s"}), 0), HostReport::Absent);
}

#[test]
fn a_bad_exchange_field_is_refused_with_a_fixed_reason_and_nothing_is_written() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.on_events(MISSION, &[research()]);
    let r = t.on_host_exchange(MISSION, &json!({"prompt": "x", "asked": 4}), 0);
    assert!(matches!(&r, HostReport::Refused(why) if why.contains("asked")), "{r:?}");
    assert!(records(ws.path()).is_empty());
    assert_eq!(t.on_host_exchange(MISSION, &exchange(), 0).word(), "recorded", "a refused call does not use up the trigger");
}

#[test]
fn a_project_marked_off_says_so_and_writes_nothing() {
    let ws = crate::history_consent::answered_ws();
    std::fs::create_dir_all(ws.path().join("local/projects/home")).unwrap();
    std::fs::write(ws.path().join("local/projects/home/history.off"), "").unwrap();
    let t = tap(ws.path(), on());
    t.on_events(MISSION, &[research()]);
    let r = t.on_host_exchange(MISSION, &exchange(), 0);
    assert_eq!(r, HostReport::Appended(Appended::ProjectOff));
    assert_eq!(r.word(), "off for this project");
    assert!(!ws.path().join("local/history").exists());
}

#[test]
fn the_merge_trigger_concludes_the_daemons_prompt_with_the_pull_request_number_and_nothing_of_the_ledger_row() {
    let ws = crate::history_consent::answered_ws();
    let t = tap(ws.path(), on());
    t.note_operator_prompt("t_1", "land the queue fix");
    let row = crate::history_merge::read_new(&{
        let p = crate::history_merge::ledger_path(ws.path());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, format!("{}\n", json!({"kind": "merged", "pr": 42, "quote": "merge 42 now, key sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWX"}))).unwrap();
        p
    }, 0)
    .rows
    .remove(0);
    let rec = match t.on_merge_row(MISSION, &row) {
        Some(Appended::Written(r)) => *r,
        other => panic!("{other:?}"),
    };
    assert_eq!((rec.trigger, rec.kind), (Trigger::Merge, Kind::Observed));
    assert_eq!(rec.observed.ids, vec!["pr_42".to_string()]);
    assert_eq!(rec.prompt.unwrap().text, "land the queue fix");
    assert!(!store_text(ws.path()).contains("sk-ant"), "the ledger's quote never reaches the store");
}
