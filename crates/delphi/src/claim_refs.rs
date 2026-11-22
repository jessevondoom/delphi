//! Claims across machines (spec 135 §14.11): a claim is also a ref `refs/delphi/claims/<spec>/<SLICE>` on `origin`, written with a
//! compare-and-swap push (`--force-with-lease`), so sessions on other machines see it. The local file of `slice_claims` stays the
//! offline fallback: a remote that cannot be reached never fails a command. A claim is a courtesy, never a gate: nothing reads
//! these refs to allow or refuse anything, and `--take` always works.

use crate::slice_claims::{self, ClaimReq, Out};
use crate::tracker;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The remote namespace, and the local one a fetch copies it into.
pub const NS: &str = "refs/delphi/claims";
const SEEN: &str = "refs/delphi/seen-claims";
/// The first path part of a spec-number reservation (§14.11 item 8).
pub const SPEC_NUMBER: &str = "spec-number";
const TIMEOUT: Duration = Duration::from_secs(20);
const MAIN: &str = "refs/remotes/origin/main";

/// The JSON in a claim commit's message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefClaim {
    pub version: u32,
    pub spec: String,
    pub slice: String,
    pub by: String,
    pub machine: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub at: String,
    pub expires: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_from: Option<String>,
}

impl RefClaim {
    pub fn live(&self, now: i64) -> bool {
        delphi_core::time::parse_rfc3339(&self.expires).map(|e| e > now).unwrap_or(false)
    }
    /// Text read from a ref is data: no control characters, nothing long.
    fn cleaned(mut self) -> RefClaim {
        let c = |s: &mut String| *s = s.chars().filter(|c| !c.is_control()).take(120).collect();
        for s in [&mut self.spec, &mut self.slice, &mut self.by, &mut self.machine, &mut self.at, &mut self.expires] {
            c(s);
        }
        for s in [self.branch.as_mut(), self.took_from.as_mut()].into_iter().flatten() {
            c(s);
        }
        self
    }
}

/// One ref read from the remote: its path under [`NS`], its commit, and the claim if its message held one.
#[derive(Debug, Clone)]
pub struct Seen {
    pub path: String,
    pub oid: String,
    pub claim: Option<RefClaim>,
}

impl Seen {
    /// An unreadable ref counts as expired, so it never stands in anyone's way.
    pub fn live(&self, now: i64) -> bool {
        self.claim.as_ref().is_some_and(|c| c.live(now))
    }
}

/// What a compare-and-swap push did.
#[derive(Debug, PartialEq, Eq)]
pub enum Push {
    Done,
    /// The lease did not hold: another session wrote the ref first.
    Lost,
    /// Anything else (no network, no credentials, a server that refuses the namespace): the offline case.
    Failed(String),
}

/// The ref path of a slice claim, `<spec>/<SLICE>`, or `None` when the slice cannot be part of a ref name.
pub fn ref_path(spec: &str, slice: &str) -> Option<String> {
    let s = slice.to_ascii_uppercase();
    let ok = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) && !p.starts_with(['.', '-']) && !p.contains("..") && !p.ends_with('.') && !p.to_ascii_lowercase().ends_with(".lock");
    (ok(spec) && ok(&s)).then(|| format!("{}/{}", spec, s))
}

/// This machine's random id (`m-` and 12 hex digits), made once in `<home>/forge/machine-id`. It names no host and no person.
pub fn machine_id(home: &Path) -> String {
    let p = slice_claims::dir(home).join("machine-id");
    let valid = |s: &str| s.len() == 14 && s.starts_with("m-") && s[2..].chars().all(|c| c.is_ascii_hexdigit());
    if let Ok(s) = std::fs::read_to_string(&p) {
        if valid(s.trim()) {
            return s.trim().to_string();
        }
    }
    let mut b = [0u8; 6];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) ^ std::process::id() as u128;
        b.copy_from_slice(&n.to_le_bytes()[..6]);
    }
    let id = format!("m-{}", b.iter().map(|x| format!("{:02x}", x)).collect::<String>());
    let _ = std::fs::create_dir_all(slice_claims::dir(home));
    let _ = std::fs::write(&p, format!("{}\n", id));
    id
}

/// The checkout's `origin`, when its configuration has one.
pub struct Remote {
    top: PathBuf,
}

impl Remote {
    pub fn of(repo: &Path) -> Option<Remote> {
        let facts = slice_claims::git_facts(repo).ok()?;
        let config = std::fs::read_to_string(facts.common.join("config")).ok()?;
        config.contains("[remote \"origin\"]").then_some(Remote { top: facts.top })
    }

    /// The one place this module starts git (class `git` in `spawn_sites.toml`): the checkout's own git and the operator's
    /// configuration (a push needs their credentials), hooks off, no prompt, killed after [`TIMEOUT`]. Returns success, stdout
    /// and stderr.
    fn run(&self, args: &[&str], input: Option<&[u8]>) -> Result<(bool, String, String), String> {
        let mut c = Command::new("git");
        c.arg("-C").arg(&self.top);
        for kv in ["core.hooksPath=/dev/null", "commit.gpgSign=false", "user.name=delphi", "user.email=claims@delphi.invalid"] {
            c.arg("-c").arg(kv);
        }
        c.args(args).env("GIT_TERMINAL_PROMPT", "0").stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| format!("cannot run git: {}", e))?;
        if let (Some(mut stdin), Some(bytes)) = (child.stdin.take(), input) {
            let _ = stdin.write_all(bytes);
        }
        let reader = |p: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut v = Vec::new();
                if let Some(mut p) = p {
                    let _ = p.read_to_end(&mut v);
                }
                String::from_utf8_lossy(&v).into_owned()
            })
        };
        let out = reader(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
        let err = reader(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
        let deadline = Instant::now() + TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    // The readers are left: a helper git started (ssh) may hold the pipes open.
                    return Err(format!("git {} took longer than {} s", args.first().unwrap_or(&""), TIMEOUT.as_secs()));
                }
            }
        };
        Ok((status.success(), out.join().unwrap_or_default(), err.join().unwrap_or_default()))
    }

    fn git(&self, args: &[&str], input: Option<&[u8]>) -> Result<String, String> {
        match self.run(args, input)? {
            (true, out, _) => Ok(out),
            (false, out, err) => Err(last_line(&err).or_else(|| last_line(&out)).unwrap_or_else(|| "git failed".into())),
        }
    }

    /// Fetch the claims (and `main`, when asked) and read them: one fetch, one `for-each-ref`.
    pub fn fetch(&self, with_main: bool) -> Result<Vec<Seen>, String> {
        let claims = format!("+{}/*:{}/*", NS, SEEN);
        let main = format!("+refs/heads/main:{}", MAIN);
        let mut args = vec!["fetch", "--quiet", "--no-tags", "--prune", "--no-write-fetch-head", "origin", &claims];
        if with_main {
            args.push(&main);
        }
        self.git(&args, None)?;
        let out = self.git(&["for-each-ref", "--format=%(objectname) %(refname)%00%(contents:body)%00", &format!("{}/", SEEN)], None)?;
        let parts: Vec<&str> = out.split('\0').collect();
        let prefix = format!("{}/", SEEN);
        Ok(parts
            .chunks(2)
            .filter_map(|p| {
                let (oid, name) = p[0].trim().split_once(' ')?;
                let path = name.strip_prefix(&prefix)?.to_string();
                let claim = p.get(1).and_then(|b| serde_json::from_str::<RefClaim>(b.trim()).ok()).map(RefClaim::cleaned);
                Some(Seen { path, oid: oid.to_string(), claim })
            })
            .collect())
    }

    /// A parentless commit with the empty tree whose message carries the claim.
    fn commit(&self, title: &str, c: &RefClaim) -> Result<String, String> {
        let tree = self.git(&["hash-object", "-t", "tree", "-w", "--stdin"], Some(b""))?;
        let msg = format!("{}\n\n{}\n", title, serde_json::to_string(c).map_err(|e| e.to_string())?);
        Ok(self.git(&["commit-tree", tree.trim(), "-F", "-"], Some(msg.as_bytes()))?.trim().to_string())
    }

    /// Set (`new`) or delete (`None`) `<NS>/<path>` only if it is still `expect` (`None`: it must not exist).
    pub fn push(&self, path: &str, expect: Option<&str>, new: Option<&str>) -> Push {
        let r = format!("{}/{}", NS, path);
        let lease = format!("--force-with-lease={}:{}", r, expect.unwrap_or(""));
        let spec = format!("{}:{}", new.unwrap_or(""), r);
        match self.run(&["push", "--porcelain", "--no-verify", &lease, "origin", &spec], None) {
            Err(e) => Push::Failed(e),
            Ok((true, _, _)) => Push::Done,
            Ok((false, out, err)) => {
                let rejected = out.lines().chain(err.lines()).any(|l| l.starts_with('!') && l.contains("[rejected]") && ["stale info", "fetch first", "non-fast-forward", "already exists"].iter().any(|w| l.contains(w)));
                if rejected {
                    Push::Lost
                } else {
                    Push::Failed(last_line(&err).or_else(|| last_line(&out)).unwrap_or_else(|| "git push failed".into()))
                }
            }
        }
    }

    /// Write claim `c` at `path` over `expect`.
    pub fn write(&self, path: &str, expect: Option<&str>, c: &RefClaim) -> Push {
        match self.commit(&format!("delphi claim {} {}", c.spec, c.slice), c) {
            Ok(oid) => self.push(path, expect, Some(&oid)),
            Err(e) => Push::Failed(e),
        }
    }
}

fn last_line(s: &str) -> Option<String> {
    s.lines().map(str::trim).rfind(|l| !l.is_empty()).map(|l| l.chars().filter(|c| !c.is_control()).take(200).collect())
}

fn fmt_time(s: i64) -> String {
    delphi_core::time::format_unix_millis(s, 0)
}

/// A remote claim, described for a person.
pub fn describe(c: &RefClaim, now: i64) -> String {
    let what = if c.spec == SPEC_NUMBER { format!("spec number {}", c.slice) } else { format!("{} {}", c.spec, c.slice) };
    format!(
        "{} by {} on {}{}{}, made {}, {} {}",
        what,
        c.by,
        c.machine,
        c.branch.as_ref().filter(|b| **b != c.by).map(|b| format!(" (branch {})", b)).unwrap_or_default(),
        c.took_from.as_ref().map(|t| format!(", taken from {}", t)).unwrap_or_default(),
        c.at,
        if c.live(now) { "expires" } else { "expired" },
        c.expires
    )
}

fn offline(why: &str, what: &str) -> String {
    format!("warning: the remote could not be reached ({}); {}\n", why, what)
}

/// What the remote said about one claim: a line to print, a refusal, or the offline case.
enum Outcome {
    Ok(String),
    Refused(String),
    Offline(String),
}

/// The §4.2 table on the remote, for the claim `want` at `path`.
fn claim_remote(remote: &Remote, path: &str, mut want: RefClaim, take: bool, now: i64) -> Outcome {
    let seen = match remote.fetch(false) {
        Ok(s) => s,
        Err(e) => return Outcome::Offline(e),
    };
    let cur = seen.into_iter().find(|s| s.path == path);
    let mut note = String::new();
    if let Some(s) = &cur {
        match &s.claim {
            Some(c) if c.by == want.by => {
                note = " (renewed)".into();
                want.took_from = want.took_from.take().or_else(|| c.took_from.clone());
            }
            Some(c) if c.live(now) && !take => return Outcome::Refused(format!("taken on the remote: {}\nAsk its holder, or pass --take to take it over (it is printed and recorded).", describe(c, now))),
            Some(c) if c.live(now) => {
                note = format!(" (taken over from {} on {})", c.by, c.machine);
                want.took_from = Some(c.by.clone());
            }
            Some(c) => note = format!(" (the claim by {} had expired)", c.by),
            None => note = " (replaced an unreadable ref)".into(),
        }
    }
    match remote.write(path, cur.as_ref().map(|s| s.oid.as_str()), &want) {
        Push::Done => Outcome::Ok(format!("on the remote: {}/{} for {} on {} until {}{}\n", NS, path, want.by, want.machine, want.expires, note)),
        Push::Lost => {
            let who = remote.fetch(false).ok().and_then(|s| s.into_iter().find(|s| s.path == path)).and_then(|s| s.claim).map(|c| describe(&c, now)).unwrap_or_else(|| "another session".into());
            Outcome::Refused(format!("taken on the remote just now: {}", who))
        }
        Push::Failed(e) => Outcome::Offline(e),
    }
}

/// `forge claim`: the local claim (§4.2), then the remote (§14.11 item 4).
pub fn claim(r: &ClaimReq, local_only: bool) -> Out {
    let mut o = slice_claims::claim(r);
    if o.code != 0 || local_only {
        return o;
    }
    let Some(remote) = Remote::of(r.repo) else { return o };
    let Ok(facts) = slice_claims::git_facts(r.repo) else { return o };
    let Ok(t) = slice_claims::target(&facts.top, r.spec, r.slice) else { return o };
    let by = r.by.map(str::to_string).or_else(|| facts.branch.clone()).unwrap_or_default();
    let spec = tracker::spec_number(&t.stem).unwrap_or_else(|| t.stem.clone());
    let Some(path) = ref_path(&spec, &t.slice) else {
        o.err.push_str(&format!("note: slice `{}` cannot be a ref name; claimed on this machine only\n", t.slice));
        return o;
    };
    let want = RefClaim { version: 1, spec, slice: t.slice.clone(), by: by.clone(), machine: machine_id(r.home), branch: facts.branch.clone(), at: fmt_time(r.now), expires: fmt_time(r.now + r.ttl_secs), took_from: None };
    match claim_remote(&remote, &path, want, r.take, r.now) {
        Outcome::Ok(line) => o.out.push_str(&line),
        Outcome::Offline(e) => o.err.push_str(&offline(&e, "claimed on this machine only")),
        Outcome::Refused(why) => {
            let _ = slice_claims::release(r.repo, r.home, r.spec, r.slice, Some(&by), r.now);
            return Out { code: 1, out: String::new(), err: format!("{}{}", o.err, why) };
        }
    }
    o
}

/// Delete the ref at `path` when `by` holds it or it has expired; another holder's live claim is only named.
fn release_remote(remote: &Remote, path: &str, by: &str, now: i64) -> String {
    let seen = match remote.fetch(false) {
        Ok(s) => s,
        Err(e) => return offline(&e, "released on this machine only"),
    };
    let Some(s) = seen.into_iter().find(|s| s.path == path) else { return String::new() };
    if let Some(c) = s.claim.as_ref().filter(|c| c.by != by && c.live(now)) {
        return format!("note: not yours on the remote, left as it is: {}\n", describe(c, now));
    }
    match remote.push(path, Some(&s.oid), None) {
        Push::Done => format!("released on the remote: {}/{}\n", NS, path),
        Push::Lost => format!("note: {}/{} changed on the remote meanwhile; left as it is\n", NS, path),
        Push::Failed(e) => offline(&e, "released on this machine only"),
    }
}

/// `forge release`: the local release (§4.2), then the remote (§14.11 item 6).
pub fn release(repo: &Path, home: &Path, spec: &str, slice: &str, by: Option<&str>, local_only: bool, now: i64) -> Out {
    let mut o = slice_claims::release(repo, home, spec, slice, by, now);
    if o.code != 0 || local_only {
        return o;
    }
    let (Some(remote), Ok(facts)) = (Remote::of(repo), slice_claims::git_facts(repo)) else { return o };
    let Ok(stem) = delphi_engine::spec_lookup::spec_stems(&facts.top).and_then(|s| delphi_engine::spec_lookup::resolve(&s, spec)) else { return o };
    let by = by.map(str::to_string).or(facts.branch).unwrap_or_default();
    if let Some(path) = ref_path(&tracker::spec_number(&stem).unwrap_or(stem), slice) {
        let line = release_remote(&remote, &path, &by, now);
        if line.starts_with("released") {
            o.out.push_str(&line);
        } else {
            o.err.push_str(&line);
        }
    }
    o
}

/// The branches merged into `origin/main` by a merge commit (`Merge pull request #N from <owner>/<branch>`), and the spec numbers
/// with a file in `specs/` there. Read after a fetch with `main`.
fn main_facts(remote: &Remote) -> Result<(std::collections::HashSet<String>, std::collections::HashSet<u32>), String> {
    let log = remote.git(&["log", "--merges", "--format=%s", "-n", "5000", MAIN], None)?;
    let merged = log.lines().filter_map(|s| s.rfind(" from ").and_then(|i| s[i + 6..].trim().split_once('/')).map(|(_, b)| b.to_string())).collect();
    let specs = remote.git(&["ls-tree", "--name-only", MAIN, "specs/"], None)?;
    Ok((merged, spec_numbers(specs.lines().map(|l| l.trim_start_matches("specs/").to_string())).into_iter().collect()))
}

/// The numbers of spec files (`NNN-<slug>.md`) among `names`.
fn spec_numbers(names: impl Iterator<Item = String>) -> Vec<u32> {
    names.filter(|n| n.ends_with(".md")).filter_map(|n| n.split_once('-').and_then(|(d, _)| (d.len() >= 3 && d.chars().all(|c| c.is_ascii_digit())).then(|| d.parse().ok()).flatten())).collect()
}

/// Delete, each with the lease on the value read, the claims that are done (§14.11 item 7). Returns the refs kept and one line
/// per ref swept or left.
fn sweep(remote: &Remote, seen: Vec<Seen>, now: i64) -> (Vec<Seen>, String) {
    let (merged, on_main) = match main_facts(remote) {
        Ok(f) => f,
        Err(e) => return (seen, offline(&e, "nothing swept")),
    };
    let (mut kept, mut lines) = (Vec::new(), String::new());
    for s in seen {
        let number = s.path.strip_prefix(&format!("{}/", SPEC_NUMBER)).and_then(|n| n.parse::<u32>().ok());
        let why = if !s.live(now) {
            Some("expired")
        } else if number.is_some_and(|n| on_main.contains(&n)) {
            Some("its spec is on origin/main")
        } else if s.claim.as_ref().and_then(|c| c.branch.as_ref()).is_some_and(|b| merged.contains(b)) {
            Some("its branch merged into origin/main")
        } else {
            None
        };
        let Some(why) = why else {
            kept.push(s);
            continue;
        };
        match remote.push(&s.path, Some(&s.oid), None) {
            Push::Done => lines.push_str(&format!("swept {}/{} ({})\n", NS, s.path, why)),
            Push::Lost => {
                lines.push_str(&format!("left {}/{}: it changed on the remote meanwhile\n", NS, s.path));
                kept.push(s);
            }
            Push::Failed(e) => {
                lines.push_str(&offline(&e, &format!("{}/{} not swept", NS, s.path)));
                kept.push(s);
            }
        }
    }
    (kept, lines)
}

/// `forge list`: the local claims, then the remote ones (§14.11 item 7); with `sweep`, the remote ones that are done go first.
pub fn list(repo: &Path, home: &Path, all: bool, json: bool, sweep_first: bool, local_only: bool, now: i64) -> Out {
    let local = slice_claims::list(repo, home, all, json, now);
    let remote = if local_only { None } else { Remote::of(repo) };
    let Some(remote) = remote.filter(|_| local.code == 0) else { return local };
    let (mut notes, mut swept) = (String::new(), String::new());
    let seen = match remote.fetch(sweep_first) {
        Ok(s) if sweep_first => {
            let (kept, lines) = sweep(&remote, s, now);
            swept = lines;
            Some(kept)
        }
        Ok(s) => Some(s),
        Err(e) => {
            notes = offline(&e, "only this machine's claims are shown");
            None
        }
    };
    let shown: Option<Vec<&Seen>> = seen.as_ref().map(|s| s.iter().filter(|x| all || x.live(now)).collect());
    if json {
        let local: serde_json::Value = serde_json::from_str(&local.out).unwrap_or(serde_json::Value::Null);
        let remote = shown.map(|v| v.iter().map(|s| serde_json::json!({"ref": format!("{}/{}", NS, s.path), "claim": s.claim, "live": s.live(now)})).collect::<Vec<_>>());
        let v = serde_json::json!({"local": local, "remote": remote, "swept": swept.lines().collect::<Vec<_>>()});
        return Out { code: 0, out: format!("{}\n", serde_json::to_string_pretty(&v).unwrap_or_default()), err: notes };
    }
    let mut out = format!("{}{}", local.out, swept);
    if let Some(v) = shown {
        out.push_str(&match v.is_empty() {
            true if all => "no claims on the remote\n".to_string(),
            true => "no live claims on the remote\n".to_string(),
            false => format!("on the remote:\n{}", v.iter().map(|s| format!("  {}\n", s.claim.as_ref().map(|c| describe(c, now)).unwrap_or_else(|| format!("{}/{} (unreadable, counts as expired)", NS, s.path)))).collect::<String>()),
        });
    }
    Out { code: 0, out, err: format!("{}{}", local.err, notes) }
}

#[cfg(test)]
#[path = "claim_refs_tests.rs"]
mod tests;
