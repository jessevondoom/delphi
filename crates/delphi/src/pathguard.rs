//! Logical path guard (`specs/001` §2.2, amendments 001-A1 and 001-A2). Step 1 ships it as a tested
//! library; the daemon's file tools use it from Step 2.
//!
//! Order of operations: canonicalize → apply policy on the **case-folded** path (macOS volumes are
//! case-insensitive, so `Events.jsonl` is `events.jsonl`) → map to exactly one zone root → open each
//! component with `openat(O_NOFOLLOW)`, rejecting non-regular files, hard links and protected inodes.
//! The caller receives open descriptors, never a path to re-open, so nothing can be swapped in between.

use std::collections::HashSet;
use std::ffi::CString;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::RawFd;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Denied {
    NotLogical(String),
    GitSegment,
    NeverReadable(String),
    NeverWritable(String),
    NotInScope,
    NoZone,
    Escapes,
    Symlink,
    NotRegular,
    HardLink,
    ProtectedInode,
    NotFound,
    Io(String),
}

/// Physical roots of one workspace (`specs/006` §3.1).
pub struct Zones {
    pub local: PathBuf,
    pub team: PathBuf,
    pub base: PathBuf,
    /// `local/ratified/`: the only copies of protected files the daemon (and agents) ever read.
    pub ratified: PathBuf,
    pub worktree: Option<PathBuf>,
    /// Registered projects by name, read-only through `@project/<name>/**` (001-A2).
    pub projects: Vec<(String, PathBuf)>,
}

pub struct Guard {
    pub zones: Zones,
    pub role: String,
    pub read_globs: Vec<String>,
    pub write_globs: Vec<String>,
    /// (st_dev, st_ino) of protected files (ratified.lock, events.jsonl, state.db, …).
    pub protected: HashSet<(u64, u64)>,
}

/// A successful resolution: the open parent directory, the final component's name, and the open
/// file when it exists. Callers use these descriptors (`openat`, `read`) rather than re-opening a path.
#[derive(Debug)]
pub struct Resolved {
    pub dir: OwnedFd,
    pub name: String,
    pub file: Option<OwnedFd>,
}

fn segments(key: &str) -> impl Iterator<Item = &str> {
    key.split('/')
}

/// `skills/custom/<skill>/…/scripts/…/<file>`: a file *inside* a `scripts/` directory of a named custom
/// skill, never the skill's `SKILL.md`. (`**` can match zero segments, so membership of a `scripts`
/// segment alone would admit `skills/custom/scripts/SKILL.md`.) Expects a case-folded key.
pub fn is_custom_skill_script(key: &str) -> bool {
    let segs: Vec<&str> = key.split('/').collect();
    segs.len() >= 5
        && segs[0] == "skills"
        && segs[1] == "custom"
        && !segs[2].is_empty()
        && segs[3..segs.len() - 1].contains(&"scripts")
        && !key.ends_with("/skill.md")
}

/// Files whose agent-visible copy is the ratified one (`specs/006` §3.1, `specs/004` §2.1).
fn is_ratified_kind(key: &str) -> bool {
    key == "governance.md"
        || key == "review.md"
        || key == "context/heuristics.md"
        || key == "context/team-heuristics.md"
        || key == "heuristics.md"
        || (key.starts_with("agents/") && key.ends_with("/agent.md"))
        || (key.starts_with("skills/") && (key.ends_with("/skill.md") || key.contains("/scripts/") || key.ends_with("/scripts")))
        || (key.starts_with("themes/") && key.ends_with("/theme.md"))
        || (key.starts_with("memory/agents/") && key.ends_with("/lessons.md"))
}

/// Logical paths no agent may read (§2.2 + 001-A1): event log, Council barrier, daemon state,
/// staged proposals, ratification and provenance state, other roles' memory, the operator home.
fn never_readable(key: &str, role: &str) -> Option<String> {
    if key.ends_with("events.jsonl") || segments(key).any(|s| s == "events") && key.starts_with("missions/") && key.ends_with(".jsonl") {
        return Some("event log".into());
    }
    if segments(key).any(|s| s == ".barrier") {
        return Some("Council barrier".into());
    }
    for p in ["run/", "state.db", "local/", "proposals/", "operator/", "ratified", "provenance", "memory/operator/", "config.toml"] {
        if key.starts_with(p) {
            return Some(p.trim_end_matches('/').into());
        }
    }
    if let Some(rest) = key.strip_prefix("memory/agents/") {
        let owner = rest.split('/').next().unwrap_or("");
        if owner != role {
            return Some(format!("another role's memory ({})", owner));
        }
    }
    None
}

/// Logical paths no agent may write, whatever its globs: daemon state, constitution, memory and
/// heuristics (ratified proposals only), built-in and custom manifests and skills, event log, barrier.
fn never_writable(key: &str) -> Option<String> {
    if let Some(why) = never_readable(key, "") {
        if !why.starts_with("another role") {
            return Some(why);
        }
    }
    let exact = ["governance.md", "spec.md", "review.md", "context/heuristics.md", "context/team-heuristics.md", "heuristics.md"];
    if exact.contains(&key) {
        return Some("constitution or heuristics (ratified proposals only)".into());
    }
    for p in ["base/", "memory/", "agents/", "themes/"] {
        if key.starts_with(p) {
            return Some(format!("{} (ratified proposals only)", p.trim_end_matches('/')));
        }
    }
    if key.starts_with("skills/") && !is_custom_skill_script(key) {
        return Some("skills (only skills/custom/**/scripts/** is writable; SKILL.md via ratified proposals)".into());
    }
    if key.starts_with("@project/") {
        return Some("project repos are read-only (changes happen in @worktree)".into());
    }
    None
}

/// NFC-normalize, reject absolute/`..`/empty, and drop `.` segments. Returns the path in its
/// original case; policy is applied to `to_lowercase()` of it.
pub fn canonical(logical: &str) -> Result<String, Denied> {
    let s: String = logical.nfc().collect();
    if s.is_empty() || s.starts_with('/') || s.starts_with('~') || s.contains('\0') {
        return Err(Denied::NotLogical(logical.into()));
    }
    let mut parts = Vec::new();
    for seg in s.split('/') {
        match seg {
            "" | "." => {}
            ".." => return Err(Denied::Escapes),
            x => parts.push(x),
        }
    }
    if parts.is_empty() {
        return Err(Denied::NotLogical(logical.into()));
    }
    if parts.iter().any(|p| p.to_lowercase() == ".git") {
        return Err(Denied::GitSegment);
    }
    Ok(parts.join("/"))
}

/// Minimal glob: `*` and `?` within a segment, `**` across segments. Case-sensitive; callers fold.
pub fn glob_match(glob: &str, path: &str) -> bool {
    fn seg_match(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => seg_match(&p[1..], s) || (!s.is_empty() && seg_match(p, &s[1..])),
            (Some(b'?'), Some(_)) => seg_match(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a == b => seg_match(&p[1..], &s[1..]),
            _ => false,
        }
    }
    fn go(g: &[&str], p: &[&str]) -> bool {
        match g.first() {
            None => p.is_empty(),
            Some(&"**") => go(&g[1..], p) || (!p.is_empty() && go(g, &p[1..])),
            Some(seg) => !p.is_empty() && seg_match(seg.as_bytes(), p[0].as_bytes()) && go(&g[1..], &p[1..]),
        }
    }
    let g: Vec<&str> = glob.split('/').collect();
    let p: Vec<&str> = path.split('/').collect();
    go(&g, &p)
}

impl Guard {
    /// Map a canonical logical path to (zone root, path inside it). `key` is the case-folded form.
    fn zone_of(&self, path: &str, key: &str, access: Access) -> Option<(PathBuf, String)> {
        if key.starts_with("@worktree/") {
            return self.zones.worktree.clone().map(|w| (w, path["@worktree/".len()..].to_string()));
        }
        if let Some(rest) = key.strip_prefix("@project/") {
            let name = rest.split('/').next()?;
            let (_, root) = self.zones.projects.iter().find(|(n, _)| n.to_lowercase() == name)?;
            let inner = path.splitn(3, '/').nth(2)?.to_string();
            return Some((root.clone(), inner));
        }
        // Reads of protected files come from their ratified copies. The only writable protected
        // kind is a custom skill script; writes go to the live team file, which then needs
        // ratification before it loads (specs/001 §2.3).
        if is_ratified_kind(key) && access == Access::Read {
            // Protected files are served from their ratified copies (`local/ratified/<logical>`),
            // never the live file.
            return Some((self.zones.ratified.clone(), path.to_string()));
        }
        if key.starts_with("missions/") {
            return Some((self.zones.local.clone(), path.into()));
        }
        if key.starts_with("memory/agents/") {
            return Some((self.zones.local.clone(), path.into()));
        }
        if key == "spec.md" {
            return Some((self.zones.base.clone(), path.into()));
        }
        if key.starts_with("context/") || key.starts_with("specs/") || key.starts_with("agents/custom/") || key.starts_with("skills/custom/") {
            return Some((self.zones.team.clone(), path.into()));
        }
        if key.starts_with("agents/") || key.starts_with("skills/") || key.starts_with("themes/") {
            return Some((self.zones.base.clone(), path.into()));
        }
        None
    }

    /// Full check; on success returns open descriptors for the target.
    pub fn check(&self, logical: &str, access: Access) -> Result<Resolved, Denied> {
        let path = canonical(logical)?;
        let key = path.to_lowercase();
        if let Some(why) = never_readable(&key, &self.role) {
            return Err(Denied::NeverReadable(why));
        }
        if access == Access::Write {
            if let Some(why) = never_writable(&key) {
                return Err(Denied::NeverWritable(why));
            }
        }
        let globs = if access == Access::Write { &self.write_globs } else { &self.read_globs };
        let allowed = globs.iter().any(|g| (g == "*" && access == Access::Read) || glob_match(&g.to_lowercase(), &key));
        if !allowed {
            return Err(Denied::NotInScope);
        }
        let (root, inner) = self.zone_of(&path, &key, access).ok_or(Denied::NoZone)?;
        resolve_nofollow(&root, &inner, access, &self.protected)
    }
}

fn openat_nofollow(dirfd: RawFd, name: &str, flags: libc::c_int) -> Result<OwnedFd, Denied> {
    let c = CString::new(name.as_bytes()).map_err(|_| Denied::NotLogical("nul".into()))?;
    let fd = unsafe { libc::openat(dirfd, c.as_ptr(), flags | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
    if fd < 0 {
        let err = std::io::Error::last_os_error();
        return Err(match err.raw_os_error() {
            Some(libc::ELOOP) | Some(libc::ENOTDIR) => Denied::Symlink,
            Some(libc::ENOENT) => Denied::NotFound,
            _ => Denied::Io(err.to_string()),
        });
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Walk `inner` under `root` one component at a time with `openat(O_NOFOLLOW)`. The final
/// component must be a regular, singly-linked, unprotected file; it is opened non-blocking so a FIFO
/// can't hang the daemon. For writes, a missing final component is allowed (`file: None`).
pub fn resolve_nofollow(root: &Path, inner: &str, access: Access, protected: &HashSet<(u64, u64)>) -> Result<Resolved, Denied> {
    use std::os::fd::AsRawFd;
    let root_c = CString::new(root.as_os_str().as_bytes()).map_err(|_| Denied::Io("root".into()))?;
    let raw = unsafe { libc::open(root_c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
    if raw < 0 {
        return Err(Denied::Io(format!("zone root {}: {}", root.display(), std::io::Error::last_os_error())));
    }
    let mut dir = unsafe { OwnedFd::from_raw_fd(raw) };
    let comps: Vec<&str> = inner.split('/').collect();
    let (last, parents) = comps.split_last().ok_or(Denied::NotLogical(inner.into()))?;
    for comp in parents {
        dir = openat_nofollow(dir.as_raw_fd(), comp, libc::O_RDONLY | libc::O_DIRECTORY)?;
    }
    let file = match openat_nofollow(dir.as_raw_fd(), last, libc::O_RDONLY | libc::O_NONBLOCK) {
        Ok(fd) => {
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
                return Err(Denied::Io("fstat".into()));
            }
            if (st.st_mode & libc::S_IFMT) != libc::S_IFREG {
                return Err(Denied::NotRegular);
            }
            if protected.contains(&(st.st_dev as u64, st.st_ino as u64)) {
                return Err(Denied::ProtectedInode);
            }
            if st.st_nlink > 1 {
                return Err(Denied::HardLink);
            }
            Some(fd)
        }
        Err(Denied::NotFound) if access == Access::Write => None,
        Err(e) => return Err(e),
    };
    Ok(Resolved { dir, name: last.to_string(), file })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{symlink, MetadataExt};

    fn inode(p: &Path) -> (u64, u64) {
        let m = fs::metadata(p).unwrap();
        (m.dev(), m.ino())
    }

    fn setup() -> (tempfile::TempDir, Guard) {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path();
        for d in ["local/missions/m1/councils/.barrier", "local/memory/agents/red-team", "team/context", "base/agents/builder", "ratified/memory/agents/builder", "wt/src", "proj/src"] {
            fs::create_dir_all(ws.join(d)).unwrap();
        }
        fs::write(ws.join("local/missions/m1/events.jsonl"), "{}").unwrap();
        fs::write(ws.join("local/missions/m1/notes.md"), "n").unwrap();
        fs::write(ws.join("local/missions/m1/councils/.barrier/r1.md"), "stance").unwrap();
        fs::write(ws.join("local/ratified.lock"), "{}").unwrap();
        fs::write(ws.join("ratified/GOVERNANCE.md"), "ratified governance").unwrap();
        fs::write(ws.join("local/memory/agents/red-team/lessons.md"), "secret").unwrap();
        fs::write(ws.join("wt/src/lib.rs"), "x").unwrap();
        fs::write(ws.join("wt/plain.txt"), "p").unwrap();
        fs::write(ws.join("wt/.git"), "gitdir: /x").unwrap();
        fs::write(ws.join("proj/src/main.rs"), "fn main(){}").unwrap();
        let mut protected = HashSet::new();
        protected.insert(inode(&ws.join("local/ratified.lock")));
        protected.insert(inode(&ws.join("local/missions/m1/events.jsonl")));
        let g = Guard {
            zones: Zones {
                local: ws.join("local"),
                team: ws.join("team"),
                base: ws.join("base"),
                ratified: ws.join("ratified"),
                worktree: Some(ws.join("wt")),
                projects: vec![("app".into(), ws.join("proj"))],
            },
            role: "builder".into(),
            read_globs: vec!["*".into()],
            write_globs: vec!["@worktree/**".into(), "missions/**".into()],
            protected,
        };
        (t, g)
    }

    #[test]
    fn allowed_read_and_write_return_descriptors() {
        let (_t, g) = setup();
        assert!(g.check("@worktree/src/lib.rs", Access::Write).unwrap().file.is_some());
        let new = g.check("@worktree/src/new.rs", Access::Write).unwrap();
        assert!(new.file.is_none() && new.name == "new.rs");
        assert!(g.check("missions/m1/notes.md", Access::Read).is_ok());
    }

    // specs/001 §7 test 6 (read and write)
    #[test]
    fn path_escapes_rejected_for_reads_and_writes() {
        let (t, g) = setup();
        for a in [Access::Read, Access::Write] {
            assert_eq!(g.check("@worktree/../local/ratified.lock", a).unwrap_err(), Denied::Escapes);
            assert_eq!(g.check("/etc/passwd", a).unwrap_err(), Denied::NotLogical("/etc/passwd".into()));
        }
        symlink(t.path().join("local/memory/agents/red-team/lessons.md"), t.path().join("wt/src/link.md")).unwrap();
        assert_eq!(g.check("@worktree/src/link.md", Access::Read).unwrap_err(), Denied::Symlink);
        assert_eq!(g.check("@worktree/src/link.md", Access::Write).unwrap_err(), Denied::Symlink);
        // A case variant of a never-writable zone is still never-writable.
        assert!(matches!(g.check("Memory/agents/builder/lessons.md", Access::Write), Err(Denied::NeverWritable(_))));
    }

    // specs/001 §7 test 7, including case variants and links in a writable directory
    #[test]
    fn event_log_unreadable_by_any_route() {
        let (t, g) = setup();
        assert!(matches!(g.check("missions/m1/events.jsonl", Access::Read), Err(Denied::NeverReadable(_))));
        assert!(matches!(g.check("missions/m1/Events.JSONL", Access::Read), Err(Denied::NeverReadable(_))));
        assert!(matches!(g.check("missions/m1/Events.jsonl", Access::Write), Err(Denied::NeverReadable(_))));
        symlink(t.path().join("local/missions/m1/events.jsonl"), t.path().join("local/missions/m1/view.md")).unwrap();
        assert_eq!(g.check("missions/m1/view.md", Access::Read).unwrap_err(), Denied::Symlink);
        fs::hard_link(t.path().join("local/missions/m1/events.jsonl"), t.path().join("local/missions/m1/copy.md")).unwrap();
        assert_eq!(g.check("missions/m1/copy.md", Access::Read).unwrap_err(), Denied::ProtectedInode);
    }

    #[test]
    fn council_barrier_unreachable() {
        let (_t, g) = setup();
        assert!(matches!(g.check("missions/m1/councils/.barrier/r1.md", Access::Read), Err(Denied::NeverReadable(_))));
        assert!(matches!(g.check("missions/m1/councils/.BARRIER/r1.md", Access::Write), Err(Denied::NeverReadable(_))));
    }

    // specs/001 §7 test 11
    #[test]
    fn git_segments_rejected() {
        let (_t, g) = setup();
        assert_eq!(g.check("@worktree/.git", Access::Write).unwrap_err(), Denied::GitSegment);
        assert_eq!(g.check("@worktree/sub/.git/config", Access::Write).unwrap_err(), Denied::GitSegment);
        assert_eq!(g.check("@worktree/.GIT/hooks/x", Access::Write).unwrap_err(), Denied::GitSegment);
    }

    // specs/001 §7 test 12: hard links and protected inodes are separate checks
    #[test]
    fn hard_links_rejected_even_when_unprotected() {
        let (t, g) = setup();
        fs::hard_link(t.path().join("wt/plain.txt"), t.path().join("wt/src/alias.txt")).unwrap();
        assert_eq!(g.check("@worktree/src/alias.txt", Access::Read).unwrap_err(), Denied::HardLink);
        assert_eq!(g.check("@worktree/src/alias.txt", Access::Write).unwrap_err(), Denied::HardLink);
        fs::hard_link(t.path().join("local/ratified.lock"), t.path().join("wt/src/lock.json")).unwrap();
        assert_eq!(g.check("@worktree/src/lock.json", Access::Read).unwrap_err(), Denied::ProtectedInode);
    }

    #[test]
    fn fifo_is_rejected_without_blocking() {
        let (t, g) = setup();
        let c = CString::new(t.path().join("wt/src/pipe").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert_eq!(g.check("@worktree/src/pipe", Access::Read).unwrap_err(), Denied::NotRegular);
    }

    #[test]
    fn protected_files_served_from_ratified_copies() {
        let (_t, g) = setup();
        let r = g.check("GOVERNANCE.md", Access::Read).unwrap();
        assert!(r.file.is_some());
        assert!(matches!(g.check("GOVERNANCE.md", Access::Write), Err(Denied::NeverWritable(_))));
    }

    #[test]
    fn zone_table_and_custom_skills() {
        let (t, mut g) = setup();
        fs::write(t.path().join("base/SPEC.md"), "spec").unwrap();
        fs::create_dir_all(t.path().join("local/memory/agents/builder")).unwrap();
        fs::write(t.path().join("local/memory/agents/builder/notes.md"), "mine").unwrap();
        assert!(g.check("SPEC.md", Access::Read).is_ok());
        assert!(g.check("memory/agents/builder/notes.md", Access::Read).is_ok());
        g.write_globs.push("skills/custom/**/scripts/**".into());
        fs::create_dir_all(t.path().join("team/skills/custom/x/scripts")).unwrap();
        assert!(g.check("skills/custom/x/scripts/run.sh", Access::Write).is_ok());
        g.write_globs.push("skills/custom/**".into());
        for k in ["skills/custom/x/SKILL.md", "skills/custom/scripts/SKILL.md", "skills/custom/x/scripts/SKILL.md", "skills/custom/x/scripts"] {
            assert!(matches!(g.check(k, Access::Write), Err(Denied::NeverWritable(_))), "{}", k);
        }
        // Reads of a custom script come from the ratified copy, which doesn't exist yet.
        fs::write(t.path().join("team/skills/custom/x/scripts/run.sh"), "live").unwrap();
        assert_eq!(g.check("skills/custom/x/scripts/run.sh", Access::Read).unwrap_err(), Denied::NotFound);
    }

    #[test]
    fn project_root_is_read_only() {
        let (_t, g) = setup();
        assert!(g.check("@project/app/src/main.rs", Access::Read).unwrap().file.is_some());
        assert!(matches!(g.check("@project/app/src/main.rs", Access::Write), Err(Denied::NeverWritable(_))));
        assert_eq!(g.check("@project/app/.git/config", Access::Read).unwrap_err(), Denied::GitSegment);
    }

    #[test]
    fn other_roles_memory_and_operator_unreadable() {
        let (_t, g) = setup();
        assert!(matches!(g.check("memory/agents/red-team/lessons.md", Access::Read), Err(Denied::NeverReadable(_))));
        assert!(matches!(g.check("memory/agents/RED-TEAM/lessons.md", Access::Read), Err(Denied::NeverReadable(_))));
        assert!(matches!(g.check("operator/profile.md", Access::Read), Err(Denied::NeverReadable(_))));
    }

    #[test]
    fn nfc_normalization() {
        assert_eq!(canonical("missions/cafe\u{301}.md").unwrap(), "missions/caf\u{e9}.md");
    }

    #[test]
    fn globs() {
        assert!(glob_match("missions/**/research/**", "missions/m1/research/a.md"));
        assert!(glob_match("@worktree/**", "@worktree/a/b"));
        assert!(!glob_match("missions/*.md", "missions/a/b.md"));
        assert!(glob_match("*git*", ".git"));
    }
}
