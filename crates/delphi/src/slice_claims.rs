//! Slice claims (spec 135 §4): a courtesy between sessions that says "this slice is being built", kept in
//! `$DELPHI_HOME/forge/claims.json` under a lock. Not a lock on the repository: it binds only sessions that ask, and nothing
//! refuses a pull request for a claimed slice: `delphi merge` (F0b) releases the merged branch's claims and only notes another holder's.
//!
//! Named `slice_claims`, not `forge`: `merge/forge.rs` is spec 094's "forge", the GitHub host.

use crate::tracker;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The default lifetime of a claim (spec 135 OD-1).
pub const DEFAULT_TTL_SECS: i64 = 8 * 3600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub repo: String,
    pub spec: String,
    pub slice: String,
    pub by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    pub at: String,
    pub expires: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_from: Option<String>,
}

impl Claim {
    pub fn live(&self, now: i64) -> bool {
        delphi_core::time::parse_rfc3339(&self.expires).map(|e| e > now).unwrap_or(false)
    }
    fn same_slice(&self, repo: &str, spec: &str, slice: &str) -> bool {
        self.repo == repo && self.spec == spec && self.slice.eq_ignore_ascii_case(slice)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct File {
    pub version: u32,
    #[serde(default)]
    pub claims: Vec<Claim>,
}

pub fn dir(home: &Path) -> PathBuf {
    home.join("forge")
}

fn file_path(home: &Path) -> PathBuf {
    dir(home).join("claims.json")
}

fn lock_path(home: &Path) -> PathBuf {
    dir(home).join("claims.lock")
}

/// Read the file; a missing file is empty, a malformed one is an error and is never overwritten.
fn load(home: &Path) -> Result<File, String> {
    let p = file_path(home);
    match std::fs::read(&p) {
        Ok(b) => serde_json::from_slice::<File>(&b).map_err(|e| format!("{} is not a claims file ({}); fix or remove it by hand", p.display(), e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(File { version: 1, claims: vec![] }),
        Err(e) => Err(format!("cannot read {}: {}", p.display(), e)),
    }
}

fn save(home: &Path, f: &File) -> Result<(), String> {
    let d = dir(home);
    std::fs::create_dir_all(&d).map_err(|e| format!("cannot create {}: {}", d.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }
    let bytes = serde_json::to_vec_pretty(f).map_err(|e| e.to_string())?;
    let p = file_path(home);
    delphi_core::lock::write_atomic(&p, &bytes).map_err(|e| format!("cannot write {}: {}", p.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Run `f` on the file under the lock, saving it when `f` says so.
fn with_file<T>(home: &Path, f: impl FnOnce(&mut File) -> Result<(T, bool), String>) -> Result<T, String> {
    std::fs::create_dir_all(dir(home)).map_err(|e| format!("cannot create {}: {}", dir(home).display(), e))?;
    delphi_core::lock::with_lock(&lock_path(home), || {
        let mut file = load(home)?;
        let (out, changed) = f(&mut file)?;
        if changed {
            file.version = 1;
            save(home, &file)?;
        }
        Ok(out)
    })
    .map_err(|e| format!("cannot lock {}: {}", lock_path(home).display(), e))?
}

/// What git says about a checkout, read from its files: no git process runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFacts {
    /// The checkout's top (the directory holding `.git`).
    pub top: PathBuf,
    /// The common git directory, the same for every worktree of one repository.
    pub common: PathBuf,
    /// The branch checked out, if any.
    pub branch: Option<String>,
}

/// Follow `start` up to the directory holding `.git`, then a worktree's `.git` file to its git directory and `commondir`.
pub fn git_facts(start: &Path) -> Result<GitFacts, String> {
    let start = std::fs::canonicalize(start).map_err(|e| format!("{}: {}", start.display(), e))?;
    let mut top = start.as_path();
    let dotgit = loop {
        let g = top.join(".git");
        if g.exists() {
            break g;
        }
        top = top.parent().ok_or_else(|| format!("{} is not inside a git checkout", start.display()))?;
    };
    let (gitdir, common) = if dotgit.is_dir() {
        (dotgit.clone(), dotgit.clone())
    } else {
        let text = std::fs::read_to_string(&dotgit).map_err(|e| format!("{}: {}", dotgit.display(), e))?;
        let rel = text.trim().strip_prefix("gitdir:").map(str::trim).ok_or_else(|| format!("{} does not name a git directory", dotgit.display()))?;
        let gitdir = if Path::new(rel).is_absolute() { PathBuf::from(rel) } else { top.join(rel) };
        let common = match std::fs::read_to_string(gitdir.join("commondir")) {
            Ok(c) => {
                let c = c.trim();
                if Path::new(c).is_absolute() {
                    PathBuf::from(c)
                } else {
                    gitdir.join(c)
                }
            }
            Err(_) => gitdir.clone(),
        };
        (gitdir, common)
    };
    let common = std::fs::canonicalize(&common).unwrap_or(common);
    let branch = std::fs::read_to_string(gitdir.join("HEAD")).ok().and_then(|h| h.trim().strip_prefix("ref: refs/heads/").map(str::to_string));
    Ok(GitFacts { top: top.to_path_buf(), common, branch })
}

/// A slice id: `^[A-Za-z0-9][A-Za-z0-9._§-]{0,15}$`.
pub fn valid_slice(s: &str) -> bool {
    let n = s.chars().count();
    (1..=16).contains(&n) && s.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false) && s.chars().all(|c| c.is_ascii_alphanumeric() || "._§-".contains(c))
}

/// What a command prints and returns.
#[derive(Debug, Default)]
pub struct Out {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Out {
    fn ok(out: String) -> Out {
        Out { code: 0, out, err: String::new() }
    }
    fn fail(code: i32, err: impl Into<String>) -> Out {
        Out { code, out: String::new(), err: err.into() }
    }
}

/// A spec and slice checked against the repository (§4.1): the spec exists and is approved, the slice id is well formed, and
/// the tracker's view is turned into warnings.
pub struct Target {
    pub stem: String,
    pub slice: String,
    pub warnings: Vec<String>,
}

pub fn target(repo: &Path, spec: &str, slice: &str) -> Result<Target, Out> {
    if !valid_slice(slice) {
        return Err(Out::fail(2, format!("`{}` is not a slice id: up to 16 letters, digits and ._§- starting with a letter or digit", slice)));
    }
    let stems = delphi_engine::spec_lookup::spec_stems(repo).map_err(|e| Out::fail(2, e))?;
    let stem = delphi_engine::spec_lookup::resolve(&stems, spec).map_err(|e| Out::fail(2, e))?;
    let text = std::fs::read_to_string(repo.join("specs").join(format!("{}.md", stem))).map_err(|e| Out::fail(4, e.to_string()))?;
    match tracker::spec_status(&text).as_deref() {
        Some("approved") => {}
        s => return Err(Out::fail(1, format!("{} is {}, not approved: a draft is a proposal, not permission to build (No Spec, No Build)", stem, s.unwrap_or("without a status")))),
    }
    let mut warnings = Vec::new();
    if let (Some(num), Ok(t)) = (tracker::spec_number(&stem), std::fs::read_to_string(tracker::path(repo))) {
        let all = tracker::rows(&t);
        let mine = tracker::rows_for(&all, &num);
        if !mine.is_empty() {
            match mine.iter().find(|r| r.slice.eq_ignore_ascii_case(slice)) {
                None => warnings.push(format!("the tracker lists no slice {} for spec {}; it lists {}", slice, num, mine.iter().map(|r| r.slice.as_str()).collect::<Vec<_>>().join(", "))),
                Some(r) if tracker::state_merged(&r.state) => warnings.push(format!("the tracker says {} {} is already merged (line {}): {}", num, r.slice, r.line, r.state)),
                Some(_) => {}
            }
        }
    }
    Ok(Target { stem, slice: slice.to_string(), warnings })
}

fn age(at: &str, now: i64) -> String {
    match delphi_core::time::parse_rfc3339(at) {
        Some(t) if now >= t => {
            let s = now - t;
            if s < 3600 {
                format!("{} min ago", s / 60)
            } else {
                format!("{:.1} h ago", s as f64 / 3600.0)
            }
        }
        _ => at.to_string(),
    }
}

fn describe(c: &Claim, now: i64) -> String {
    format!(
        "{} {} by {}{}{}, made {}, {} {}",
        tracker::spec_number(&c.spec).unwrap_or_else(|| c.spec.clone()),
        c.slice,
        c.by,
        c.branch.as_ref().filter(|b| **b != c.by).map(|b| format!(" (branch {})", b)).unwrap_or_default(),
        c.worktree.as_ref().map(|w| format!(" in {}", w)).unwrap_or_default(),
        age(&c.at, now),
        if c.live(now) { "expires" } else { "expired" },
        c.expires
    )
}

pub struct ClaimReq<'a> {
    pub repo: &'a Path,
    pub home: &'a Path,
    pub spec: &'a str,
    pub slice: &'a str,
    pub by: Option<&'a str>,
    pub ttl_secs: i64,
    pub take: bool,
    pub now: i64,
}

pub fn claim(r: &ClaimReq) -> Out {
    let facts = match git_facts(r.repo) {
        Ok(f) => f,
        Err(e) => return Out::fail(2, e),
    };
    let t = match target(&facts.top, r.spec, r.slice) {
        Ok(t) => t,
        Err(o) => return o,
    };
    let Some(by) = r.by.map(str::to_string).or_else(|| facts.branch.clone()) else {
        return Out::fail(2, "no branch is checked out here: name the holder with --by");
    };
    let repo_id = facts.common.display().to_string();
    let fmt = |s: i64| delphi_core::time::format_unix_millis(s, 0);
    let res = with_file(r.home, |f| {
        let existing = f.claims.iter().position(|c| c.same_slice(&repo_id, &t.stem, &t.slice));
        let mut note = String::new();
        let mut took_from = None;
        if let Some(i) = existing {
            let c = &f.claims[i];
            if c.by == by {
                note = "renewed".into();
                took_from = c.took_from.clone();
            } else if c.live(r.now) && !r.take {
                return Ok((Err(format!("taken: {}\nAsk its holder, or pass --take to take it over (it is printed and recorded).", describe(c, r.now))), false));
            } else if c.live(r.now) {
                note = format!("taken over from {}", c.by);
                took_from = Some(c.by.clone());
            } else {
                note = format!("the claim by {} had expired", c.by);
            }
            f.claims.remove(i);
        }
        let c = Claim { repo: repo_id.clone(), spec: t.stem.clone(), slice: t.slice.clone(), by: by.clone(), branch: facts.branch.clone(), worktree: Some(facts.top.display().to_string()), at: fmt(r.now), expires: fmt(r.now + r.ttl_secs), took_from };
        let line = format!("claimed {} {} for {} until {}{}", tracker::spec_number(&c.spec).unwrap_or_else(|| c.spec.clone()), c.slice, c.by, c.expires, if note.is_empty() { String::new() } else { format!(" ({})", note) });
        f.claims.push(c);
        Ok((Ok(line), true))
    });
    let mut warn = t.warnings.iter().map(|w| format!("warning: {}\n", w)).collect::<String>();
    match res {
        Ok(Ok(line)) => Out { code: 0, out: format!("{}\n", line), err: std::mem::take(&mut warn) },
        Ok(Err(why)) => Out { code: 1, out: String::new(), err: format!("{}{}", warn, why) },
        Err(e) => Out::fail(4, e),
    }
}

pub fn release(repo: &Path, home: &Path, spec: &str, slice: &str, by: Option<&str>, now: i64) -> Out {
    let facts = match git_facts(repo) {
        Ok(f) => f,
        Err(e) => return Out::fail(2, e),
    };
    let stems = match delphi_engine::spec_lookup::spec_stems(&facts.top) {
        Ok(s) => s,
        Err(e) => return Out::fail(2, e),
    };
    let stem = match delphi_engine::spec_lookup::resolve(&stems, spec) {
        Ok(s) => s,
        Err(e) => return Out::fail(2, e),
    };
    let Some(by) = by.map(str::to_string).or_else(|| facts.branch.clone()) else {
        return Out::fail(2, "no branch is checked out here: name the holder with --by");
    };
    let repo_id = facts.common.display().to_string();
    let res = with_file(home, |f| {
        let Some(i) = f.claims.iter().position(|c| c.same_slice(&repo_id, &stem, slice)) else {
            return Ok((Ok(format!("no claim on {} {}; nothing to release", stem, slice)), false));
        };
        let c = &f.claims[i];
        if c.by != by && c.live(now) {
            return Ok((Err(format!("not yours: {}", describe(c, now))), false));
        }
        let line = format!("released {} {} (held by {})", tracker::spec_number(&c.spec).unwrap_or_else(|| c.spec.clone()), c.slice, c.by);
        f.claims.remove(i);
        Ok((Ok(line), true))
    });
    match res {
        Ok(Ok(line)) => Out::ok(format!("{}\n", line)),
        Ok(Err(why)) => Out::fail(1, why),
        Err(e) => Out::fail(4, e),
    }
}

pub fn list(repo: &Path, home: &Path, all: bool, json: bool, now: i64) -> Out {
    let repo_id = git_facts(repo).ok().map(|f| f.common.display().to_string());
    let file = match with_file(home, |f| Ok((std::mem::take(&mut f.claims), false))) {
        Ok(c) => c,
        Err(e) => return Out::fail(4, e),
    };
    let shown: Vec<&Claim> = file.iter().filter(|c| all || (c.live(now) && Some(&c.repo) == repo_id.as_ref())).collect();
    if json {
        let v: Vec<serde_json::Value> = shown.iter().map(|c| serde_json::json!({"claim": c, "live": c.live(now)})).collect();
        return Out::ok(format!("{}\n", serde_json::to_string_pretty(&v).unwrap_or_default()));
    }
    if shown.is_empty() {
        return Out::ok(if all { "no claims\n".into() } else { "no live claims on this repository\n".into() });
    }
    Out::ok(shown.iter().map(|c| format!("{}{}\n", describe(c, now), if all { format!("  [{}]", c.repo) } else { String::new() })).collect())
}

/// After `delphi merge` merged a pull request (F0b, §14.9 item 4): drop every claim in repository `repo_id` whose branch is the
/// merged head branch, live or expired. Returns one line per claim released. A missing file releases nothing; a malformed one is
/// an error and is left as it is.
pub fn release_branch(home: &Path, repo_id: &str, branch: &str) -> Result<Vec<String>, String> {
    if branch.is_empty() || !file_path(home).exists() {
        return Ok(Vec::new());
    }
    with_file(home, |f| {
        let (gone, kept): (Vec<Claim>, Vec<Claim>) = std::mem::take(&mut f.claims).into_iter().partition(|c| c.repo == repo_id && c.branch.as_deref() == Some(branch));
        f.claims = kept;
        let lines: Vec<String> = gone.iter().map(|c| format!("released the claim on {} {} (held by {}; its branch {} merged)", tracker::spec_number(&c.spec).unwrap_or_else(|| c.spec.clone()), c.slice, c.by, branch)).collect();
        let changed = !gone.is_empty();
        Ok((lines, changed))
    })
}

/// The live claims in repository `repo_id` on spec `spec_path` (a `specs/<stem>.md` path) held by a branch other than `branch`,
/// described for a person. Read without the lock: it only informs.
pub fn others_on_spec(home: &Path, repo_id: &str, spec_path: &str, branch: &str, now: i64) -> Result<Vec<String>, String> {
    let stem = spec_path.trim_start_matches("specs/").trim_end_matches(".md");
    let f = load(home)?;
    Ok(f.claims.iter().filter(|c| c.repo == repo_id && c.spec == stem && c.live(now) && c.branch.as_deref() != Some(branch)).map(|c| describe(c, now)).collect())
}

/// The live claim of a branch in this repository, for the finisher (§5.3) and its renewal (§5.4).
pub fn claim_of_branch(repo: &Path, home: &Path, now: i64) -> Option<Claim> {
    let facts = git_facts(repo).ok()?;
    let branch = facts.branch?;
    let repo_id = facts.common.display().to_string();
    let path = file_path(home);
    let bytes = std::fs::read(path).ok()?;
    let f: File = serde_json::from_slice(&bytes).ok()?;
    f.claims.into_iter().find(|c| c.repo == repo_id && c.branch.as_deref() == Some(branch.as_str()) && c.live(now))
}

#[cfg(test)]
#[path = "slice_claims_tests.rs"]
mod tests;
