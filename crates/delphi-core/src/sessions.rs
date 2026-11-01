//! Where a session's files live on disk (spec 058-A9).
//!
//! New sessions are written under `local/sessions/<id>/`. Sessions written before that change stay
//! under `local/missions/<id>/` and are read, and kept written, where they are: nothing is moved or
//! copied. An id that exists in both places resolves to the old directory, so no log is ever split.

use std::path::{Path, PathBuf};

pub const CURRENT: &str = "local/sessions";
pub const LEGACY: &str = "local/missions";

/// Both session roots, the current one first.
pub fn roots(ws: &Path) -> [PathBuf; 2] {
    [ws.join(CURRENT), ws.join(LEGACY)]
}

/// The directory of session `id`: the existing legacy one, otherwise the current-format path (which
/// may not exist yet). `id` must already be a plain name; callers validate it.
pub fn dir(ws: &Path, id: &str) -> PathBuf {
    let old = ws.join(LEGACY).join(id);
    if old.exists() {
        return old;
    }
    ws.join(CURRENT).join(id)
}

/// Every session directory in either root, each id once (the legacy copy wins, as in [`dir`]).
pub fn dirs(ws: &Path) -> Vec<(String, PathBuf)> {
    let mut seen = std::collections::BTreeMap::new();
    for root in roots(ws).into_iter().rev() {
        for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                seen.insert(e.file_name().to_string_lossy().to_string(), e.path());
            }
        }
    }
    seen.into_iter().collect()
}

/// The current-format root, created when missing.
pub fn ensure_root(ws: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(ws.join(CURRENT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_go_to_sessions_and_old_ones_stay_where_they_are() {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path();
        std::fs::create_dir_all(ws.join("local/missions/old")).unwrap();
        std::fs::create_dir_all(ws.join("local/sessions/new")).unwrap();
        assert_eq!(dir(ws, "old"), ws.join("local/missions/old"));
        assert_eq!(dir(ws, "new"), ws.join("local/sessions/new"));
        assert_eq!(dir(ws, "fresh"), ws.join("local/sessions/fresh"));
        std::fs::create_dir_all(ws.join("local/sessions/old")).unwrap();
        assert_eq!(dir(ws, "old"), ws.join("local/missions/old"), "an id in both places resolves to the old log");
        let ids: Vec<String> = dirs(ws).into_iter().map(|(i, _)| i).collect();
        assert_eq!(ids, ["new", "old"]);
    }
}
