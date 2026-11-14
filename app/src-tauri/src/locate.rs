//! Finding the daemon (spec 111 §3.2 item 1). The app asks the operator's own `delphi` where its sockets are and how roles
//! look (`delphi daemon where`, the command the CLI keeps for clients in other languages). A window started from the Finder has
//! no shell `PATH`, so the program is looked for in the places an install puts it. If it cannot be asked, the narrator socket of
//! the default workspace is computed the way the daemon computes it, and the window uses its built-in avatar table.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

pub struct Located {
    /// What `delphi daemon where` printed, or the little the app can say by itself.
    pub info: Value,
    pub narrator: PathBuf,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// The Delphi home: `DELPHI_HOME`, else `~/.delphi` (as `crates/delphi/src/home.rs` resolves it).
pub fn delphi_home() -> Option<PathBuf> {
    env_path("DELPHI_HOME").or_else(|| env_path("HOME").map(|h| h.join(".delphi")))
}

/// Where `delphi` may be: `DELPHI_EXE` first, then the standard install locations.
pub fn candidates(user_home: Option<&Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = env_path("DELPHI_EXE").into_iter().collect();
    if let Some(h) = user_home {
        v.push(h.join(".local/bin/delphi"));
    }
    v.push(PathBuf::from("/opt/homebrew/bin/delphi"));
    v.push(PathBuf::from("/usr/local/bin/delphi"));
    v
}

/// The narrator socket of a workspace, as `delphi_core::spawn::layout` names it: `<home>/run/<h>.narrator.sock`, `h` the first
/// twelve hex characters of the blake3 hash of the workspace's canonical path.
pub fn narrator_socket(home: &Path, workspace: &Path) -> Option<PathBuf> {
    let canon = std::fs::canonicalize(workspace).ok()?;
    let hash = blake3::hash(canon.as_os_str().as_encoded_bytes()).to_hex();
    Some(home.join("run").join(format!("{}.narrator.sock", &hash[..12])))
}

/// What `delphi daemon where` printed, when it is a JSON object naming a narrator socket. The output is data: nothing in it is run.
pub fn parse_where(stdout: &[u8]) -> Option<(Value, PathBuf)> {
    let v: Value = serde_json::from_slice(stdout).ok()?;
    let sock = PathBuf::from(v.get("narrator_socket")?.as_str()?);
    (v.is_object() && sock.is_absolute()).then_some((v, sock))
}

fn ask(exe: &Path, home: Option<&Path>, workspace: &str) -> Option<(Value, PathBuf)> {
    let mut cmd = Command::new(exe);
    cmd.args(["daemon", "where", "--workspace", workspace]);
    if let Some(h) = home {
        cmd.arg("--home").arg(h);
    }
    // No terminal, no input, and a directory that exists whatever started the app.
    let out = cmd.stdin(Stdio::null()).stderr(Stdio::null()).current_dir("/").output().ok()?;
    out.status.success().then(|| parse_where(&out.stdout)).flatten()
}

/// Find the daemon's narrator socket and the roles' look. `None` when there is no Delphi home to look in.
pub fn locate() -> Option<Located> {
    let user_home = env_path("HOME");
    let home = delphi_home()?;
    let workspace = std::env::var("DELPHI_WORKSPACE").ok().filter(|w| !w.is_empty()).unwrap_or_else(|| "default".into());
    let explicit_home = env_path("DELPHI_HOME");
    for exe in candidates(user_home.as_deref()) {
        if exe.is_file() {
            if let Some((info, narrator)) = ask(&exe, explicit_home.as_deref(), &workspace) {
                return Some(Located { info, narrator });
            }
        }
    }
    let ws_dir = home.join("workspaces").join(&workspace);
    let narrator = narrator_socket(&home, &ws_dir)?;
    Some(Located { info: json!({ "narrator_socket": narrator, "workspace": ws_dir, "workspace_name": workspace, "home": home, "roles": {} }), narrator })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_is_named_as_the_daemon_names_it() {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path().join("workspaces/default");
        std::fs::create_dir_all(&ws).unwrap();
        let sock = narrator_socket(t.path(), &ws).unwrap();
        let name = sock.file_name().unwrap().to_str().unwrap();
        let hash = blake3::hash(std::fs::canonicalize(&ws).unwrap().as_os_str().as_encoded_bytes()).to_hex();
        assert_eq!(name, format!("{}.narrator.sock", &hash[..12]));
        assert_eq!(name.len(), 12 + ".narrator.sock".len());
        assert_eq!(sock.parent().unwrap(), t.path().join("run"));
        // A workspace that does not exist has no canonical path, so there is nothing to compute.
        assert!(narrator_socket(t.path(), &t.path().join("nope")).is_none());
    }

    #[test]
    fn where_output_is_read_as_data_and_must_name_an_absolute_narrator_socket() {
        let (info, sock) = parse_where(br#"{"narrator_socket":"/h/run/abc.narrator.sock","roles":{"scout":{"name":"scout"}},"version":"0.6.1"}"#).unwrap();
        assert_eq!(sock, PathBuf::from("/h/run/abc.narrator.sock"));
        assert_eq!(info["version"], "0.6.1");
        assert!(parse_where(b"usage: delphi ...").is_none());
        assert!(parse_where(br#"{"console_socket":"/x"}"#).is_none());
        assert!(parse_where(br#"{"narrator_socket":"relative.sock"}"#).is_none());
        assert!(parse_where(br#"["/h/run/abc.narrator.sock"]"#).is_none());
    }

    #[test]
    fn delphi_is_looked_for_where_an_install_puts_it() {
        let c = candidates(Some(Path::new("/h")));
        assert!(c.contains(&PathBuf::from("/h/.local/bin/delphi")));
        assert!(c.contains(&PathBuf::from("/opt/homebrew/bin/delphi")));
        assert!(c.contains(&PathBuf::from("/usr/local/bin/delphi")));
    }
}
