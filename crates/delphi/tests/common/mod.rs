//! Shared by the integration tests.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
