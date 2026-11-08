//! The observer (what the console and narrator show) can fold a log written by an earlier release (spec 093 B2-E). The log itself
//! is checked in `delphi-core/tests/previous_release_log.rs`; this reads the same fixtures and feeds every event through the
//! engine's own fold, so a change to an event kind or payload field that the fold depends on shows up as a failure here.

use delphi_engine::observe::ObserverState;
use std::path::{Path, PathBuf};

const RELEASES: &[(&str, u64)] = &[("v0.5.5", 15)];

fn fixture(release: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../delphi-core/tests/fixtures/logs").join(release)
}

#[test]
fn the_observer_folds_a_release_log_to_its_last_event() {
    for (release, count) in RELEASES {
        let t = tempfile::tempdir().unwrap();
        let dest = t.path().join("events");
        std::fs::create_dir_all(&dest).unwrap();
        for e in std::fs::read_dir(fixture(release).join("events")).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "jsonl") {
                std::fs::copy(e.path(), dest.join(e.file_name())).unwrap();
            }
        }
        let events = delphi_core::log::replay(t.path()).unwrap();
        let mut st = ObserverState::new(&["orchestrator".to_string(), "architect".to_string()]);
        for e in &events {
            st.apply(&serde_json::to_value(e).unwrap());
        }
        assert_eq!(st.last_seq, *count, "{release}: the fold reached the last event");
        assert!(st.agents.contains_key("orchestrator"), "{release}: the roster survives the fold");
    }
}
