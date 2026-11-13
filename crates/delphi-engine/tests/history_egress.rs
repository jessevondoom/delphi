//! Spec 102 S7: egress coverage for the exchange record (§11 item 3, §14 item 7, §15 "Egress").
//!
//! What these tests prove, and what they do not:
//!
//! * the history channel is registered and its sender calls the one check;
//! * every `history_*.rs` module is classified, and none of them names a network, process or socket API, so none can send anything;
//! * the only code that may read a record is on a list; a relay, mail, connector, outbound, model, debug-bundle or export file is not
//!   on it, so a record's text cannot be placed there by an existing path;
//! * `for_prompt`, the one function that could hand a record's text to a model prompt, has no caller, defaults to deny and never
//!   serves a foreign or unknown family.
//!
//! The four channels Relay, Connector, Mail and Outbound were `Unaudited` in `egress::PATHS` when this file was written; `specs/103` M2 and M3
//! wired them (they ask, refuse or replace agent-written text for a task whose private-read mark is live). That covers agent-written text
//! and the private-read mark, not the history store: what proves the store never leaves is the file lists below, which show that no such
//! file reads it today.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use delphi_engine::egress::{Wired, PATHS};
use delphi_engine::history_record::{for_prompt, Seat, Settings, ToModels};

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").canonicalize().unwrap()
}

fn sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                if !matches!(p.file_name().and_then(|n| n.to_str()), Some("target" | "tests" | "node_modules" | "fixtures")) {
                    walk(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let root = crates_dir();
    let mut files = vec![];
    walk(&root, &mut files);
    files.into_iter().map(|f| (f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read_to_string(&f).unwrap())).collect()
}

/// The production part of a source file: comments and everything from the first `#[cfg(test)]` module on dropped.
fn production(src: &str) -> String {
    let cut = src.find("#[cfg(test)]").unwrap_or(src.len());
    src[..cut].lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

fn is_test_file(rel: &str) -> bool {
    rel.ends_with("_tests.rs") || rel.contains("/tests/")
}

/// The history modules of the engine and what each is (every `history_*.rs` file that is not a test file).
const HISTORY_MODULES: [(&str, &str); 9] = [
    ("history_citation.rs", "reads a host's own transcript files by hash (local reads)"),
    ("history_consent.rs", "the install answer, one word in a local file"),
    ("history_manage.rs", "off, purge, delete, status: local files"),
    ("history_merge.rs", "reads the merge ledger, a local file"),
    ("history_pointer.rs", "keyed hashes, pointers and the local key"),
    ("history_record.rs", "the store, the tap, and the History channel's check (`release`, `for_prompt`)"),
    ("history_redact.rs", "pure text functions"),
    ("history_report.rs", "the fixed-template report, from the mission log"),
    ("history_view.rs", "the console's read-only view"),
];

const FORBIDDEN_APIS: [&str; 14] = ["reqwest", "lettre", "imap::", "ureq", "hyper::", "tungstenite", "TcpStream", "TcpListener", "UdpSocket", "UnixStream", "std::net", "std::process", "tokio::process", "Command::new"];

#[test]
fn the_history_channel_is_registered_and_its_sender_calls_the_one_check() {
    let row = PATHS.iter().find(|(c, _, _)| format!("{c:?}") == "History").expect("the History channel has no registered path");
    assert_eq!(row.2, Wired::Yes);
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(row.1)).unwrap();
    let prod = production(&src);
    assert!(prod.contains("egress::check(") && prod.contains("Channel::History"), "{} must call egress::check for Channel::History", row.1);
}

/// Spec 102 S7 pinned the four channels that carry agent-written text (Relay, Connector, Mail, Outbound) as `Unaudited`, so the history work
/// could not claim they were covered. `specs/103` M2 (Connector, Outbound) and M3 (Relay, Mail) are the change that retires them: each now asks,
/// refuses or replaces text for a task whose private-read mark is live. This test follows that state, so a channel that falls back to
/// `Unaudited` (or a new one added without an audit) fails here as well as in `egress.rs::no_channel_is_unaudited`.
#[test]
fn no_channel_that_carries_agent_text_is_unaudited_since_spec_103_m2_and_m3() {
    let un: Vec<String> = PATHS.iter().filter(|(_, _, w)| *w == Wired::Unaudited).map(|(c, _, _)| format!("{c:?}")).collect();
    assert!(un.is_empty(), "a channel is `Unaudited` again: {un:?}");
    for ch in ["Relay", "Connector", "Mail", "Outbound"] {
        assert!(
            PATHS.iter().any(|(c, _, w)| format!("{c:?}") == ch && *w == Wired::Yes),
            "{ch} is not wired through the one egress check"
        );
    }
}

#[test]
fn every_history_module_is_classified_and_none_can_reach_a_network_or_start_a_process() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeSet::new();
    for e in std::fs::read_dir(&src).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("history_") && name.ends_with(".rs") && !name.ends_with("_tests.rs") {
            found.insert(name);
        }
    }
    let listed: BTreeSet<String> = HISTORY_MODULES.iter().map(|(f, _)| f.to_string()).collect();
    assert_eq!(found, listed, "a history module was added or removed: read it, classify it in HISTORY_MODULES, and check it names no network API");
    for (f, what) in HISTORY_MODULES {
        let prod = production(&std::fs::read_to_string(src.join(f)).unwrap());
        for api in FORBIDDEN_APIS {
            assert!(!prod.contains(api), "{f} ({what}) names `{api}`: a history module must not reach the network or start a process");
        }
    }
}

/// Files that may name a history module. A new file that does is a new reader of the store (or of its settings) and fails here until
/// it is read and added. `lib.rs` and `egress.rs` only name the module and the channel.
const MAY_NAME_HISTORY: [&str; 12] = [
    "delphi/src/doctor.rs",
    "delphi/src/history_cmd.rs",
    "delphi-adapter/src/config.rs",
    "delphi-engine/src/boot.rs",
    "delphi-engine/src/egress.rs",
    "delphi-engine/src/engine.rs",
    "delphi-engine/src/host_rpc.rs",
    "delphi-engine/src/lib.rs",
    "delphi-engine/src/library.rs",
    "delphi-engine/src/mission.rs",
    "delphi-engine/src/service.rs",
    "delphi/src/main.rs",
];

#[test]
fn only_listed_files_name_a_history_module_and_no_outbound_file_is_among_them() {
    let re = regex::Regex::new(r"history_(record|view|report|pointer|manage|citation|merge|consent|redact)\b").unwrap();
    let mut named = BTreeSet::new();
    for (rel, src) in sources() {
        if is_test_file(&rel) || rel.starts_with("delphi-engine/src/history_") {
            continue;
        }
        if re.is_match(&production(&src)) {
            named.insert(rel);
        }
    }
    let allowed: BTreeSet<String> = MAY_NAME_HISTORY.iter().map(|s| s.to_string()).collect();
    let extra: Vec<_> = named.difference(&allowed).collect();
    assert!(extra.is_empty(), "these files name a history module and are not on the list; read each one: could it send a record's text anywhere? {extra:?}");
    // The outbound-looking files are named explicitly: none reads the store, whatever the list above says.
    let outbound = ["relay/", "mail", "connector", "outbound", "telegram", "smtp", "imap", "adapter/src/http.rs", "adapter/src/oauth2.rs", "adapter/src/research/", "engine/src/model.rs", "logs_cmd.rs", "elog.rs", "library_export.rs", "library_bib.rs", "bundle_pack.rs", "bundle_report.rs", "selfupdate.rs"];
    for rel in &named {
        assert!(!outbound.iter().any(|o| rel.contains(o)), "{rel} names a history module and looks like an outbound path");
    }
}

#[test]
fn no_debug_bundle_log_export_or_telemetry_source_reads_the_history_store() {
    // The store's file names and directory: a path that reads them is a reader of the records, whatever it imports.
    let store = regex::Regex::new(r#"records\.jsonl|local/history|history\.key|history\.purged"#).unwrap();
    let readers = ["delphi/src/logs_cmd.rs", "delphi-core/src/elog.rs", "delphi-engine/src/library_export.rs", "delphi-engine/src/library_bib.rs", "delphi/src/bundle_pack.rs", "delphi/src/bundle_report.rs", "delphi/src/selfupdate.rs"];
    let all = sources();
    for r in readers {
        let (_, src) = all.iter().find(|(rel, _)| rel == r).unwrap_or_else(|| panic!("{r} moved: update this list"));
        assert!(!store.is_match(&production(src)), "{r} names the history store");
    }
    // And every other production file that names the store's files is one of the history modules or the CLI's own.
    let ok = ["delphi-engine/src/history_", "delphi/src/history_cmd.rs", "delphi/src/main.rs" /* the help text of `delphi history` names the marker files */, "delphi/src/doctor.rs", "delphi-engine/src/service.rs", "delphi-engine/src/library.rs", "delphi-engine/src/mission.rs"];
    for (rel, src) in &all {
        if is_test_file(rel) || ok.iter().any(|o| rel.starts_with(o)) {
            continue;
        }
        assert!(!store.is_match(&production(src)), "{rel} names the history store's files and is not a known reader");
    }
}

#[test]
fn nothing_but_the_history_module_calls_for_prompt_and_by_default_no_record_is_served() {
    let re = regex::Regex::new(r"\bfor_prompt\(").unwrap();
    for (rel, src) in sources() {
        if is_test_file(&rel) || rel == "delphi-engine/src/history_record.rs" {
            continue;
        }
        assert!(!re.is_match(&production(&src)), "{rel} calls history_record::for_prompt: that places record text in a model prompt (spec 102 §11 item 3)");
    }
    assert_eq!(Settings::default().to_models, ToModels::Deny, "the default of [history] to_models is deny");
}

#[test]
fn for_prompt_never_serves_a_foreign_or_unknown_family_under_any_setting() {
    use delphi_engine::history_record::{build, Draft, Observed, Outcome, OutcomeValue, PromptIn, Provenance, Trigger};
    use delphi_engine::history_redact::Typing;
    let key = [3u8; 32];
    let on = Settings { enabled: true, ..Settings::default() };
    let r = build(
        &key,
        &on,
        Draft {
            trigger: Trigger::Merge,
            mission: "m1".into(),
            project: "home".into(),
            at: "2026-11-12T00:00:00Z".into(),
            prompt: PromptIn::Observed { text: "ZXQ-EGRESS-CANARY".into(), marked: vec![], typing: Typing::Typed },
            reply: None,
            outcome: Outcome { value: OutcomeValue::Done, provenance: Provenance::Observed },
            observed: Observed::default(),
        },
    );
    for to_models in [ToModels::Deny, ToModels::Allow] {
        let s = Settings { enabled: true, to_models, ..Settings::default() };
        for seat in [Seat::Foreign, Seat::Unknown] {
            assert!(for_prompt(&s, seat, &r).is_err(), "{seat:?} was served a record with to_models {to_models:?}");
        }
    }
    let deny = Settings { enabled: true, to_models: ToModels::Deny, ..Settings::default() };
    assert!(for_prompt(&deny, Seat::SameFamily, &r).is_err(), "by default not even the host's own family gets record text");
}
