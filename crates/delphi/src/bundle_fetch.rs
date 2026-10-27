//! Fetching a marketplace entry's pinned commit (`specs/025` §7.3) and `delphi bundle add --from-marketplace`.
//!
//! Everything fetched is hostile. The git run is fenced three ways: configuration comes only from this file (no system or
//! global config, no hooks, no fsmonitor, no credential helper, no prompt); only the https transport is allowed (`file` too
//! when the operator named a local repository themselves); and nothing is ever checked out. The objects land in a bare
//! repository that is not inside the staging tree, and the tree is read out with `ls-tree` and `cat-file` and written by
//! this program, so no filter, smudge, LFS or submodule machinery can run, and a symbolic link or a gitlink is refused
//! rather than created. The staging tree then goes through exactly the checks of a directory add.

use crate::bundle_add::{self, AddOpts, Mode, Opened};
use delphi_bundle::marketplace::{self, Locator};
use delphi_bundle::Host;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Which commit to fetch: the marketplace file's repository is read at its head; a bundle only ever by full sha.
/// `Tree` is a xenia bundle record's `git-tree` pin (`specs/027` §4.3): git cannot ask a server for a tree, so the
/// remote's head is fetched and the tree at the requested directory must be exactly this object id, or nothing is read.
pub enum Rev<'a> {
    Head,
    Sha(&'a str),
    Tree(&'a str),
}

/// The seam: tests stand a local repository in for the network. `subdir` limits the tree to one directory, whose
/// contents land at the root of `dest`. Returns the full id of the commit that was fetched.
pub trait Fetcher {
    fn fetch(&self, url: &str, rev: Rev, subdir: Option<&str>, dest: &Path) -> Result<String, String>;
}

/// specs/023 §2: 2,000 files outside `vectors/` plus 64 in it.
pub const MAX_ENTRIES: usize = 2064;
/// What the objects of one fetch may take on disk (the scratch repository). A hostile server cannot fill the disk past it.
pub const MAX_SCRATCH_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// 64 MiB outside `vectors/` plus 1 GiB in it (specs/023 §2, §8).
pub const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024 + 64 * 1024 * 1024;
const FETCH_SECONDS: u64 = 300;

/// `git`, every unsafe feature off. `allow_file` is true only for a repository path the operator typed.
pub struct GitFetcher {
    pub allow_file: bool,
    /// `$DELPHI_HOME`: bare repositories are made under `bundles/.incoming`, never inside a staging tree.
    pub home: PathBuf,
    /// Cap on the scratch repository's size while the fetch runs; [`MAX_SCRATCH_BYTES`] outside tests.
    pub max_scratch: u64,
}

impl GitFetcher {
    fn git(&self, repo: &Path) -> Command {
        let mut c = Command::new("git");
        c.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            // An empty directory of its own, never the repository: nothing under HOME can configure this git.
            .env("HOME", repo.parent().unwrap_or(repo).join("home"))
            // Discovery can never walk up out of the scratch directory into another repository.
            .env("GIT_CEILING_DIRECTORIES", repo.parent().unwrap_or(repo))
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_PROTOCOL_FROM_USER", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "/usr/bin/false")
            .env("GIT_ALLOW_PROTOCOL", if self.allow_file { "https:file" } else { "https" })
            .env("GIT_LFS_SKIP_SMUDGE", "1")
            .env("LC_ALL", "C")
            .current_dir(repo)
            .stdin(Stdio::null());
        for kv in [
            "core.hooksPath=/dev/null",
            "core.attributesFile=/dev/null",
            "http.followRedirects=false",
            "core.fsmonitor=false",
            "core.symlinks=false",
            "core.protectNTFS=true",
            "core.protectHFS=true",
            "protocol.allow=never",
            "protocol.https.allow=always",
            "submodule.recurse=false",
            "fetch.recurseSubmodules=false",
            "transfer.fsckObjects=true",
            "fetch.fsckObjects=true",
            "credential.helper=",
            "filter.lfs.process=",
            "filter.lfs.smudge=",
            "filter.lfs.clean=",
            "filter.lfs.required=false",
            "gc.auto=0",
            "uploadpack.packObjectsHook=",
        ] {
            c.arg("-c").arg(kv);
        }
        if self.allow_file {
            c.arg("-c").arg("protocol.file.allow=always");
        }
        c
    }
}

/// Bytes under `dir`, links not followed.
fn dir_bytes(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_bytes(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// `watch`: a directory the child writes into and the most it may grow to; past it the child is killed.
fn run(mut c: Command, what: &str, limit: u64, watch: Option<(&Path, u64)>) -> Result<Vec<u8>, String> {
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("git could not be started ({}): is git installed?", e))?;
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let t_out = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = (&mut out).take(limit).read_to_end(&mut v);
        v
    });
    let t_err = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = (&mut err).take(8192).read_to_end(&mut v);
        v
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(FETCH_SECONDS);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("git {} took more than {} seconds and was stopped", what, FETCH_SECONDS));
            }
            Ok(None) => {
                if let Some((dir, cap)) = watch {
                    if dir_bytes(dir) > cap {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(format!("git {} wrote more than {} KiB into its scratch repository and was stopped (a bundle is smaller than that)", what, cap / 1024));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    // A fast child can finish between two polls: the size is judged once more at the end.
    if let Some((_, cap)) = watch.filter(|(d, c)| dir_bytes(d) > *c) {
        return Err(format!("git {} wrote more than {} KiB into its scratch repository and was stopped (a bundle is smaller than that)", what, cap / 1024));
    }
    let (o, e) = (t_out.join().unwrap_or_default(), t_err.join().unwrap_or_default());
    if status.success() {
        Ok(o)
    } else {
        Err(format!("git {} failed: {}", what, String::from_utf8_lossy(&e).trim().chars().take(300).collect::<String>()))
    }
}

/// A path out of a tree as a safe relative path: plain components only.
fn safe_rel(p: &str) -> Option<&str> {
    let bad = p.is_empty() || p.starts_with('/') || p.contains('\\') || p.contains('\0') || p.split('/').any(|c| c.is_empty() || c == "." || c == ".." || c.eq_ignore_ascii_case(".git"));
    (!bad).then_some(p)
}

impl Fetcher for GitFetcher {
    fn fetch(&self, url: &str, rev: Rev, subdir: Option<&str>, dest: &Path) -> Result<String, String> {
        if url.starts_with('-') || url.chars().any(|c| c.is_control()) {
            return Err("the repository location is not acceptable".into());
        }
        if !self.allow_file && !url.starts_with("https://") {
            return Err("only https repositories are fetched".into());
        }
        let scratch = bundle_add::new_incoming(&self.home)?;
        let (repo, empty_home) = (scratch.join("repo"), scratch.join("home"));
        let result = std::fs::create_dir_all(&repo).and_then(|_| std::fs::create_dir_all(&empty_home)).map_err(|e| e.to_string()).and_then(|_| self.fetch_in(&repo, url, rev, subdir, dest));
        // A fetch that was stopped for its size has children (`index-pack`) that are still winding down and may write into the
        // scratch directory for a moment after their parent was killed, so removing it is retried until it is gone.
        for _ in 0..100 {
            if std::fs::remove_dir_all(&scratch).is_ok() || !scratch.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        result
    }
}

impl GitFetcher {
    fn fetch_in(&self, repo: &Path, url: &str, rev: Rev, subdir: Option<&str>, dest: &Path) -> Result<String, String> {
        let mut init = self.git(repo);
        init.args(["init", "--bare", "-q", "."]);
        run(init, "init", 4096, None)?;
        let want = match rev {
            Rev::Head | Rev::Tree(_) => "HEAD".to_string(),
            Rev::Sha(s) => s.to_string(),
        };
        let mut fetch = self.git(repo);
        fetch.args(["fetch", "--quiet", "--depth", "1", "--no-tags", "--no-recurse-submodules", "--", url, &want]);
        run(fetch, "fetch", 4096, Some((repo, self.max_scratch)))?;
        let mut rp = self.git(repo);
        rp.args(["rev-parse", "--verify", "FETCH_HEAD^{commit}"]);
        let commit = String::from_utf8_lossy(&run(rp, "rev-parse", 256, None)?).trim().to_string();
        if let Rev::Sha(s) = rev {
            // The server may hand over anything for a name; only the exact pinned commit is accepted.
            if commit != s {
                return Err(format!("the repository returned commit {} for the pinned {}; refused", commit, s));
            }
        }
        if let Rev::Tree(want_tree) = rev {
            // The pin is on the directory the bundle lives in (the repository root for `#.`), not on the commit.
            let spec = match subdir {
                Some(d) => format!("{}:{}", commit, d.trim_matches('/')),
                None => format!("{}^{{tree}}", commit),
            };
            let mut rt = self.git(repo);
            rt.args(["rev-parse", "--verify", &spec]);
            let got = String::from_utf8_lossy(&run(rt, "rev-parse", 256, None)?).trim().to_string();
            if got != want_tree {
                return Err(format!("the repository's tree is {} but the record pins {}; refused (the pinned tree must be at the remote's head)", got, want_tree));
            }
        }
        let mut ls = self.git(repo);
        ls.args(["ls-tree", "-r", "-z", "-l", &commit]);
        let listing = run(ls, "ls-tree", 64 * 1024 * 1024, None)?;
        let prefix = subdir.map(|d| format!("{}/", d.trim_matches('/')));
        let mut files: Vec<(String, String, u64)> = Vec::new();
        let mut total = 0u64;
        for rec in listing.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let text = String::from_utf8_lossy(rec).into_owned();
            let (meta, path) = text.split_once('\t').ok_or("unreadable tree listing")?;
            let mut it = meta.split_whitespace();
            let (mode, kind, oid, size) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or("0"));
            let rel = match &prefix {
                Some(p) => match path.strip_prefix(p.as_str()) {
                    Some(r) => r,
                    None => continue,
                },
                None => path,
            };
            if mode == "120000" {
                return Err(format!("{} is a symbolic link; a bundle has none", path.chars().take(80).collect::<String>()));
            }
            if kind == "commit" || mode == "160000" {
                return Err(format!("{} is a submodule; a bundle has none", path.chars().take(80).collect::<String>()));
            }
            let Some(rel) = safe_rel(rel) else { return Err(format!("{} is not a path a bundle may have", path.chars().take(80).collect::<String>())) };
            let size: u64 = size.parse().map_err(|_| "unreadable tree listing")?;
            if size > MAX_FILE_BYTES {
                return Err(format!("{} is {} MiB (the largest file a bundle may have is {} MiB)", rel, size / (1024 * 1024), MAX_FILE_BYTES / (1024 * 1024)));
            }
            total += size;
            if files.len() >= MAX_ENTRIES || total > MAX_TOTAL_BYTES {
                return Err(format!("the tree is larger than a bundle may be ({} files, {} MiB)", files.len() + 1, total / (1024 * 1024)));
            }
            files.push((rel.to_string(), oid.to_string(), size));
        }
        if files.is_empty() {
            return Err(match subdir {
                Some(d) => format!("the commit has no directory `{}`", d),
                None => "the commit has no files".into(),
            });
        }
        // One cat-file process serves every blob; each request is answered before the next is sent.
        let mut cat = self.git(repo);
        cat.args(["cat-file", "--batch"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = cat.spawn().map_err(|e| e.to_string())?;
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        let written = (|| -> Result<(), String> {
            for (rel, oid, size) in &files {
                if !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("unreadable tree listing".into());
                }
                writeln!(stdin, "{}", oid).map_err(|e| e.to_string())?;
                let mut head = String::new();
                stdout.read_line(&mut head).map_err(|e| e.to_string())?;
                let n: u64 = head.split_whitespace().nth(2).and_then(|s| s.parse().ok()).filter(|n| n == size).ok_or_else(|| format!("{}: the object is not the size the tree says", rel))?;
                let mut data = vec![0u8; n as usize];
                stdout.read_exact(&mut data).map_err(|e| e.to_string())?;
                let mut nl = [0u8; 1];
                stdout.read_exact(&mut nl).map_err(|e| e.to_string())?;
                write_new(dest, rel, &data)?;
            }
            Ok(())
        })();
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
        written?;
        Ok(commit)
    }
}

/// Create `rel` under `dest` without following or replacing anything: parents are made, the file must be new, mode 0644.
fn write_new(dest: &Path, rel: &str, data: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let p = dest.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| format!("{}: {}", rel, e))?;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o644).open(&p).map_err(|e| format!("{}: {}", rel, e))?;
    f.write_all(data).map_err(|e| format!("{}: {}", rel, e))?;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).map_err(|e| format!("{}: {}", rel, e))
}

/// `repo` as the operator typed it: a local directory, an `https://` URL, or `owner/repo` on GitHub.
fn repo_url(repo: &str) -> Result<(String, bool), String> {
    let p = Path::new(repo);
    if p.is_dir() {
        return p.canonicalize().map(|c| (c.display().to_string(), true)).map_err(|e| e.to_string());
    }
    if repo.starts_with("https://") {
        return Ok((repo.to_string(), false));
    }
    if regex::Regex::new(r"^[A-Za-z0-9_.-]{1,100}/[A-Za-z0-9_.-]{1,100}$").unwrap().is_match(repo) {
        return Ok((format!("https://github.com/{}.git", repo), false));
    }
    Err(format!("`{}` is not a local directory, an https URL or owner/repo", repo.chars().take(60).collect::<String>()))
}

/// The marketplace file of `repo`: read in place from a local checkout, else fetched (`.claude-plugin/` of the head).
fn marketplace_text(home: &Path, repo: &str, fetcher: &dyn Fetcher) -> Result<String, String> {
    let (url, local) = repo_url(repo)?;
    if local {
        let f = Path::new(&url).join(".claude-plugin/marketplace.json");
        let meta = std::fs::symlink_metadata(&f).map_err(|e| format!("{}: {}", f.display(), e))?;
        if !meta.is_file() || meta.len() > 4 * 1024 * 1024 {
            return Err(format!("{} is not a marketplace file of a sensible size", f.display()));
        }
        return std::fs::read_to_string(&f).map_err(|e| format!("{}: {}", f.display(), e));
    }
    let staging = bundle_add::new_incoming(home)?;
    let r = (|| {
        fetcher.fetch(&url, Rev::Head, Some(".claude-plugin"), &staging)?;
        let f = staging.join("marketplace.json");
        std::fs::read_to_string(&f).map_err(|_| "the repository has no .claude-plugin/marketplace.json".to_string())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    r
}

/// `delphi bundle add --from-marketplace <repo> <name>`: resolve the entry, fetch its pinned commit into a staging
/// directory under `.incoming/`, check the commit is the pinned one, and run the ordinary add on the staged tree.
pub fn add_from_marketplace(home: &Path, ws: &Path, repo: &str, name: &str, o: &AddOpts, fetcher: &dyn Fetcher, host: &dyn Host) -> i32 {
    let text = match marketplace_text(home, repo, fetcher) {
        Ok(t) => t,
        Err(e) => return bundle_add::fail(e),
    };
    let entries = match marketplace::parse(&text) {
        Ok(e) => e,
        Err(e) => return bundle_add::fail(e),
    };
    let Some(entry) = entries.iter().find(|e| e.name == name) else {
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).take(20).collect();
        return bundle_add::fail(format!("the marketplace has no entry `{}` (it lists: {})", name, names.join(", ")));
    };
    let loc: &Locator = match &entry.source {
        Ok(l) => l,
        Err(why) => return bundle_add::fail(format!("the entry `{}` is refused: {}", name, why)),
    };
    let staging = match bundle_add::new_incoming(home) {
        Ok(s) => s,
        Err(e) => return bundle_add::fail(e),
    };
    // Dropped (and the staging tree removed) on every path that does not move the tree into the cache.
    let source_path = format!("{}#{}@{}", repo, name, loc.sha);
    let opened = Opened { dir: staging.clone(), kind: "marketplace", path: source_path, incoming: Some(staging.clone()) };
    match fetcher.fetch(&loc.url, Rev::Sha(&loc.sha), loc.path.as_deref(), &staging) {
        Ok(commit) if commit == loc.sha => {}
        Ok(commit) => return bundle_add::fail(format!("fetched commit {} is not the pinned {}", commit, loc.sha)),
        Err(e) => return bundle_add::fail(format!("fetching {}: {}", name, e)),
    }
    println!("fetched {} at commit {} (pinned by the marketplace; the marketplace is a locator, the bundle is checked as any other)", name, &loc.sha[..12]);
    bundle_add::apply_opened(home, ws, opened, o, Mode::Add, host, entry.bundle_hash.as_deref())
}

/// The fetcher for `add --from-marketplace` on the command line: https only. A local marketplace is read in place and an
/// entry's URL is always https, whoever wrote it, so `file` is never needed and never allowed here.
pub fn real_fetcher(home: &Path) -> GitFetcher {
    GitFetcher { allow_file: false, home: home.to_path_buf(), max_scratch: MAX_SCRATCH_BYTES }
}

#[cfg(test)]
#[path = "bundle_fetch_tests.rs"]
mod tests;
