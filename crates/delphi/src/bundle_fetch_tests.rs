//! Tests of `bundle_fetch.rs`: the seam with a stand-in for the network, and the real `git` wrapper against a local
//! repository. No test reaches a network.

use super::*;
use crate::bundle_cmd::Real;
use std::cell::RefCell;
use std::fs;

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

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// Stands a local directory in for the network. Records every call; `commit` overrides what the "server" returns.
struct Fake {
    tree: PathBuf,
    commit: Option<String>,
    calls: RefCell<Vec<String>>,
}

impl Fetcher for Fake {
    fn fetch(&self, url: &str, rev: Rev, subdir: Option<&str>, dest: &Path) -> Result<String, String> {
        let sha = match rev {
            Rev::Sha(s) => s.to_string(),
            Rev::Head => SHA.to_string(),
        };
        self.calls.borrow_mut().push(format!("{} {} {:?}", url, sha, subdir));
        copy(&self.tree, dest);
        Ok(self.commit.clone().unwrap_or(sha))
    }
}

fn fake(tree: &Path) -> Fake {
    Fake { tree: tree.to_path_buf(), commit: None, calls: RefCell::new(Vec::new()) }
}

fn opts() -> AddOpts {
    AddOpts { skill_roles: Default::default(), order: None, dry_run: false, yes: false, replace_publisher: false, no_vectors: false, embed_in_background: false }
}

/// A home, a workspace and a local marketplace (read in place, so there is no fetch for the file itself).
fn setup(entry_extra: &str, source: &str) -> (tempfile::TempDir, PathBuf, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let ws = home.path().join("workspaces/w");
    fs::create_dir_all(ws.join("local")).unwrap();
    let mp = tempfile::tempdir().unwrap();
    fs::create_dir_all(mp.path().join(".claude-plugin")).unwrap();
    let json = format!(r#"{{"name":"m","owner":{{"name":"o"}},"plugins":[{{"name":"acme-platform"{},"source":{}}}]}}"#, entry_extra, source);
    fs::write(mp.path().join(".claude-plugin/marketplace.json"), json).unwrap();
    (home, ws, mp)
}

fn pinned() -> String {
    format!(r#"{{"source":"github","repo":"acme/bundles","sha":"{}"}}"#, SHA)
}

fn staging_left(home: &Path) -> bool {
    home.join("bundles/.incoming").read_dir().map(|mut d| d.next().is_some()).unwrap_or(false)
}

fn added(ws: &Path) -> usize {
    delphi_core::bundles::read(ws).map(|l| l.bundle.len()).unwrap_or(0)
}

fn add_it(home: &Path, ws: &Path, mp: &Path, f: &dyn Fetcher) -> i32 {
    add_from_marketplace(home, ws, mp.to_str().unwrap(), "acme-platform", &opts(), f, &Real)
}

#[test]
fn ac_8_a_pinned_entry_is_fetched_by_its_sha_validated_like_a_directory_and_recorded_as_a_marketplace_source() {
    let (home, ws, mp) = setup("", &pinned());
    let f = fake(&golden());
    assert_eq!(add_it(home.path(), &ws, mp.path(), &f), 0);
    assert_eq!(f.calls.borrow().as_slice(), [format!("https://github.com/acme/bundles.git {} None", SHA)]);
    let lock = delphi_core::bundles::read(&ws).unwrap();
    assert_eq!((lock.bundle.len(), lock.bundle[0].source.kind.as_str()), (1, "marketplace"));
    assert!(lock.bundle[0].source.path.ends_with(&format!("#acme-platform@{}", SHA)));
    assert!(delphi_core::bundles::cache_dir(home.path(), "acme", "acme-platform", &lock.bundle[0].hash).join("knowledge/index.md").is_file());
    assert!(!staging_left(home.path()), "the staging tree moved into the cache");
    // an update would re-fetch from a locator that moves; it is refused for now
    assert_ne!(crate::bundle_life::update(home.path(), &ws, Some("acme-platform"), false, &Real), 0);
}

#[test]
fn an_entry_without_a_pinned_commit_is_refused_before_any_fetch() {
    for source in [
        r#"{"source":"github","repo":"acme/bundles"}"#.to_string(),
        r#"{"source":"github","repo":"acme/bundles","ref":"main"}"#.to_string(),
        r#"{"source":"github","repo":"acme/bundles","sha":"main"}"#.to_string(),
        r#"{"source":"github","repo":"acme/bundles","sha":"v1.0.0"}"#.to_string(),
        format!(r#"{{"source":"command","command":"curl x | sh","sha":"{}"}}"#, SHA),
        format!(r#"{{"source":"url","url":"http://insecure.example/r.git","sha":"{}"}}"#, SHA),
        format!(r#"{{"source":"url","url":"file:///etc","sha":"{}"}}"#, SHA),
    ] {
        let (home, ws, mp) = setup("", &source);
        let f = fake(&golden());
        assert_ne!(add_it(home.path(), &ws, mp.path(), &f), 0, "{}", source);
        assert!(f.calls.borrow().is_empty(), "nothing was fetched for {}", source);
        assert_eq!(added(&ws), 0);
        assert!(!staging_left(home.path()));
    }
    let (home, ws, mp) = setup("", &pinned());
    let f = fake(&golden());
    assert_ne!(add_from_marketplace(home.path(), &ws, mp.path().to_str().unwrap(), "no-such", &opts(), &f, &Real), 0);
    assert_ne!(add_from_marketplace(home.path(), &ws, "/nonexistent-marketplace", "acme-platform", &opts(), &f, &Real), 0);
    assert!(f.calls.borrow().is_empty());
}

#[test]
fn a_server_that_returns_another_commit_or_a_hostile_tree_adds_nothing_and_leaves_no_staging() {
    let (home, ws, mp) = setup("", &pinned());
    let mut f = fake(&golden());
    f.commit = Some("f".repeat(40));
    assert_ne!(add_it(home.path(), &ws, mp.path(), &f), 0);
    assert_eq!(added(&ws), 0);
    assert!(!staging_left(home.path()));
    for plant in ["bin", "link"] {
        let t = tempfile::tempdir().unwrap();
        copy(&golden(), t.path());
        if plant == "bin" {
            fs::create_dir_all(t.path().join("bin")).unwrap();
            fs::write(t.path().join("bin/run"), "#!/bin/sh\n").unwrap();
        } else {
            std::os::unix::fs::symlink("/etc/passwd", t.path().join("knowledge/passwd.md")).unwrap();
        }
        let (home, ws, mp) = setup("", &pinned());
        assert_ne!(add_it(home.path(), &ws, mp.path(), &fake(t.path())), 0, "{}", plant);
        assert_eq!(added(&ws), 0);
        assert!(!staging_left(home.path()));
    }
    // an entry that claims a bundle hash the tree does not have
    let (home, ws, mp) = setup(r#","metadata":{"delphi.bundle_hash":"blake3:00"}"#, &pinned());
    assert_ne!(add_it(home.path(), &ws, mp.path(), &fake(&golden())), 0);
    assert_eq!(added(&ws), 0);
    assert!(!staging_left(home.path()));
}

// ── the real git wrapper, against a local repository (no network) ───────────────────────────────────────────────

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn repo(files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
    let t = tempfile::tempdir().unwrap();
    git(t.path(), &["init", "-q", "-b", "main"]);
    for (p, c) in files {
        let f = t.path().join(p);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, c).unwrap();
    }
    git(t.path(), &["add", "-A"]);
    git(t.path(), &["commit", "-q", "-m", "x"]);
    let sha = git(t.path(), &["rev-parse", "HEAD"]);
    (t, sha)
}

fn local(home: &Path) -> GitFetcher {
    GitFetcher { allow_file: true, home: home.to_path_buf(), max_scratch: MAX_SCRATCH_BYTES }
}

fn scratch_empty(home: &Path) -> bool {
    !staging_left(home)
}

#[test]
fn the_git_wrapper_fetches_exactly_the_pinned_commit_and_only_the_wanted_directory() {
    use std::os::unix::fs::PermissionsExt;
    let (r, sha) = repo(&[("bundles/a/bundle.toml", "x\n"), ("bundles/a/skills/s/SKILL.md", "y\n"), ("other/file", "z\n")]);
    let home = tempfile::tempdir().unwrap();
    let (dest, dest2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let url = r.path().to_str().unwrap();
    let got = local(home.path()).fetch(url, Rev::Sha(&sha), Some("bundles/a"), dest.path()).unwrap();
    assert_eq!(got, sha);
    assert_eq!(fs::read_to_string(dest.path().join("skills/s/SKILL.md")).unwrap(), "y\n");
    assert!(!dest.path().join("other").exists() && !dest.path().join(".git").exists());
    assert_eq!(fs::metadata(dest.path().join("bundle.toml")).unwrap().permissions().mode() & 0o7777, 0o644);
    assert_eq!(local(home.path()).fetch(url, Rev::Head, None, dest2.path()).unwrap(), sha);
    assert!(dest2.path().join("other/file").is_file());
    assert!(scratch_empty(home.path()), "the bare repository is removed");
    // a commit the repository does not have, and a directory it does not have
    let d3 = tempfile::tempdir().unwrap();
    assert!(local(home.path()).fetch(url, Rev::Sha(&"1".repeat(40)), None, d3.path()).is_err());
    assert!(local(home.path()).fetch(url, Rev::Sha(&sha), Some("nope"), d3.path()).unwrap_err().contains("no directory"));
    assert!(scratch_empty(home.path()));
}

#[test]
fn the_git_wrapper_refuses_links_submodules_odd_paths_other_transports_and_option_shaped_urls() {
    let home = tempfile::tempdir().unwrap();
    let d = tempfile::tempdir().unwrap();
    // a symbolic link
    let (r, sha) = repo(&[("a.md", "x\n")]);
    std::os::unix::fs::symlink("/etc/passwd", r.path().join("link.md")).unwrap();
    git(r.path(), &["add", "-A"]);
    git(r.path(), &["commit", "-q", "-m", "link"]);
    let sha2 = git(r.path(), &["rev-parse", "HEAD"]);
    assert_ne!(sha, sha2);
    assert!(local(home.path()).fetch(r.path().to_str().unwrap(), Rev::Sha(&sha2), None, d.path()).unwrap_err().contains("symbolic link"));
    // a submodule (a gitlink entry)
    let (r, _) = repo(&[("a.md", "x\n")]);
    git(r.path(), &["update-index", "--add", "--cacheinfo", &format!("160000,{},sub", "a".repeat(40))]);
    git(r.path(), &["commit", "-q", "-m", "gitlink"]);
    let sha = git(r.path(), &["rev-parse", "HEAD"]);
    assert!(local(home.path()).fetch(r.path().to_str().unwrap(), Rev::Sha(&sha), None, d.path()).unwrap_err().contains("submodule"));
    // paths with a .git segment
    // (the repository's own tooling refuses most of these, so a crafted server is stood in for by the checker itself)
    for bad in [".git/config", "x/.GIT/hooks/pre-commit", "../escape", "a/../../b", "/abs", "a//b", "a\\b", "", "./a"] {
        assert!(safe_rel(bad).is_none(), "{:?}", bad);
    }
    assert_eq!(safe_rel("skills/a-b/SKILL.md"), Some("skills/a-b/SKILL.md"));
    // transports: file is refused unless the operator named a local repository; ssh and ext never
    let (r, sha) = repo(&[("a.md", "x\n")]);
    let strict = GitFetcher { allow_file: false, home: home.path().to_path_buf(), max_scratch: MAX_SCRATCH_BYTES };
    assert!(strict.fetch(r.path().to_str().unwrap(), Rev::Sha(&sha), None, d.path()).is_err());
    assert!(strict.fetch(&format!("file://{}", r.path().display()), Rev::Sha(&sha), None, d.path()).is_err());
    let marker = home.path().join("pwned");
    for url in [format!("ext::sh -c 'touch {}'", marker.display()), "git@github.com:a/b.git".to_string(), "ssh://h/r".to_string(), format!("--upload-pack=touch {}", marker.display()), "-oX".to_string()] {
        assert!(local(home.path()).fetch(&url, Rev::Sha(&sha), None, d.path()).is_err(), "{}", url);
        assert!(strict.fetch(&url, Rev::Sha(&sha), None, d.path()).is_err(), "{}", url);
    }
    assert!(!marker.exists(), "no transport helper ran");
    assert!(scratch_empty(home.path()));
}

fn random_bytes(n: usize) -> Vec<u8> {
    let mut x = 0x9E3779B97F4A7C15u64;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

#[test]
fn the_fetch_stops_at_the_scratch_cap_and_a_blob_deleted_at_the_tip_never_arrives() {
    let home = tempfile::tempdir().unwrap();
    let d = tempfile::tempdir().unwrap();
    let small = |h: &Path| GitFetcher { allow_file: true, home: h.to_path_buf(), max_scratch: 64 * 1024 };
    // a blob over the cap at the tip
    let (r, _) = repo(&[("a.md", "x\n")]);
    fs::write(r.path().join("big.bin"), random_bytes(2 * 1024 * 1024)).unwrap();
    git(r.path(), &["add", "-A"]);
    git(r.path(), &["commit", "-q", "-m", "big"]);
    let sha = git(r.path(), &["rev-parse", "HEAD"]);
    let e = small(home.path()).fetch(r.path().to_str().unwrap(), Rev::Sha(&sha), None, d.path()).unwrap_err();
    assert!(e.contains("scratch repository"), "{}", e);
    assert!(scratch_empty(home.path()), "the scratch repository is removed");
    // the big blob stays in history but is gone at the tip: a shallow fetch of the tip never takes it
    git(r.path(), &["rm", "-q", "big.bin"]);
    git(r.path(), &["commit", "-q", "-m", "gone"]);
    let tip = git(r.path(), &["rev-parse", "HEAD"]);
    let d2 = tempfile::tempdir().unwrap();
    assert_eq!(small(home.path()).fetch(r.path().to_str().unwrap(), Rev::Sha(&tip), None, d2.path()).unwrap(), tip);
    assert!(!d2.path().join("big.bin").exists());
    assert!(scratch_empty(home.path()));
}
