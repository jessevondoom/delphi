//! A mission log written by an earlier release must still be readable, verifiable and continuable by this build (spec 093 B2-E).
//!
//! `tests/fixtures/logs/<release>/events/` holds a real log written by that release's own `EventLog` (not by this build, and
//! not hand-typed). To add one when a release is cut: check out the tag, put a throwaway test beside `crates/delphi-core/src/log.rs`
//! that appends a fixed list of representative events with `EventLog::open(..).append(..)` (the log refuses a kind that lacks
//! its required fields, so the fixture is faithful to the writer), copy the resulting `events/NNNNNN.jsonl` here, and add the
//! release to `RELEASES`. Keep the content generic: no prompts, paths or names from a real mission.
//!
//! What this catches: a change to the event envelope, the hash chain, the required fields or the segment layout that makes an
//! upgraded daemon unable to resume a mission it recorded before the upgrade.

use delphi_core::event::EventEnvelope;
use delphi_core::log::{replay, verify_chain, EventLog, LogOptions};
use serde_json::json;
use std::path::{Path, PathBuf};

/// `(release, events in its fixture)`.
const RELEASES: &[(&str, u64)] = &[("v0.5.5", 15)];

fn fixture(release: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/logs").join(release)
}

/// A scratch copy: the fixture itself is never opened for append.
fn scratch_copy(release: &str) -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    let dest = t.path().join("events");
    std::fs::create_dir_all(&dest).unwrap();
    for e in std::fs::read_dir(fixture(release).join("events")).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "jsonl") {
            std::fs::copy(e.path(), dest.join(e.file_name())).unwrap();
        }
    }
    t
}

#[test]
fn every_recorded_release_log_replays_verifies_and_can_be_continued() {
    for (release, count) in RELEASES {
        let t = scratch_copy(release);
        // Replays: contiguous from 1, a schema major this build understands.
        let events = replay(t.path()).unwrap_or_else(|e| panic!("{release}: this build cannot replay the release's log: {e}"));
        assert_eq!(events.len() as u64, *count, "{release}");
        assert!(events.iter().all(|e| e.v == delphi_core::event::SCHEMA_MAJOR), "{release}");
        // The hash chain written by that release verifies under this build's rules.
        let report = verify_chain(t.path()).unwrap();
        assert_eq!((report.broken.as_deref(), report.unchained, report.chained), (None, 0, *count), "{release}: {report:?}");
        // The daemon can reopen it and carry on: the next seq follows, and the chain still verifies.
        let mut log = EventLog::open(t.path(), LogOptions::default()).unwrap_or_else(|e| panic!("{release}: cannot reopen the release's log: {e}"));
        let mut e = EventEnvelope::new("m_fixture", "task.created", json!({"prompt": "after the update", "target": "orchestrator"}));
        e.task_id = Some("t_0003".into());
        assert_eq!(log.append(e).unwrap(), count + 1, "{release}");
        drop(log);
        let report = verify_chain(t.path()).unwrap();
        assert_eq!((report.broken.as_deref(), report.chained), (None, count + 1), "{release}: {report:?}");
        assert_eq!(replay(t.path()).unwrap().len() as u64, count + 1, "{release}");
    }
}

#[test]
fn an_unmodified_fixture_is_what_the_release_wrote_not_a_regenerated_one() {
    // The fixture is the evidence, so it must never be reopened for append in place (that would add events and rewrite its
    // tail). Opening for append works on a scratch copy only; the fixture directory holds the segment and nothing else.
    for (release, _) in RELEASES {
        let names: Vec<String> = std::fs::read_dir(fixture(release).join("events")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["000001.jsonl".to_string()], "{release}: a lock file or a second segment means something opened the fixture in place");
    }
}
