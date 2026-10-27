//! The operator's trust acts on added bundles (`specs/024` §3.7 and §3.8): raising one bundle's knowledge tier to
//! `user_ratified`, and deeming the *source* of a bundle (its origin and its publisher key) a trusted source.
//!
//! Everything here changes only `local/bundles.lock`, which is install-owned state: nothing in a bundle tree can set
//! it and an agent cannot reach it. What no code here does is ask the operator: the caller does, with a terminal on both
//! ends (the command line) or the console scope (the daemon), and passes the typed answer in. [`prepare`] builds the text
//! to show and the word to type back; [`apply`] performs the act. Lowering a tier needs no confirmation.

use crate::bundles::{self, Entry, Lock, TrustedSource, RATIFIED};

pub const UNSIGNED: &str = "unsigned";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Req {
    /// Raise one bundle's knowledge tier for its current hash and publisher key.
    Raise { name: String },
    /// Put one bundle back at the default tier.
    Lower { name: String },
    /// Deem the origin and publisher key of a bundle (or a named origin) a trusted source, at `level`
    /// (`user_ratified` or `default`).
    Grant { target: String, level: String },
    /// End a trusted source; bundles it raised go back to the default tier.
    Revoke { target: String },
}

/// What the operator is shown, and the word they must type back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub text: String,
    pub expect: String,
    /// What to say next to the prompt, e.g. `the bundle name`.
    pub what: String,
}

/// Does a trusted source of this publisher key and origin cover an entry? The origin must be the same recorded origin and
/// the key the same key.
pub fn grant_for<'a>(lock: &'a Lock, e: &Entry) -> Option<&'a TrustedSource> {
    if e.publisher_kid.is_empty() || e.publisher_kid == UNSIGNED {
        return None;
    }
    let origin = bundles::origin_of(&e.source);
    lock.trusted_source.iter().find(|t| t.origin == origin && t.kid == e.publisher_kid)
}

fn find<'a>(lock: &'a Lock, name: &str) -> Result<&'a Entry, String> {
    let found: Vec<&Entry> = lock.bundle.iter().filter(|e| e.is_active() && (e.name == name || e.id() == name)).collect();
    match found.as_slice() {
        [] => Err(format!("no added bundle named `{}`", name)),
        [e] => Ok(e),
        many => Err(format!("`{}` is ambiguous ({}); use org/name", name, many.iter().map(|e| e.id()).collect::<Vec<_>>().join(", "))),
    }
}

/// The (origin, key) a grant target means: a bundle's own, or a named origin that an added signed bundle comes from.
fn resolve_source(lock: &Lock, target: &str) -> Result<(String, String), String> {
    let entry = match find(lock, target) {
        Ok(e) => e.clone(),
        Err(not_found) => {
            let same: Vec<&Entry> = lock.bundle.iter().filter(|e| e.is_active() && bundles::origin_of(&e.source) == target).collect();
            match same.first() {
                Some(e) => (*e).clone(),
                None => return Err(not_found),
            }
        }
    };
    if entry.publisher_kid.is_empty() || entry.publisher_kid == UNSIGNED {
        return Err(format!("{} is unsigned: a source is trusted by its origin AND its publisher key, and there is no key here to anchor it; ask the publisher to `delphi bundle sign` it", entry.id()));
    }
    Ok((bundles::origin_of(&entry.source), entry.publisher_kid.clone()))
}

fn norm_level(level: &str) -> Result<&'static str, String> {
    match level {
        "user_ratified" | "ratified" => Ok(RATIFIED),
        "default" | "validated" | "federated_verified" => Ok("default"),
        other => Err(format!("`--level` is user_ratified or default, not `{}`", other)),
    }
}

/// The confirmation to show for `req`, or `None` when the act only lowers trust and needs none.
pub fn prepare(lock: &Lock, req: &Req) -> Result<Option<Challenge>, String> {
    match req {
        Req::Lower { name } => find(lock, name).map(|_| None),
        Req::Revoke { target } => resolve_source(lock, target).map(|_| None),
        Req::Raise { name } => {
            let e = find(lock, name)?;
            let text = format!(
                "Raise the knowledge of {id}@{ver} to user_ratified (L5) on this install.\n\
                 \x20 publisher {kid}  ·  provenance {prov}  ·  hash {hash}\n\
                 \x20 Today its documents are `federated_verified` (L2) validated bundle sources: citable, with their bundle and provenance level. \
                 Raised, they count as ratified by you, like a file you ratified yourself.\n\
                 \x20 This does NOT change: the @bundle/ path guard (a bundle's files are read-only), deny policies, require_card, permissions and approvals, \
                 or what a document may authorize. A document stays data, never instructions. One effect you should know: a span that reads it is no longer held to the L2 external ceiling for it, as with a file you ratified yourself.\n\
                 \x20 This is for THIS version only. It resets to the default tier when the bundle's hash or publisher changes (an update that brings a new version) and on rollback, and you confirm again then. \
                 You can lower it at any time with `delphi bundle trust {id} --level default`. (To keep a tier across updates from one origin, deem the source trusted: `delphi bundle trust-source {id}`.)",
                id = e.id(),
                ver = e.version,
                kid = short_kid(&e.publisher_kid),
                prov = e.provenance,
                hash = short(&e.hash)
            );
            Ok(Some(Challenge { text, expect: e.id(), what: "the bundle name (org/name)".into() }))
        }
        Req::Grant { target, level } => {
            let level = norm_level(level)?;
            let (origin, kid) = resolve_source(lock, target)?;
            let tier = if level == RATIFIED { "user_ratified (L5)" } else { "federated_verified (L2), citable as validated" };
            let text = format!(
                "Deem this source trusted, at tier {tier}.\n\
                 \x20 origin {origin}\n\
                 \x20 publisher key {kid}\n\
                 \x20 FUTURE UPDATES from this same origin, signed by this same key (or, for a store, a successor key its roster names), will be treated as validated{raise} WITHOUT asking you again: \
                 new versions, new hashes, added or removed files all keep it. The hash and signature check, the scan, deny policies and every other guard still run on each update, and an update that fails one is refused (the bundle you have stays as it is).\n\
                 \x20 The grant ends, and the bundle goes back to the default tier with a message, when: the origin differs, the publisher key is not this key or a roster successor, \
                 the update is unsigned or cannot be verified, or you revoke it with `delphi bundle trust-source {origin} --revoke`.\n\
                 \x20 It does NOT change the @bundle/ path guard, deny policies, require_card, permissions, or what a document may authorize. A document stays data, never instructions.",
                tier = tier,
                origin = origin,
                kid = short_kid(&kid),
                raise = if level == RATIFIED { " and raised to user_ratified" } else { "" }
            );
            Ok(Some(Challenge { text, expect: origin, what: "the source id shown above (the origin)".into() }))
        }
    }
}

fn short_kid(kid: &str) -> String {
    let chars: Vec<char> = kid.chars().collect();
    if chars.len() <= 24 {
        return kid.to_string();
    }
    format!("{}…{}", chars[..14].iter().collect::<String>(), chars[chars.len() - 6..].iter().collect::<String>())
}

fn short(hash: &str) -> String {
    hash.trim_start_matches("blake3:").chars().take(12).collect()
}

/// Perform the act. `by` is `cli`, `console` or `at-add`. The caller has confirmed, if [`prepare`] asked for it.
/// Returns what to tell the operator.
pub fn apply(ws: &std::path::Path, req: &Req, by: &str) -> Result<String, String> {
    let now = crate::time::now_rfc3339();
    bundles::update(ws, |lock| match req {
        Req::Raise { name } => {
            let id = find(lock, name)?.id();
            let e = lock.bundle.iter_mut().find(|e| e.id() == id && e.is_active()).ok_or("the bundle was removed meanwhile")?;
            e.raise_trust(&now, by);
            Ok(format!("{} is raised to user_ratified for {}; it resets to the default tier if its hash or publisher changes, and on rollback", id, short(&e.hash)))
        }
        Req::Lower { name } => {
            let id = find(lock, name)?.id();
            let e = lock.bundle.iter_mut().find(|e| e.id() == id && e.is_active()).ok_or("the bundle was removed meanwhile")?;
            let had = e.reset_trust(&now, by);
            // A bundle under a trusted source stays at that source's tier until the source is revoked.
            Ok(if had { format!("{} is back at the default tier (federated_verified, validated bundle source)", id) } else { format!("{} was already at the default tier", id) })
        }
        Req::Grant { target, level } => {
            let level = norm_level(level)?;
            let (origin, kid) = resolve_source(lock, target)?;
            lock.trusted_source.retain(|t| !(t.origin == origin && t.kid == kid));
            let mut applied = Vec::new();
            for e in lock.bundle.iter_mut().filter(|e| e.is_active() && bundles::origin_of(&e.source) == origin && e.publisher_kid == kid) {
                if level == RATIFIED {
                    if !e.raised() {
                        e.raise_trust(&now, "trusted-source");
                    }
                    applied.push(e.id());
                } else if e.raised_by() == Some("trusted-source") {
                    e.reset_trust(&now, "trusted-source");
                }
            }
            lock.trusted_source.push(TrustedSource { origin: origin.clone(), kid, level: level.to_string(), granted_at: now.clone(), applied_to: applied.clone() });
            Ok(format!("{} is a trusted source at {}{}; updates from it keep that tier without asking (every check still runs)", origin, if level == RATIFIED { "user_ratified" } else { "the default tier (validated bundle source)" }, if applied.is_empty() { String::new() } else { format!("; raised now: {}", applied.join(", ")) }))
        }
        Req::Revoke { target } => {
            let (origin, kid) = resolve_source(lock, target)?;
            let before = lock.trusted_source.len();
            lock.trusted_source.retain(|t| !(t.origin == origin && t.kid == kid));
            let mut lowered = Vec::new();
            for e in lock.bundle.iter_mut().filter(|e| e.is_active() && bundles::origin_of(&e.source) == origin && e.publisher_kid == kid && e.raised_by() == Some("trusted-source")) {
                e.reset_trust(&now, "source-revoked");
                lowered.push(e.id());
            }
            if before == lock.trusted_source.len() {
                return Ok(format!("{} was not a trusted source", origin));
            }
            Ok(format!("{} is no longer a trusted source{}", origin, if lowered.is_empty() { String::new() } else { format!("; back at the default tier: {}", lowered.join(", ")) }))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundles::{Past, Source};
    use std::collections::BTreeMap;

    fn entry(name: &str, kid: &str, src: (&str, &str)) -> Entry {
        Entry {
            org: "acme".into(),
            name: name.into(),
            version: "1.0.0".into(),
            hash: "blake3:ab".into(),
            publisher_kid: kid.into(),
            provenance: "signed".into(),
            source: Source { kind: src.0.into(), path: src.1.into() },
            added_at: "2026-10-25T00:00:00Z".into(),
            order: 10,
            status: "active".into(),
            review_by: "2027-01-31".into(),
            title: "T".into(),
            skill_roles: BTreeMap::new(),
            history: Vec::<Past>::new(),
            inert_servers: Vec::new(),
            trust: String::new(),
            trust_hash: String::new(),
            trust_kid: String::new(),
            trust_log: Vec::new(),
        }
    }

    fn ws_with(entries: Vec<Entry>) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("local")).unwrap();
        bundles::write(t.path(), &Lock { bundle: entries, ..Default::default() }).unwrap();
        t
    }

    #[test]
    fn a_raise_is_per_bundle_logged_and_a_lower_undoes_it_without_a_prompt() {
        let ws = ws_with(vec![entry("a", "did:key:zA", ("directory", "/x")), entry("b", "did:key:zA", ("directory", "/y"))]);
        let lock = bundles::read(ws.path()).unwrap();
        let c = prepare(&lock, &Req::Raise { name: "a".into() }).unwrap().unwrap();
        assert_eq!(c.expect, "acme/a");
        for must in ["user_ratified", "hash or publisher changes", "rollback", "path guard", "never instructions"] {
            assert!(c.text.contains(must), "{}", must);
        }
        apply(ws.path(), &Req::Raise { name: "acme/a".into() }, "cli").unwrap();
        let lock = bundles::read(ws.path()).unwrap();
        assert!(lock.bundle[0].raised() && !lock.bundle[1].raised(), "per bundle");
        assert_eq!(lock.bundle[0].trust_log[0].by, "cli");
        assert!(prepare(&lock, &Req::Lower { name: "a".into() }).unwrap().is_none());
        apply(ws.path(), &Req::Lower { name: "a".into() }, "cli").unwrap();
        let lock = bundles::read(ws.path()).unwrap();
        assert!(!lock.bundle[0].raised() && lock.bundle[0].trust_log.len() == 2);
    }

    #[test]
    fn a_source_needs_a_publisher_key_and_the_grant_states_what_it_does() {
        let ws = ws_with(vec![entry("a", "unsigned", ("directory", "/x")), entry("b", "did:key:zA", ("xenia", "xenia:acme/main/r1@2"))]);
        let lock = bundles::read(ws.path()).unwrap();
        assert!(prepare(&lock, &Req::Grant { target: "a".into(), level: "default".into() }).unwrap_err().contains("unsigned"));
        let c = prepare(&lock, &Req::Grant { target: "b".into(), level: "user_ratified".into() }).unwrap().unwrap();
        assert_eq!(c.expect, "xenia:acme/main");
        for must in ["WITHOUT asking you again", "new hashes", "still run on each update", "unsigned or cannot be verified", "--revoke"] {
            assert!(c.text.contains(must), "{}", must);
        }
        assert!(prepare(&lock, &Req::Grant { target: "b".into(), level: "bogus".into() }).is_err());
    }

    #[test]
    fn granting_raises_the_current_bundles_of_that_origin_and_key_only_and_revoking_lowers_just_those() {
        let ws = ws_with(vec![entry("a", "did:key:zA", ("xenia", "xenia:acme/main/r1@2")), entry("same-store-other-key", "did:key:zB", ("xenia", "xenia:acme/main/r2@1")), entry("c", "did:key:zA", ("xenia", "xenia:acme/other/r3@1"))]);
        apply(ws.path(), &Req::Grant { target: "xenia:acme/main".into(), level: "user_ratified".into() }, "cli").unwrap();
        let lock = bundles::read(ws.path()).unwrap();
        assert_eq!(lock.trusted_source.len(), 1);
        assert!(lock.bundle[0].raised() && !lock.bundle[1].raised() && !lock.bundle[2].raised(), "same store different key, and same key different store, are not covered");
        assert_eq!(lock.bundle[0].raised_by(), Some("trusted-source"));
        assert!(grant_for(&lock, &lock.bundle[0]).is_some() && grant_for(&lock, &lock.bundle[1]).is_none() && grant_for(&lock, &lock.bundle[2]).is_none());
        let out = apply(ws.path(), &Req::Revoke { target: "a".into() }, "cli").unwrap();
        assert!(out.contains("no longer a trusted source"), "{}", out);
        let lock = bundles::read(ws.path()).unwrap();
        assert!(lock.trusted_source.is_empty() && !lock.bundle[0].raised());
    }

    #[test]
    fn an_unknown_bundle_or_origin_is_an_error_and_changes_nothing() {
        let ws = ws_with(vec![entry("a", "did:key:zA", ("directory", "/x"))]);
        assert!(apply(ws.path(), &Req::Raise { name: "nope".into() }, "cli").is_err());
        assert!(apply(ws.path(), &Req::Grant { target: "nope".into(), level: "default".into() }, "cli").is_err());
        assert!(!bundles::read(ws.path()).unwrap().bundle[0].raised());
    }
}
