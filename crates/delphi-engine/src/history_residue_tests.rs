//! Spec 102 S7, the zero-residue purge scan (§11 item 7, §15 "Purge residue"): a canary is planted through the real write paths (the
//! daemon's own task start and tap, the merge ledger trigger, a host's exchange fields), `purge` and `delete` run, and the WHOLE
//! scratch home is scanned for the canary, for its hex and base64 forms and for its plain SHA-256.
//!
//! What "residue" means here: a copy the history made. The operator's prompt is also the task's own input, so the mission's own
//! event log and the session log hold it by design, with history on or off (specs/030). The scan therefore compares with a baseline
//! run with history off: after a purge every file that still holds the canary must be a file the baseline holds it in too. A copy the
//! history added and the purge missed is a failure.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::history_manage::{erase, projects, Which};
use crate::history_record::{self as rec, Settings, Tap};
use crate::model::Step;
use crate::testkit::{setup, Fx};

const PROMPT: &str = "ZXQ-RESIDUE-PROMPT-7731 plan the queue migration";
const HOST_PROMPT: &str = "ZXQ-RESIDUE-HOSTPROMPT-5520 ship the report";
const ASKED: &str = "ZXQ-RESIDUE-ASKED-1180";
const DID: &str = "ZXQ-RESIDUE-DID-2290";
const OPEN: &str = "ZXQ-RESIDUE-OPEN-3340";

fn forms(s: &str) -> Vec<String> {
    use base64::Engine as _;
    let sha: String = Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    let hex: String = s.bytes().map(|b| format!("{b:02x}")).collect();
    vec![s.to_string(), sha, hex, base64::engine::general_purpose::STANDARD.encode(s), base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s)]
}

/// Every file under `root` (any kind, any depth, hidden ones too) whose bytes hold `needle`, as paths relative to `root`.
fn files_holding(root: &Path, needle: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for e in walkdir::WalkDir::new(root).into_iter().flatten().filter(|e| e.file_type().is_file()) {
        let bytes = std::fs::read(e.path()).unwrap_or_default();
        if bytes.windows(needle.len()).any(|w| w == needle.as_bytes()) {
            out.insert(e.path().strip_prefix(root).unwrap().to_string_lossy().into_owned());
        }
    }
    out
}

fn on(f: &Fx) -> Tap {
    Tap::new(f.engine.ws.dir.clone(), f.engine.settings.history.clone(), "home")
}

fn merged(pr: u64) -> serde_json::Value {
    json!({"kind": "merged", "pr": pr})
}

async fn exchange(history: bool) -> Fx {
    let f = setup(vec![Step::text("done"), Step::text("noted")], 5.0, |s| s.history.enabled = history);
    if history {
        // The install question has been answered (a record is written only after that, `specs/102` S8).
        crate::history_consent::answer_yes_in(&f.engine.ws.dir);
    }
    f.m.set_history(on(&f));
    // The operator's prompt through the real task start, then a merge the daemon watched concludes it.
    f.engine.run_task(&f.m, "orchestrator", PROMPT).await.unwrap();
    f.m.history_merge_row(&merged(42));
    // A second exchange the host reports: a fresh tap that holds no prompt of its own, a trigger it could not attribute, then the
    // host's summary with the exchange fields.
    f.m.set_history(on(&f));
    f.m.history_merge_row(&merged(43));
    let now = delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap();
    let r = f.m.history_host_exchange(&json!({"summary": "s", "prompt": HOST_PROMPT, "asked": ASKED, "did": DID, "open": OPEN}), now);
    if history {
        assert!(matches!(r, rec::HostReport::Appended(rec::Appended::Written(_))), "{r:?}");
    }
    // A later task, with the records in the store: by default no record text enters any model request (§11 item 3).
    f.engine.run_task(&f.m, "orchestrator", "what did I ask earlier?").await.unwrap();
    for req in f.model.requests() {
        let body = req.body.to_string();
        for c in [HOST_PROMPT, ASKED, DID, OPEN] {
            assert!(!body.contains(c), "record text {c} reached a model request");
        }
    }
    f
}

#[tokio::test(flavor = "multi_thread")]
async fn purge_and_delete_leave_no_copy_the_history_made_anywhere_under_the_home() {
    let baseline = exchange(false).await;
    let f = exchange(true).await;
    let ws = f.engine.ws.dir.clone();
    assert_eq!(rec::read(&ws, "home").unwrap().counted().count(), 2, "both exchanges were recorded");
    // The canaries really reached the store (the scan is not vacuous) and the host's reply text is only ever in the store.
    for c in [PROMPT, HOST_PROMPT, ASKED, DID, OPEN] {
        assert!(files_holding(&f.root, c).iter().any(|p| p.ends_with("records.jsonl")), "{c} never reached the store");
    }
    // The first record is deleted by id, the rest purged; both paths are scanned.
    let id = rec::read(&ws, "home").unwrap().counted().next().unwrap().id.clone();
    erase(&ws, "home", Which::Id(&id), "2026-11-20T00:00:00Z").unwrap();
    for c in forms(PROMPT) {
        let left = files_holding(&f.root, &c);
        assert!(!left.iter().any(|p| p.contains("history")), "after `delete` the prompt form {c:?} is still in {left:?}");
    }
    for p in projects(&ws) {
        erase(&ws, &p, Which::All, "2026-11-20T00:00:01Z").unwrap();
    }
    for c in [PROMPT, HOST_PROMPT, ASKED, DID, OPEN] {
        for form in forms(c) {
            let allowed = files_holding(baseline.root.as_path(), &form);
            let left = files_holding(&f.root, &form);
            // The baseline's paths carry the same relative names (the fixture is deterministic), so compare by name.
            let extra: Vec<_> = left.iter().filter(|p| !allowed.contains(*p)).collect();
            assert!(extra.is_empty(), "after `purge` the form {form:?} of {c:?} is still in {extra:?}");
            if c != PROMPT {
                assert!(left.is_empty(), "{c:?} exists only in the history, yet survives in {left:?}");
            }
        }
    }
    let status = rec::read(&ws, "home").unwrap();
    assert_eq!(status.counted().count(), 0);
}

#[test]
fn a_prompt_the_daemon_still_holds_in_memory_is_not_written_back_after_a_purge() {
    let ws = crate::history_consent::answered_ws();
    let tap = Tap::new(ws.path().to_path_buf(), Settings { enabled: true, ..Settings::default() }, "home");
    tap.note_operator_prompt("t_1", "ZXQ-RESIDUE-HELD-9001 first");
    let first = tap.on_merge_row("m1", &merged(1));
    assert!(matches!(first, Some(rec::Appended::Written(_))), "{first:?}");
    // A second prompt is noted and NOT yet concluded when the operator purges.
    std::thread::sleep(std::time::Duration::from_millis(5));
    tap.note_operator_prompt("t_2", "ZXQ-RESIDUE-HELD-9002 second");
    std::thread::sleep(std::time::Duration::from_millis(5));
    for p in projects(ws.path()) {
        erase(ws.path(), &p, Which::All, "2026-11-20T00:00:00Z").unwrap();
    }
    // The next trigger must not resurrect the purged operator's words.
    let later = tap.on_merge_row("m1", &merged(2));
    assert!(!matches!(later, Some(rec::Appended::Written(_))), "the purged prompt was written back: {later:?}");
    assert!(files_holding(ws.path(), "ZXQ-RESIDUE-HELD").is_empty(), "text survived: {:?}", files_holding(ws.path(), "ZXQ-RESIDUE-HELD"));
    // A prompt noted AFTER the purge is a new exchange and is kept as usual.
    std::thread::sleep(std::time::Duration::from_millis(5));
    tap.note_operator_prompt("t_3", "ZXQ-RESIDUE-HELD-9003 third");
    assert!(matches!(tap.on_merge_row("m1", &merged(3)), Some(rec::Appended::Written(_))));
}

#[test]
fn the_rewrite_leaves_no_temporary_file_and_the_marker_holds_no_text() {
    let ws = crate::history_consent::answered_ws();
    let tap = Tap::new(ws.path().to_path_buf(), Settings { enabled: true, ..Settings::default() }, "home");
    tap.note_operator_prompt("t_1", "ZXQ-RESIDUE-TMP-4411");
    tap.on_merge_row("m1", &merged(1));
    erase(ws.path(), "home", Which::All, "2026-11-20T00:00:00Z").unwrap();
    let names: Vec<String> = walkdir::WalkDir::new(ws.path()).into_iter().flatten().filter(|e| e.file_type().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(!names.iter().any(|n| n.ends_with(".tmp") || n.ends_with('~') || n.ends_with(".bak") || n.ends_with(".orig")), "{names:?}");
    let marker = std::fs::read_to_string(rec::purged_marker_path(ws.path())).unwrap();
    assert!(marker.trim().chars().all(|c| c.is_ascii_digit()), "the purge marker holds a time and nothing else: {marker:?}");
}
