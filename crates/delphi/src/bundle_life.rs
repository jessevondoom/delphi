//! `delphi bundle update|rollback|gc` (`specs/024` §4.3, §4.4): the rest of a bundle's life after `add`.
//!
//! `update` re-adds from the lock's recorded source under the publisher-continuity rule and prints a changelog notice.
//! `rollback` re-points the lock at an earlier hash whose tree is still in the cache. `gc` prunes the cache. None of them
//! calls a model or raises a card (OD-1); the lock's `history` is the record.

use crate::bundle_add::{self, fail, matches, short, AddOpts, Mode};
use delphi_bundle::{changes, provenance, Host, Options};
use delphi_core::bundles::{self, Entry, Past};
use std::collections::BTreeSet;
use std::path::Path;

/// The notice after an update (`specs/024` §4.3): the top changelog entry, the version bump and whether it is the smallest
/// honest one, and what changed per section. `new_dir` is the new version's tree in the cache.
pub fn print_update_notice(home: &Path, old: &Entry, m: &delphi_bundle::Manifest, new_dir: &Path, new_files_json: &str, host: &dyn Host) {
    println!("\nupdate notice: {} {} -> {}", old.id(), old.version, m.version);
    match std::fs::read_to_string(new_dir.join("CHANGELOG.md")).map(|t| changes::changelog_top(&t)) {
        Ok(top) if !top.is_empty() => {
            println!("  CHANGELOG.md:");
            for l in top {
                println!("    {}", l);
            }
        }
        _ => println!("  CHANGELOG.md: none, or no entry"),
    }
    let old_dir = bundles::cache_dir(home, &old.org, &old.name, &old.hash);
    match changes::version_bump(new_dir, &old_dir, host) {
        Some((true, msg)) => println!("  version bump: matches `validate --against` ({})", msg),
        Some((false, msg)) => println!("  version bump: DOES NOT MATCH `validate --against`: {}", msg),
        None => println!("  version bump: not checked (the previous tree is not in the cache)"),
    }
    let sections = std::fs::read(old_dir.join(".delphi-bundle/files.json")).map(|old_fj| changes::section_changes(&old_fj, new_files_json.as_bytes()));
    match sections {
        Ok(c) if c.is_empty() => println!("  files: none changed"),
        Ok(c) => {
            println!("  files changed:");
            for l in changes::render_sections(&c) {
                println!("    {}", l);
            }
            if ["skills", "agents", "behavior"].iter().any(|s| c.contains_key(s)) {
                println!("  changed behavior loads from the next task");
            }
        }
        Err(_) => println!("  files: not compared (the previous tree is not in the cache)"),
    }
}

/// `delphi bundle update [name]`: add again from each recorded source. A publisher change stops the one bundle with the
/// re-add message and changes nothing; the others still update.
pub fn update(home: &Path, ws: &Path, name: Option<&str>, background: bool, host: &dyn Host) -> i32 {
    let lock = match bundles::read(ws) {
        Ok(l) => l,
        Err(e) => return fail(e),
    };
    let targets: Vec<&Entry> = lock.bundle.iter().filter(|e| e.is_active() && name.is_none_or(|n| matches(e, n))).collect();
    if targets.is_empty() {
        return match name {
            Some(n) => fail(format!("no added bundle named `{}`", n)),
            None => {
                println!("no bundles added");
                0
            }
        };
    }
    if let Some(n) = name {
        if targets.len() > 1 {
            return fail(format!("`{}` is ambiguous ({}); use org/name", n, targets.iter().map(|e| e.id()).collect::<Vec<_>>().join(", ")));
        }
    }
    let mut code = 0;
    for e in targets {
        if !matches!(e.source.kind.as_str(), "directory" | "archive") {
            eprintln!("delphi bundle: {} came from a {} source, which this build cannot fetch from", e.id(), e.source.kind);
            code = 1;
            continue;
        }
        let id = e.id();
        let opts = AddOpts { skill_roles: Default::default(), order: None, dry_run: false, yes: false, replace_publisher: false, no_vectors: false, embed_in_background: background };
        if bundle_add::apply(home, ws, Path::new(&e.source.path), &opts, Mode::Update(&id), host) != 0 {
            code = 1;
        }
    }
    code
}

/// A `--to` value: a hash or a prefix of one, at least 8 lowercase hex digits, with or without `blake3:`.
pub fn hash_prefix(s: &str) -> Option<String> {
    let h = s.strip_prefix("blake3:").unwrap_or(s);
    (h.len() >= 8 && h.len() <= 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))).then(|| h.to_string())
}

/// `delphi bundle rollback <name> [--to <hash>]`.
pub fn rollback(home: &Path, ws: &Path, name: &str, to: Option<&str>, background: bool, host: &dyn Host) -> i32 {
    let lock = match bundles::read(ws) {
        Ok(l) => l,
        Err(e) => return fail(e),
    };
    let found: Vec<&Entry> = lock.bundle.iter().filter(|e| e.is_active() && matches(e, name)).collect();
    let e = match found.as_slice() {
        [] => return fail(format!("no added bundle named `{}`", name)),
        [e] => *e,
        many => return fail(format!("`{}` is ambiguous ({}); use org/name", name, many.iter().map(|e| e.id()).collect::<Vec<_>>().join(", "))),
    };
    let target: &Past = match to {
        None => match e.history.first() {
            Some(p) => p,
            None => return fail(format!("{} has no earlier version in its history", e.id())),
        },
        Some(t) => {
            let Some(prefix) = hash_prefix(t) else { return fail("--to wants a bundle hash: at least 8 hex digits, as `delphi bundle list` and the history show them") };
            if e.hash.trim_start_matches("blake3:").starts_with(&prefix) {
                return fail(format!("{} is already at {}", e.id(), short(&e.hash)));
            }
            let hits: Vec<&Past> = e.history.iter().filter(|p| p.hash.trim_start_matches("blake3:").starts_with(&prefix)).collect();
            match hits.as_slice() {
                [] => return fail(format!("{} has no version {} in its history ({})", e.id(), prefix, e.history.iter().map(|p| format!("{} {}", p.version, p.short())).collect::<Vec<_>>().join(", "))),
                [p] => p,
                _ => return fail(format!("{} matches more than one version; give more digits", prefix)),
            }
        }
    };
    let dest = bundles::cache_dir(home, &e.org, &e.name, &target.hash);
    if target.gone || !dest.is_dir() {
        return fail(format!("the tree of {} {} is no longer in the cache (`delphi bundle gc` removed it); add it again from its source to restore it", target.version, target.short()));
    }
    // The cached tree is verified again before the lock points at it: a cache entry edited since is not a rollback target.
    let i = delphi_bundle::inspect(&dest, &Options::default(), host);
    let (Some(m), Some(hash)) = (i.manifest.clone(), i.hash.clone()) else { return fail(format!("the cached tree of {} {} no longer validates; nothing changed", target.version, target.short())) };
    if !i.report.passed() || hash != target.hash {
        print!("{}", i.report.render(&dest.display().to_string()));
        return fail(format!("the cached tree of {} {} no longer validates; nothing changed", target.version, target.short()));
    }
    let (kid, level) = if target.publisher_kid.is_empty() {
        match &i.report.signer {
            Some(k) => (k.clone(), provenance::Level::Signed.as_str().to_string()),
            None => (provenance::UNSIGNED.to_string(), provenance::Level::Unsigned.as_str().to_string()),
        }
    } else {
        (target.publisher_kid.clone(), target.provenance.clone())
    };
    let now = delphi_core::time::now_rfc3339();
    let (id, wanted) = (e.id(), target.hash.clone());
    let inert = crate::bundle_connect::inert_servers(&dest);
    let done = bundles::update(ws, |lock| {
        let e = lock.bundle.iter_mut().find(|x| x.id() == id && x.is_active()).ok_or("the bundle was removed meanwhile")?;
        let pos = e.history.iter().position(|p| p.hash == wanted).ok_or("the history changed meanwhile")?;
        let from = e.clone();
        e.history.remove(pos);
        let mut left = Past::of(&from, &now);
        left.left_by = "rollback".into();
        e.history.insert(0, left);
        e.version = m.version.clone();
        e.hash = wanted.clone();
        e.publisher_kid = kid.clone();
        e.provenance = level.clone();
        e.review_by = m.review_by.clone();
        e.title = m.title.clone();
        e.added_at = now.clone();
        e.inert_servers = inert.clone();
        Ok(from)
    });
    let from = match done {
        Ok(f) => f,
        Err(err) => return fail(err),
    };
    match delphi_engine::kb::sync_workspace(ws) {
        Ok(r) => println!("rolled {} back: {} ({}) -> {} ({}); knowledge follows the lock now ({} documents added, {} updated, {} removed), prompt content from the next task", id, from.version, short(&from.hash), m.version, short(&wanted), r.added, r.updated, r.removed),
        Err(err) => return fail(format!("rolled back, but the index could not be updated: {} (run `delphi kb index`)", err)),
    }
    // The older tree's vector verdict (beside it in the cache) still stands, so its shipped vectors apply again at once;
    // anything without a vector is embedded as after an add.
    let o = AddOpts { skill_roles: Default::default(), order: None, dry_run: false, yes: false, replace_publisher: false, no_vectors: false, embed_in_background: background };
    bundle_add::vectors(home, ws, &e.org, &e.name, &o);
    0
}

/// Every `(org, name, hash)` that some workspace's lock still points at: the current hash of an active bundle and every
/// history entry not marked gone. The cache is shared between workspaces, so a tree is deleted only when no lock needs it.
fn referenced(home: &Path) -> BTreeSet<(String, String, String)> {
    let mut out = BTreeSet::new();
    let Ok(rd) = std::fs::read_dir(home.join("workspaces")) else { return out };
    for w in rd.filter_map(|e| e.ok()) {
        let Ok(lock) = bundles::read(&w.path()) else {
            // An unreadable lock may need any tree: keep the whole cache rather than guess.
            out.insert(("*".into(), "*".into(), "*".into()));
            continue;
        };
        for e in lock.bundle.iter().filter(|e| e.is_active()) {
            out.insert((e.org.clone(), e.name.clone(), e.hash.trim_start_matches("blake3:").to_string()));
            for p in e.history.iter().filter(|p| !p.gone) {
                out.insert((e.org.clone(), e.name.clone(), p.hash.trim_start_matches("blake3:").to_string()));
            }
        }
    }
    out
}

/// `delphi bundle gc`: keep the current tree and the newest earlier ones up to `KEEP_HASHES` per active bundle
/// (marking the rest `gone` in this workspace's lock), drop the trees of removed bundles, then delete every cached tree
/// that no workspace's lock refers to, and stale staging directories.
pub fn gc(home: &Path, ws: &Path) -> i32 {
    let marked = bundles::update(ws, |lock| {
        let mut n = 0;
        for e in lock.bundle.iter_mut() {
            let keep = if e.is_active() { bundles::KEEP_HASHES.saturating_sub(1) } else { 0 };
            for p in e.history.iter_mut().filter(|p| !p.gone).skip(keep) {
                p.gone = true;
                n += 1;
            }
        }
        Ok(n)
    });
    let marked = match marked {
        Ok(n) => n,
        Err(e) => return fail(e),
    };
    let live = referenced(home);
    let keep_all = live.contains(&("*".into(), "*".into(), "*".into()));
    let mut gone = Vec::new();
    let root = home.join("bundles");
    let dirs = |p: &Path| -> Vec<std::path::PathBuf> {
        let mut v: Vec<_> = std::fs::read_dir(p).map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink())).map(|e| e.path()).collect()).unwrap_or_default();
        v.sort();
        v
    };
    let leaf = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    for org in dirs(&root).into_iter().filter(|p| leaf(p) != ".incoming") {
        for name in dirs(&org) {
            for h in dirs(&name) {
                let key = (leaf(&org), leaf(&name), leaf(&h));
                if !keep_all && !live.contains(&key) {
                    gone.push(h);
                }
            }
        }
    }
    for d in &gone {
        // The install's vector verdicts for a tree sit beside it (`bundle_vectors`): they go with it.
        let _ = std::fs::remove_file(delphi_engine::bundle_vectors::verdicts_path(d));
        if let Err(e) = std::fs::remove_dir_all(d) {
            eprintln!("delphi bundle: {}: {}", d.display(), e);
        }
    }
    // A verdict file whose tree is gone (an interrupted run, a hand-deleted tree) is dropped too, then the empty
    // <org>/<name> and <org> directories this leaves.
    for org in dirs(&root).into_iter().filter(|p| leaf(p) != ".incoming") {
        for name in dirs(&org) {
            for f in std::fs::read_dir(&name).into_iter().flatten().filter_map(|e| e.ok()) {
                let file = f.file_name().to_string_lossy().to_string();
                if let Some(tree) = file.strip_suffix(".vectors.json") {
                    if !name.join(tree).is_dir() {
                        let _ = std::fs::remove_file(f.path());
                    }
                }
            }
            let _ = std::fs::remove_dir(&name);
        }
        let _ = std::fs::remove_dir(&org);
    }
    // Staging directories from an add that died; an hour is long past any add still running.
    let old = std::time::Duration::from_secs(3600);
    let mut stale = 0;
    for d in dirs(&root.join(".incoming")) {
        let aged = std::fs::metadata(&d).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > old);
        if aged {
            stale += 1;
            let _ = std::fs::remove_dir_all(&d);
        }
    }
    println!(
        "removed {} cached tree(s) no workspace needs and {} old staging director{}; {} older version(s) marked gone in this workspace's lock (the cache keeps the current and {} earlier per bundle)",
        gone.len(),
        stale,
        if stale == 1 { "y" } else { "ies" },
        marked,
        bundles::KEEP_HASHES - 1
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle_cmd::Real;
    use crate::bundle_pack::{self, Challenge};
    use delphi_xenia::identity::Identity;
    use std::fs;
    use std::path::PathBuf;

    fn golden() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dev/bundles/acme-platform")
    }

    fn copy(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for e in fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() { copy(&e.path(), &to.join(e.file_name())) } else { fs::copy(e.path(), to.join(e.file_name())).unwrap(); }
        }
    }

    struct Fx {
        home: tempfile::TempDir,
        ws: PathBuf,
        src: tempfile::TempDir,
        keys: tempfile::TempDir,
    }

    fn fx() -> Fx {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        fs::create_dir_all(ws.join("local")).unwrap();
        let src = tempfile::tempdir().unwrap();
        copy(&golden(), src.path());
        Fx { home, ws, src, keys: tempfile::tempdir().unwrap() }
    }

    fn opts() -> AddOpts {
        AddOpts { skill_roles: Default::default(), order: None, dry_run: false, yes: false, replace_publisher: false, no_vectors: false, embed_in_background: false }
    }

    fn key(f: &Fx, name: &str) -> (PathBuf, Identity) {
        let p = f.keys.path().join(name).join("identity.key");
        let id = Identity::load_or_create(&p).unwrap();
        (p, id)
    }

    fn toml_edit(dir: &Path, f: impl FnOnce(String) -> String) {
        let p = dir.join("bundle.toml");
        fs::write(&p, f(fs::read_to_string(&p).unwrap())).unwrap();
    }

    /// Name the publisher key (and optionally predecessors) in `bundle.toml`.
    fn publisher(dir: &Path, did: &str, predecessors: &[&str]) {
        toml_edit(dir, |t| {
            let kept: Vec<&str> = t.lines().filter(|l| !l.starts_with("did = ") && !l.starts_with("predecessors = ")).collect();
            let mut extra = format!("did = \"{}\"\n", did);
            if !predecessors.is_empty() {
                extra.push_str(&format!("predecessors = [{}]\n", predecessors.iter().map(|p| format!("\"{}\"", p)).collect::<Vec<_>>().join(", ")));
            }
            kept.join("\n").replacen("[publisher]", &format!("[publisher]\n{}", extra.trim_end()), 1) + "\n"
        });
    }

    fn set_version(dir: &Path, to: &str) {
        toml_edit(dir, |t| {
            let line = t.lines().find(|l| l.starts_with("version = ")).unwrap().to_string();
            t.replacen(&line, &format!("version = \"{}\"", to), 1)
        });
        let cl = dir.join("CHANGELOG.md");
        let old = fs::read_to_string(&cl).unwrap();
        fs::write(&cl, old.replacen("# Changelog\n", &format!("# Changelog\n\n## {}\n- Changed the glossary.\n", to), 1)).unwrap();
        let g = dir.join("knowledge/glossary.md");
        fs::write(&g, fs::read_to_string(&g).unwrap() + &format!("\nNote for {}.\n", to)).unwrap();
    }

    /// Write the manifest and sign the directory with the key file, as an operator at a terminal would.
    fn publish(dir: &Path, key_path: &Path) -> i32 {
        delphi_bundle::write_manifest(dir, true, &Real).unwrap();
        bundle_pack::sign(dir, key_path, None, &Real, &|_: &Challenge| Ok(true))
    }

    fn lock(f: &Fx) -> bundles::Lock {
        bundles::read(&f.ws).unwrap()
    }

    fn staging_empty(f: &Fx) -> bool {
        !f.home.path().join("bundles/.incoming").read_dir().map(|mut d| d.next().is_some()).unwrap_or(false)
    }

    /// A signed first version by key `a`, added from the directory.
    fn first_signed(f: &Fx, a: &(PathBuf, Identity)) {
        publisher(f.src.path(), &a.1.did(), &[]);
        assert_eq!(publish(f.src.path(), &a.0), 0);
        assert_eq!(bundle_add::add(f.home.path(), &f.ws, f.src.path(), &opts(), &Real), 0);
    }

    #[test]
    fn a_signed_bundle_adds_as_signed_pins_its_key_and_an_unsigned_one_adds_as_unsigned() {
        let f = fx();
        assert_eq!(bundle_add::add(f.home.path(), &f.ws, f.src.path(), &opts(), &Real), 0);
        assert_eq!((lock(&f).bundle[0].provenance.as_str(), lock(&f).bundle[0].publisher_kid.as_str()), ("unsigned", "unsigned"));
        assert_eq!(bundle_add::remove(&f.ws, "acme-platform"), 0);
        let a = key(&f, "a");
        first_signed(&f, &a);
        let e = &lock(&f).bundle[0];
        assert_eq!((e.provenance.as_str(), e.publisher_kid.as_str(), e.status.as_str()), ("signed", a.1.did().as_str(), "active"));
        // the cache holds the signature that was verified
        assert!(bundles::cache_dir(f.home.path(), "acme", "acme-platform", &e.hash).join(".delphi-bundle/sig.json").is_file());
    }

    #[test]
    fn a_flipped_byte_a_changed_version_or_a_signature_under_another_kid_each_fail_add_and_leave_no_trace() {
        let f = fx();
        let (a, b) = (key(&f, "a"), key(&f, "b"));
        publisher(f.src.path(), &a.1.did(), &[]);
        assert_eq!(publish(f.src.path(), &a.0), 0);
        let tampered = tempfile::tempdir().unwrap();
        for case in 0..3 {
            copy(f.src.path(), tampered.path());
            match case {
                0 => fs::write(tampered.path().join("README.md"), fs::read_to_string(tampered.path().join("README.md")).unwrap() + "x\n").unwrap(),
                1 => {
                    toml_edit(tampered.path(), |t| t.replacen("version = \"1.4.0\"", "version = \"1.4.1\"", 1));
                    delphi_bundle::write_manifest(tampered.path(), true, &Real).unwrap();
                }
                _ => {
                    let sig = fs::read_to_string(tampered.path().join(".delphi-bundle/sig.json")).unwrap();
                    fs::write(tampered.path().join(".delphi-bundle/sig.json"), sig.replace(&a.1.did(), &b.1.did())).unwrap();
                }
            }
            assert_eq!(bundle_add::add(f.home.path(), &f.ws, tampered.path(), &opts(), &Real), 1, "case {case}");
            assert!(!f.ws.join("local/bundles.lock").exists() && !f.home.path().join("bundles").join("acme").exists(), "case {case}");
        }
    }

    #[test]
    fn signing_needs_a_terminal_and_the_publishers_own_key_and_a_changed_tree_needs_a_fresh_manifest() {
        let f = fx();
        let a = key(&f, "a");
        publisher(f.src.path(), &a.1.did(), &[]);
        delphi_bundle::write_manifest(f.src.path(), true, &Real).unwrap();
        // under test there is no terminal: the real challenge refuses
        assert_eq!(bundle_pack::sign(f.src.path(), &a.0, None, &Real, &bundle_pack::tty_confirm), 1);
        assert!(!f.src.path().join(".delphi-bundle/sig.json").exists());
        // a "no" signs nothing
        assert_eq!(bundle_pack::sign(f.src.path(), &a.0, None, &Real, &|_: &Challenge| Ok(false)), 1);
        // the wrong key for publisher.did
        let b = key(&f, "b");
        assert_eq!(bundle_pack::sign(f.src.path(), &b.0, None, &Real, &|_: &Challenge| Ok(true)), 1);
        // the challenge shows the manifest and asks for the hash digits
        let seen = std::cell::RefCell::new(String::new());
        assert_eq!(bundle_pack::sign(f.src.path(), &a.0, None, &Real, &|c: &Challenge| { *seen.borrow_mut() = format!("{}|{}", c.text, c.expect); Ok(true) }), 0);
        let seen = seen.into_inner();
        assert!(seen.contains("knowledge/index.md") && seen.contains("acme/acme-platform@1.4.0") && seen.contains(&a.1.did()), "{seen}");
        assert_eq!(seen.rsplit('|').next().unwrap().len(), 8);
        // a file edited after the manifest was written: refused until `manifest` runs again
        fs::write(f.src.path().join("README.md"), "changed\n").unwrap();
        assert_eq!(bundle_pack::sign(f.src.path(), &a.0, None, &Real, &|_: &Challenge| Ok(true)), 1);
        // no key material in anything printed or written: the sig file carries the public did only
        let sig = fs::read_to_string(f.src.path().join(".delphi-bundle/sig.json")).unwrap();
        let seed = fs::read_to_string(&a.0).unwrap();
        assert!(!sig.contains(seed.lines().nth(1).unwrap()));
    }

    #[test]
    fn signing_refuses_a_version_below_the_smallest_honest_bump_over_against() {
        let f = fx();
        let a = key(&f, "a");
        publisher(f.src.path(), &a.1.did(), &[]);
        let prev = tempfile::tempdir().unwrap();
        copy(f.src.path(), prev.path());
        // new content under the same version: below the bump
        fs::write(f.src.path().join("knowledge/glossary.md"), fs::read_to_string(f.src.path().join("knowledge/glossary.md")).unwrap() + "\nmore\n").unwrap();
        delphi_bundle::write_manifest(f.src.path(), true, &Real).unwrap();
        assert_eq!(bundle_pack::sign(f.src.path(), &a.0, Some(prev.path().to_path_buf()), &Real, &|_: &Challenge| Ok(true)), 1);
        assert!(!f.src.path().join(".delphi-bundle/sig.json").exists());
    }

    #[test]
    fn package_is_byte_reproducible_and_an_archive_adds_with_its_source_recorded_and_nothing_left_in_staging() {
        let f = fx();
        let a = key(&f, "a");
        publisher(f.src.path(), &a.1.did(), &[]);
        assert_eq!(publish(f.src.path(), &a.0), 0);
        let out = tempfile::tempdir().unwrap();
        let (x, y) = (out.path().join("one.xbundle"), out.path().join("two.xbundle"));
        assert_eq!(bundle_pack::package(f.src.path(), Some(&x), &Real), 0);
        assert_eq!(bundle_pack::package(f.src.path(), Some(&y), &Real), 0);
        assert_eq!(fs::read(&x).unwrap(), fs::read(&y).unwrap(), "two archives of one tree are byte-identical");
        // a copy of the tree in another place packs to the same bytes
        let other = tempfile::tempdir().unwrap();
        copy(f.src.path(), other.path());
        let z = out.path().join("three.xbundle");
        assert_eq!(bundle_pack::package(other.path(), Some(&z), &Real), 0);
        assert_eq!(fs::read(&x).unwrap(), fs::read(&z).unwrap());
        // the archive may not be written inside the bundle
        assert_eq!(bundle_pack::package(f.src.path(), Some(&f.src.path().join("inside.xbundle")), &Real), 1);
        // add from the archive, as an isolated machine would
        assert_eq!(bundle_add::add(f.home.path(), &f.ws, &x, &opts(), &Real), 0);
        let e = &lock(&f).bundle[0];
        assert_eq!((e.source.kind.as_str(), e.provenance.as_str()), ("archive", "signed"));
        assert!(e.source.path.ends_with("one.xbundle"));
        assert!(bundles::cache_dir(f.home.path(), "acme", "acme-platform", &e.hash).join("knowledge/index.md").is_file());
        assert!(staging_empty(&f));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(bundles::cache_dir(f.home.path(), "acme", "acme-platform", &e.hash).join("bundle.toml")).unwrap().permissions().mode() & 0o111, 0);
    }

    #[test]
    fn a_hostile_or_truncated_archive_is_refused_and_staging_is_cleaned() {
        let f = fx();
        let out = tempfile::tempdir().unwrap();
        let good = out.path().join("g.xbundle");
        assert_eq!(bundle_pack::package(f.src.path(), Some(&good), &Real), 0);
        let bytes = fs::read(&good).unwrap();
        let cut = out.path().join("cut.xbundle");
        fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
        let junk = out.path().join("junk.xbundle");
        fs::write(&junk, b"this is not a tar file at all").unwrap();
        // an entry that is a symbolic link, made by flipping the type flag of the first header
        let mut link = bytes.clone();
        link[156] = b'2';
        let sum: u64 = link[..512].iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u64 }).sum();
        link[148..154].copy_from_slice(format!("{:06o}", sum).as_bytes());
        let sym = out.path().join("sym.xbundle");
        fs::write(&sym, &link).unwrap();
        for bad in [&cut, &junk, &sym] {
            assert_eq!(bundle_add::add(f.home.path(), &f.ws, bad, &opts(), &Real), 1, "{}", bad.display());
        }
        assert!(staging_empty(&f), "`.incoming/` is removed on any failure");
        assert!(!f.ws.join("local/bundles.lock").exists());
    }

    #[test]
    fn same_key_updates_a_different_key_stops_and_changes_nothing_and_replace_publisher_accepts_it() {
        let f = fx();
        let (a, b) = (key(&f, "a"), key(&f, "b"));
        first_signed(&f, &a);
        let v1 = lock(&f).bundle[0].clone();
        // 1.5.0 by the same key: an update, known-publisher, with the old hash in history
        set_version(f.src.path(), "1.5.0");
        assert_eq!(publish(f.src.path(), &a.0), 0);
        assert_eq!(update(f.home.path(), &f.ws, Some("acme-platform"), false, &Real), 0);
        let e = lock(&f).bundle[0].clone();
        assert_eq!((e.version.as_str(), e.provenance.as_str(), e.publisher_kid.as_str(), e.history.len()), ("1.5.0", "known-publisher", a.1.did().as_str(), 1));
        assert_eq!(e.history[0].hash, v1.hash);
        // 1.6.0 by another key: stops, the lock is byte-for-byte unchanged
        set_version(f.src.path(), "1.6.0");
        publisher(f.src.path(), &b.1.did(), &[]);
        assert_eq!(publish(f.src.path(), &b.0), 0);
        let before = fs::read(f.ws.join("local/bundles.lock")).unwrap();
        assert_eq!(update(f.home.path(), &f.ws, None, false, &Real), 1);
        assert_eq!(bundle_add::add(f.home.path(), &f.ws, f.src.path(), &opts(), &Real), 1, "a plain add stops the same way");
        assert_eq!(fs::read(f.ws.join("local/bundles.lock")).unwrap(), before);
        // the explicit re-add accepts and pins the new key
        assert_eq!(bundle_add::add(f.home.path(), &f.ws, f.src.path(), &AddOpts { replace_publisher: true, ..opts() }, &Real), 0);
        let e = lock(&f).bundle[0].clone();
        assert_eq!((e.version.as_str(), e.provenance.as_str(), e.publisher_kid.as_str()), ("1.6.0", "signed", b.1.did().as_str()));
    }

    #[test]
    fn a_listed_predecessors_successor_is_an_update_and_an_unlisted_one_is_not() {
        let f = fx();
        let (a, b, c) = (key(&f, "a"), key(&f, "b"), key(&f, "c"));
        first_signed(&f, &a);
        set_version(f.src.path(), "1.5.0");
        publisher(f.src.path(), &b.1.did(), &[&a.1.did()]);
        assert_eq!(publish(f.src.path(), &b.0), 0);
        assert_eq!(update(f.home.path(), &f.ws, Some("acme-platform"), false, &Real), 0);
        assert_eq!((lock(&f).bundle[0].provenance.as_str(), lock(&f).bundle[0].publisher_kid.as_str()), ("known-publisher", b.1.did().as_str()));
        // c lists only a, which is no longer the pinned key
        set_version(f.src.path(), "1.6.0");
        publisher(f.src.path(), &c.1.did(), &[&a.1.did()]);
        assert_eq!(publish(f.src.path(), &c.0), 0);
        assert_eq!(update(f.home.path(), &f.ws, Some("acme-platform"), false, &Real), 1);
        assert_eq!(lock(&f).bundle[0].version, "1.5.0");
    }

    #[test]
    fn an_unsigned_bundles_later_version_is_a_re_add_and_a_signed_one_turned_unsigned_is_a_publisher_change() {
        let f = fx();
        assert_eq!(bundle_add::add(f.home.path(), &f.ws, f.src.path(), &opts(), &Real), 0);
        set_version(f.src.path(), "1.4.1");
        delphi_bundle::write_manifest(f.src.path(), true, &Real).unwrap();
        assert_eq!(update(f.home.path(), &f.ws, None, false, &Real), 1);
        assert_eq!(lock(&f).bundle[0].version, "1.4.0");
        assert_eq!(bundle_add::add(f.home.path(), &f.ws, f.src.path(), &opts(), &Real), 0, "the re-add is the operator's act");
        assert_eq!(lock(&f).bundle[0].version, "1.4.1");
        // signed, then the publisher ships an unsigned build
        let g = fx();
        let a = key(&g, "a");
        first_signed(&g, &a);
        set_version(g.src.path(), "1.5.0");
        fs::remove_file(g.src.path().join(".delphi-bundle/sig.json")).unwrap();
        delphi_bundle::write_manifest(g.src.path(), true, &Real).unwrap();
        assert_eq!(update(g.home.path(), &g.ws, None, false, &Real), 1);
        assert_eq!(bundle_add::add(g.home.path(), &g.ws, g.src.path(), &AddOpts { replace_publisher: true, ..opts() }, &Real), 0);
        assert_eq!((lock(&g).bundle[0].provenance.as_str(), lock(&g).bundle[0].publisher_kid.as_str()), ("unsigned", "unsigned"));
    }

    #[test]
    fn update_from_an_archive_source_works_and_a_source_that_holds_another_bundle_is_refused() {
        let f = fx();
        let a = key(&f, "a");
        publisher(f.src.path(), &a.1.did(), &[]);
        assert_eq!(publish(f.src.path(), &a.0), 0);
        let out = tempfile::tempdir().unwrap();
        let arc = out.path().join("b.xbundle");
        assert_eq!(bundle_pack::package(f.src.path(), Some(&arc), &Real), 0);
        assert_eq!(bundle_add::add(f.home.path(), &f.ws, &arc, &opts(), &Real), 0);
        set_version(f.src.path(), "1.5.0");
        assert_eq!(publish(f.src.path(), &a.0), 0);
        assert_eq!(bundle_pack::package(f.src.path(), Some(&arc), &Real), 0); // the same path now holds 1.5.0
        assert_eq!(update(f.home.path(), &f.ws, None, false, &Real), 0);
        assert_eq!(lock(&f).bundle[0].version, "1.5.0");
        assert_eq!(update(f.home.path(), &f.ws, None, false, &Real), 0, "up to date is not an error");
        assert_eq!(lock(&f).bundle[0].history.len(), 1);
        // the archive path now holds some other bundle
        toml_edit(f.src.path(), |t| t.replacen("name = \"acme-platform\"", "name = \"acme-other\"", 1));
        for entry in walk(f.src.path()) {
            if entry.extension().is_some_and(|x| x == "md") {
                let t = fs::read_to_string(&entry).unwrap();
                fs::write(&entry, t.replace("source_mission: acme-platform", "source_mission: acme-other")).unwrap();
            }
        }
        assert_eq!(publish(f.src.path(), &a.0), 0);
        assert_eq!(bundle_pack::package(f.src.path(), Some(&arc), &Real), 0);
        assert_eq!(update(f.home.path(), &f.ws, None, false, &Real), 1);
        assert_eq!(lock(&f).bundle.len(), 1);
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut v = Vec::new();
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() { v.extend(walk(&p)) } else { v.push(p) }
        }
        v
    }

    #[test]
    fn rollback_re_points_the_lock_at_an_earlier_cached_version_with_its_provenance_and_marks_the_move() {
        let f = fx();
        let (a, b) = (key(&f, "a"), key(&f, "b"));
        first_signed(&f, &a);
        let v1 = lock(&f).bundle[0].clone();
        set_version(f.src.path(), "1.5.0");
        publisher(f.src.path(), &b.1.did(), &[&a.1.did()]);
        assert_eq!(publish(f.src.path(), &b.0), 0);
        assert_eq!(update(f.home.path(), &f.ws, None, false, &Real), 0);
        let v2 = lock(&f).bundle[0].clone();
        assert_eq!(rollback(f.home.path(), &f.ws, "no-such", None, false, &Real), 1);
        assert_eq!(rollback(f.home.path(), &f.ws, "acme-platform", Some("00000000"), false, &Real), 1, "not in the history");
        assert_eq!(rollback(f.home.path(), &f.ws, "acme-platform", Some(&short(&v2.hash)), false, &Real), 1, "already current");
        assert_eq!(rollback(f.home.path(), &f.ws, "acme-platform", None, false, &Real), 0);
        let e = lock(&f).bundle[0].clone();
        assert_eq!((e.version.as_str(), e.hash.as_str(), e.publisher_kid.as_str(), e.provenance.as_str()), ("1.4.0", v1.hash.as_str(), a.1.did().as_str(), "signed"));
        assert_eq!((e.history.len(), e.history[0].hash.as_str(), e.history[0].left_by.as_str()), (1, v2.hash.as_str(), "rollback"));
        // the knowledge of 1.5.0 left the index with the move
        let kb = delphi_engine::kb::Kb::open(&f.ws.join("local/state.db")).unwrap();
        assert!(kb.search("Note for 1.5.0", &delphi_engine::kb::SearchOpts { legs: delphi_engine::kb::Legs::LexicalOnly, ..Default::default() }).unwrap().iter().all(|h| !h.preview.contains("Note for 1.5.0")));
        // and forward again by hash prefix
        assert_eq!(rollback(f.home.path(), &f.ws, "acme-platform", Some(&format!("blake3:{}", &short(&v2.hash)[..10])), false, &Real), 0);
        assert_eq!(lock(&f).bundle[0].version, "1.5.0");
        assert_eq!(lock(&f).bundle[0].history[0].version, "1.4.0");
    }

    #[test]
    fn rollback_refuses_a_tree_that_changed_in_the_cache_or_is_gone() {
        let f = fx();
        let a = key(&f, "a");
        first_signed(&f, &a);
        let v1 = lock(&f).bundle[0].clone();
        set_version(f.src.path(), "1.5.0");
        assert_eq!(publish(f.src.path(), &a.0), 0);
        assert_eq!(update(f.home.path(), &f.ws, None, false, &Real), 0);
        let cached = bundles::cache_dir(f.home.path(), "acme", "acme-platform", &v1.hash);
        let readme = cached.join("README.md");
        let original = fs::read(&readme).unwrap();
        fs::write(&readme, "tampered\n").unwrap();
        assert_eq!(rollback(f.home.path(), &f.ws, "acme-platform", None, false, &Real), 1);
        assert_eq!(lock(&f).bundle[0].version, "1.5.0", "nothing changed");
        fs::write(&readme, original).unwrap();
        fs::remove_dir_all(&cached).unwrap();
        assert_eq!(rollback(f.home.path(), &f.ws, "acme-platform", None, false, &Real), 1);
    }

    #[test]
    fn gc_keeps_three_hashes_per_bundle_marks_the_rest_gone_and_spares_trees_another_workspace_needs() {
        let f = fx();
        let a = key(&f, "a");
        first_signed(&f, &a);
        let first = lock(&f).bundle[0].clone();
        // a second workspace has the first version too
        let ws2 = f.home.path().join("workspaces/w2");
        fs::create_dir_all(ws2.join("local")).unwrap();
        assert_eq!(bundle_add::add(f.home.path(), &ws2, f.src.path(), &opts(), &Real), 0);
        for v in ["1.5.0", "1.6.0", "1.7.0"] {
            set_version(f.src.path(), v);
            assert_eq!(publish(f.src.path(), &a.0), 0);
            assert_eq!(update(f.home.path(), &f.ws, None, false, &Real), 0);
        }
        assert_eq!(lock(&f).bundle[0].history.len(), 3);
        assert_eq!(gc(f.home.path(), &f.ws), 0);
        let e = lock(&f).bundle[0].clone();
        assert_eq!(e.history.iter().map(|p| p.gone).collect::<Vec<_>>(), vec![false, false, true], "current plus two earlier are kept");
        let dir = |h: &str| bundles::cache_dir(f.home.path(), "acme", "acme-platform", h);
        assert!(dir(&e.hash).is_dir() && dir(&e.history[0].hash).is_dir() && dir(&e.history[1].hash).is_dir());
        assert!(dir(&first.hash).is_dir(), "the other workspace still needs the first version");
        assert_eq!(rollback(f.home.path(), &f.ws, "acme-platform", Some(&short(&first.hash)), false, &Real), 1, "the lock says it is gone");
        // once that workspace drops it as well, the tree goes
        assert_eq!(bundle_add::remove(&ws2, "acme-platform"), 0);
        assert_eq!(gc(f.home.path(), &f.ws), 0);
        assert!(!dir(&first.hash).exists());
        assert!(dir(&e.hash).is_dir());
        // the verdict file beside a tree goes with it, and an orphan one is swept
        let verdict = |h: &str| delphi_engine::bundle_vectors::verdicts_path(&dir(h));
        fs::write(verdict(&e.hash), "{}").unwrap();
        fs::write(verdict("deadbeef"), "{}").unwrap();
        assert_eq!(gc(f.home.path(), &f.ws), 0);
        assert!(verdict(&e.hash).is_file() && !verdict("deadbeef").exists(), "a live tree keeps its verdict; an orphan is removed");
        // a removed bundle's trees go at gc, with their verdicts and the directories above them
        assert_eq!(bundle_add::remove(&f.ws, "acme-platform"), 0);
        assert_eq!(gc(f.home.path(), &f.ws), 0);
        assert!(!verdict(&e.hash).exists() && !dir(&e.hash).exists(), "tree and verdict are both gone");
        assert!(!f.home.path().join("bundles/acme").exists(), "the empty org and name directories are removed");
        assert!(lock(&f).bundle[0].history.iter().all(|p| p.gone));
    }

    #[test]
    fn update_starts_a_new_hash_without_a_vector_verdict_and_rollback_keeps_the_older_trees_verdict() {
        let f = fx();
        let a = key(&f, "a");
        first_signed(&f, &a);
        let v1 = lock(&f).bundle[0].clone();
        let verdict = |h: &str| delphi_engine::bundle_vectors::verdicts_path(&bundles::cache_dir(f.home.path(), "acme", "acme-platform", h));
        fs::write(verdict(&v1.hash), "{\"caches\":{}}").unwrap();
        set_version(f.src.path(), "1.5.0");
        assert_eq!(publish(f.src.path(), &a.0), 0);
        assert_eq!(update(f.home.path(), &f.ws, None, false, &Real), 0);
        let v2 = lock(&f).bundle[0].clone();
        assert!(!verdict(&v2.hash).exists(), "the new hash starts without a verdict; the vector pass writes its own");
        assert!(verdict(&v1.hash).is_file(), "the old tree keeps its verdict while it is in the cache");
        assert_eq!(rollback(f.home.path(), &f.ws, "acme-platform", None, false, &Real), 0);
        assert_eq!(fs::read_to_string(verdict(&v1.hash)).unwrap(), "{\"caches\":{}}", "a rollback leaves the verdict as it was");
        assert_eq!(lock(&f).bundle[0].hash, v1.hash);
    }

    #[test]
    fn hash_prefixes_are_strict() {
        for ok in ["abcdef12", "blake3:abcdef12", &"a".repeat(64)] {
            assert!(hash_prefix(ok).is_some(), "{ok}");
        }
        for bad in ["", "abc", "ABCDEF12", "--yes", "abcdefg1", "../../etc", &"a".repeat(65)] {
            assert!(hash_prefix(bad).is_none(), "{bad}");
        }
    }
}
