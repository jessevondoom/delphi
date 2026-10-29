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
    /// Origins the operator deemed trusted sources (`specs/024` §3.8).
    #[serde(default, rename = "trusted_source", skip_serializing_if = "Vec::is_empty")]
    pub trusted_source: Vec<TrustedSource>,
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
    /// `unsigned`, `signed` or `known-publisher` for a directory or archive bundle; `endorsed` or `member-signed` for a
    /// store bundle (`specs/024` §2.2).
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
    /// The MCP servers the bundle declares (`.mcp.json`), recorded **inert** (`specs/025` §4.1): nothing here is connected,
    /// listed, launched or offered to a role. Replaced with the bundle's declarations on every add and update.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inert_servers: Vec<InertServer>,
    /// The operator's raise of this bundle's knowledge tier (`specs/024` §3.7): `user_ratified`, or empty for the default
    /// (`federated_verified`). INSTALL-OWNED: it lives only here, in `local/bundles.lock`, never in the bundle tree or
    /// anything a bundle ships, and no manifest or frontmatter key can set it. It counts only while `trust_hash` and
    /// `trust_kid` are still the bundle's `hash` and `publisher_kid` (see [`Entry::raised`]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trust: String,
    /// The bundle hash and publisher key the operator raised the tier for.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trust_hash: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trust_kid: String,
    /// Every raise, lowering and reset, newest first, so that the lock history says what happened to the tier.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trust_log: Vec<TrustChange>,
}

/// One change to a bundle's raised tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustChange {
    pub at: String,
    /// `user_ratified` (raised) or `default` (lowered or reset).
    pub level: String,
    pub hash: String,
    /// `at-add`, `cli`, `console`, `trusted-source`, `update`, `rollback`, `remove`, `source-revoked`.
    pub by: String,
}

/// An origin the operator deemed a trusted source (`specs/024` §3.8): later versions fetched from this same origin and
/// signed by this same publisher key (or a successor the store's roster names) keep the bundle validated at `level`
/// without a new confirmation. Install-owned, in the lock, per origin and publisher key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedSource {
    /// [`origin_of`]: `xenia:<org>/<store>` or `<kind>:<path>`.
    pub origin: String,
    pub kid: String,
    /// `user_ratified` or `default` (citable as validated, the tier every bundle has).
    pub level: String,
    pub granted_at: String,
    /// Bundles whose tier this origin raised, for the log: `org/name`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applied_to: Vec<String>,
}

/// The identity of where a bundle is fetched from: a store is `xenia:<org>/<store>` (not the record or revision), anything
/// else is its kind and the recorded path or repository URL.
pub fn origin_of(s: &Source) -> String {
    if s.kind == "xenia" {
        let rest = s.path.strip_prefix("xenia:").unwrap_or(&s.path);
        let mut it = rest.split('/');
        return format!("xenia:{}/{}", it.next().unwrap_or(""), it.next().unwrap_or(""));
    }
    // A marketplace source is recorded as `<repo>#<name>@<commit>`: the commit changes with every version and is not the origin.
    let path = if s.kind == "marketplace" { s.path.split('#').next().unwrap_or(&s.path) } else { s.path.as_str() };
    format!("{}:{}", s.kind, path)
}

/// The publisher key a source's `origin` records, `None` for an unsigned bundle.
pub fn origin_kid(publisher_kid: &str) -> Option<String> {
    (publisher_kid != "unsigned" && !publisher_kid.is_empty()).then(|| publisher_kid.to_string())
}

/// Who vouches for a bundle's content (`specs/024` §4.1a, `specs/052` §3.1): the org of an **active lock entry** that
/// still holds the bundle hash and publisher key the content came in with. The answer comes only from the install-owned
/// lock, never from a frontmatter field, a `meta.json` or any file a bundle ships. `bundle` is `org/name`.
pub fn vouched_org<'a>(entries: &'a [Entry], bundle: &str, hash: &str, kid: Option<&str>) -> Option<&'a str> {
    entries.iter().find(|e| e.is_active() && e.id() == bundle && e.hash == hash && e.publisher_kid == kid.unwrap_or("unsigned")).map(|e| e.org.as_str())
}

/// The raised level's name, as written in the lock and on the command line.
pub const RATIFIED: &str = "user_ratified";
/// The tier every added bundle's knowledge has unless the operator raises it.
pub const DEFAULT_TIER: &str = "federated_verified";
/// What a bundle's documents are called wherever they are cited or their trust is described to a model.
pub const VALIDATED_LABEL: &str = "validated bundle source";

/// One declared server, as far as the lock keeps it: its name and transport, the `url` of an `http` one and the `command`
/// of a `stdio` one (shown to the operator, never run), and the *names* of its headers and env entries. A value is never
/// stored, so the lock cannot leak one.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InertServer {
    pub name: String,
    pub transport: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// `directory` | `archive` | `xenia` | `marketplace`. For `xenia` the path is `xenia:<org>/<store>/<record id>@<rev>`
    /// (`xenia_bundle::source_path`).
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
    /// The publisher key and provenance level this version had, so that a rollback restores them (empty in a lock written
    /// before slice B6; the rollback then works them out from the cached tree).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub publisher_kid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provenance: String,
    /// `gc` removed this version's tree from the cache: it can no longer be rolled back to (`specs/024` §4.4).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
    /// `rollback` when it was a rollback that replaced this version (the newest entry then says the current hash is an
    /// earlier one, and the log records `bundle.rolled_back` rather than `bundle.updated`); empty for an update.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub left_by: String,
    /// Where this version came from, so that a rollback judges a trusted source's grant by the origin the version had and
    /// not by the origin of the version it replaces (`None` in a lock written before this field: then no grant applies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

impl Past {
    /// This entry as the version a bundle had before it was replaced at `to`.
    pub fn of(e: &Entry, to: &str) -> Past {
        Past { hash: e.hash.clone(), version: e.version.clone(), from: e.added_at.clone(), to: to.to_string(), publisher_kid: e.publisher_kid.clone(), provenance: e.provenance.clone(), gone: false, left_by: String::new(), source: Some(e.source.clone()) }
    }

    /// The short form of the hash for lines a person reads.
    pub fn short(&self) -> String {
        self.hash.trim_start_matches("blake3:").chars().take(12).collect()
    }
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

    /// Is the operator's raise in force? Only for the very hash and publisher key it was made for, so that any change to
    /// either (an update, a rollback, a replaced publisher, an edited lock) puts the bundle back at the default.
    pub fn raised(&self) -> bool {
        self.trust == RATIFIED && !self.hash.is_empty() && self.trust_hash == self.hash && self.trust_kid == self.publisher_kid
    }

    /// The knowledge tier now in force: `user_ratified` when raised, else `federated_verified`. Never read from the bundle.
    pub fn tier_name(&self) -> &'static str {
        if self.raised() { RATIFIED } else { DEFAULT_TIER }
    }

    /// `acme/acme-platform@1.4.0 · unsigned · validated bundle source`, the breadcrumb's bundle label (`specs/024` §3.6):
    /// the bundle, how it was signed, that its documents are validated sources, and a raise if the operator made one.
    pub fn label(&self, today: &str) -> String {
        format!("{}@{} · {} · {}{}{}", self.id(), self.version, self.provenance, VALIDATED_LABEL, if self.raised() { " · ratified by the operator" } else { "" }, if self.is_stale(today) { " · stale" } else { "" })
    }

    /// Drop the raise and say so in the log. Returns whether there was one on record.
    pub fn reset_trust(&mut self, at: &str, by: &str) -> bool {
        let had = !self.trust.is_empty();
        if had {
            self.trust_log.insert(0, TrustChange { at: at.into(), level: "default".into(), hash: self.hash.clone(), by: by.into() });
            self.trust_log.truncate(20);
        }
        self.trust.clear();
        self.trust_hash.clear();
        self.trust_kid.clear();
        had
    }

    /// Raise the knowledge tier for the current hash and publisher key.
    pub fn raise_trust(&mut self, at: &str, by: &str) {
        self.trust = RATIFIED.into();
        self.trust_hash = self.hash.clone();
        self.trust_kid = self.publisher_kid.clone();
        self.trust_log.insert(0, TrustChange { at: at.into(), level: RATIFIED.into(), hash: self.hash.clone(), by: by.into() });
        self.trust_log.truncate(20);
    }

    /// Who last raised it, if the raise is in force (`trusted-source` for an automatic one).
    pub fn raised_by(&self) -> Option<&str> {
        if !self.raised() {
            return None;
        }
        self.trust_log.iter().find(|c| c.level == RATIFIED).map(|c| c.by.as_str())
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
            history: vec![Past { hash: "blake3:aa".into(), version: "0.9.0".into(), from: "a".into(), to: "b".into(), publisher_kid: String::new(), provenance: String::new(), gone: false, left_by: String::new(), source: None }],
            inert_servers: Vec::new(),
            trust: String::new(),
            trust_hash: String::new(),
            trust_kid: String::new(),
            trust_log: Vec::new(),
        }
    }

    #[test]
    fn the_lock_round_trips_and_a_missing_one_is_empty_but_a_corrupt_one_is_an_error() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(read(t.path()).unwrap(), Lock::default());
        let lock = Lock { bundle: vec![entry("a"), entry("b")], ..Default::default() };
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
        assert_eq!(e.label("2026-10-25"), "acme/a@1.0.0 · unsigned · validated bundle source");
        assert!(e.label("2028-01-01").ends_with("· stale"));
    }

    #[test]
    fn a_raise_counts_only_for_the_hash_and_publisher_it_was_made_for() {
        let mut e = entry("a");
        assert_eq!(e.tier_name(), "federated_verified");
        e.raise_trust("t", "cli");
        assert!(e.raised() && e.tier_name() == "user_ratified" && e.raised_by() == Some("cli"));
        assert!(e.label("2026-10-25").contains("ratified by the operator"));
        let mut moved = e.clone();
        moved.hash = "blake3:cd".into();
        assert!(!moved.raised(), "a different hash is not the raised one");
        let mut rekeyed = e.clone();
        rekeyed.publisher_kid = "other".into();
        assert!(!rekeyed.raised(), "a different publisher is not the raised one");
        let mut forged = entry("a");
        forged.trust = "user_ratified".into();
        assert!(!forged.raised(), "a bare trust field without the binding is nothing");
        assert!(e.reset_trust("t2", "update") && !e.raised() && e.trust_log.len() == 2 && !e.reset_trust("t3", "update"));
    }

    #[test]
    fn the_raise_and_the_trusted_sources_survive_a_round_trip_and_an_old_lock_without_them_still_reads() {
        let t = tempfile::tempdir().unwrap();
        let mut e = entry("a");
        e.raise_trust("t", "cli");
        let ts = TrustedSource { origin: "xenia:acme/main".into(), kid: "did:key:z".into(), level: RATIFIED.into(), granted_at: "t".into(), applied_to: vec![] };
        write(t.path(), &Lock { bundle: vec![e.clone()], trusted_source: vec![ts.clone()] }).unwrap();
        let back = read(t.path()).unwrap();
        assert!(back.bundle[0].raised() && back.trusted_source == vec![ts]);
        // A lock written by a release that knew none of this reads as the default tier: a Delphi update neither adds nor drops a raise.
        std::fs::write(lock_path(t.path()), "[[bundle]]\norg = \"acme\"\nname = \"a\"\nversion = \"1\"\nhash = \"blake3:ab\"\npublisher_kid = \"unsigned\"\nprovenance = \"unsigned\"\nadded_at = \"t\"\norder = 1\nstatus = \"active\"\nreview_by = \"\"\ntitle = \"T\"\n[bundle.source]\nkind = \"directory\"\npath = \"/x\"\n").unwrap();
        let old = read(t.path()).unwrap();
        assert!(!old.bundle[0].raised() && old.trusted_source.is_empty());
    }

    #[test]
    fn a_store_origin_is_the_org_and_store_not_the_record_and_anything_else_is_kind_and_path() {
        let s = |k: &str, p: &str| Source { kind: k.into(), path: p.into() };
        assert_eq!(origin_of(&s("xenia", "xenia:acme/main/rec-1@3")), "xenia:acme/main");
        assert_eq!(origin_of(&s("xenia", "xenia:acme/main/rec-2@9")), "xenia:acme/main");
        assert_ne!(origin_of(&s("xenia", "xenia:acme/other/rec-1@3")), "xenia:acme/main");
        assert_eq!(origin_of(&s("marketplace", "https://github.com/acme/b#plat@abc123")), "marketplace:https://github.com/acme/b");
        assert_eq!(origin_of(&s("marketplace", "https://github.com/acme/b#plat@def456")), origin_of(&s("marketplace", "https://github.com/acme/b#plat@abc123")), "a new commit is the same origin");
        assert_ne!(origin_of(&s("marketplace", "https://github.com/evil/b#plat@abc123")), origin_of(&s("marketplace", "https://github.com/acme/b#plat@abc123")));
    }

    #[test]
    fn active_roots_lists_only_active_bundles_whose_cache_entry_exists() {
        let home = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let (mut gone, mut removed) = (entry("gone"), entry("removed"));
        removed.status = "removed".into();
        gone.hash = "blake3:ff".into();
        write(ws.path(), &Lock { bundle: vec![entry("a"), gone, removed.clone()], ..Default::default() }).unwrap();
        std::fs::create_dir_all(cache_dir(home.path(), "acme", "a", "blake3:ab").join("knowledge")).unwrap();
        std::fs::create_dir_all(cache_dir(home.path(), "acme", "removed", "blake3:ab").join("knowledge")).unwrap();
        let roots = active_roots(home.path(), ws.path());
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].0, "acme/a");
        assert!(plain_id("acme-platform") && !plain_id("../x") && !plain_id("A") && !plain_id(""));
    }
}
