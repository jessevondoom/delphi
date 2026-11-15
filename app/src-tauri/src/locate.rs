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

/// The app socket of a workspace (spec 120 §3 item 1), named as the daemon names it: `<home>/run/<h>.app.sock`, beside the
/// narrator's and with the same `h`.
pub fn app_socket(home: &Path, workspace: &Path) -> Option<PathBuf> {
    let narrator = narrator_socket(home, workspace)?;
    let name = narrator.file_name()?.to_str()?.strip_suffix(".narrator.sock")?.to_string();
    Some(narrator.with_file_name(format!("{name}.app.sock")))
}

/// The app socket `delphi daemon where` named (`app_socket`), when it is an absolute path. A `delphi` from before spec 120
/// names none, and the app then stays on the narrator socket.
pub fn app_socket_in(info: &Value) -> Option<PathBuf> {
    info["app_socket"].as_str().map(PathBuf::from).filter(|p| p.is_absolute())
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
    let app = app_socket(&home, &ws_dir);
    Some(Located { info: json!({ "narrator_socket": narrator, "app_socket": app, "workspace": ws_dir, "workspace_name": workspace, "home": home, "roles": {} }), narrator })
}

/// A document's place on disk: `rel` (a path the daemon gave, relative to the workspace) under `root`, only when it names a
/// file that really is inside `root` once links are followed. Anything absolute, with `..`, or leading outside is refused.
pub fn inside(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return None;
    }
    let root = std::fs::canonicalize(root).ok()?;
    let full = std::fs::canonicalize(root.join(rel)).ok()?;
    (full.starts_with(&root) && full.is_file()).then_some(full)
}

/// The kinds of file the app will hand to the system to open (spec 111 §2.6): a page or a deck, a document to read, a picture,
/// a recording. Never a program, a script or an archive.
pub const OPENABLE: [&str; 18] = ["html", "htm", "pdf", "png", "jpg", "jpeg", "gif", "webp", "svg", "mp4", "mov", "m4v", "webm", "mp3", "wav", "m4a", "md", "txt"];

pub fn openable(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).is_some_and(|e| OPENABLE.contains(&e.as_str()))
}

/// The folders a path the daemon or an agent named may be looked for in: the workspace, then each registered project's root
/// (`projects` in what `delphi daemon where` printed). Only absolute folders that exist.
pub fn roots(info: &Value) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = info["workspace"].as_str().map(PathBuf::from).into_iter().collect();
    out.extend(info["projects"].as_array().into_iter().flatten().filter_map(|p| p["root"].as_str().map(PathBuf::from)));
    out.retain(|p| p.is_absolute() && p.is_dir());
    out.dedup();
    out
}

/// The first of `roots` that holds `rel` as a file inside it.
pub fn find(roots: &[PathBuf], rel: &str) -> Option<PathBuf> {
    roots.iter().find_map(|r| inside(r, rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_file_is_looked_for_in_the_workspace_then_in_registered_projects_and_only_some_kinds_open() {
        let t = tempfile::tempdir().unwrap();
        let (ws, proj) = (t.path().join("ws"), t.path().join("proj"));
        std::fs::create_dir_all(ws.join("missions")).unwrap();
        std::fs::create_dir_all(proj.join("decks")).unwrap();
        std::fs::write(ws.join("missions/report.md"), "x").unwrap();
        std::fs::write(proj.join("decks/pitch.html"), "x").unwrap();
        std::fs::write(proj.join("run.sh"), "x").unwrap();
        let info = json!({"workspace": ws, "projects": [{"name": "default", "root": ws, "home": true}, {"name": "p", "root": proj, "home": false}, {"name": "gone", "root": t.path().join("nope")}, {"name": "rel", "root": "relative"}]});
        let roots = roots(&info);
        assert_eq!(roots.len(), 2, "{roots:?}");
        assert!(find(&roots, "missions/report.md").unwrap().starts_with(std::fs::canonicalize(&ws).unwrap()));
        assert!(find(&roots, "decks/pitch.html").unwrap().ends_with("decks/pitch.html"));
        assert!(find(&roots, "../proj/run.sh").is_none());
        assert!(find(&roots, "decks/none.html").is_none());
        for (name, yes) in [("a.html", true), ("a.HTM", true), ("a.pdf", true), ("a.PNG", true), ("a.mp4", true), ("a.m4a", true), ("a.md", true), ("a.sh", false), ("a.app", false), ("a.command", false), ("a.zip", false), ("a.py", false), ("a", false), ("a.html.sh", false)] {
            assert_eq!(openable(Path::new(name)), yes, "{name}");
        }
    }

    #[test]
    fn only_a_file_inside_the_workspace_can_be_shown() {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path().join("ws");
        std::fs::create_dir_all(ws.join("missions/m")).unwrap();
        std::fs::write(ws.join("missions/m/report.md"), "x").unwrap();
        std::fs::write(t.path().join("outside.md"), "x").unwrap();
        std::os::unix::fs::symlink(t.path().join("outside.md"), ws.join("link.md")).unwrap();
        assert!(inside(&ws, "missions/m/report.md").unwrap().ends_with("missions/m/report.md"));
        for bad in ["../outside.md", "missions/../../outside.md", "/etc/hosts", "", "missions/m", "missions/m/none.md", "link.md", "./missions/m/report.md"] {
            assert!(inside(&ws, bad).is_none(), "{bad}");
        }
    }

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
        // The app socket is beside it, with the same hash (spec 120 §3 item 1).
        assert_eq!(app_socket(t.path(), &ws).unwrap(), t.path().join("run").join(format!("{}.app.sock", &hash[..12])));
        assert!(app_socket(t.path(), &t.path().join("nope")).is_none());
        // What `delphi daemon where` names is used only when it is an absolute path; an older `delphi` names none.
        assert_eq!(app_socket_in(&json!({"app_socket": "/h/run/abc.app.sock"})), Some(PathBuf::from("/h/run/abc.app.sock")));
        assert_eq!(app_socket_in(&json!({"app_socket": "run/abc.app.sock"})), None);
        assert_eq!(app_socket_in(&json!({"narrator_socket": "/h/run/abc.narrator.sock"})), None);
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
