//! The operator's own marks on the encyclopedia (spec 117 §5): which page versions were read, notes on passages, pinned
//! topics. Kept by the app in its own folder, `<Delphi home>/app/marks.json`, for the operator alone (mode 0600 in a
//! folder of mode 0700, beside the key file). **Never sent to the daemon**: nothing in this file is given to a `Client`,
//! and no daemon method takes it; an agent has no way to read it but the file system the operator's own programs share.
//!
//! The window decides what the marks mean (`app/src/marks.ts`); this keeps and returns one JSON object, and refuses
//! anything that is not one, is too large, or would be written through a link. An exported page (§5 item 4) is written
//! beside it, under `app/exports/`, by a name made only of letters, digits, `-`, `_` and `.`.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The most the marks file may hold. Notes are short; a window that asks to keep more is refused.
pub const MARKS_MAX: usize = 4 * 1024 * 1024;
/// The most one exported page may hold.
pub const EXPORT_MAX: usize = 8 * 1024 * 1024;

pub fn path(home: &Path) -> PathBuf {
    home.join("app").join("marks.json")
}

fn plain_file(p: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_file() => Ok(true),
        Ok(_) => Err(format!("{} is not a plain file; it is left alone", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{} could not be read: {e}", p.display())),
    }
}

/// The marks as kept: an empty object when there is no file yet. Reading never creates the file or its folder.
pub fn load(home: &Path) -> Result<Value, String> {
    let p = path(home);
    if !plain_file(&p)? {
        return Ok(Value::Object(Default::default()));
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("the marks could not be read: {e}"))?;
    if bytes.len() > MARKS_MAX {
        return Err("the marks file is larger than this app keeps; it is left alone".into());
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(v @ Value::Object(_)) => Ok(v),
        _ => Err("the marks file is not what this app wrote; it is left alone".into()),
    }
}

/// A folder of mode 0700 for the operator alone, made if it is not there; a link in its place is refused.
fn own_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let io = |e: std::io::Error| format!("{} could not be made: {e}", dir.display());
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Err(format!("{} is not a folder; nothing is written there", dir.display())),
        Err(_) => std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(io)?,
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(io)
}

/// Write `bytes` to `to` for the operator alone: a new file beside it, then a rename, so a reader never sees half of it.
fn write_own(to: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let io = |e: std::io::Error| format!("{} could not be written: {e}", to.display());
    plain_file(to)?;
    let tmp = to.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(io)?;
    f.write_all(bytes).map_err(io)?;
    f.sync_all().map_err(io)?;
    drop(f);
    std::fs::rename(&tmp, to).map_err(io)
}

/// Keep the marks. Only a JSON object, and only up to [`MARKS_MAX`].
pub fn save(home: &Path, marks: &Value) -> Result<(), String> {
    if !marks.is_object() {
        return Err("the marks are one JSON object".into());
    }
    let bytes = serde_json::to_vec(marks).map_err(|e| e.to_string())?;
    if bytes.len() > MARKS_MAX {
        return Err("that is more than this app keeps as marks".into());
    }
    let p = path(home);
    own_dir(p.parent().ok_or("the marks file has no folder")?)?;
    write_own(&p, &bytes)
}

/// A name for an exported page: letters, digits, `-`, `_` and `.`, not hidden, and never a path.
pub fn export_name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 100 && !name.starts_with('.') && !name.contains("..") && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Write an exported page (spec 117 §5 item 4) to `<home>/app/exports/<name>.md` and say where. The text must carry the
/// generated-page banner: a page is exported with it or not at all.
pub fn export(home: &Path, name: &str, text: &str) -> Result<PathBuf, String> {
    if !export_name_ok(name) {
        return Err("that is not a name an exported page is given".into());
    }
    if text.len() > EXPORT_MAX {
        return Err("that page is larger than this app exports".into());
    }
    if !text.contains("Generated page. Not authoritative.") {
        return Err("an exported page carries its \"Generated page. Not authoritative.\" banner; this text has none".into());
    }
    let dir = home.join("app").join("exports");
    own_dir(home.join("app").as_path())?;
    own_dir(&dir)?;
    let to = dir.join(format!("{name}.md"));
    write_own(&to, text.as_bytes())?;
    Ok(to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn marks_are_kept_for_the_operator_alone_and_read_back_and_reading_creates_nothing() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        assert_eq!(load(&home).unwrap(), json!({}));
        assert!(!home.exists(), "reading the marks makes no file and no folder");

        let marks = json!({"v": 1, "read": {"tides": {"version": 2}}, "pins": ["tides"], "notes": []});
        save(&home, &marks).unwrap();
        assert_eq!(load(&home).unwrap(), marks);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!((mode(&path(&home)), mode(&home.join("app"))), (0o600, 0o700));
        // Kept again: the file is replaced, and no temporary file is left.
        save(&home, &json!({"v": 1})).unwrap();
        assert_eq!(load(&home).unwrap(), json!({"v": 1}));
        let left: Vec<String> = std::fs::read_dir(home.join("app")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left, ["marks.json"]);
    }

    #[test]
    fn only_an_object_of_a_bounded_size_is_kept_and_a_file_this_app_did_not_write_is_left_alone() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        for not in [json!([1, 2]), json!("text"), json!(null), json!(7)] {
            assert!(save(&home, &not).is_err(), "{not}");
        }
        assert!(save(&home, &json!({"notes": "x".repeat(MARKS_MAX)})).is_err());
        assert!(!home.exists(), "a refused save writes nothing");

        // Something else's file where the marks would be: not parsed as marks, not written over through a link.
        std::fs::create_dir_all(home.join("app")).unwrap();
        std::fs::write(path(&home), "not json").unwrap();
        assert!(load(&home).unwrap_err().contains("not what this app wrote"));
        std::fs::remove_file(path(&home)).unwrap();
        let elsewhere = t.path().join("elsewhere.json");
        std::fs::write(&elsewhere, "{}").unwrap();
        std::os::unix::fs::symlink(&elsewhere, path(&home)).unwrap();
        assert!(load(&home).is_err() && save(&home, &json!({"v": 1})).is_err());
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "{}", "a link is never written through");
    }

    #[test]
    fn an_exported_page_keeps_its_banner_and_goes_only_under_the_apps_own_folder() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let page = "Generated page. Not authoritative. Written by the writer role on 2026-11-15 from the sources listed below.\n\n# Tides\n";
        let to = export(&home, "tides-v2", page).unwrap();
        assert_eq!(to, home.join("app/exports/tides-v2.md"));
        assert_eq!(std::fs::read_to_string(&to).unwrap(), page);
        assert_eq!(std::fs::metadata(&to).unwrap().permissions().mode() & 0o777, 0o600);
        // No banner, no export.
        assert!(export(&home, "tides", "# Tides\n\nJust text.").unwrap_err().contains("banner"));
        // A name is never a path.
        for bad in ["", "../x", "a/b", ".hidden", "x y", "a\\b", "..", &"n".repeat(101)] {
            assert!(export(&home, bad, page).is_err(), "{bad}");
        }
        assert_eq!(std::fs::read_dir(home.join("app/exports")).unwrap().count(), 1);
    }
}
