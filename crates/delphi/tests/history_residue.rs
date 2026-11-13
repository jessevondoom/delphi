//! Spec 102 S7: the surfaces an operator can reach that might carry a record's text out of the store (a debug bundle, `doctor`,
//! `status`, `logs`, the error log), checked BEFORE a purge with the records present, and the whole home scanned AFTER it.
//! Obviously fake text in a temporary home (`--home`); nothing here touches a real `~/.delphi`.

use delphi_core::event::EventEnvelope;
use delphi_engine::history_record::{Settings, Tap};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");
const CANARY: &str = "ZXQ-CLI-RESIDUE-8812";

fn delphi(home: &Path, args: &[&str]) -> Output {
    Command::new(DELPHI).args(args).arg("--workspace").arg("w").arg("--home").arg(home).env_remove("DELPHI_HOME").env_remove("DELPHI_BRIDGE_SOCKET").env_remove("DELPHI_BRIDGE_TOKEN").env("DELPHI_LOG", "debug").stdin(Stdio::null()).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let proj = t.path().join("proj");
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    let init = Command::new(DELPHI).args(["init", "--non-interactive", "--workspace", "w", "--project"]).arg(&proj).arg("--home").arg(&home).env_remove("DELPHI_HOME").stdin(Stdio::null()).output().unwrap();
    assert!(init.status.success(), "{}", text(&init));
    let ws = home.join("workspaces/w");
    // The install question has been answered yes, as it is on a real install after its first terminal start (`specs/102` S8): a record
    // is written only after that. `init --non-interactive` above did not ask, which is the behaviour under test elsewhere.
    std::fs::create_dir_all(ws.join("local")).unwrap();
    std::fs::write(ws.join("local/history.consent"), "yes\n").unwrap();
    let tap = Tap::new(ws.clone(), Settings { enabled: true, ..Settings::default() }, "home");
    for i in 0..2 {
        tap.note_operator_prompt("t_1", &format!("{CANARY} prompt number {i}"));
        let wrote = tap.on_events("m1", &[EventEnvelope::new("m1", "research.finished", json!({"topic": "queues", "new": 1, "updated": 0, "held": 0, "outcome": "done"}))]);
        assert_eq!(wrote.len(), 1);
    }
    (t, home, ws)
}

fn holders(root: &Path, needle: &str) -> Vec<String> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter(|e| String::from_utf8_lossy(&std::fs::read(e.path()).unwrap_or_default()).contains(needle))
        .map(|e| e.path().display().to_string())
        .collect()
}

#[test]
fn no_operator_surface_prints_or_packs_record_text_and_a_purge_leaves_nothing_under_the_home() {
    let (t, home, ws) = fixture();
    assert!(!holders(&ws, CANARY).is_empty(), "the canary reached the store");
    // Read-only commands with the records present: none of them shows a record's words.
    let mut seen = String::new();
    for args in [&["history", "status"][..], &["doctor"], &["logs"]] {
        seen.push_str(&text(&delphi(&home, args)));
    }
    let bundle = t.path().join("out.tgz");
    let b = delphi(&home, &["debug", "bundle", "-o", bundle.to_str().unwrap()]);
    seen.push_str(&text(&b));
    assert!(!seen.contains(CANARY), "an operator surface printed record text: {seen}");
    assert!(bundle.exists(), "the bundle was not written: {}", text(&b));
    {
        let un = t.path().join("unpacked");
        std::fs::create_dir_all(&un).unwrap();
        let x = Command::new("tar").arg("-xzf").arg(&bundle).arg("-C").arg(&un).output().unwrap();
        assert!(x.status.success(), "{}", text(&x));
        assert!(holders(&un, CANARY).is_empty(), "the debug bundle holds record text: {:?}", holders(&un, CANARY));
    }
    // Purge, then the WHOLE scratch tree (the home, the project, the bundle) is scanned, hidden files included.
    let o = delphi(&home, &["history", "purge", "--yes"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("erased 2 record(s)"), "{}", text(&o));
    // The commands above ran again after the purge; they leave a log, which must not hold the text either.
    for args in [&["history", "status"][..], &["doctor"], &["logs"]] {
        let _ = delphi(&home, args);
    }
    let left = holders(t.path(), CANARY);
    assert!(left.is_empty(), "text survived the purge in {left:?}");
    for plain in ["prompt number 0", "prompt number 1"] {
        assert!(holders(t.path(), plain).is_empty(), "{plain:?} survived in {:?}", holders(t.path(), plain));
    }
}
