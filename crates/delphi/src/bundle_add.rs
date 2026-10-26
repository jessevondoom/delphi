//! `delphi bundle add|list|remove` (`specs/024` §4): adopt an org bundle from a directory.
//!
//! Adding is the operator's explicit act (OD-1): it runs every check of `specs/023` §10 on this install, prints what the
//! bundle brings, copies the verified tree into the content-addressed cache, records it in the workspace's
//! `local/bundles.lock` and indexes its knowledge so the words are searchable at once. No model is called and no card is
//! raised. Behavior, skills and agents load in slice B4; shipped vectors are verified and used in B3 (`bundle_vectors`);
//! signatures are verified in B6.

use delphi_bundle::{Host, Inspected, Options};
use delphi_core::bundles::{self, Entry, Past, Source};
use std::path::Path;

pub struct AddOpts {
    /// `--skill-roles <skill>=<role,role>`: narrow the roles a skill loads for (`specs/023` §5A).
    pub skill_roles: std::collections::BTreeMap<String, Vec<String>>,
    pub order: Option<i64>,
    pub dry_run: bool,
    pub yes: bool,
    /// `--no-vectors`: shipped vector caches are ignored and every chunk is embedded here (`specs/027` §5.2).
    pub no_vectors: bool,
    /// Start the embedding of chunks without a vector in a detached process, as the command line does. Off in tests.
    pub embed_in_background: bool,
}

fn fail(msg: impl AsRef<str>) -> i32 {
    eprintln!("delphi bundle: {}", msg.as_ref());
    1
}

/// `name` or `org/name` against an entry.
fn matches(e: &Entry, name: &str) -> bool {
    e.name == name || e.id() == name
}

fn short(hash: &str) -> String {
    hash.trim_start_matches("blake3:").chars().take(12).collect()
}

fn print_summary(i: &Inspected, m: &delphi_bundle::Manifest, hash: &str, dir: &Path) {
    let s = &i.summary;
    println!("{} ({}) {}@{}", m.title, m.name, m.org, m.version);
    println!("  publisher: {} · provenance: unsigned (signatures are verified from slice B6) · bundle {}", m.publisher_name, short(hash));
    println!("  review by {} · audience {}", m.review_by, m.audience);
    println!("\n  README.md:");
    for l in &s.readme_head {
        println!("    {}", l);
    }
    if s.readme_more > 0 {
        println!("    … {} more lines in {}/README.md", s.readme_more, dir.display());
    }
    println!("\n  brings:");
    println!("    knowledge: {} files, {} KiB (searched, never always loaded; tier federated_verified)", s.knowledge_files, s.knowledge_bytes / 1024);
    if !s.skills.is_empty() {
        let list: Vec<String> = s.skills.iter().map(|(n, r)| format!("{} ({})", n, r.join(", "))).collect();
        println!("    skills: {} (load with slice B4)", list.join("; "));
    }
    if !s.agents.is_empty() {
        let list: Vec<String> = s
            .agents
            .iter()
            .map(|(n, role, dropped)| match role {
                Some(r) if dropped.is_empty() => format!("{} (as {})", n, r),
                Some(r) => format!("{} (as {}, drops {})", n, r, dropped.join(", ")),
                None => format!("{} (Claude Code only)", n),
            })
            .collect();
        println!("    agents: {} (dispatchable with slice B4)", list.join("; "));
    }
    if s.goals + s.perspectives + s.rules + s.policy_entries + s.guidance_roles.len() > 0 {
        println!(
            "    behavior: {} goals, {} perspectives, {} rules, {} policy entries, guidance for {} (loads with slice B4)",
            s.goals,
            s.perspectives,
            s.rules,
            s.policy_entries,
            if s.guidance_roles.is_empty() { "no role".to_string() } else { s.guidance_roles.join(", ") }
        );
    }
    if !s.vector_caches.is_empty() {
        println!("    vectors: caches for {} (used only if they match this install's embedder and pass a local spot check)", s.vector_caches.join(", "));
    }
    if !m.touches_connectors.is_empty() {
        println!("    connectors: {} (inert until mapped)", m.touches_connectors.join(", "));
    }
    println!("    touches roles: {}", m.touches_roles.join(", "));
}

/// Write the verified tree into a staging directory next to the cache, then move it into place. Files get no execute bit.
fn stage(home: &Path, tree: &delphi_bundle::Tree, files_json: &str, dest: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let incoming = home.join("bundles/.incoming").join(format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
    let write = |rel: &str, bytes: &[u8]| -> Result<(), String> {
        let p = incoming.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| format!("{}: {}", rel, e))?;
        std::fs::write(&p, bytes).map_err(|e| format!("{}: {}", rel, e))?;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).map_err(|e| format!("{}: {}", rel, e))
    };
    let result = (|| {
        for e in &tree.entries {
            if e.path == ".delphi-bundle/files.json" {
                continue;
            }
            write(&e.path, &tree.read(&e.path).ok_or_else(|| format!("{}: unreadable", e.path))?)?;
        }
        write(".delphi-bundle/files.json", files_json.as_bytes())?;
        std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(&incoming, dest).map_err(|e| format!("{}: {}", dest.display(), e))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&incoming);
    }
    result
}

pub fn add(home: &Path, ws: &Path, dir: &Path, o: &AddOpts, host: &dyn Host) -> i32 {
    let dir = match dir.canonicalize() {
        Ok(d) => d,
        Err(e) => return fail(format!("{}: {}", dir.display(), e)),
    };
    // 2. Every step of specs/023 §10 runs here, on this install: nothing trusts a publisher's "it passed".
    let i = delphi_bundle::inspect(&dir, &Options::default(), host);
    for w in &i.report.warns {
        eprintln!("warning: step {}: {}{}", w.step, if w.at.is_empty() { String::new() } else { format!("{}: ", w.at) }, w.msg);
    }
    let (Some(m), Some(hash), Some(files_json)) = (i.manifest.clone(), i.hash.clone(), i.files_json.clone()) else {
        print!("{}", i.report.render(&dir.display().to_string()));
        return fail("the bundle failed validation; nothing was added");
    };
    if !i.report.passed() {
        print!("{}", i.report.render(&dir.display().to_string()));
        return fail("the bundle failed validation; nothing was added");
    }
    if !bundles::plain_id(&m.org) || !bundles::plain_id(&m.name) {
        return fail("org and name must be plain ids");
    }
    for (skill, roles) in &o.skill_roles {
        let Some((_, offered)) = i.summary.skills.iter().find(|(n, _)| n == skill) else {
            return fail(format!("--skill-roles: the bundle has no skill `{}`", skill));
        };
        if let Some(bad) = roles.iter().find(|r| !offered.contains(r)) {
            return fail(format!("--skill-roles: `{}` is not a role the skill `{}` names ({}); a bundle can be narrowed, never widened", bad, skill, offered.join(", ")));
        }
    }
    // 4. Compatibility, dependencies and supersession.
    if let Some(req) = &m.compat_delphi {
        let running = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
        if !semver::VersionReq::parse(req).is_ok_and(|r| r.matches(&running)) {
            return fail(format!("this bundle needs delphi {} and this is {}", req, running));
        }
    }
    let lock = match bundles::read(ws) {
        Ok(l) => l,
        Err(e) => return fail(e),
    };
    for dep in &m.depends_on {
        let (name, req) = dep.split_once('@').unwrap();
        let ok = lock.bundle.iter().any(|e| e.is_active() && matches(e, name) && semver::Version::parse(&e.version).is_ok_and(|v| semver::VersionReq::parse(req).is_ok_and(|r| r.matches(&v))));
        if !ok {
            return fail(format!("it depends on {}; add a satisfying version first", dep));
        }
    }
    let superseded: Vec<&Entry> = lock.bundle.iter().filter(|e| e.is_active() && m.supersedes.iter().any(|s| matches(e, s))).collect();
    // 7.1. Two bundles must not define the same id unless the later one supersedes the earlier.
    for other in lock.bundle.iter().filter(|e| e.is_active() && !(e.org == m.org && e.name == m.name) && !m.supersedes.iter().any(|s| matches(e, s))) {
        let theirs = delphi_bundle::ids_of(&bundles::cache_dir(home, &other.org, &other.name, &other.hash));
        let clash: Vec<&String> = i.ids.intersection(&theirs).collect();
        if !clash.is_empty() {
            let shown: Vec<&str> = clash.iter().take(5).map(|s| s.as_str()).collect();
            return fail(format!("{} also defines {} (and {} more); name it in overlays.supersedes or rename: nothing was added", other.id(), shown.join(", "), clash.len().saturating_sub(5)));
        }
    }
    // 5. The summary, non-blocking.
    print_summary(&i, &m, &hash, &dir);
    for s in &superseded {
        println!("  supersedes {} ({})", s.id(), if o.yes { "will be removed" } else { "kept; pass --yes to remove it" });
    }
    if o.dry_run {
        println!("\n(dry run: nothing was added)");
        return 0;
    }
    // 6. Cache, lock, index.
    let dest = bundles::cache_dir(home, &m.org, &m.name, &hash);
    if !dest.is_dir() {
        let tree = delphi_bundle::Tree::open(&dir);
        if let Err(e) = stage(home, &tree, &files_json, &dest) {
            return fail(e);
        }
    }
    let now = delphi_core::time::now_rfc3339();
    let source = Source { kind: "directory".into(), path: dir.display().to_string() };
    let skill_roles = o.skill_roles.clone();
    let removed: Vec<String> = if o.yes { superseded.iter().map(|e| e.id()).collect() } else { Vec::new() };
    let applied = bundles::update(ws, |lock| {
        for e in lock.bundle.iter_mut().filter(|e| removed.contains(&e.id())) {
            e.status = "removed".into();
        }
        let next_order = lock.bundle.iter().filter(|e| e.is_active()).map(|e| e.order).max().unwrap_or(0) + 10;
        let fresh = |order: i64| Entry {
            org: m.org.clone(),
            name: m.name.clone(),
            version: m.version.clone(),
            hash: hash.clone(),
            publisher_kid: "unsigned".into(),
            provenance: "unsigned".into(),
            source: source.clone(),
            added_at: now.clone(),
            order,
            status: "active".into(),
            review_by: m.review_by.clone(),
            title: m.title.clone(),
            skill_roles: skill_roles.clone(),
            history: Vec::new(),
        };
        match lock.bundle.iter_mut().find(|e| e.org == m.org && e.name == m.name) {
            Some(e) => {
                let mut history = e.history.clone();
                if e.hash != hash {
                    history.insert(0, Past { hash: e.hash.clone(), version: e.version.clone(), from: e.added_at.clone(), to: now.clone() });
                }
                let order = o.order.unwrap_or(e.order);
                let kept = if skill_roles.is_empty() { e.skill_roles.clone() } else { skill_roles.clone() };
                *e = Entry { history, skill_roles: kept, ..fresh(order) };
                Ok("updated")
            }
            None => {
                lock.bundle.push(fresh(o.order.unwrap_or(next_order)));
                Ok("added")
            }
        }
    });
    let verb = match applied {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    match delphi_engine::kb::sync_workspace(ws) {
        Ok(r) => println!("\n{} {}/{}@{}: indexed its knowledge ({} added, {} updated, {} removed)", verb, m.org, m.name, m.version, r.added, r.updated, r.removed),
        Err(e) => return fail(format!("added, but the index could not be updated: {} (run `delphi kb index`)", e)),
    }
    for id in &removed {
        println!("removed superseded {}", id);
    }
    vectors(home, ws, &m.org, &m.name, o);
    println!("search it with `delphi kb search <words> --bundle {}/{}`; its files are @bundle/{}/{}/knowledge/**", m.org, m.name, m.org, m.name);
    0
}

/// Verify and apply a shipped vector cache, say how far the bundle's vectors are, and leave the rest to a detached
/// embedding pass that a restart or a second `add` simply continues (`specs/027` §5.4). Nothing here fails the add:
/// without vectors the bundle is searched by its words.
fn vectors(home: &Path, ws: &Path, org: &str, name: &str, o: &AddOpts) {
    let Some(entry) = bundles::read(ws).ok().and_then(|l| l.bundle.into_iter().find(|e| e.is_active() && e.org == org && e.name == name)) else { return };
    let a = match delphi_engine::bundle_vectors::adopt(home, ws, &entry, o.no_vectors) {
        Ok(a) => a,
        Err(e) => return println!("vectors: not prepared ({}); the words are searchable and `delphi kb index` embeds the rest", e),
    };
    for n in &a.notes {
        println!("{}", n);
    }
    let (done, total) = a.counts;
    if total == 0 || done >= total {
        return;
    }
    if !a.model {
        println!("vectors: {} chunk(s) await an embedding model; until one is installed this bundle is searched by its words", total - done);
    } else if o.embed_in_background && spawn_embedding(home, ws) {
        println!("vectors: {}% ready; embedding the other {} chunk(s) in the background (`delphi bundle list` shows progress; search stays lexical for this bundle until then)", done * 100 / total, total - done);
    } else {
        println!("vectors: {}% ready; `delphi kb index` embeds the other {} chunk(s)", done * 100 / total, total - done);
    }
}

/// `delphi kb index` in a detached process: it embeds every chunk that has no vector and, being resumable, carries on
/// from the table after any interruption.
fn spawn_embedding(home: &Path, ws: &Path) -> bool {
    use std::os::unix::process::CommandExt;
    let (Ok(exe), Some(name)) = (std::env::current_exe(), ws.file_name()) else { return false };
    let null = std::process::Stdio::null;
    std::process::Command::new(exe).args(["kb", "index", "--home"]).arg(home).arg("--workspace").arg(name).stdin(null()).stdout(null()).stderr(null()).process_group(0).spawn().is_ok()
}

pub fn list(home: &Path, ws: &Path) -> i32 {
    let lock = match bundles::read(ws) {
        Ok(l) => l,
        Err(e) => return fail(e),
    };
    let today = bundles::today();
    let shown: Vec<&Entry> = lock.bundle.iter().filter(|e| e.is_active()).collect();
    if shown.is_empty() {
        println!("no bundles added; `delphi bundle add <dir>` adds one");
        return 0;
    }
    let entries: Vec<Entry> = shown.iter().map(|e| (*e).clone()).collect();
    let status = delphi_engine::bundle_vectors::statuses(home, ws, &entries);
    for e in shown {
        let vectors = status.get(&e.id()).map(|s| format!("  vectors {}", s.label())).unwrap_or_default();
        println!("{:<28} {:<10} {:<10} order {:<4} {}{}{}", e.id(), e.version, e.provenance, e.order, short(&e.hash), vectors, if e.is_stale(&today) { format!("  STALE (review by {})", e.review_by) } else { String::new() });
    }
    0
}

pub fn remove(ws: &Path, name: &str) -> i32 {
    let found = bundles::update(ws, |lock| {
        let ids: Vec<String> = lock.bundle.iter().filter(|e| e.is_active() && matches(e, name)).map(|e| e.id()).collect();
        match ids.len() {
            0 => Err(format!("no added bundle named `{}`", name)),
            1 => {
                lock.bundle.iter_mut().filter(|e| e.id() == ids[0]).for_each(|e| e.status = "removed".into());
                Ok(ids[0].clone())
            }
            _ => Err(format!("`{}` is ambiguous ({}); use org/name", name, ids.join(", "))),
        }
    });
    let id = match found {
        Ok(id) => id,
        Err(e) => return fail(e),
    };
    match delphi_engine::kb::sync_workspace(ws) {
        Ok(r) => println!("removed {}: {} documents left the index; the cache entry stays until `gc`", id, r.removed),
        Err(e) => return fail(format!("removed, but the index could not be updated: {} (run `delphi kb index`)", e)),
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle_cmd::Real;
    use delphi_engine::kb::{Kb, SearchOpts};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
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

    /// A home with one empty workspace `w`, and a copy of the golden bundle that can be edited.
    fn setup() -> (tempfile::TempDir, PathBuf, tempfile::TempDir) {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        fs::create_dir_all(ws.join("local")).unwrap();
        let src = tempfile::tempdir().unwrap();
        copy(&golden(), src.path());
        (home, ws, src)
    }

    fn opts() -> AddOpts {
        AddOpts { skill_roles: Default::default(), order: None, dry_run: false, yes: false, no_vectors: false, embed_in_background: false }
    }

    /// Re-write the copy's manifest after an edit, so step 5 sees a consistent tree.
    fn repack(dir: &Path) {
        delphi_bundle::write_manifest(dir, true, &Real).unwrap();
    }

    fn edit(dir: &Path, rel: &str, from: &str, to: &str) {
        let p = dir.join(rel);
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains(from), "{} lacks {}", rel, from);
        fs::write(p, text.replacen(from, to, 1)).unwrap();
    }

    /// `source_mission` is the bundle name in every document (specs/023 §4.2).
    fn rename_mission(dir: &Path, from: &str, to: &str) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                rename_mission(&p, from, to);
            } else if p.extension().is_some_and(|x| x == "md") {
                let t = fs::read_to_string(&p).unwrap();
                fs::write(&p, t.replace(&format!("source_mission: {}", from), &format!("source_mission: {}", to))).unwrap();
            }
        }
    }

    fn search(ws: &Path, q: &str) -> Vec<String> {
        let mut kb = Kb::open(&ws.join("local/state.db")).unwrap();
        kb.sync_lexical(&delphi_engine::kb::sources(ws), "t").unwrap();
        kb.search(q, &SearchOpts { legs: delphi_engine::kb::Legs::LexicalOnly, ..Default::default() }).unwrap().into_iter().map(|h| h.pointer).collect()
    }

    #[test]
    fn adding_the_golden_bundle_caches_locks_and_indexes_it_with_no_model_and_no_card() {
        let (home, ws, src) = setup();
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 0);
        let lock = bundles::read(&ws).unwrap();
        assert_eq!(lock.bundle.len(), 1);
        let e = &lock.bundle[0];
        assert_eq!((e.id().as_str(), e.version.as_str(), e.provenance.as_str(), e.status.as_str(), e.order), ("acme/acme-platform", "1.4.0", "unsigned", "active", 10));
        assert!(e.hash.starts_with("blake3:") && e.history.is_empty());
        // the cache entry is the verified tree, files without the execute bit, with the manifest that names its identity
        let cache = bundles::cache_dir(home.path(), "acme", "acme-platform", &e.hash);
        for rel in ["bundle.toml", "knowledge/index.md", "behavior/goals.md", ".delphi-bundle/files.json"] {
            let mode = fs::metadata(cache.join(rel)).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0, "{} must not be executable", rel);
        }
        let files_json = fs::read(cache.join(".delphi-bundle/files.json")).unwrap();
        assert_eq!(delphi_bundle::bundle_hash(&files_json), e.hash);
        assert!(!home.path().join("bundles/.incoming").read_dir().map(|mut d| d.next().is_some()).unwrap_or(false), "no staging directory is left behind");
        // knowledge is searchable at once by its words, under the agent-readable path
        let hits = search(&ws, "why not DynamoDB for billing");
        assert_eq!(hits.first().map(String::as_str), Some("@bundle/acme/acme-platform/knowledge/decisions/0042-postgres-over-dynamo.md#L14-L15"), "{:?}", hits);
        // behavior, skills and agents are not readable as files (they load as what they are, in slice B4)
        assert!(!ws.join("team").exists(), "nothing was written into the team's own files");
    }

    #[test]
    fn a_shipped_vector_cache_is_never_trusted_without_a_model_and_the_add_still_succeeds() {
        let (home, ws, src) = setup();
        let texts = delphi_bundle::Tree::open(src.path()).texts("knowledge/", ".md");
        let entries: Vec<_> = delphi_engine::bundle_vectors::chunk_keys("acme", "acme-platform", &texts).into_iter().map(|k| (k, vec![vec![0.5f32, 0.5]])).collect();
        fs::create_dir_all(src.path().join("vectors")).unwrap();
        fs::write(src.path().join("vectors/m.dlphvec"), delphi_engine::embed::vectors::VecCache::serialize("m", 2, "1", &entries)).unwrap();
        let index = format!("{{\"vectors\":1,\"caches\":[{{\"file\":\"m.dlphvec\",\"model_id\":\"m\",\"dim\":2,\"embedding_format\":\"1\",\"chunker_version\":\"2\",\"chunks\":{},\"windows\":{},\"dtype\":\"f32\"}}]}}\n", entries.len(), entries.len());
        fs::write(src.path().join("vectors/index.json"), index).unwrap();
        edit(src.path(), "bundle.toml", "knowledge = true", "knowledge = true\nvectors = true");
        repack(src.path());
        for no_vectors in [false, true] {
            assert_eq!(add(home.path(), &ws, src.path(), &AddOpts { no_vectors, ..opts() }, &Real), 0);
            let e = bundles::read(&ws).unwrap().bundle.remove(0);
            let s = &delphi_engine::bundle_vectors::statuses(home.path(), &ws, &[e])["acme/acme-platform"];
            assert!(s.total > 0 && s.done == 0 && !s.shipped, "no model: nothing from the cache is used ({:?})", s);
            assert_eq!(s.label(), "0%");
        }
        assert_eq!(list(home.path(), &ws), 0);
    }

    #[test]
    fn a_dry_run_changes_nothing_and_an_invalid_bundle_is_refused_before_anything_is_written() {
        let (home, ws, src) = setup();
        assert_eq!(add(home.path(), &ws, src.path(), &AddOpts { dry_run: true, ..opts() }, &Real), 0);
        assert!(!ws.join("local/bundles.lock").exists() && !home.path().join("bundles").exists());
        fs::write(src.path().join("README.md"), "changed after packaging\n").unwrap(); // no longer matches files.json
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 1);
        fs::remove_file(src.path().join("README.md")).unwrap();
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 1);
        assert!(!ws.join("local/bundles.lock").exists() && !home.path().join("bundles").exists(), "a failed add leaves no trace");
    }

    #[test]
    fn a_new_version_updates_the_entry_and_keeps_the_old_hash_in_history() {
        let (home, ws, src) = setup();
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 0);
        let first = bundles::read(&ws).unwrap().bundle[0].hash.clone();
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 0, "adding the same hash again is harmless");
        assert!(bundles::read(&ws).unwrap().bundle[0].history.is_empty());
        edit(src.path(), "knowledge/glossary.md", "Budget", "Latency budget");
        edit(src.path(), "bundle.toml", "version = \"1.4.0\"", "version = \"1.4.1\"");
        repack(src.path());
        assert_eq!(add(home.path(), &ws, src.path(), &AddOpts { order: Some(30), ..opts() }, &Real), 0);
        let lock = bundles::read(&ws).unwrap();
        assert_eq!(lock.bundle.len(), 1);
        let e = &lock.bundle[0];
        assert_eq!((e.version.as_str(), e.order, e.history.len()), ("1.4.1", 30, 1));
        assert_eq!((e.history[0].hash.as_str(), e.history[0].version.as_str()), (first.as_str(), "1.4.0"));
        assert_ne!(e.hash, first);
        assert!(bundles::cache_dir(home.path(), "acme", "acme-platform", &first).is_dir(), "the old tree stays for rollback");
        assert!(!search(&ws, "latency budget").is_empty());
    }

    #[test]
    fn two_bundles_may_not_define_the_same_id_unless_the_later_one_supersedes_the_earlier() {
        let (home, ws, src) = setup();
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 0);
        // another bundle (other name) that reuses the same ids: refused, nothing added
        let other = tempfile::tempdir().unwrap();
        copy(&golden(), other.path());
        edit(other.path(), "bundle.toml", "name = \"acme-platform\"", "name = \"acme-copy\"");
        rename_mission(other.path(), "acme-platform", "acme-copy"); // valid on its own; only the ids collide
        repack(other.path());
        assert!(delphi_bundle::validate(other.path(), &delphi_bundle::Options::default(), &Real).passed(), "the copy is a valid bundle");
        assert_eq!(add(home.path(), &ws, other.path(), &opts(), &Real), 1);
        assert_eq!(bundles::read(&ws).unwrap().bundle.len(), 1, "refused for the collision, nothing added");
        // naming the earlier bundle in overlays.supersedes lifts the refusal; --yes also removes it
        edit(other.path(), "bundle.toml", "[touches]", "[overlays]\nsupersedes = [\"acme-platform\"]\n\n[touches]");
        repack(other.path());
        assert_eq!(add(home.path(), &ws, other.path(), &AddOpts { yes: true, ..opts() }, &Real), 0);
        let lock = bundles::read(&ws).unwrap();
        let status = |n: &str| lock.bundle.iter().find(|e| e.name == n).map(|e| e.status.clone());
        assert_eq!((status("acme-platform").as_deref(), status("acme-copy").as_deref()), (Some("removed"), Some("active")));
    }

    #[test]
    fn dependencies_and_the_compat_range_are_checked_before_anything_is_written() {
        let (home, ws, src) = setup();
        edit(src.path(), "bundle.toml", "[touches]", "[overlays]\ndepends_on = [\"acme-security@>=2.0.0\"]\n\n[touches]");
        repack(src.path());
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 1, "a dependency that is not added");
        edit(src.path(), "bundle.toml", "depends_on = [\"acme-security@>=2.0.0\"]", "depends_on = []");
        edit(src.path(), "bundle.toml", "delphi = \">=0.2.0\"", "delphi = \">=99.0.0\"");
        repack(src.path());
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 1, "a bundle for a newer delphi");
        assert!(!ws.join("local/bundles.lock").exists());
    }

    #[test]
    fn skill_roles_narrow_a_skill_at_add_never_widen_it_and_survive_a_re_add() {
        let (home, ws, src) = setup();
        let narrow = |skill: &str, roles: &[&str]| AddOpts { skill_roles: [(skill.to_string(), roles.iter().map(|r| r.to_string()).collect())].into(), ..opts() };
        assert_eq!(add(home.path(), &ws, src.path(), &narrow("no-such-skill", &["builder"]), &Real), 1, "a skill the bundle lacks");
        assert_eq!(add(home.path(), &ws, src.path(), &narrow("migration-plan", &["strategist"]), &Real), 1, "a role the skill does not name is a widening");
        assert!(!ws.join("local/bundles.lock").exists(), "refused before anything is written");
        assert_eq!(add(home.path(), &ws, src.path(), &narrow("migration-plan", &["builder"]), &Real), 0);
        let narrowed = |ws: &Path| bundles::read(ws).unwrap().bundle[0].skill_roles.clone();
        assert_eq!(narrowed(&ws).get("migration-plan"), Some(&vec!["builder".to_string()]));
        // an update without the flag keeps what the operator chose
        edit(src.path(), "knowledge/glossary.md", "Budget", "Latency budget");
        edit(src.path(), "bundle.toml", "version = \"1.4.0\"", "version = \"1.4.1\"");
        repack(src.path());
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 0);
        assert_eq!(narrowed(&ws).get("migration-plan"), Some(&vec!["builder".to_string()]));
    }

    #[test]
    fn remove_takes_the_knowledge_out_of_search_and_leaves_the_cache_for_gc() {
        let (home, ws, src) = setup();
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 0);
        assert_eq!(remove(&ws, "no-such"), 1);
        assert_eq!(remove(&ws, "acme-platform"), 0);
        assert!(search(&ws, "DynamoDB").is_empty());
        let e = &bundles::read(&ws).unwrap().bundle[0];
        assert_eq!(e.status, "removed");
        assert!(bundles::cache_dir(home.path(), "acme", "acme-platform", &e.hash).is_dir());
        assert_eq!(list(home.path(), &ws), 0);
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 0, "a removed bundle can be added again");
        assert!(!search(&ws, "DynamoDB").is_empty());
    }

    #[test]
    fn a_stale_bundle_is_listed_as_stale_and_still_searchable() {
        let (home, ws, src) = setup();
        edit(src.path(), "bundle.toml", "2027-01-31", "2020-01-31");
        repack(src.path());
        assert_eq!(add(home.path(), &ws, src.path(), &opts(), &Real), 0, "a past review_by warns, it does not refuse");
        assert!(bundles::read(&ws).unwrap().bundle[0].is_stale(&bundles::today()));
        assert!(!search(&ws, "DynamoDB").is_empty(), "nothing is unloaded");
    }
}
