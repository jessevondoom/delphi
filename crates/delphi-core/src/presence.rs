//! The presence file (`specs/130` §2): `<home>/run/<h>.json`, written by a daemon once it is ready and removed on a clean shutdown.
//!
//! It is data a reader never trusts: a crash leaves it behind, so a reader confirms the daemon over its socket and compares the
//! instance. It carries no token, key or ticket. A reader never deletes one; the daemon's own restart overwrites it.

use crate::spawn::Layout;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub const VERSION: u32 = 1;
/// The largest presence file a reader looks at.
pub const MAX_BYTES: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presence {
    pub version: u32,
    pub instance: String,
    pub pid: u32,
    pub binary_version: String,
    pub workspace: String,
    pub workspace_name: String,
    pub home: String,
    pub started: String,
    pub narrator_socket: String,
    pub app_socket: String,
    /// The newest stable release the daemon found (`1.2.3`), when it found one newer than itself (`specs/120` §13.2 item 12). An
    /// optional field: `version` stays 1, a reader that does not know it ignores it, and a file without it means no release is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_available: Option<String>,
    /// Set after an update was applied: this daemon still runs the old code until it is restarted. Cleared by the restart, which writes a
    /// new file.
    #[serde(default, skip_serializing_if = "is_false")]
    pub restart_pending: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Presence {
    /// The record for this process, from what the daemon already knows.
    pub fn new(layout: &Layout, instance: &str, binary_version: &str, workspace: &Path, home: &Path) -> Presence {
        Presence {
            version: VERSION,
            instance: instance.to_string(),
            pid: std::process::id(),
            binary_version: binary_version.to_string(),
            workspace: workspace.to_string_lossy().into_owned(),
            workspace_name: workspace.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            home: home.to_string_lossy().into_owned(),
            started: crate::time::now_rfc3339(),
            narrator_socket: layout.narrator_sock.to_string_lossy().into_owned(),
            app_socket: layout.app_sock.to_string_lossy().into_owned(),
            update_available: None,
            restart_pending: false,
        }
    }
}

/// Write the file: a new file readable by the operator only, in the run folder, moved into place.
pub fn write(layout: &Layout, p: &Presence) -> std::io::Result<()> {
    let tmp = layout.presence.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut text = serde_json::to_string_pretty(p).map_err(std::io::Error::other)?;
    text.push('\n');
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    drop(f);
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&tmp, &layout.presence)
}

/// Remove the file (a clean shutdown). A missing file is not an error.
pub fn remove(layout: &Layout) {
    let _ = std::fs::remove_file(&layout.presence);
}

/// Read a presence file as data: at most 4 KiB, a regular file (not a symbolic link) owned by this user with no group or other
/// bits, and `version` 1. Anything else is `None`.
pub fn read(path: &Path) -> Option<Presence> {
    let mut f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path).ok()?;
    let meta = f.metadata().ok()?;
    // SAFETY: geteuid has no preconditions.
    if !meta.file_type().is_file() || meta.len() > MAX_BYTES || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        return None;
    }
    let mut text = String::new();
    (&mut f).take(MAX_BYTES + 1).read_to_string(&mut text).ok()?;
    let p: Presence = serde_json::from_str(&text).ok()?;
    (p.version == VERSION).then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Layout) {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("work");
        std::fs::create_dir_all(&ws).unwrap();
        let home = dir.path().join("home");
        let l = crate::spawn::layout(&home, &ws).unwrap();
        crate::spawn::ensure_run_dir(&l.run_dir).unwrap();
        (dir, l)
    }

    fn sample(l: &Layout) -> Presence {
        Presence::new(l, "0123456789abcdef0123456789abcdef", "0.6.5", Path::new("/w/default"), Path::new("/h"))
    }

    #[test]
    fn written_private_with_the_spec_fields_and_removed() {
        let (_d, l) = fixture();
        let p = sample(&l);
        write(&l, &p).unwrap();
        assert!(l.presence.to_string_lossy().ends_with(".json"));
        assert_eq!(std::fs::metadata(&l.presence).unwrap().permissions().mode() & 0o777, 0o600);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&l.presence).unwrap()).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["app_socket", "binary_version", "home", "instance", "narrator_socket", "pid", "started", "version", "workspace", "workspace_name"]);
        assert_eq!((v["version"].as_u64(), v["workspace_name"].as_str(), v["pid"].as_u64()), (Some(1), Some("default"), Some(std::process::id() as u64)));
        assert!(crate::time::parse_rfc3339(v["started"].as_str().unwrap()).is_some());
        assert!(v["narrator_socket"].as_str().unwrap().ends_with(".narrator.sock") && v["app_socket"].as_str().unwrap().ends_with(".app.sock"));
        assert!(!l.presence.with_extension("json.tmp").exists(), "no temporary file is left");
        assert_eq!(read(&l.presence), Some(p));
        remove(&l);
        assert!(!l.presence.exists());
        remove(&l);
    }

    /// `specs/120` §13.2 item 12: the two update fields are optional. Absent they are not written (the file is what it was), present they
    /// are read back, a reader of the older shape ignores them, and the file is still `version` 1.
    #[test]
    fn the_update_fields_are_optional_and_ignored_by_a_reader_that_does_not_know_them() {
        let (_d, l) = fixture();
        let mut p = sample(&l);
        write(&l, &p).unwrap();
        let text = std::fs::read_to_string(&l.presence).unwrap();
        assert!(!text.contains("update_available") && !text.contains("restart_pending"), "nothing known, nothing written");
        p.update_available = Some("0.6.6".into());
        p.restart_pending = true;
        write(&l, &p).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&l.presence).unwrap()).unwrap();
        assert_eq!((v["version"].as_u64(), v["update_available"].as_str(), v["restart_pending"].as_bool()), (Some(1), Some("0.6.6"), Some(true)));
        assert_eq!(read(&l.presence), Some(p.clone()));
        // A reader of the file as it was before: it names only the fields it knew, and the extra keys do not stop it.
        #[derive(serde::Deserialize)]
        struct Older {
            version: u32,
            instance: String,
        }
        let older: Older = serde_json::from_str(&std::fs::read_to_string(&l.presence).unwrap()).unwrap();
        assert_eq!((older.version, older.instance.len()), (1, 32));
        // A file written without the fields reads as none known.
        let mut stripped = v.clone();
        stripped.as_object_mut().unwrap().remove("update_available");
        stripped.as_object_mut().unwrap().remove("restart_pending");
        let back: Presence = serde_json::from_value(stripped).unwrap();
        assert_eq!((back.update_available, back.restart_pending), (None, false));
        for k in v.as_object().unwrap().keys() {
            for bad in ["token", "secret", "ticket", "key"] {
                assert!(!k.to_lowercase().contains(bad), "{k} looks like a secret");
            }
        }
    }

    #[test]
    fn no_key_names_a_token_a_secret_a_ticket_or_a_key() {
        let (_d, l) = fixture();
        write(&l, &sample(&l)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&l.presence).unwrap()).unwrap();
        for k in v.as_object().unwrap().keys() {
            for bad in ["token", "secret", "ticket", "key"] {
                assert!(!k.to_lowercase().contains(bad), "{k} looks like a secret");
            }
        }
    }

    #[test]
    fn a_restart_overwrites_a_stale_file() {
        let (_d, l) = fixture();
        let mut old = sample(&l);
        old.pid = 1;
        write(&l, &old).unwrap();
        let new = Presence { instance: "f".repeat(32), ..sample(&l) };
        write(&l, &new).unwrap();
        assert_eq!(read(&l.presence).unwrap().instance, "f".repeat(32));
    }

    #[test]
    fn the_reader_refuses_what_it_should_not_trust() {
        let (d, l) = fixture();
        let good = sample(&l);
        write(&l, &good).unwrap();
        let text = std::fs::read_to_string(&l.presence).unwrap();
        // a mode with a group or other bit
        std::fs::set_permissions(&l.presence, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(read(&l.presence), None);
        std::fs::set_permissions(&l.presence, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read(&l.presence).is_some());
        // a symbolic link
        let link = d.path().join("link.json");
        std::os::unix::fs::symlink(&l.presence, &link).unwrap();
        assert_eq!(read(&link), None);
        // not JSON, too large, another version, a missing file, a folder
        let other = d.path().join("o.json");
        let put = |s: &str| {
            let _ = std::fs::remove_file(&other);
            std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&other).unwrap().write_all(s.as_bytes()).unwrap();
        };
        put("not json");
        assert_eq!(read(&other), None);
        put(&format!("{}{}", text, " ".repeat(5000)));
        assert_eq!(read(&other), None);
        put(&text.replace("\"version\": 1", "\"version\": 2"));
        assert_eq!(read(&other), None);
        assert_eq!(read(&d.path().join("none.json")), None);
        assert_eq!(read(d.path()), None);
    }
}
