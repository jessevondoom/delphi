//! Tests of `bundle_xenia.rs`: a bundle through a real (local) git store, played by the attacker. The store is a signed org
//! with a git remote standing in for the host; the bundle trees are real git repositories fetched by the real hardened
//! fetcher (only the https address is mapped to a local path); every record and signature is real. No test reaches a
//! network.

use super::*;
use crate::bundle_cmd::Real;
use crate::bundle_pack;
use delphi_core::xenia_fixture::{self as xf, Fx};
use delphi_core::xenia_publish::{Approver, Decision, Preview};
use delphi_xenia::testkit_records::{self as rk, tree_block, BundleSpec, SignedRecord};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::process::Command;

const URL: &str = "https://git.acme.example/platform/bundle.git";

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

fn b64url(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = c.iter().fold(0u32, |a, b| (a << 8) | *b as u32) << (8 * (3 - c.len()));
        for i in 0..=c.len() {
            out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// The key file of the fixture's member `name` (the fixture derives a key from a one-byte seed).
fn key_file(fx: &Fx, name: &str, dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let seed = (1u8..=40).find(|n| Identity::from_seed([*n; 32]).did() == fx.id(name).did()).expect("a fixture key");
    let p = dir.join(name).join("identity.key");
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, format!("xenia-key/1\n{}\n", b64url(&[seed; 32]))).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
    p
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@t.example", "-c", "commit.gpgsign=false", "-c", "protocol.file.allow=always"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The golden bundle as another org's: the org and every id that carries it.
fn as_vendor(d: &Path) {
    toml_edit(d, |t| t.replacen("org = \"acme\"", "org = \"vendor\"", 1));
    for f in ["behavior/goals.md", "behavior/perspectives.md", "behavior/heuristics.md"] {
        let p = d.join(f);
        fs::write(&p, fs::read_to_string(&p).unwrap().replace("-acme-", "-vendor-")).unwrap();
    }
}

fn toml_edit(dir: &Path, f: impl FnOnce(String) -> String) {
    let p = dir.join("bundle.toml");
    fs::write(&p, f(fs::read_to_string(&p).unwrap())).unwrap();
}

fn set_publisher(dir: &Path, did: &str) {
    toml_edit(dir, |t| {
        let kept: Vec<&str> = t.lines().filter(|l| !l.starts_with("did = ") && !l.starts_with("predecessors = ")).collect();
        kept.join("\n").replacen("[publisher]", &format!("[publisher]\ndid = \"{}\"", did), 1) + "\n"
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

/// What a built bundle is: where it lives, its identity, and the git repository that serves it.
struct Built {
    dir: PathBuf,
    hash: String,
    tree: String,
    repo: PathBuf,
}

struct Site {
    fx: Fx,
    home: PathBuf,
    ws: PathBuf,
    keys: PathBuf,
    repos: RefCell<HashMap<String, PathBuf>>,
    n: RefCell<usize>,
}

fn opts() -> AddOpts {
    AddOpts { skill_roles: Default::default(), order: None, dry_run: false, yes: false, replace_publisher: false, no_vectors: false, embed_in_background: false }
}

/// Maps the record's https address onto a local repository and runs the real fetcher against it.
struct Repos<'a> {
    site: &'a Site,
    max_scratch: u64,
    calls: RefCell<Vec<String>>,
}

impl Fetcher for Repos<'_> {
    fn fetch(&self, url: &str, rev: Rev, subdir: Option<&str>, dest: &Path) -> Result<String, String> {
        self.calls.borrow_mut().push(url.to_string());
        let local = self.site.repos.borrow().get(url).cloned().ok_or_else(|| format!("no such repository: {url}"))?;
        crate::bundle_fetch::GitFetcher { allow_file: true, home: self.site.home.clone(), max_scratch: self.max_scratch }.fetch(local.to_str().unwrap(), rev, subdir, dest)
    }
}

impl Site {
    fn new() -> (tempfile::TempDir, Site) {
        let tmp = tempfile::tempdir().unwrap();
        let (fx, _) = Fx::installed(tmp.path(), &[]);
        let home = fx.dir.join("home");
        let ws = home.join("workspaces/w");
        fs::create_dir_all(ws.join("local")).unwrap();
        let keys = tmp.path().join("keys");
        let site = Site { fx, home, ws, keys, repos: RefCell::new(HashMap::new()), n: RefCell::new(0) };
        (tmp, site)
    }

    fn repos(&self) -> Repos<'_> {
        Repos { site: self, max_scratch: crate::bundle_fetch::MAX_SCRATCH_BYTES, calls: RefCell::new(Vec::new()) }
    }

    fn next(&self) -> usize {
        *self.n.borrow_mut() += 1;
        *self.n.borrow()
    }

    /// A bundle signed by `who` at `version` (a copy of the golden bundle), served from its own git repository at `URL`.
    fn build(&self, who: &str, version: &str, edit: impl FnOnce(&Path)) -> Built {
        let n = self.next();
        let dir = self.keys.parent().unwrap().join(format!("src-{n}"));
        copy(&golden(), &dir);
        let _ = fs::remove_dir_all(dir.join(".delphi-bundle"));
        if version != "1.4.0" {
            set_version(&dir, version);
        }
        let key = key_file(&self.fx, who, &self.keys);
        set_publisher(&dir, &self.fx.id(who).did());
        edit(&dir);
        delphi_bundle::write_manifest(&dir, true, &Real).unwrap();
        assert_eq!(bundle_pack::sign(&dir, &key, None, &Real, &|_: &bundle_pack::Challenge| Ok(true)), 0);
        let i = delphi_bundle::inspect(&dir, &Options::default(), &Real);
        assert!(i.report.passed(), "{}", i.report.render("built"));
        self.serve(&dir, i.hash.unwrap())
    }

    /// Put the directory in a fresh git repository and map `URL` to it.
    fn serve(&self, dir: &Path, hash: String) -> Built {
        let repo = dir.parent().unwrap().join(format!("repo-{}", self.next()));
        copy(dir, &repo);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["add", "-A", "-f"]);
        git(&repo, &["commit", "-q", "-m", "bundle"]);
        let tree = git(&repo, &["rev-parse", "HEAD^{tree}"]);
        self.repos.borrow_mut().insert(URL.to_string(), repo.clone());
        Built { dir: dir.to_path_buf(), hash, tree, repo }
    }

    /// A record of `built` signed by `who`, at `rev`, in the store and synced.
    fn record(&self, who: &str, slot: usize, rev: u64, built: &Built, version: &str, tweak: impl FnOnce(&mut BundleSpec)) -> SignedRecord {
        let mut spec = BundleSpec::new("acme-platform", version, &built.hash);
        spec.roles = "[\"strategist\", \"architect\", \"builder\", \"red-team\"]".into();
        spec.connectors = "[]".into();
        spec.compat = ">=0.2.0".into();
        spec.tree = tree_block(&format!("{URL}#."), "git-tree", &built.tree);
        tweak(&mut spec);
        let rec = rk::bundle(&self.fx.id(who), &xf::u(slot), rev, &spec);
        self.fx.put_commit(&rec, &format!("bundle record {slot} rev {rev}"));
        self.refresh();
        rec
    }

    /// A new freshness statement for the head, issued a little later than the last: an equal `issued_at` with other bytes is a
    /// replay (spec 011 §4.1), which would leave the store stale. Then a verified sync.
    fn refresh(&self) {
        let head = self.fx.remote.commit("noop");
        let n = self.next() as i64;
        self.fx.fresh_at(&head, xf::now() - 3600 + 60 * n, 7 * xf::DAY);
        let r = self.fx.sync();
        assert!(matches!(r, delphi_core::xenia_sync::VerifiedSync::Accepted { .. }), "{r:?}");
    }

    fn endorse(&self, who: &str, eid: usize, rec: &SignedRecord, rev: u64) {
        let e = xf::endorse(&self.fx.id(who), eid, rec, rev, "payments");
        self.fx.put_commit(&e, "endorse");
        self.refresh();
    }

    fn add(&self, id: &str, o: &AddOpts) -> i32 {
        add(&self.home, &self.ws, &self.fx.store, &xf::cfg(), id, o, &self.repos(), &Real)
    }

    fn lock(&self) -> bundles::Lock {
        bundles::read(&self.ws).unwrap()
    }

    fn entry(&self) -> Entry {
        self.lock().bundle.into_iter().find(|e| e.is_active()).expect("an added bundle")
    }

    fn update(&self) -> i32 {
        let e = self.entry();
        update_with(&self.home, &self.ws, &self.fx.store, &xf::cfg(), &e, &opts(), &self.repos(), &Real)
    }

    fn added(&self) -> usize {
        self.lock().bundle.iter().filter(|e| e.is_active()).count()
    }

    fn staging_left(&self) -> bool {
        self.home.join("bundles/.incoming").read_dir().map(|mut d| d.next().is_some()).unwrap_or(false)
    }

    fn nothing_changed_for_the_worse(&self) {
        assert!(!self.staging_left(), "the staging tree is gone");
    }
}

fn id(n: usize) -> String {
    xf::u(n)
}

#[test]
fn a_member_signed_bundle_is_added_from_the_store_and_an_endorsement_makes_it_endorsed() {
    let (_t, s) = Site::new();
    let b = s.build("mia", "1.4.0", |_| {});
    let rec = s.record("mia", 1, 1, &b, "1.4.0", |_| {});
    assert_eq!(s.add(&id(1), &opts()), 0);
    let e = s.entry();
    assert_eq!((e.org.as_str(), e.name.as_str(), e.version.as_str(), e.provenance.as_str()), ("acme", "acme-platform", "1.4.0", "member-signed"));
    assert_eq!((e.source.kind.as_str(), e.source.path.as_str()), ("xenia", format!("xenia:acme/acme-main/{}@1", id(1)).as_str()));
    assert_eq!((e.publisher_kid.as_str(), e.hash.as_str()), (s.fx.id("mia").did().as_str(), b.hash.as_str()));
    assert!(bundles::cache_dir(&s.home, "acme", "acme-platform", &b.hash).join("knowledge/index.md").is_file());
    s.nothing_changed_for_the_worse();
    // A steward of another person endorses exactly these bytes: the same hash, the next sync says endorsed.
    s.endorse("sam", 2, &rec, 1);
    assert_eq!(s.add(&id(1), &opts()), 0);
    assert_eq!(s.entry().provenance, "endorsed");
    assert_eq!(s.lock().bundle.len(), 1, "re-adding the same record updates the one entry");
}

#[test]
fn a_changed_byte_in_the_pinned_tree_a_lying_hash_or_a_moved_head_adds_nothing() {
    let (_t, s) = Site::new();
    let b = s.build("mia", "1.4.0", |_| {});
    // 1. The publisher's record says one hash; the tree they signed has another (the record was not made from this tree).
    s.record("mia", 1, 1, &b, "1.4.0", |sp| sp.bundle_hash = rk_hash(b"another bundle"));
    assert_ne!(s.add(&id(1), &opts()), 0);
    assert_eq!(s.added(), 0);
    // 2. The remote's head is not the pinned tree (a new commit after the record was signed): refused before any file is read.
    let b2 = s.build("mia", "1.4.0", |_| {});
    fs::write(b2.repo.join("README.md"), "changed after the record was signed\n").unwrap();
    git(&b2.repo, &["commit", "-q", "-am", "late edit"]);
    s.record("mia", 2, 1, &b2, "1.4.0", |_| {});
    assert_ne!(s.add(&id(2), &opts()), 0);
    assert_eq!(s.added(), 0);
    // 3. One changed byte in a file whose hash files.json lists, signed into the pinned tree by a careless publisher: the
    //    tree is the pinned one, and the install's own validation catches the byte.
    let b3 = s.build("mia", "1.4.0", |_| {});
    let g = b3.repo.join("knowledge/glossary.md");
    fs::write(&g, fs::read_to_string(&g).unwrap().replacen("a", "b", 1)).unwrap();
    git(&b3.repo, &["commit", "-q", "-am", "tampered"]);
    let tampered_tree = git(&b3.repo, &["rev-parse", "HEAD^{tree}"]);
    s.record("mia", 3, 1, &Built { tree: tampered_tree, ..b3 }, "1.4.0", |_| {});
    assert_ne!(s.add(&id(3), &opts()), 0);
    assert_eq!(s.added(), 0);
    s.nothing_changed_for_the_worse();
}

fn rk_hash(b: &[u8]) -> String {
    delphi_xenia::hash::of(b)
}

fn commit_all(r: &Path) {
    git(r, &["add", "-A", "-f"]);
    git(r, &["commit", "-q", "-m", "evil"]);
}

/// A commit whose root tree is the current one plus one entry written as `git mktree` input, built without `git add` (which
/// refuses some paths a hostile server need not refuse). `false` when git itself would not build such a tree.
fn commit_with_root_entry(r: &Path, entry: &str) -> bool {
    use std::io::Write;
    let base = git(r, &["rev-parse", "HEAD^{tree}"]);
    let listing = git(r, &["ls-tree", &base]);
    let mut c = Command::new("git").current_dir(r).args(["mktree"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(format!("{listing}\n{entry}\n").as_bytes()).unwrap();
    let out = c.wait_with_output().unwrap();
    if !out.status.success() {
        return false;
    }
    let tree = String::from_utf8(out.stdout).unwrap().trim().to_string();
    let commit = git(r, &["-c", "user.name=t", "-c", "user.email=t@t.example", "commit-tree", &tree, "-p", "HEAD", "-m", "evil"]);
    git(r, &["update-ref", "HEAD", &commit]);
    true
}

#[test]
fn hostile_trees_symlinks_submodules_oversize_files_and_bad_paths_are_refused_and_leave_nothing() {
    let (t, s) = Site::new();
    let base = s.build("mia", "1.4.0", |_| {});
    // Each variant is a commit on top of the signed bundle in its own repository, pinned by commit with the real bundle hash.
    // `refused` is what the fetch must say; None means the fetch is fine and the install's own validation must stop it.
    type Make = Box<dyn Fn(&Path) -> bool>;
    let variants: Vec<(&str, Option<&str>, Make)> = vec![
        ("a symbolic link", Some("symbolic link"), Box::new(|r| {
            std::os::unix::fs::symlink("/etc/passwd", r.join("knowledge/passwd-link.md")).unwrap();
            commit_all(r);
            true
        })),
        ("a submodule", Some("submodule"), Box::new(|r| {
            git(r, &["add", "-A", "-f"]);
            let sha = git(r, &["rev-parse", "HEAD"]);
            git(r, &["update-index", "--add", "--cacheinfo", &format!("160000,{sha},vendor/sub")]);
            git(r, &["commit", "-q", "-m", "evil"]);
            true
        })),
        ("a .git directory at the root", Some(""), Box::new(|r| {
            let blob = git(r, &["hash-object", "-w", "README.md"]);
            let sub = {
                use std::io::Write;
                let mut c = Command::new("git").current_dir(r).args(["mktree"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
                c.stdin.take().unwrap().write_all(format!("100644 blob {blob}\tconfig\n").as_bytes()).unwrap();
                String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap().trim().to_string()
            };
            commit_with_root_entry(r, &format!("040000 tree {sub}\t.git"))
        })),
        ("a path `..`", Some(""), Box::new(|r| {
            let blob = git(r, &["hash-object", "-w", "README.md"]);
            commit_with_root_entry(r, &format!("100644 blob {blob}\t.."))
        })),
        ("a file past the scratch cap", Some("scratch repository"), Box::new(|r| {
            let mut x = 0x9e3779b97f4a7c15u64;
            let noise: Vec<u8> = (0..400_000).map(|_| { x ^= x << 13; x ^= x >> 7; x ^= x << 17; (x >> 24) as u8 }).collect();
            fs::write(r.join("knowledge/huge.md"), noise).unwrap();
            commit_all(r);
            true
        })),
        ("a file outside files.json", None, Box::new(|r| {
            fs::write(r.join("knowledge/extra.md"), "not in the manifest\n").unwrap();
            commit_all(r);
            true
        })),
        ("a script", None, Box::new(|r| {
            fs::write(r.join("scripts-evil.sh"), "#!/bin/sh\necho owned\n").unwrap();
            commit_all(r);
            true
        })),
    ];
    let mut built = 0;
    for (n, (what, refused, make)) in variants.iter().enumerate() {
        let repo = t.path().join(format!("evil-{n}"));
        copy(&base.repo, &repo);
        if !make(&repo) {
            // git itself will not build this tree; there is nothing for a client to refuse.
            continue;
        }
        built += 1;
        let (tree, commit) = (git(&repo, &["rev-parse", "HEAD^{tree}"]), git(&repo, &["rev-parse", "HEAD"]));
        s.repos.borrow_mut().insert(URL.to_string(), repo);
        s.record("mia", 10 + n, 1, &Built { dir: base.dir.clone(), hash: base.hash.clone(), tree, repo: base.repo.clone() }, "1.4.0", |sp| sp.tree = tree_block(&format!("{URL}#."), "git-commit", &commit));
        let f = Repos { site: &s, max_scratch: 128 * 1024, calls: RefCell::new(Vec::new()) };
        if let Some(why) = refused {
            let probe = t.path().join(format!("probe-{n}"));
            fs::create_dir_all(&probe).unwrap();
            let e = f.fetch(URL, Rev::Sha(&commit), None, &probe).expect_err(what);
            assert!(e.to_lowercase().contains(why), "{what}: {e}");
        }
        let code = add(&s.home, &s.ws, &s.fx.store, &xf::cfg(), &id(10 + n), &opts(), &f, &Real);
        assert_ne!(code, 0, "{what} was added");
        assert_eq!(s.added(), 0, "{what}");
        s.nothing_changed_for_the_worse();
        let cache = s.home.join("bundles/acme/acme-platform");
        assert!(!cache.exists() || cache.read_dir().unwrap().next().is_none(), "{what} left a cache entry");
    }
    assert!(built >= 5, "the variants that git can build were built ({built})");
    // The control: the honest commit with the same record shape is added by the same code.
    s.repos.borrow_mut().insert(URL.to_string(), base.repo.clone());
    let commit = git(&base.repo, &["rev-parse", "HEAD"]);
    s.record("mia", 30, 1, &base, "1.4.0", |sp| sp.tree = tree_block(&format!("{URL}#."), "git-commit", &commit));
    assert_eq!(s.add(&id(30), &opts()), 0);
}

#[test]
fn a_tree_signed_by_someone_else_or_by_no_one_or_for_another_org_or_name_is_not_the_published_bundle() {
    let (_t, s) = Site::new();
    // Signed by wes, published by mia: a member cannot present a colleague's signed bundle as their own.
    let wes = s.build("wes", "1.4.0", |_| {});
    s.record("mia", 1, 1, &wes, "1.4.0", |_| {});
    assert_ne!(s.add(&id(1), &opts()), 0);
    // Unsigned.
    let un = s.build("mia", "1.4.0", |_| {});
    let _ = fs::remove_file(un.repo.join(".delphi-bundle/sig.json"));
    git(&un.repo, &["add", "-A"]);
    git(&un.repo, &["commit", "-q", "-m", "unsigned"]);
    let un = Built { tree: git(&un.repo, &["rev-parse", "HEAD^{tree}"]), ..un };
    s.record("mia", 2, 1, &un, "1.4.0", |_| {});
    assert_ne!(s.add(&id(2), &opts()), 0);
    // A bundle for another org (a store's bundles live in its own org's namespace).
    let other = s.build("mia", "1.4.0", as_vendor);
    s.record("mia", 3, 1, &other, "1.4.0", |_| {});
    assert_ne!(s.add(&id(3), &opts()), 0);
    // The record names a different bundle or version than the tree is.
    let b = s.build("mia", "1.4.0", |_| {});
    s.record("mia", 4, 1, &b, "1.4.0", |sp| sp.name = "something-else".into());
    assert_ne!(s.add(&id(4), &opts()), 0);
    s.record("mia", 5, 1, &b, "1.4.0", |sp| sp.version = "9.9.9".into());
    assert_ne!(s.add(&id(5), &opts()), 0);
    // The record under-declares what the bundle touches.
    s.record("mia", 6, 1, &b, "1.4.0", |sp| sp.roles = "[\"strategist\"]".into());
    assert_ne!(s.add(&id(6), &opts()), 0);
    assert_eq!(s.added(), 0);
    s.nothing_changed_for_the_worse();
}

#[test]
fn nothing_in_a_record_or_a_bundle_can_state_its_own_level() {
    let (_t, s) = Site::new();
    let b = s.build("mia", "1.4.0", |_| {});
    // A record that claims to be endorsed is not a record at all.
    let text = String::from_utf8(rk::bundle(&s.fx.id("mia"), &id(1), 1, &BundleSpec::new("acme-platform", "1.4.0", &b.hash)).md).unwrap().replacen("bundle_name:", "endorsed: true\nbundle_name:", 1);
    let fake = rk::sign_record(&s.fx.id("mia"), delphi_xenia::layout::Kind::Bundle, &id(1), text.into_bytes());
    s.fx.put_commit(&fake, "claims endorsement");
    s.refresh();
    assert_ne!(s.add(&id(1), &opts()), 0, "a record with a trust key is held back, so there is nothing to add");
    // A plain member's endorsement, a steward of another area's, and the publisher's own steward-self-endorsement do nothing.
    let rec = s.record("mia", 2, 1, &b, "1.4.0", |_| {});
    s.endorse("erin", 3, &rec, 1);
    assert_eq!(s.add(&id(2), &opts()), 0);
    assert_eq!(s.entry().provenance, "member-signed");
    let (_t2, s2) = Site::new();
    let b2 = s2.build("sam", "1.4.0", |_| {});
    let rec2 = s2.record("sam", 1, 1, &b2, "1.4.0", |_| {});
    s2.endorse("sam", 2, &rec2, 1);
    assert_eq!(s2.add(&id(1), &opts()), 0);
    assert_eq!(s2.entry().provenance, "member-signed", "a steward cannot endorse their own bundle");
}

#[test]
fn a_stale_store_endorses_nothing_and_sync_brings_the_lock_down_with_it() {
    let (_t, s) = Site::new();
    let b = s.build("mia", "1.4.0", |_| {});
    let rec = s.record("mia", 1, 1, &b, "1.4.0", |_| {});
    s.endorse("sam", 2, &rec, 1);
    assert_eq!(s.add(&id(1), &opts()), 0);
    assert_eq!(s.entry().provenance, "endorsed");
    // Eight days later nobody refreshed the statement (it is good for seven).
    let late = VerifyConfig::at(xf::now() + 8 * xf::DAY);
    let r = sync_bundles(&s.home, &s.ws, &s.fx.store, &late, false, false, &s.repos(), &Real).unwrap();
    assert_eq!(r.relevelled, vec![("acme/acme-platform".to_string(), "endorsed".to_string(), "member-signed".to_string())]);
    assert_eq!(s.entry().provenance, "member-signed");
    // The statement is current again at the test clock: the level comes back.
    let r = sync_bundles(&s.home, &s.ws, &s.fx.store, &xf::cfg(), false, false, &s.repos(), &Real).unwrap();
    assert_eq!(r.relevelled.len(), 1);
    assert_eq!(s.entry().provenance, "endorsed");
}

#[test]
fn a_new_rev_is_an_update_the_old_endorsement_does_not_carry_and_the_old_version_stays_in_history() {
    let (_t, s) = Site::new();
    let b1 = s.build("mia", "1.4.0", |_| {});
    let r1 = s.record("mia", 1, 1, &b1, "1.4.0", |_| {});
    s.endorse("sam", 2, &r1, 1);
    assert_eq!(s.add(&id(1), &opts()), 0);
    let b2 = s.build("mia", "1.5.0", |_| {});
    s.record("mia", 1, 2, &b2, "1.5.0", |_| {});
    assert_eq!(s.update(), 0);
    let e = s.entry();
    assert_eq!((e.version.as_str(), e.provenance.as_str(), e.history.len()), ("1.5.0", "member-signed", 1));
    assert_eq!(e.history[0].hash, b1.hash);
    assert_eq!(e.source.path, format!("xenia:acme/acme-main/{}@2", id(1)));
    assert_eq!(e.publisher_kid, s.fx.id("mia").did());
    // Rolling back is the operator's, and works from the lock as for any bundle.
    assert_eq!(crate::bundle_life::rollback(&s.home, &s.ws, "acme-platform", None, false, &Real), 0);
    assert_eq!(s.entry().version, "1.4.0");
    s.nothing_changed_for_the_worse();
}

#[test]
fn automatic_updates_never_lower_the_level_change_the_publisher_or_follow_a_withdrawn_record() {
    let (_t, s) = Site::new();
    let b1 = s.build("mia", "1.4.0", |_| {});
    let r1 = s.record("mia", 1, 1, &b1, "1.4.0", |_| {});
    s.endorse("sam", 2, &r1, 1);
    assert_eq!(s.add(&id(1), &opts()), 0);
    // Rev 2 is waiting but nobody has endorsed it: reported, not applied.
    let b2 = s.build("mia", "1.5.0", |_| {});
    let r2 = s.record("mia", 1, 2, &b2, "1.5.0", |_| {});
    let rep = sync_bundles(&s.home, &s.ws, &s.fx.store, &xf::cfg(), true, false, &s.repos(), &Real).unwrap();
    assert_eq!(rep.available, vec![("acme/acme-platform".to_string(), "1.5.0".to_string(), 2)]);
    assert!(rep.updated.is_empty() && rep.skipped.len() == 1, "{rep:?}");
    assert_eq!(s.entry().version, "1.4.0");
    // Listing never applies anything.
    let before = s.lock();
    let rep = sync_bundles(&s.home, &s.ws, &s.fx.store, &xf::cfg(), false, false, &s.repos(), &Real).unwrap();
    assert!(rep.updated.is_empty());
    assert_eq!(s.lock(), before);
    // Endorsed: now it applies on its own.
    s.endorse("sam", 3, &r2, 2);
    let rep = sync_bundles(&s.home, &s.ws, &s.fx.store, &xf::cfg(), true, false, &s.repos(), &Real).unwrap();
    assert_eq!(rep.updated, vec!["acme/acme-platform".to_string()]);
    assert_eq!((s.entry().version.as_str(), s.entry().provenance.as_str()), ("1.5.0", "endorsed"));
    // The author withdraws the record: nothing follows it, the installed version stays.
    let t = rk::tombstone_with(&s.fx.id("mia"), &id(4), 1, &id(1), "withdraw", "mistake");
    s.fx.put_commit(&t, "withdraw");
    s.refresh();
    let rep = sync_bundles(&s.home, &s.ws, &s.fx.store, &xf::cfg(), true, false, &s.repos(), &Real).unwrap();
    assert!(rep.updated.is_empty() && rep.skipped[0].1.contains("withdrew"), "{rep:?}");
    assert_ne!(s.update(), 0);
    assert_ne!(s.add(&id(1), &opts()), 0);
    assert_eq!(s.entry().version, "1.5.0");
}

#[test]
fn an_older_rev_a_lower_version_and_a_rewound_store_are_replays_not_updates() {
    let (_t, s) = Site::new();
    let b2 = s.build("mia", "1.5.0", |_| {});
    s.record("mia", 1, 2, &b2, "1.5.0", |_| {});
    assert_eq!(s.add(&id(1), &opts()), 0);
    // The lock remembers revision 2; a store that serves revision 1 again (a host replaying an old state) is refused.
    let lock_rev5 = s.entry();
    bundles::update(&s.ws, |l| {
        l.bundle[0].source.path = format!("xenia:acme/acme-main/{}@5", id(1));
        Ok(())
    })
    .unwrap();
    assert_ne!(s.update(), 0, "the store's revision 2 is older than the 5 this install has");
    assert!(s.entry().history.is_empty());
    bundles::update(&s.ws, |l| {
        l.bundle[0].source = lock_rev5.source.clone();
        Ok(())
    })
    .unwrap();
    // A higher rev that offers a lower version is a downgrade (a hostile or careless publisher), refused.
    let b1 = s.build("mia", "1.4.0", |_| {});
    s.record("mia", 1, 3, &b1, "1.4.0", |_| {});
    assert_ne!(s.update(), 0);
    assert_eq!(s.entry().version, "1.5.0");
    // A host that rewinds its history is not a continuation of what was verified: nothing is applied from it.
    let head = s.fx.remote.head();
    s.fx.remote.reset_hard(&format!("{head}~3"));
    s.fx.remote.force_push();
    assert_ne!(s.update(), 0);
    assert_eq!(s.entry().version, "1.5.0");
}

#[test]
fn another_member_publishing_the_same_bundle_under_their_own_key_is_a_publisher_change() {
    let (_t, s) = Site::new();
    let b = s.build("mia", "1.4.0", |_| {});
    s.record("mia", 1, 1, &b, "1.4.0", |_| {});
    assert_eq!(s.add(&id(1), &opts()), 0);
    // wes publishes a record for the same org/name (names are not owned), tree signed by wes.
    let w = s.build("wes", "1.6.0", |_| {});
    s.record("wes", 2, 1, &w, "1.6.0", |_| {});
    let code = s.add(&id(2), &opts());
    assert_ne!(code, 0, "the pinned publisher is mia");
    assert_eq!(s.entry().version, "1.4.0");
    // The operator may accept it, deliberately.
    let mut o = opts();
    o.replace_publisher = true;
    assert_eq!(s.add(&id(2), &o), 0);
    assert_eq!((s.entry().version.as_str(), s.entry().publisher_kid.as_str()), ("1.6.0", s.fx.id("wes").did().as_str()));
    // And the first record's updates no longer apply to the entry that now belongs to the second.
    let w2 = s.build("mia", "1.7.0", |_| {});
    s.record("mia", 1, 2, &w2, "1.7.0", |_| {});
    assert_ne!(s.update(), 0);
}

#[test]
fn publishing_goes_through_the_gate_builds_the_record_from_the_signed_bundle_and_keeps_the_rules() {
    let (_t, s) = Site::new();
    let b = s.build("mia", "1.4.0", |_| {});
    let who = s.fx.id("mia");
    struct Shows(RefCell<Vec<Preview>>);
    impl Approver for &Shows {
        fn decide(&mut self, p: &Preview) -> Decision {
            self.0.borrow_mut().push(p.clone());
            Decision::Approve(p.approve(true))
        }
    }
    let seen = Shows(RefCell::new(Vec::new()));
    let args = |dir: &Path, id: Option<String>, sha: &str| PublishArgs { dir: dir.to_path_buf(), url: URL.into(), subdir: None, pin_kind: PinKind::GitTree, pin: sha.into(), id, tags: Some(vec!["payments".into()]), role: Role::Operator, ceiling: SourceCeiling::InternalGenerated, review_by: None };
    let f = s.repos();
    // The tree the operator points at is fetched and must be this bundle: a wrong pin is caught before anything is signed.
    let wrong = publish(&s.home, &s.fx.store, &xf::cfg(), &who, &args(&b.dir, None, &"0".repeat(40)), Some(&f), &Real, &mut &seen);
    assert!(wrong.is_err() && seen.0.borrow().is_empty(), "{wrong:?}");
    let (rid, rev, _) = publish(&s.home, &s.fx.store, &xf::cfg(), &who, &args(&b.dir, None, &b.tree), Some(&f), &Real, &mut &seen).unwrap();
    assert_eq!(rev, 1);
    let shown = String::from_utf8(seen.0.borrow()[0].items[0].bytes.clone()).unwrap();
    assert!(shown.contains("bundle_name: \"acme-platform\"") && shown.contains(&b.hash) && shown.contains(&b.tree) && shown.contains(&format!("{URL}#.")), "{shown}");
    // The published record is addable at once, as member-signed.
    assert_eq!(s.add(&rid, &opts()), 0);
    assert_eq!(s.entry().provenance, "member-signed");
    // A second record for the same name is refused (publish a new revision instead).
    let again = publish(&s.home, &s.fx.store, &xf::cfg(), &who, &args(&b.dir, None, &b.tree), None, &Real, &mut &seen);
    assert!(again.unwrap_err().contains("already has a record"));
    // A new revision must go forward.
    let b2 = s.build("mia", "1.5.0", |_| {});
    let (_, rev2, _) = publish(&s.home, &s.fx.store, &xf::cfg(), &who, &args(&b2.dir, Some(rid.clone()), &b2.tree), Some(&f), &Real, &mut &seen).unwrap();
    assert_eq!(rev2, 2);
    assert!(publish(&s.home, &s.fx.store, &xf::cfg(), &who, &args(&b.dir, Some(rid.clone()), &b.tree), None, &Real, &mut &seen).unwrap_err().contains("does not go forward"));
    // A bundle signed by another key, or unsigned, or for another org, is not published.
    let theirs = s.build("wes", "1.4.0", |_| {});
    assert!(publish(&s.home, &s.fx.store, &xf::cfg(), &who, &args(&theirs.dir, None, &theirs.tree), None, &Real, &mut &seen).unwrap_err().contains("signed by another key"));
    let other_org = s.build("mia", "1.4.0", as_vendor);
    assert!(publish(&s.home, &s.fx.store, &xf::cfg(), &who, &args(&other_org.dir, None, &other_org.tree), None, &Real, &mut &seen).unwrap_err().contains("belongs to"));
    let unsigned = s.build("mia", "1.4.0", |_| {});
    fs::remove_file(unsigned.dir.join(".delphi-bundle/sig.json")).unwrap();
    assert!(publish(&s.home, &s.fx.store, &xf::cfg(), &who, &args(&unsigned.dir, None, &unsigned.tree), None, &Real, &mut &seen).is_err());
    // Another member cannot publish a revision of mia's record.
    let wes = s.fx.id("wes");
    let w = s.build("wes", "1.9.0", |_| {});
    assert!(publish(&s.home, &s.fx.store, &xf::cfg(), &wes, &args(&w.dir, Some(rid), &w.tree), None, &Real, &mut &seen).is_err());
    // The approver was asked once per accepted publish and for nothing else.
    assert_eq!(seen.0.borrow().len(), 2);
}

#[test]
fn the_store_listing_shows_levels_names_the_ambiguity_and_changes_nothing() {
    let (_t, s) = Site::new();
    let b = s.build("mia", "1.4.0", |_| {});
    let r1 = s.record("mia", 1, 1, &b, "1.4.0", |_| {});
    s.record("wes", 2, 1, &b, "1.4.0", |_| {});
    s.endorse("sam", 3, &r1, 1);
    let set = BundleSet::offline(&s.fx.store, &xf::cfg()).unwrap();
    assert_eq!(set.bundles.len(), 2);
    assert_eq!(set.find(&id(1)).unwrap().level, BundleLevel::Endorsed);
    assert_eq!(set.find(&id(2)).unwrap().level, BundleLevel::MemberSigned);
    assert_eq!(set.named("acme-platform").len(), 2, "the same name twice is visible to the listing");
    let before = s.lock();
    assert_eq!(list(&s.fx.store, &xf::cfg(), &s.ws), 0);
    assert_eq!(s.lock(), before);
}

struct Answer {
    yes: bool,
    seen: Vec<Preview>,
}

impl Approver for Answer {
    fn decide(&mut self, p: &Preview) -> Decision {
        self.seen.push(p.clone());
        if self.yes { Decision::Approve(p.approve(true)) } else { Decision::Deny }
    }
}

fn argv(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The install's xenia key becomes the fixture member's, and the workspace exists as a command line finds it.
fn as_member(s: &Site, who: &str) {
    crate::xenia_cmd::TEST_NOW.store(xf::now(), std::sync::atomic::Ordering::Relaxed);
    let key = crate::xenia_cmd::key_path(&s.home);
    let _ = fs::remove_file(&key);
    s.fx.id(who).store_new(&key).unwrap();
    fs::write(s.ws.join("config.toml"), "").unwrap();
}

#[test]
fn xenia_publish_bundle_shows_the_card_and_a_no_sends_nothing() {
    let (_t, s) = Site::new();
    as_member(&s, "mia");
    let b = s.build("mia", "1.4.0", |_| {});
    let h = s.home.to_str().unwrap();
    let dir = b.dir.to_str().unwrap();
    let pin = format!("git-tree:{}", b.tree);
    let line = |extra: &[&str]| {
        let mut v = vec!["publish", "--bundle", dir, "--url", URL, "--pin", pin.as_str(), "--no-verify-tree", "--tags", "payments", "--store", "acme/acme-main", "--home", h];
        v.extend_from_slice(extra);
        argv(&v)
    };
    let before = s.fx.remote.host_head();
    let mut no = Answer { yes: false, seen: vec![] };
    assert_ne!(crate::xenia_cmd::run_with(&line(&[]), &mut no), 0);
    assert_eq!(no.seen.len(), 1, "the card was shown and answered no");
    assert_eq!(s.fx.remote.host_head(), before, "a no sends nothing");
    let mut yes = Answer { yes: true, seen: vec![] };
    assert_eq!(crate::xenia_cmd::run_with(&line(&[]), &mut yes), 0);
    assert_ne!(s.fx.remote.host_head(), before);
    let shown = String::from_utf8(yes.seen[0].items[0].bytes.clone()).unwrap();
    assert!(shown.contains("kind: bundle") && shown.contains(&b.hash), "{shown}");
    // The store now holds it, member-signed.
    s.fx.sync();
    let set = BundleSet::offline(&s.fx.store, &xf::cfg()).unwrap();
    assert_eq!((set.bundles.len(), set.bundles[0].name.as_str(), set.bundles[0].level), (1, "acme-platform", BundleLevel::MemberSigned));
    // Not a directory, and a key that did not sign the bundle: refused before any card.
    let mut none = Answer { yes: true, seen: vec![] };
    assert_ne!(crate::xenia_cmd::run_with(&argv(&["publish", "--bundle", "/nonexistent", "--url", URL, "--pin", pin.as_str(), "--store", "acme/acme-main", "--home", h]), &mut none), 0);
    as_member(&s, "wes");
    assert_ne!(crate::xenia_cmd::run_with(&line(&["--id", &set.bundles[0].id]), &mut none), 0);
    assert!(none.seen.is_empty(), "a refusal comes before there is anything to sign");
}

#[test]
fn xenia_sync_keeps_the_locks_levels_true_and_the_auto_update_switch_needs_a_workspace() {
    let (_t, s) = Site::new();
    as_member(&s, "mia");
    let b = s.build("mia", "1.4.0", |_| {});
    let rec = s.record("mia", 1, 1, &b, "1.4.0", |_| {});
    s.endorse("sam", 2, &rec, 1);
    assert_eq!(s.add(&id(1), &opts()), 0);
    assert_eq!(s.entry().provenance, "endorsed");
    let h = s.home.to_str().unwrap();
    let mut no = Answer { yes: false, seen: vec![] };
    // The steward withdraws the endorsement: the store says member-signed, and the lock follows at the next sync.
    // (A changed verdict is a higher revision of the steward's own record, not a second record: spec 012 §5.2.)
    let w = rk::endorsement_with(&s.fx.id("sam"), &id(2), 2, &rec.id, 1, &rec.hash(), "withdraw", "payments");
    s.fx.put_commit(&w, "withdraw endorsement");
    s.refresh();
    assert_eq!(crate::xenia_cmd::run_with(&argv(&["sync", "--store", "acme/acme-main", "--workspace", "w", "--home", h]), &mut no), 0);
    assert_eq!(s.entry().provenance, "member-signed");
    // The switch without a workspace is a usage error and changes nothing.
    let before = s.lock();
    let nowhere = argv(&["sync", "--store", "acme/acme-main", "--auto-update-bundles", "--workspace", "nope", "--home", h]);
    assert_ne!(crate::xenia_cmd::run_with(&nowhere, &mut no), 0);
    assert_eq!(s.lock(), before);
}
