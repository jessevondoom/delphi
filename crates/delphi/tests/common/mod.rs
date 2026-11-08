//! Shared by the integration tests.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The test families a CI job can require to really run (spec 093 K2). A test that cannot run here (no Node, no
/// sandbox) calls `skip_or_fail` with its family; where CI requires the family that is a failure, not a quiet pass.
/// `tests/must_run.rs` checks that every family here is required by a CI job and that CI requires nothing else.
#[allow(dead_code)]
pub const MUST_RUN_FAMILIES: &[&str] = &["ui", "sandbox"];

/// Whether `family` is required, given `DELPHI_REQUIRE` (a comma-separated list) and the family's older single
/// switch (`DELPHI_REQUIRE_UI=1`). Pure, so it can be tested without touching the environment.
#[allow(dead_code)]
pub fn required_in(family: &str, list: Option<&str>, legacy: Option<&str>) -> bool {
    list.is_some_and(|l| l.split(',').any(|f| f.trim() == family)) || legacy.is_some_and(|v| !v.is_empty() && v != "0")
}

/// `required_in` for the real environment.
#[allow(dead_code)]
pub fn required(family: &str) -> bool {
    let legacy = std::env::var(format!("DELPHI_REQUIRE_{}", family.to_uppercase())).ok();
    required_in(family, std::env::var("DELPHI_REQUIRE").ok().as_deref(), legacy.as_deref())
}

/// A test cannot run on this machine: fail where the family is required, otherwise say so and let the caller return.
#[allow(dead_code)]
pub fn skip_or_fail(family: &str, why: &str) {
    assert!(!required(family), "the `{family}` tests are required to run here (DELPHI_REQUIRE={family}) but cannot: {why}");
    eprintln!("skipped: {why}");
}

/// Stops every daemon started under a test's home when the test ends, however it ends.
///
/// A daemon inherits the stdio of the process that started it, so one left behind by a failed test keeps its
/// pseudo-terminal open for good; after enough of them `openpty` fails and unrelated tests fail with it. Declare it
/// before the temp directory it watches, so it runs first (fields drop in order) and the directory still exists.
pub struct DaemonReaper(pub PathBuf);

impl DaemonReaper {
    pub fn new(home: &Path) -> DaemonReaper {
        DaemonReaper(home.to_path_buf())
    }
}

impl Drop for DaemonReaper {
    fn drop(&mut self) {
        let delphi = env!("CARGO_BIN_EXE_delphi");
        if let Ok(dirs) = std::fs::read_dir(self.0.join("workspaces")) {
            for d in dirs.flatten() {
                let name = d.file_name().to_string_lossy().to_string();
                let _ = Command::new(delphi).args(["daemon", "stop", "--workspace", &name, "--home"]).arg(&self.0).stdout(Stdio::null()).stderr(Stdio::null()).status();
            }
        }
        // Whatever did not stop politely: any daemon whose workspace is under this home.
        let _ = Command::new("pkill").arg("-f").arg(format!("__core --daemon --workspace {}/workspaces", self.0.display())).status();
    }
}
