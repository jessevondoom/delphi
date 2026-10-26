//! Added org bundles on this install (`specs/024` §4.1): the content-addressed cache and the workspace's `bundles.lock`.
//!
//! The cache lives at `$DELPHI_HOME/bundles/<org>/<name>/<bundle-hash>/`, outside every workspace zone. The lock is
//! daemon state, `<ws>/local/bundles.lock` (TOML, with history), and is never readable or writable by an agent
//! (`local/` is on the pathguard's deny lists). Agents reach a bundle's knowledge only as `@bundle/<org>/<name>/knowledge/**`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const LOCK_FILE: &str = "local/bundles.lock";
/// Hashes kept in the cache per bundle for rollback (`specs/024` §4.4).
pub const KEEP_HASHES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Lock {
    #[serde(default)]
    pub bundle: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub org: String,
    pub name: String,
    pub version: String,
    /// `blake3:<hex>` of the bundle's `files.json` (`specs/023` §3.4).
    pub hash: String,
    /// The pinned publisher key, or `unsigned` (`specs/024` §2.4).
    pub publisher_kid: String,
    /// `unsigned` until B6 verifies signatures (`specs/024` §2.2).
    pub provenance: String,
    pub source: Source,
    pub added_at: String,
    /// Overlay order among bundles; later wins on ties (`specs/024` §5.2).
    pub order: i64,
    /// `active` | `removed`. Staleness is computed from `review_by`, never stored (`specs/024` §4.6).
    pub status: String,
    pub review_by: String,
    pub title: String,
    /// `--skill-roles <skill>=<roles>` at add: the operator narrowed the roles a skill loads for (`specs/023` §5A).
    /// A skill with no entry loads for every role its `authorized_agents` names.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skill_roles: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub history: Vec<Past>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// `directory` | `archive` | `xenia` | `marketplace`; only `directory` exists so far.
    pub kind: String,
    pub path: String,
}

/// A hash this install has had for the bundle, newest first, for rollback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Past {
    pub hash: String,
    pub version: String,
    pub from: String,
    pub to: String,
}

impl Entry {
    pub fn id(&self) -> String {
        format!("{}/{}", self.org, self.name)
    }

    pub fn is_active(&self) -> bool {
        self.status == "active"
    }

    /// Past `review_by` the bundle is stale: it ranks lower and shows so, and nothing is unloaded (`specs/024` §4.6).
    pub fn is_stale(&self, today: &str) -> bool {
        !self.review_by.is_empty() && self.review_by.as_str() < today
    }

    /// `acme/acme-platform@1.4.0 · unsigned`, the breadcrumb's bundle label (`specs/024` §3.6).
    pub fn label(&self, today: &str) -> String {
        format!("{}@{} · {}{}", self.id(), self.version, self.provenance, if self.is_stale(today) { " · stale" } else { "" })
    }
}

/// Today's date, `YYYY-MM-DD` (UTC).
pub fn today() -> String {
    crate::time::now_rfc3339()[..10].to_string()
}

pub fn lock_path(ws: &Path) -> PathBuf {
    ws.join(LOCK_FILE)
}

/// `$DELPHI_HOME/bundles/<org>/<name>/<hash without the blake3: prefix>`.
pub fn cache_dir(home: &Path, org: &str, name: &str, hash: &str) -> PathBuf {
    home.join("bundles").join(org).join(name).join(hash.trim_start_matches("blake3:"))
}

/// A plain id segment: what an org or a bundle name may be. Anything else never reaches a path.
pub fn plain_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 48 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !s.starts_with('-')
}

/// The workspace's lock. A missing file is an empty lock; an unreadable one is an error, never silently empty.
pub fn read(ws: &Path) -> Result<Lock, String> {
    match std::fs::read_to_string(lock_path(ws)) {
        Ok(t) => toml::from_str(&t).map_err(|e| format!("{}: {}", lock_path(ws).display(), e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Lock::default()),
        Err(e) => Err(format!("{}: {}", lock_path(ws).display(), e)),
    }
}

/// Write the lock atomically.
pub fn write(ws: &Path, lock: &Lock) -> Result<(), String> {
    let text = toml::to_string_pretty(lock).map_err(|e| e.to_string())?;
    crate::lock::write_atomic(&lock_path(ws), text.as_bytes()).map_err(|e| format!("{}: {}", lock_path(ws).display(), e))
}

/// Read-modify-write the lock under an exclusive advisory lock, so two `bundle add`s cannot interleave.
pub fn update<T>(ws: &Path, f: impl FnOnce(&mut Lock) -> Result<T, String>) -> Result<T, String> {
    let guard = ws.join("local/bundles.lock.lck");
    crate::lock::with_lock(&guard, || {
        let mut lock = read(ws)?;
        let out = f(&mut lock)?;
        write(ws, &lock)?;
        Ok(out)
    })
    .map_err(|e| e.to_string())?
}

/// The active bundles of a workspace in overlay order: lowest `order` first, so that a later one wins on a tie of names
/// (`specs/024` §5.2); equal orders go by when they were added.
pub fn active_entries(ws: &Path) -> Vec<Entry> {
    let mut v: Vec<Entry> = read(ws).map(|l| l.bundle).unwrap_or_default().into_iter().filter(|e| e.is_active() && plain_id(&e.org) && plain_id(&e.name)).collect();
    v.sort_by(|a, b| a.order.cmp(&b.order).then(a.added_at.cmp(&b.added_at)));
    v
}

/// What the pathguard and the index need: (`org/name`, the active hash's `knowledge/` directory) for every active bundle.
/// A bundle whose id is not a plain id, or whose cache entry is missing, is left out.
pub fn active_roots(home: &Path, ws: &Path) -> Vec<(String, PathBuf)> {
    let Ok(lock) = read(ws) else { return Vec::new() };
    lock.bundle
        .iter()
        .filter(|e| e.is_active() && plain_id(&e.org) && plain_id(&e.name))
        .map(|e| (e.id(), cache_dir(home, &e.org, &e.name, &e.hash).join("knowledge")))
        .filter(|(_, p)| p.is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> Entry {
        Entry {
            org: "acme".into(),
            name: name.into(),
            version: "1.0.0".into(),
            hash: "blake3:ab".into(),
            publisher_kid: "unsigned".into(),
            provenance: "unsigned".into(),
            source: Source { kind: "directory".into(), path: "/x".into() },
            added_at: "2026-10-25T00:00:00Z".into(),
            order: 10,
            status: "active".into(),
            review_by: "2027-01-31".into(),
            title: "T".into(),
            skill_roles: BTreeMap::new(),
            history: vec![Past { hash: "blake3:aa".into(), version: "0.9.0".into(), from: "a".into(), to: "b".into() }],
        }
    }

    #[test]
    fn the_lock_round_trips_and_a_missing_one_is_empty_but_a_corrupt_one_is_an_error() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(read(t.path()).unwrap(), Lock::default());
        let lock = Lock { bundle: vec![entry("a"), entry("b")] };
        write(t.path(), &lock).unwrap();
        assert_eq!(read(t.path()).unwrap(), lock);
        std::fs::write(lock_path(t.path()), "bundle = 3 [").unwrap();
        assert!(read(t.path()).is_err());
    }

    #[test]
    fn update_is_read_modify_write_and_a_failing_closure_writes_nothing() {
        let t = tempfile::tempdir().unwrap();
        update(t.path(), |l| { l.bundle.push(entry("a")); Ok(()) }).unwrap();
        assert!(update(t.path(), |l| { l.bundle.push(entry("b")); Err::<(), _>("no".to_string()) }).is_err());
        assert_eq!(read(t.path()).unwrap().bundle.len(), 1);
    }

    #[test]
    fn staleness_is_computed_from_review_by_and_shown_in_the_label() {
        let e = entry("a");
        assert!(!e.is_stale("2027-01-31") && e.is_stale("2027-02-01"));
        assert_eq!(e.label("2026-10-25"), "acme/a@1.0.0 · unsigned");
        assert!(e.label("2028-01-01").ends_with("· stale"));
    }

    #[test]
    fn active_roots_lists_only_active_bundles_whose_cache_entry_exists() {
        let home = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let (mut gone, mut removed) = (entry("gone"), entry("removed"));
        removed.status = "removed".into();
        gone.hash = "blake3:ff".into();
        write(ws.path(), &Lock { bundle: vec![entry("a"), gone, removed.clone()] }).unwrap();
        std::fs::create_dir_all(cache_dir(home.path(), "acme", "a", "blake3:ab").join("knowledge")).unwrap();
        std::fs::create_dir_all(cache_dir(home.path(), "acme", "removed", "blake3:ab").join("knowledge")).unwrap();
        let roots = active_roots(home.path(), ws.path());
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].0, "acme/a");
        assert!(plain_id("acme-platform") && !plain_id("../x") && !plain_id("A") && !plain_id(""));
    }
}
