// The operator's read-only view of the exchange records and the project report (`specs/102` §5.1 last bullet, slice S5).
//
// This is not the agent path. It has no model and no agent in it: the daemon answers `history.view` to a console only, and the console
// draws what comes back. It reads the store and the mission log and writes nothing: no key is made, no file is created, nothing is
// repaired. Every string is cleaned **here, at read time**, and cut to its cap again, so that an escape sequence stored in a record
// (or planted in a hand-edited file) cannot reach a terminal (§5.1). The console cleans again before it draws.
//
// What the view shows is what is stored and no more: a pasted block is a keyed hash (the first twelve characters) and a length, never
// text; a record that failed the chain check is shown as `chain broken` and never as valid; a host's account of its own reply is
// labelled `self-reported, not checked`. No count here is an outcome statistic: the only counts are how many records verified, how many
// did not, and how many are hidden by the list's length (§11 item 2).

use std::path::Path;

use serde_json::{json, Value};

use crate::history_record::{self as rec, Checked, Kind, OutcomeValue, Provenance, Record, Settings, Status, Trigger};
use crate::history_report;

/// How many records one call returns, newest first.
pub const LIST_MAX: usize = 50;
/// The caps of what is drawn, applied again at read time (the stored caps are 2,000 and 200).
const PROMPT_CAP: usize = 2000;
const FIELD_CAP: usize = 200;
const ID_CAP: usize = 64;
const PATH_CAP: usize = 200;
/// The report is the daemon's own fixed-template text; it is cleaned and cut here too.
const REPORT_CAP: usize = 16 * 1024;

pub const REPLY_LABEL: &str = "self-reported, not checked";
/// The history is on but nobody has answered the install question (`specs/102` §7 item 4): nothing is recorded until a person does.
pub const WAITING_MESSAGE: &str = "history is on, but nothing is recorded until you answer the install question: run `delphi` in a terminal";

pub const OFF_MESSAGE: &str = "history is off: nothing is being recorded. it is off because `[history] enabled = false` is in config.toml or `delphi history off` was run; `delphi doctor` shows the setting";

/// Strip every control character (the C0 and C1 ranges and DEL, so an escape sequence loses its lead byte), turn line breaks and tabs
/// into spaces, and cut to `cap` characters. The result has nothing in it that a terminal acts on.
pub fn plain(s: &str, cap: usize) -> String {
    let mut out = String::new();
    let mut n = 0usize;
    for c in s.chars() {
        let c = match c {
            '\n' | '\r' | '\t' => ' ',
            c if c.is_control() => continue,
            // The bidirectional overrides and isolates reorder text on screen: they are not text.
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' => continue,
            c => c,
        };
        if n >= cap {
            out.push('…');
            break;
        }
        out.push(c);
        n += 1;
    }
    out
}

fn trigger_word(t: Trigger) -> &'static str {
    match t {
        Trigger::Merge => "merge",
        Trigger::ResearchRun => "research run",
        Trigger::Panel => "panel",
        Trigger::Council => "council",
        Trigger::Artifact => "work artifact",
    }
}

fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Observed => "observed",
        Kind::Resolved => "resolved",
        Kind::Reported => "reported",
    }
}

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Verified => "verified",
        Status::ChainBroken => "chain broken",
        Status::Unverifiable => "unverifiable",
    }
}

fn outcome_word(v: OutcomeValue) -> &'static str {
    match v {
        OutcomeValue::Done => "done",
        OutcomeValue::Partly => "partly",
        OutcomeValue::Stopped => "stopped",
        OutcomeValue::Unknown => "unknown",
    }
}

fn row(c: &Checked) -> Value {
    let r: &Record = &c.record;
    let ids: Vec<String> = r.observed.ids.iter().filter_map(|i| history_report::safe_id(i)).take(20).collect();
    let counts: serde_json::Map<String, Value> = r.observed.counts.iter().take(20).filter_map(|(k, v)| history_report::safe_id(k).map(|k| (k, json!(v)))).collect();
    json!({
        "id": plain(&r.id, ID_CAP),
        "at": plain(&r.at, 40),
        "mission": plain(&r.mission, ID_CAP),
        "trigger": trigger_word(r.trigger),
        "kind": kind_word(r.kind),
        "status": status_word(c.status),
        "prompt": r.prompt.as_ref().map(|p| json!({"text": plain(&p.text, PROMPT_CAP), "cut": p.cut})),
        "artifacts": r.artifacts.iter().take(20).map(|a| json!({"artifact": plain(&a.artifact, PATH_CAP), "log": plain(&a.log, PATH_CAP)})).collect::<Vec<_>>(),
        // A pasted block is its keyed hash and its length. The hash is shortened for the eye; the full one stays in the store.
        "pasted": r.pasted.iter().take(50).map(|b| json!({"hash": plain(&b.hash, 64).chars().take(12).collect::<String>(), "chars": b.chars})).collect::<Vec<_>>(),
        "reply": r.reply.as_ref().map(|y| json!({"label": REPLY_LABEL, "asked": plain(&y.asked, FIELD_CAP), "did": plain(&y.did, FIELD_CAP), "open": plain(&y.open, FIELD_CAP)})),
        "outcome": {
            "value": outcome_word(r.outcome.value),
            "provenance": match r.outcome.provenance { Provenance::Observed => "observed", Provenance::SelfReported => "self-reported" },
        },
        "observed": {"ids": ids, "counts": counts},
        "private": r.private,
    })
}

/// The answer to `history.view`. `mission_dir` is the open mission's folder (its `scope` file names the project; the report is
/// folded from its log). Reads only. Never fails: a problem is a `state` and a `message`, so the console always has something to say.
pub fn view(ws_dir: &Path, settings: &Settings, mission_dir: &Path) -> Value {
    let project = rec::Tap::project_of(mission_dir);
    let waiting = settings.enabled && crate::history_consent::blocks_writes(ws_dir, crate::history_consent::SHOW_AT_INSTALL);
    let capture = if rec::project_off(ws_dir, &project) {
        "off for this project"
    } else if waiting {
        "waiting for your answer"
    } else if settings.enabled {
        "on"
    } else {
        "off"
    };
    let report = match history_report::timeline_of(mission_dir) {
        Ok(t) => json!({"text": plain_report(&history_report::report(&t)), "error": Value::Null}),
        Err(_) => json!({"text": Value::Null, "error": "the report could not be built from this mission's log"}),
    };
    let base = |state: &str, message: Option<String>, records: Vec<Value>, total: usize, hidden: usize, counts: (usize, usize, usize), broken_at: Option<usize>| {
        json!({
            "state": state,
            "message": message,
            "project": project,
            "capture": capture,
            "setting": "[history] enabled in config.toml",
            "records": records,
            "total": total,
            "hidden": hidden,
            "verified": counts.0,
            "chain_broken": counts.1,
            "unverifiable": counts.2,
            "broken_at": broken_at,
            "report": report,
            "read_only": true,
        })
    };
    let read = match rec::read(ws_dir, &project) {
        Ok(r) => r,
        Err(e) => return base("unreadable", Some(plain(&e, FIELD_CAP)), Vec::new(), 0, 0, (0, 0, 0), None),
    };
    if read.records.is_empty() {
        if waiting {
            return base("off", Some(WAITING_MESSAGE.into()), Vec::new(), 0, 0, (0, 0, 0), None);
        }
        return if settings.enabled && capture == "on" {
            base("empty", Some("no records yet: one is written after a merge, a research run, a panel, a Council, or a work artifact".into()), Vec::new(), 0, 0, (0, 0, 0), None)
        } else {
            base("off", Some(OFF_MESSAGE.into()), Vec::new(), 0, 0, (0, 0, 0), None)
        };
    }
    let count = |s: Status| read.records.iter().filter(|c| c.status == s).count();
    let counts = (count(Status::Verified), count(Status::ChainBroken), count(Status::Unverifiable));
    let total = read.records.len();
    // Newest first. The chain is checked over the whole store, not over what is listed.
    let records: Vec<Value> = read.records.iter().rev().take(LIST_MAX).map(row).collect();
    let hidden = total - records.len();
    let message = (capture != "on").then(|| format!("capture is {capture}: the records below were written earlier"));
    base("records", message, records, total, hidden, counts, read.broken_at)
}

/// The report text: control characters stripped, line breaks kept (it is a fixed-template block), cut to a cap.
fn plain_report(s: &str) -> String {
    s.lines().map(|l| plain(l, 400)).collect::<Vec<_>>().join("\n").chars().take(REPORT_CAP).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_record::{append, Appended, Draft, Observed, Outcome, PromptIn};
    use crate::history_redact::Typing;
    use std::fs;

    fn on() -> Settings {
        Settings { enabled: true, ..Settings::default() }
    }

    fn draft(prompt: &str, mission: &str) -> Draft {
        Draft {
            trigger: Trigger::Council,
            mission: mission.into(),
            project: "home".into(),
            at: "2026-11-12T10:00:00Z".into(),
            prompt: PromptIn::Observed { text: prompt.into(), marked: Vec::new(), typing: Typing::Typed },
            reply: None,
            outcome: Outcome { value: OutcomeValue::Done, provenance: Provenance::Observed },
            observed: Observed::default(),
        }
    }

    fn written(a: Appended) -> Record {
        match a {
            Appended::Written(r) => *r,
            _ => panic!("expected a written record"),
        }
    }

    fn mission_dir(ws: &Path) -> std::path::PathBuf {
        let d = ws.join("missions/m1");
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn plain_leaves_nothing_a_terminal_acts_on() {
        let s = plain("a\u{1b}[31mred\u{7}\u{0}\r\nb\tc\u{85}\u{202e}d", 100);
        assert_eq!(s, "a[31mred  b cd");
        assert!(s.chars().all(|c| !c.is_control()));
        assert_eq!(plain("abcdef", 3), "abc…");
        assert_eq!(plain("", 3), "");
    }

    #[test]
    fn a_dormant_install_with_no_store_says_history_is_off_and_where_the_setting_is_and_writes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let md = mission_dir(t.path());
        let v = view(t.path(), &Settings { enabled: false, ..Settings::default() }, &md);
        assert_eq!(v["state"], "off");
        assert_eq!(v["capture"], "off");
        assert_eq!(v["read_only"], true);
        assert!(v["message"].as_str().unwrap().contains("[history] enabled"));
        assert_eq!(v["setting"], "[history] enabled in config.toml");
        assert_eq!(v["records"].as_array().unwrap().len(), 0);
        assert!(!t.path().join("local").exists(), "reading made no key and no store");
    }

    #[test]
    fn an_enabled_install_with_no_records_is_empty_not_off() {
        let t = crate::history_consent::answered_ws();
        let v = view(t.path(), &on(), &mission_dir(t.path()));
        assert_eq!(v["state"], "empty");
        assert_eq!(v["capture"], "on");
        assert!(!t.path().join("local/history.key").exists(), "a view never makes the key");
    }

    #[test]
    fn records_are_listed_newest_first_verified_and_a_paste_is_a_hash_and_a_length() {
        let t = crate::history_consent::answered_ws();
        let md = mission_dir(t.path());
        written(append(t.path(), &on(), draft("first goal", "m1")));
        let big = "p".repeat(900);
        // Nothing says the second prompt was typed, so the paste rule's length threshold applies to it.
        let mut second = draft(&format!("second {big}"), "m1");
        second.prompt = PromptIn::Observed { text: format!("second {big}"), marked: Vec::new(), typing: Typing::NotSeen };
        written(append(t.path(), &on(), second));
        let v = view(t.path(), &on(), &md);
        assert_eq!(v["state"], "records");
        assert_eq!(v["total"], 2);
        assert_eq!(v["verified"], 2);
        assert_eq!(v["chain_broken"], 0);
        let rows = v["records"].as_array().unwrap();
        assert_eq!(rows[0]["id"], "ex_0002", "newest first");
        assert_eq!(rows[0]["status"], "verified");
        assert_eq!(rows[0]["trigger"], "council");
        assert_eq!(rows[1]["prompt"]["text"], "first goal");
        let pasted = rows[0]["pasted"].as_array().unwrap();
        assert!(!pasted.is_empty(), "the long block was held as a paste");
        assert_eq!(pasted[0]["hash"].as_str().unwrap().len(), 12);
        assert!(pasted[0]["chars"].as_u64().unwrap() >= 900);
        assert!(!rows[0].to_string().contains(&big), "the pasted text is not in the view");
        assert!(v["report"]["text"].is_string(), "the report is there");
    }

    #[test]
    fn a_tampered_record_and_everything_after_it_read_chain_broken_never_verified() {
        let t = crate::history_consent::answered_ws();
        let md = mission_dir(t.path());
        for p in ["one", "two", "three"] {
            written(append(t.path(), &on(), draft(p, "m1")));
        }
        let path = rec::store_path(t.path(), "home");
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, text.replacen("\"two\"", "\"forged\"", 1)).unwrap();
        let v = view(t.path(), &on(), &md);
        assert_eq!(v["verified"], 1);
        assert_eq!(v["chain_broken"], 2);
        assert_eq!(v["broken_at"], 1);
        let rows = v["records"].as_array().unwrap();
        assert_eq!(rows[0]["status"], "chain broken");
        assert_eq!(rows[1]["status"], "chain broken");
        assert_eq!(rows[2]["status"], "verified");
        assert!(rows.iter().filter(|r| r["status"] == "verified").count() == 1);
    }

    #[test]
    fn a_missing_key_reads_unverifiable_not_verified() {
        let t = crate::history_consent::answered_ws();
        let md = mission_dir(t.path());
        written(append(t.path(), &on(), draft("one", "m1")));
        fs::remove_file(t.path().join("local/history.key")).unwrap();
        let v = view(t.path(), &on(), &md);
        assert_eq!(v["records"][0]["status"], "unverifiable");
        assert_eq!(v["verified"], 0);
        assert!(!t.path().join("local/history.key").exists(), "a view does not make a key either");
    }

    #[test]
    fn control_characters_in_a_stored_record_do_not_reach_the_view() {
        let t = crate::history_consent::answered_ws();
        let md = mission_dir(t.path());
        written(append(t.path(), &on(), draft("hello", "m1")));
        // Hand-edit the store: an escape sequence in the prompt, the reply and the mission, and an oversized reply field.
        let path = rec::store_path(t.path(), "home");
        let text = fs::read_to_string(&path).unwrap();
        let evil = text.replacen("hello", "hi\\u001b]0;pwned\\u0007 there", 1).replacen("\"mission\":\"m1\"", "\"mission\":\"m\\u001b[2Jx\"", 1);
        fs::write(&path, evil).unwrap();
        let v = view(t.path(), &on(), &md);
        let all = v.to_string();
        assert!(!all.contains("\\u001b") && !all.contains('\u{1b}') && !all.contains("\\u0007"), "{all}");
        assert_eq!(v["records"][0]["prompt"]["text"], "hi]0;pwned there");
        assert_eq!(v["records"][0]["mission"], "m[2Jx");
        assert_eq!(v["records"][0]["status"], "chain broken", "an edited record is also not valid");
    }

    #[test]
    fn a_reply_is_labelled_self_reported_and_cut_again_at_read_time() {
        let t = crate::history_consent::answered_ws();
        let md = mission_dir(t.path());
        let mut d = draft("goal", "m1");
        d.reply = Some(rec::Reply { asked: "a".into(), did: "d".into(), open: "o".into() });
        written(append(t.path(), &on(), d));
        let path = rec::store_path(t.path(), "home");
        let long = "x".repeat(500);
        fs::write(&path, fs::read_to_string(&path).unwrap().replacen("\"did\":\"d\"", &format!("\"did\":\"{long}\""), 1)).unwrap();
        let v = view(t.path(), &on(), &md);
        let reply = &v["records"][0]["reply"];
        assert_eq!(reply["label"], REPLY_LABEL);
        assert_eq!(reply["asked"], "a");
        assert!(reply["did"].as_str().unwrap().chars().count() <= FIELD_CAP + 1);
        assert_eq!(v["records"][0]["outcome"]["provenance"], "observed");
    }

    #[test]
    fn records_written_before_history_was_turned_off_are_still_shown_with_a_note() {
        let t = crate::history_consent::answered_ws();
        let md = mission_dir(t.path());
        written(append(t.path(), &on(), draft("earlier", "m1")));
        let v = view(t.path(), &Settings { enabled: false, ..Settings::default() }, &md);
        assert_eq!(v["state"], "records");
        assert_eq!(v["capture"], "off");
        assert!(v["message"].as_str().unwrap().contains("written earlier"));
        assert_eq!(v["records"][0]["status"], "verified");
    }

    #[test]
    fn the_list_is_capped_and_says_how_many_are_hidden() {
        let t = crate::history_consent::answered_ws();
        let md = mission_dir(t.path());
        for i in 0..(LIST_MAX + 3) {
            written(append(t.path(), &on(), draft(&format!("g{i}"), "m1")));
        }
        let v = view(t.path(), &on(), &md);
        assert_eq!(v["records"].as_array().unwrap().len(), LIST_MAX);
        assert_eq!(v["total"], LIST_MAX + 3);
        assert_eq!(v["hidden"], 3);
        assert_eq!(v["verified"], LIST_MAX + 3, "the whole chain is checked, not the listed part");
    }
}
