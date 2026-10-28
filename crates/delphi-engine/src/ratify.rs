//! Ratification: turning live edits into trusted state (`specs/006` §3.1, `specs/009` §5, `specs/005` §3.1 `/ratify`).
//!
//! Everything the daemon trusts is a *ratified copy* pinned by hash in a `ratified.lock`. A live file
//! that differs from its pin is a **pending proposal**: it has no effect until the operator ratifies it.
//! Ratifying is compare-and-swap: the caller names the hash of the version it reviewed, and the change
//! applies only if the live file still has exactly that hash, so a file edited after review is never
//! blessed. Each scope's changes are serialized with an advisory lock and written atomically.
//!
//! Scopes: the workspace (`local/ratified.lock`) and the operator home (`operator/ratified.lock`).
//! Approved specs are pinned by hash only (no copy). Endpoint/access/auth config is pinned as
//! `config.toml#access` with its ratified copy in `local/ratified/config.access.toml`.

use delphi_core::lock::{self, access_blocks, hash, is_protected, LockFile};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_KEY: &str = "config.toml#access";
pub const CONFIG_COPY: &str = "config.access.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Workspace,
    Operator,
}

impl Scope {
    fn prefix(&self) -> &'static str {
        match self {
            Scope::Workspace => "ws",
            Scope::Operator => "op",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    New,
    Changed,
    Deleted,
    ConfigAccess,
    SpecApproval,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::New => "new",
            Kind::Changed => "changed",
            Kind::Deleted => "deleted",
            Kind::ConfigAccess => "config-access",
            Kind::SpecApproval => "spec-approval",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Proposal {
    /// `ws:agents/x/AGENT.md`, `op:heuristics.md`, `ws:config.toml#access`.
    pub id: String,
    pub scope: Scope,
    pub logical: String,
    pub kind: Kind,
    pub live_blake3: Option<String>,
    pub ratified_blake3: Option<String>,
    pub summary: String,
    pub diff: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RatifyError {
    NoSuchProposal(String),
    /// The live file changed after it was reviewed.
    Stale { reviewed: String, now: String },
    Unsupported(String),
    Io(String),
}

impl std::fmt::Display for RatifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RatifyError::NoSuchProposal(id) => write!(f, "no pending proposal `{}`", id),
            RatifyError::Stale { reviewed, now } => write!(f, "the file changed after you reviewed it (reviewed {}…, now {}…); review it again", &reviewed[..reviewed.len().min(8)], &now[..now.len().min(8)]),
            RatifyError::Unsupported(m) | RatifyError::Io(m) => write!(f, "{}", m),
        }
    }
}

impl std::error::Error for RatifyError {}

fn io(e: impl std::fmt::Display) -> RatifyError {
    RatifyError::Io(e.to_string())
}

/// Where a scope keeps its pieces.
struct Layout {
    lock: PathBuf,
    ratified: PathBuf,
    lock_guard: PathBuf,
}

fn layout(home: &Path, ws_dir: &Path, scope: Scope) -> Layout {
    match scope {
        Scope::Workspace => Layout { lock: ws_dir.join("local/ratified.lock"), ratified: ws_dir.join("local/ratified"), lock_guard: ws_dir.join("locks/ratify.lock") },
        Scope::Operator => Layout { lock: home.join("operator/ratified.lock"), ratified: home.join("operator/ratified"), lock_guard: home.join("operator/locks/ratify.lock") },
    }
}

fn walk(root: &Path, prefix: &str, out: &mut BTreeMap<String, PathBuf>) {
    for e in walkdir::WalkDir::new(root).into_iter().flatten().filter(|e| e.file_type().is_file()) {
        let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        out.insert(format!("{}{}", prefix, rel), e.path().to_path_buf());
    }
}

/// Live files that could be pinned, by the logical key they would have in the lock.
fn live_files(home: &Path, ws_dir: &Path, scope: Scope) -> BTreeMap<String, PathBuf> {
    let mut m = BTreeMap::new();
    match scope {
        Scope::Workspace => {
            // The release snapshot first: `delphi update` refreshes it, and the difference from what was ratified
            // is then a proposal. Operator customizations in core/ and team files override it.
            walk(&ws_dir.join("base"), "", &mut m);
            walk(&ws_dir.join("core"), "", &mut m);
            walk(&ws_dir.join("team"), "", &mut m);
            walk(&ws_dir.join("local/memory"), "memory/", &mut m);
            walk(&ws_dir.join("local/connectors"), "connectors/", &mut m);
            walk(&home.join("themes"), "themes/", &mut m);
            if ws_dir.join("local/heuristics.md").exists() {
                m.insert("heuristics.md".into(), ws_dir.join("local/heuristics.md"));
            }
        }
        Scope::Operator => {
            let op = home.join("operator");
            for f in ["profile.md", "heuristics.md"] {
                if op.join(f).exists() {
                    m.insert(f.into(), op.join(f));
                }
            }
            walk(&op.join("memory"), "memory/", &mut m);
            walk(&op.join("core"), "core/", &mut m);
            walk(&op.join("skills"), "skills/", &mut m);
        }
    }
    m
}

fn wants_pin(scope: Scope, logical: &str) -> bool {
    match scope {
        Scope::Workspace => is_protected(logical) || (logical.starts_with("specs/") && logical.ends_with(".md")),
        // Operator keys are relative to operator/: profile.md and heuristics.md are pinned too.
        Scope::Operator => logical == "profile.md" || logical == "heuristics.md" || logical.ends_with("/lessons.md") || logical.starts_with("core/") || logical.starts_with("skills/"),
    }
}

fn is_approved_spec(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("---\n") else { return false };
    let Some(end) = rest.find("\n---\n") else { return false };
    let fm: serde_yaml::Value = serde_yaml::from_str(&rest[..end]).unwrap_or(serde_yaml::Value::Null);
    fm.get("approved_sections").is_some() || fm.get("status").and_then(|v| v.as_str()) == Some("approved")
}

/// A small line diff: `-`/`+` lines with up to two lines of context, long unchanged runs elided.
pub fn line_diff(old: &str, new: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    if a.len() > 3000 || b.len() > 3000 {
        return format!("(too large to diff here: {} lines before, {} after)", a.len(), b.len());
    }
    let (n, m) = (a.len(), b.len());
    let mut t = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i][j] = if a[i] == b[j] { t[i + 1][j + 1] + 1 } else { t[i + 1][j].max(t[i][j + 1]) };
        }
    }
    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((' ', a[i]));
            i += 1;
            j += 1;
        } else if j < m && (i == n || t[i][j + 1] >= t[i + 1][j]) {
            ops.push(('+', b[j]));
            j += 1;
        } else {
            ops.push(('-', a[i]));
            i += 1;
        }
    }
    let changed: Vec<usize> = ops.iter().enumerate().filter(|(_, (c, _))| *c != ' ').map(|(k, _)| k).collect();
    let mut keep = vec![false; ops.len()];
    for k in changed {
        let (lo, hi) = (k.saturating_sub(2), (k + 3).min(ops.len()));
        keep[lo..hi].iter_mut().for_each(|x| *x = true);
    }
    let mut out = String::new();
    let mut skipped = false;
    for (k, (c, l)) in ops.iter().enumerate() {
        if keep[k] {
            out.push_str(&format!("{} {}\n", c, l));
            skipped = false;
        } else if !skipped {
            out.push_str("  …\n");
            skipped = true;
        }
    }
    out
}

fn read_text(p: &Path) -> String {
    fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

/// Every pending proposal across both scopes.
pub fn list(home: &Path, ws_dir: &Path) -> Vec<Proposal> {
    let mut out = Vec::new();
    for scope in [Scope::Workspace, Scope::Operator] {
        let l = layout(home, ws_dir, scope);
        let Ok(lockfile) = lock::read(&l.lock) else {
            // An unreadable lock is itself a finding; nothing can be compared against it.
            continue;
        };
        let live = live_files(home, ws_dir, scope);
        let base = ws_dir.join("base");
        for (logical, path) in &live {
            if !wants_pin(scope, logical) {
                continue;
            }
            let text = read_text(path);
            if logical.starts_with("specs/") && !is_approved_spec(&text) {
                continue; // a draft is not a proposal
            }
            let live_hash = hash(text.as_bytes());
            let pinned = lockfile.files.get(logical);
            if pinned == Some(&live_hash) {
                continue;
            }
            let ratified_text = read_text(&l.ratified.join(logical));
            let (kind, diff, summary) = if logical.starts_with("specs/") {
                (Kind::SpecApproval, String::new(), format!("approve spec {} at its current content", logical))
            } else if pinned.is_none() {
                (Kind::New, text.lines().take(40).map(|x| format!("+ {}\n", x)).collect::<String>(), format!("new file {}", logical))
            } else {
                (Kind::Changed, line_diff(&ratified_text, &text), format!("changed {}", logical))
            };
            out.push(Proposal { id: format!("{}:{}", scope.prefix(), logical), scope, logical: logical.clone(), kind, live_blake3: Some(live_hash), ratified_blake3: pinned.cloned(), summary, diff });
        }
        // Pinned files whose live source is gone (and that are not release files in base/).
        if scope == Scope::Workspace {
            for logical in lockfile.files.keys().filter(|k| !k.contains('#')) {
                if !live.contains_key(logical) && !base.join(logical).exists() && wants_pin(scope, logical) && !ws_dir.join("local/ratified").join(logical).as_os_str().is_empty() && l.ratified.join(logical).exists() {
                    out.push(Proposal { id: format!("ws:{}", logical), scope, logical: logical.clone(), kind: Kind::Deleted, live_blake3: None, ratified_blake3: lockfile.files.get(logical).cloned(), summary: format!("deleted {}", logical), diff: String::new() });
                }
            }
            // Endpoint, access and credential config.
            if let Ok(cfg) = fs::read_to_string(ws_dir.join("config.toml")) {
                let blocks = access_blocks(&cfg);
                let h = hash(blocks.as_bytes());
                if lockfile.files.get(CONFIG_KEY) != Some(&h) {
                    let old = read_text(&l.ratified.join(CONFIG_COPY));
                    out.push(Proposal {
                        id: format!("ws:{}", CONFIG_KEY),
                        scope,
                        logical: CONFIG_KEY.into(),
                        kind: Kind::ConfigAccess,
                        live_blake3: Some(h),
                        ratified_blake3: lockfile.files.get(CONFIG_KEY).cloned(),
                        summary: "endpoint, access policy or credential settings changed (Red: this moves where requests go and which credentials they use)".into(),
                        diff: line_diff(&old, &blocks),
                    });
                }
            }
        }
    }
    out
}

fn find(home: &Path, ws_dir: &Path, id: &str) -> Result<Proposal, RatifyError> {
    list(home, ws_dir).into_iter().find(|p| p.id == id).ok_or_else(|| RatifyError::NoSuchProposal(id.to_string()))
}

fn live_path(home: &Path, ws_dir: &Path, p: &Proposal) -> Option<PathBuf> {
    live_files(home, ws_dir, p.scope).get(&p.logical).cloned()
}

/// Ratify `id`, but only if the live version still has `expected_blake3`. Returns the new pin.
pub fn apply(home: &Path, ws_dir: &Path, id: &str, expected_blake3: &str) -> Result<String, RatifyError> {
    let p = find(home, ws_dir, id)?;
    let l = layout(home, ws_dir, p.scope);
    lock::with_lock(&l.lock_guard, || -> Result<String, RatifyError> {
        // Re-check under the lock: the file may have changed since `find`.
        let p = find(home, ws_dir, id)?;
        let mut lockfile: LockFile = lock::read(&l.lock).map_err(RatifyError::Io)?;
        let new_pin = match p.kind {
            Kind::ConfigAccess => {
                let blocks = access_blocks(&read_text(&ws_dir.join("config.toml")));
                let h = hash(blocks.as_bytes());
                if h != expected_blake3 {
                    return Err(RatifyError::Stale { reviewed: expected_blake3.into(), now: h });
                }
                lock::write_atomic(&l.ratified.join(CONFIG_COPY), blocks.as_bytes()).map_err(io)?;
                lockfile.files.insert(CONFIG_KEY.into(), h.clone());
                h
            }
            Kind::SpecApproval | Kind::New | Kind::Changed => {
                let path = live_path(home, ws_dir, &p).ok_or_else(|| RatifyError::NoSuchProposal(id.into()))?;
                let bytes = fs::read(&path).map_err(io)?;
                let h = hash(&bytes);
                if h != expected_blake3 {
                    return Err(RatifyError::Stale { reviewed: expected_blake3.into(), now: h });
                }
                if p.kind != Kind::SpecApproval {
                    lock::write_atomic(&l.ratified.join(&p.logical), &bytes).map_err(io)?;
                }
                lockfile.files.insert(p.logical.clone(), h.clone());
                h
            }
            Kind::Deleted => {
                if p.ratified_blake3.as_deref() != Some(expected_blake3) {
                    return Err(RatifyError::Stale { reviewed: expected_blake3.into(), now: p.ratified_blake3.unwrap_or_default() });
                }
                let _ = fs::remove_file(l.ratified.join(&p.logical));
                lockfile.files.remove(&p.logical);
                String::new()
            }
        };
        lock::write(&l.lock, &lockfile).map_err(io)?;
        Ok(new_pin)
    })
    .map_err(io)?
}

/// Edit one pinned rules file (`heuristics.md` of a scope) as an operator-approved change, in one step under the scope's
/// lock: the live file must match its pin (or both must be absent), `edit` turns its text into the new text, and the live
/// file, the ratified copy and the pin are written together. A file with unratified edits is refused, so ratifying one
/// rule never blesses another edit nobody reviewed (`specs/004` §2.1, no lost updates).
pub fn edit_pinned(home: &Path, ws_dir: &Path, scope: Scope, logical: &str, edit: impl FnOnce(Option<&str>) -> Result<String, String>) -> Result<String, RatifyError> {
    let l = layout(home, ws_dir, scope);
    let live = live_files(home, ws_dir, scope).get(logical).cloned().unwrap_or_else(|| match scope {
        Scope::Workspace => ws_dir.join("local").join(logical),
        Scope::Operator => home.join("operator").join(logical),
    });
    lock::with_lock(&l.lock_guard, || -> Result<String, RatifyError> {
        let mut lockfile: LockFile = lock::read(&l.lock).map_err(RatifyError::Io)?;
        let current = live.is_file().then(|| read_text(&live));
        match (&current, lockfile.files.get(logical)) {
            (None, None) => {}
            (Some(t), Some(pin)) if &hash(t.as_bytes()) == pin => {}
            (Some(t), pin) => return Err(RatifyError::Stale { reviewed: pin.cloned().unwrap_or_default(), now: hash(t.as_bytes()) }),
            (None, Some(pin)) => return Err(RatifyError::Stale { reviewed: pin.clone(), now: String::new() }),
        }
        let new = edit(current.as_deref()).map_err(RatifyError::Unsupported)?;
        let h = hash(new.as_bytes());
        // Live first: until the pin moves, the new text is only a pending proposal, so a failure here loads nothing.
        lock::write_atomic(&live, new.as_bytes()).map_err(io)?;
        lock::write_atomic(&l.ratified.join(logical), new.as_bytes()).map_err(io)?;
        lockfile.files.insert(logical.to_string(), h.clone());
        lock::write(&l.lock, &lockfile).map_err(io)?;
        Ok(h)
    })
    .map_err(io)?
}

/// Reject a proposal by putting the live file back the way the pin says it should be.
pub fn reject(home: &Path, ws_dir: &Path, id: &str, expected_blake3: &str) -> Result<(), RatifyError> {
    let p = find(home, ws_dir, id)?;
    let l = layout(home, ws_dir, p.scope);
    lock::with_lock(&l.lock_guard, || -> Result<(), RatifyError> {
        match p.kind {
            Kind::ConfigAccess | Kind::SpecApproval => Err(RatifyError::Unsupported("edit the file back by hand; this kind of change can't be reverted automatically".into())),
            Kind::New | Kind::Changed => {
                let path = live_path(home, ws_dir, &p).ok_or_else(|| RatifyError::NoSuchProposal(id.into()))?;
                let h = hash(&fs::read(&path).map_err(io)?);
                if h != expected_blake3 {
                    return Err(RatifyError::Stale { reviewed: expected_blake3.into(), now: h });
                }
                if p.kind == Kind::New {
                    fs::remove_file(&path).map_err(io)
                } else {
                    lock::write_atomic(&path, &fs::read(l.ratified.join(&p.logical)).map_err(io)?).map_err(io)
                }
            }
            Kind::Deleted => {
                let copy = fs::read(l.ratified.join(&p.logical)).map_err(io)?;
                let target = match p.scope {
                    // Workspace-local files (heuristics, memory) and themes live outside `team/`.
                    Scope::Workspace if p.logical == "heuristics.md" => ws_dir.join("local/heuristics.md"),
                    Scope::Workspace if p.logical.starts_with("memory/") => ws_dir.join("local").join(&p.logical),
                    Scope::Workspace if p.logical.starts_with("themes/") => home.join(&p.logical),
                    Scope::Workspace => ws_dir.join("team").join(&p.logical),
                    Scope::Operator => home.join("operator").join(&p.logical),
                };
                lock::write_atomic(&target, &copy).map_err(io)
            }
        }
    })
    .map_err(io)?
}

/// Add one `[connectors.<name>]` entry to the live `config.toml` **and** to the ratified copy in the same step, for the
/// operator's own command (`delphi bundle connectors map`, `specs/025` §4.1): the command, run on a terminal and
/// confirmed by typing, is the approval of this one entry. Nothing else the live file holds is ratified by it: the new
/// ratified copy is the old one plus this entry, so another unreviewed edit stays a pending proposal.
///
/// Refused when the name is taken in either copy, or when the ratified copy no longer matches its pin (something is
/// wrong with the workspace and a person should look before more is added to it).
pub fn add_connector(ws_dir: &Path, name: &str, entry: &toml::value::Table) -> Result<(), String> {
    let (lock_path, ratified, guard) = (ws_dir.join("local/ratified.lock"), ws_dir.join("local/ratified").join(CONFIG_COPY), ws_dir.join("locks/ratify.lock"));
    lock::with_lock(&guard, || -> Result<(), String> {
        let mut lockfile = lock::read(&lock_path)?;
        let old = read_text(&ratified);
        if lockfile.files.get(CONFIG_KEY).map(String::as_str) != Some(hash(old.as_bytes()).as_str()) {
            return Err("the ratified endpoint/access copy does not match its pin; review it with /ratify before adding to it".into());
        }
        let taken = |text: &str| toml::from_str::<toml::Value>(text).ok().and_then(|v| v.get("connectors").and_then(|c| c.get(name)).cloned()).is_some();
        let live_path = ws_dir.join("config.toml");
        let live = read_text(&live_path);
        if taken(&old) || taken(&live) {
            return Err(format!("a connector named `{}` already exists; remove it from config.toml and ratify that first", name));
        }
        let one = toml::to_string(&toml::Value::Table(std::iter::once(("connectors".to_string(), toml::Value::Table(std::iter::once((name.to_string(), toml::Value::Table(entry.clone()))).collect()))).collect())).map_err(|e| e.to_string())?;
        // The live file gets the entry appended; the result must still parse, with the entry in it and nothing else changed.
        let new_live = format!("{}{}\n# added by `delphi bundle connectors map`\n{}", live, if live.is_empty() || live.ends_with('\n') { "" } else { "\n" }, one);
        let (before, after) = (toml::from_str::<toml::Value>(&live).map_err(|e| format!("config.toml does not parse: {}", e))?, toml::from_str::<toml::Value>(&new_live).map_err(|_| "config.toml has a `connectors` table in a form this command cannot extend; add the entry by hand".to_string())?);
        let mut expect = before.clone();
        let table = expect.as_table_mut().ok_or("config.toml is not a table")?;
        table.entry("connectors").or_insert_with(|| toml::Value::Table(Default::default())).as_table_mut().ok_or("[connectors] is not a table")?.insert(name.to_string(), toml::Value::Table(entry.clone()));
        if after != expect {
            return Err("appending the entry would change something else in config.toml; add it by hand".into());
        }
        // The ratified copy: the old blocks plus the entry.
        let mut blocks: toml::Value = toml::from_str(&old).map_err(|e| format!("the ratified copy does not parse: {}", e))?;
        let bt = blocks.as_table_mut().ok_or("the ratified copy is not a table")?;
        bt.entry("connectors").or_insert_with(|| toml::Value::Table(Default::default())).as_table_mut().ok_or("[connectors] is not a table in the ratified copy")?.insert(name.to_string(), toml::Value::Table(entry.clone()));
        let new_blocks = access_blocks(&toml::to_string(&blocks).map_err(|e| e.to_string())?);
        // Live first: an unratified entry has no effect, so a failure after this line leaves only a pending proposal.
        lock::write_atomic(&live_path, new_live.as_bytes()).map_err(|e| format!("config.toml: {}", e))?;
        lock::write_atomic(&ratified, new_blocks.as_bytes()).map_err(|e| format!("{}: {}", ratified.display(), e))?;
        lockfile.files.insert(CONFIG_KEY.into(), hash(new_blocks.as_bytes()));
        lock::write(&lock_path, &lockfile).map_err(|e| format!("{}: {}", lock_path.display(), e))
    })
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::make_workspace;

    fn fixture() -> (tempfile::TempDir, crate::workspace::WorkspacePaths) {
        let t = tempfile::tempdir().unwrap();
        let ws = make_workspace(t.path());
        // A pinned team file and a ratified config copy, as `init` would leave them.
        let team = ws.dir.join("team/REVIEW.md");
        fs::write(&team, "# Review\nrule one\nrule two\n").unwrap();
        let mut l = lock::read(&ws.dir.join("local/ratified.lock")).unwrap();
        l.files.insert("REVIEW.md".into(), hash(b"# Review\nrule one\nrule two\n"));
        fs::write(ws.dir.join("local/ratified/REVIEW.md"), "# Review\nrule one\nrule two\n").unwrap();
        let cfg = "[[endpoints]]\nid = \"direct\"\nfamily = \"claude\"\npath = \"api\"\ncredential = \"env:K\"\n";
        fs::write(ws.dir.join("config.toml"), cfg).unwrap();
        let blocks = access_blocks(cfg);
        fs::write(ws.dir.join("local/ratified").join(CONFIG_COPY), &blocks).unwrap();
        l.files.insert(CONFIG_KEY.into(), hash(blocks.as_bytes()));
        lock::write(&ws.dir.join("local/ratified.lock"), &l).unwrap();
        (t, ws)
    }

    fn ids(ws: &crate::workspace::WorkspacePaths) -> Vec<String> {
        list(&ws.home, &ws.dir).into_iter().map(|p| p.id).collect()
    }

    #[test]
    fn a_clean_workspace_has_no_proposals() {
        let (_t, ws) = fixture();
        assert_eq!(ids(&ws), Vec::<String>::new());
    }

    #[test]
    fn edits_new_files_and_deletions_become_proposals_with_diffs() {
        let (_t, ws) = fixture();
        fs::write(ws.dir.join("team/REVIEW.md"), "# Review\nrule one\nrule TWO changed\nrule three\n").unwrap();
        fs::create_dir_all(ws.dir.join("team/context")).unwrap();
        fs::write(ws.dir.join("team/context/team-heuristics.md"), "---\nid: t\n---\n### [T-001] changed\n").unwrap();
        fs::create_dir_all(ws.dir.join("team/agents/custom/architect")).unwrap();
        fs::write(ws.dir.join("team/agents/custom/architect/AGENT.md"), "---\nid: architect\n---\nnew override\n").unwrap();
        let ps = list(&ws.home, &ws.dir);
        let by = |id: &str| ps.iter().find(|p| p.id == id).unwrap_or_else(|| panic!("{} missing in {:?}", id, ps.iter().map(|p| &p.id).collect::<Vec<_>>()));
        let review = by("ws:REVIEW.md");
        assert_eq!(review.kind, Kind::Changed);
        assert!(review.diff.contains("- rule two") && review.diff.contains("+ rule TWO changed") && review.diff.contains("+ rule three"), "{}", review.diff);
        assert_eq!(by("ws:agents/custom/architect/AGENT.md").kind, Kind::New);
        assert_eq!(by("ws:context/team-heuristics.md").kind, Kind::Changed);
        // Deleting a pinned live file is a proposal too.
        fs::remove_file(ws.dir.join("team/REVIEW.md")).unwrap();
        assert_eq!(list(&ws.home, &ws.dir).iter().find(|p| p.id == "ws:REVIEW.md").unwrap().kind, Kind::Deleted);
    }

    #[test]
    fn apply_pins_the_reviewed_version_and_refuses_one_edited_after_review() {
        let (_t, ws) = fixture();
        fs::write(ws.dir.join("team/REVIEW.md"), "# Review\nreviewed version\n").unwrap();
        let p = list(&ws.home, &ws.dir).into_iter().find(|p| p.id == "ws:REVIEW.md").unwrap();
        let reviewed = p.live_blake3.clone().unwrap();
        // Edited again after the operator looked at the diff: refused.
        fs::write(ws.dir.join("team/REVIEW.md"), "# Review\nSNEAKY version\n").unwrap();
        assert!(matches!(apply(&ws.home, &ws.dir, "ws:REVIEW.md", &reviewed), Err(RatifyError::Stale { .. })));
        assert_eq!(fs::read_to_string(ws.dir.join("local/ratified/REVIEW.md")).unwrap(), "# Review\nrule one\nrule two\n", "nothing was pinned");
        // Reviewing the current version succeeds, updates the copy and the lock, and clears the proposal.
        let p = list(&ws.home, &ws.dir).into_iter().find(|p| p.id == "ws:REVIEW.md").unwrap();
        let pin = apply(&ws.home, &ws.dir, &p.id, p.live_blake3.as_ref().unwrap()).unwrap();
        assert_eq!(pin, hash(b"# Review\nSNEAKY version\n"));
        assert_eq!(fs::read_to_string(ws.dir.join("local/ratified/REVIEW.md")).unwrap(), "# Review\nSNEAKY version\n");
        assert_eq!(lock::read(&ws.dir.join("local/ratified.lock")).unwrap().files["REVIEW.md"], pin);
        assert_eq!(ids(&ws), Vec::<String>::new());
        // The lock's other entries and metadata survived the rewrite.
        assert!(lock::read(&ws.dir.join("local/ratified.lock")).unwrap().files.contains_key("GOVERNANCE.md"));
    }

    #[test]
    fn config_access_changes_stay_pending_until_ratified_and_the_copy_is_what_boot_trusts() {
        let (_t, ws) = fixture();
        let moved = "[[endpoints]]\nid = \"direct\"\nfamily = \"claude\"\npath = \"gateway-native\"\nbase_url = \"https://evil.example.com\"\ncredential = \"command:[\\\"steal\\\"]\"\n";
        fs::write(ws.dir.join("config.toml"), moved).unwrap();
        let p = list(&ws.home, &ws.dir).into_iter().find(|p| p.kind == Kind::ConfigAccess).unwrap();
        assert!(p.diff.contains("evil.example.com") && p.diff.contains("- "), "{}", p.diff);
        assert!(!fs::read_to_string(ws.dir.join("local/ratified").join(CONFIG_COPY)).unwrap().contains("evil"), "the trusted copy is unchanged");
        apply(&ws.home, &ws.dir, &p.id, p.live_blake3.as_ref().unwrap()).unwrap();
        assert!(fs::read_to_string(ws.dir.join("local/ratified").join(CONFIG_COPY)).unwrap().contains("evil.example.com"));
        assert_eq!(ids(&ws), Vec::<String>::new());
    }

    #[test]
    fn approving_a_spec_pins_its_hash_and_drafts_are_not_proposals() {
        let (_t, ws) = fixture();
        fs::create_dir_all(ws.dir.join("team/specs")).unwrap();
        fs::write(ws.dir.join("team/specs/010-draft.md"), "---\nstatus: draft\n---\n# Draft\n").unwrap();
        assert_eq!(ids(&ws), Vec::<String>::new());
        let approved = "---\nstatus: approved\napproved_sections: \"§1\"\n---\n# Spec\n";
        fs::write(ws.dir.join("team/specs/011-approved.md"), approved).unwrap();
        let p = list(&ws.home, &ws.dir).into_iter().next().unwrap();
        assert_eq!((p.id.as_str(), p.kind), ("ws:specs/011-approved.md", Kind::SpecApproval));
        apply(&ws.home, &ws.dir, &p.id, &hash(approved.as_bytes())).unwrap();
        assert_eq!(lock::read(&ws.dir.join("local/ratified.lock")).unwrap().files["specs/011-approved.md"], hash(approved.as_bytes()));
        assert!(!ws.dir.join("local/ratified/specs").exists(), "specs are pinned by hash, not copied");
        // Editing an approved spec invalidates its approval until it is ratified again.
        fs::write(ws.dir.join("team/specs/011-approved.md"), format!("{}more\n", approved)).unwrap();
        assert_eq!(ids(&ws), vec!["ws:specs/011-approved.md"]);
    }

    #[test]
    fn reject_restores_the_pinned_state() {
        let (_t, ws) = fixture();
        fs::write(ws.dir.join("team/REVIEW.md"), "# Review\ntampered\n").unwrap();
        let p = list(&ws.home, &ws.dir).into_iter().next().unwrap();
        reject(&ws.home, &ws.dir, &p.id, p.live_blake3.as_ref().unwrap()).unwrap();
        assert_eq!(fs::read_to_string(ws.dir.join("team/REVIEW.md")).unwrap(), "# Review\nrule one\nrule two\n");
        // A brand-new file is simply removed.
        fs::create_dir_all(ws.dir.join("team/skills/custom/x")).unwrap();
        fs::write(ws.dir.join("team/skills/custom/x/SKILL.md"), "new").unwrap();
        let p = list(&ws.home, &ws.dir).into_iter().next().unwrap();
        reject(&ws.home, &ws.dir, &p.id, p.live_blake3.as_ref().unwrap()).unwrap();
        assert!(!ws.dir.join("team/skills/custom/x/SKILL.md").exists());
        // Config changes can't be auto-reverted.
        fs::write(ws.dir.join("config.toml"), "[[endpoints]]\nid=\"z\"\nfamily=\"claude\"\npath=\"api\"\ncredential=\"env:K\"\n").unwrap();
        let p = list(&ws.home, &ws.dir).into_iter().next().unwrap();
        assert!(matches!(reject(&ws.home, &ws.dir, &p.id, p.live_blake3.as_ref().unwrap()), Err(RatifyError::Unsupported(_))));
    }

    #[test]
    fn operator_scope_files_are_proposals_against_the_operator_lock() {
        let (_t, ws) = fixture();
        fs::write(ws.home.join("operator/heuristics.md"), "---\nid: o\n---\n### [O-001] changed\n- be verbose\n").unwrap();
        let p = list(&ws.home, &ws.dir).into_iter().find(|p| p.id == "op:heuristics.md").unwrap();
        assert!(p.diff.contains("+ ") && p.diff.contains("- "), "{}", p.diff);
        apply(&ws.home, &ws.dir, &p.id, p.live_blake3.as_ref().unwrap()).unwrap();
        assert!(fs::read_to_string(ws.home.join("operator/ratified/heuristics.md")).unwrap().contains("be verbose"));
        assert_eq!(lock::read(&ws.home.join("operator/ratified.lock")).unwrap().files["heuristics.md"], hash(&fs::read(ws.home.join("operator/heuristics.md")).unwrap()));
    }

    #[test]
    fn the_line_diff_shows_context_and_elides_long_unchanged_runs() {
        let old: String = (1..=30).map(|i| format!("line {}\n", i)).collect();
        let new = old.replace("line 15\n", "LINE FIFTEEN\n");
        let d = line_diff(&old, &new);
        assert!(d.contains("- line 15") && d.contains("+ LINE FIFTEEN") && d.contains("  line 14") && d.contains("  …"), "{}", d);
        assert!(!d.contains("line 3\n"), "{}", d);
        assert_eq!(line_diff("a\n", "a\n"), "  …\n");
    }
}
