//! Logical path guard (`specs/001` §2.2, amendment 001-A1). Step 1 ships it as a tested library;
//! the daemon's file tools use it from Step 2.
//!
//! Order of operations: match logical globs → map to exactly one zone root → resolve inside that
//! root with per-component `openat(O_NOFOLLOW)`, rejecting `.git` segments, hard links and
//! protected inodes.

use std::collections::HashSet;
use std::ffi::CString;
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
    HardLink,
    ProtectedInode,
    Io(String),
}

/// Physical roots of one workspace (`specs/006` §3.1).
pub struct Zones {
    pub local: PathBuf,
    pub team: PathBuf,
    pub core: PathBuf,
    pub base: PathBuf,
    pub worktree: Option<PathBuf>,
}

pub struct Guard {
    pub zones: Zones,
    pub role: String,
    pub read_globs: Vec<String>,
    pub write_globs: Vec<String>,
    /// (st_dev, st_ino) of protected files (ratified.lock, events.jsonl, state.db, …).
    pub protected: HashSet<(u64, u64)>,
}

/// Logical paths no agent may read (§2.2): event log, daemon run dir, state DB, staged proposals,
/// ratification state, and anything under the operator home.
fn never_readable(logical: &str, role: &str) -> Option<String> {
    if logical.ends_with("events.jsonl") {
        return Some("event log".into());
    }
    for p in ["run/", "state.db", "local/", "proposals/", "operator/", "ratified"] {
        if logical.starts_with(p) || logical.contains(&format!("/{}", p)) && p == "ratified" {
            return Some(p.trim_end_matches('/').into());
        }
    }
    if let Some(rest) = logical.strip_prefix("memory/agents/") {
        let owner = rest.split('/').next().unwrap_or("");
        if owner != role {
            return Some(format!("another role's memory ({})", owner));
        }
    }
    if logical.starts_with("memory/operator/") {
        return Some("operator memory (snapshot only)".into());
    }
    None
}

fn never_writable(logical: &str) -> Option<String> {
    for p in ["run/", "state.db", "config.toml", "base/", "local/", "proposals/", "operator/", "GOVERNANCE.md", "SPEC.md", "REVIEW.md"] {
        if logical == p.trim_end_matches('/') || logical.starts_with(p) {
            return Some(p.trim_end_matches('/').into());
        }
    }
    if logical.ends_with("events.jsonl") {
        return Some("event log".into());
    }
    if logical.starts_with("agents/") && !logical.starts_with("agents/custom/") || logical.starts_with("skills/") && !logical.starts_with("skills/custom/") {
        return Some("built-in manifests and skills".into());
    }
    None
}

/// NFC-normalize, reject absolute/`..`/empty, and drop `.` segments.
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
    if parts.iter().any(|p| p.eq_ignore_ascii_case(".git")) {
        return Err(Denied::GitSegment);
    }
    Ok(parts.join("/"))
}

/// Minimal glob: `*` within a segment, `**` across segments.
pub fn glob_match(glob: &str, path: &str) -> bool {
    fn seg_match(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => seg_match(&p[1..], s) || (!s.is_empty() && seg_match(p, &s[1..])),
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
    /// Map a canonical logical path to (zone root, path inside the zone).
    fn zone_of(&self, logical: &str) -> Option<(PathBuf, String)> {
        if let Some(rest) = logical.strip_prefix("@worktree/") {
            return self.zones.worktree.clone().map(|w| (w, rest.to_string()));
        }
        if logical.starts_with("missions/") || logical.starts_with("memory/agents/") {
            return Some((self.zones.local.clone(), logical.into()));
        }
        if logical.starts_with("context/") || logical.starts_with("specs/") || logical == "REVIEW.md" || logical.starts_with("agents/custom/") || logical.starts_with("skills/custom/") {
            return Some((self.zones.team.clone(), logical.into()));
        }
        if logical == "GOVERNANCE.md" || logical == "SPEC.md" || logical.starts_with("agents/") || logical.starts_with("skills/") {
            let customized = self.zones.core.join(logical);
            let root = if customized.exists() { self.zones.core.clone() } else { self.zones.base.clone() };
            return Some((root, logical.into()));
        }
        None
    }

    /// Full check; on success returns the resolved physical path.
    pub fn check(&self, logical: &str, access: Access) -> Result<PathBuf, Denied> {
        let path = canonical(logical)?;
        if let Some(why) = never_readable(&path, &self.role) {
            return Err(Denied::NeverReadable(why));
        }
        if access == Access::Write {
            if let Some(why) = never_writable(&path) {
                return Err(Denied::NeverWritable(why));
            }
        }
        let globs = if access == Access::Write { &self.write_globs } else { &self.read_globs };
        if !globs.iter().any(|g| g == "*" && access == Access::Read || glob_match(g, &path)) {
            return Err(Denied::NotInScope);
        }
        let (root, inner) = self.zone_of(&path).ok_or(Denied::NoZone)?;
        resolve_nofollow(&root, &inner, access, &self.protected)
    }
}

/// Walk `inner` under `root` one component at a time with `openat(O_NOFOLLOW)`. For writes the
/// final component may not exist yet; its parent must.
pub fn resolve_nofollow(root: &Path, inner: &str, access: Access, protected: &HashSet<(u64, u64)>) -> Result<PathBuf, Denied> {
    let open = |dirfd: RawFd, name: &[u8], flags: libc::c_int| -> Result<RawFd, Denied> {
        let c = CString::new(name).map_err(|_| Denied::NotLogical("nul".into()))?;
        let fd = unsafe { libc::openat(dirfd, c.as_ptr(), flags | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
        if fd < 0 {
            let err = std::io::Error::last_os_error();
            return Err(match err.raw_os_error() {
                Some(libc::ELOOP) => Denied::Symlink,
                Some(libc::ENOTDIR) => Denied::Symlink,
                _ => Denied::Io(err.to_string()),
            });
        }
        Ok(fd)
    };
    let root_c = CString::new(root.as_os_str().as_bytes()).map_err(|_| Denied::Io("root".into()))?;
    let mut dirfd = unsafe { libc::open(root_c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
    if dirfd < 0 {
        return Err(Denied::Io(format!("zone root {}: {}", root.display(), std::io::Error::last_os_error())));
    }
    let comps: Vec<&str> = inner.split('/').collect();
    let mut result = Ok(root.join(inner));
    for (i, comp) in comps.iter().enumerate() {
        let last = i + 1 == comps.len();
        if !last {
            match open(dirfd, comp.as_bytes(), libc::O_RDONLY | libc::O_DIRECTORY) {
                Ok(fd) => {
                    unsafe { libc::close(dirfd) };
                    dirfd = fd;
                }
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
            continue;
        }
        match open(dirfd, comp.as_bytes(), libc::O_RDONLY) {
            Ok(fd) => {
                let mut st: libc::stat = unsafe { std::mem::zeroed() };
                let ok = unsafe { libc::fstat(fd, &mut st) } == 0;
                unsafe { libc::close(fd) };
                if !ok {
                    result = Err(Denied::Io("fstat".into()));
                } else if protected.contains(&(st.st_dev as u64, st.st_ino as u64)) {
                    result = Err(Denied::ProtectedInode);
                } else if (st.st_mode & libc::S_IFMT) == libc::S_IFREG && st.st_nlink > 1 {
                    result = Err(Denied::HardLink);
                }
            }
            Err(Denied::Io(e)) if access == Access::Write && e.contains("No such file") => {}
            Err(e) => result = Err(e),
        }
    }
    unsafe { libc::close(dirfd) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{symlink, MetadataExt};

    fn setup() -> (tempfile::TempDir, Guard) {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path();
        for d in ["local/missions/m1", "local/memory/agents/builder", "local/memory/agents/red-team", "team/context", "core", "base/agents/builder", "wt/src"] {
            fs::create_dir_all(ws.join(d)).unwrap();
        }
        fs::write(ws.join("local/missions/m1/events.jsonl"), "{}").unwrap();
        fs::write(ws.join("local/missions/m1/notes.md"), "n").unwrap();
        fs::write(ws.join("local/ratified.lock"), "{}").unwrap();
        fs::write(ws.join("local/memory/agents/red-team/lessons.md"), "secret").unwrap();
        fs::write(ws.join("wt/src/lib.rs"), "x").unwrap();
        fs::write(ws.join("wt/.git"), "gitdir: /x").unwrap();
        let lock_meta = fs::metadata(ws.join("local/ratified.lock")).unwrap();
        let mut protected = HashSet::new();
        protected.insert((lock_meta.dev(), lock_meta.ino()));
        let g = Guard {
            zones: Zones { local: ws.join("local"), team: ws.join("team"), core: ws.join("core"), base: ws.join("base"), worktree: Some(ws.join("wt")) },
            role: "builder".into(),
            read_globs: vec!["*".into()],
            write_globs: vec!["@worktree/**".into(), "missions/**".into()],
            protected,
        };
        (t, g)
    }

    #[test]
    fn allowed_read_and_write() {
        let (_t, g) = setup();
        assert!(g.check("@worktree/src/lib.rs", Access::Write).is_ok());
        assert!(g.check("@worktree/src/new.rs", Access::Write).is_ok());
        assert!(g.check("missions/m1/notes.md", Access::Read).is_ok());
    }

    // specs/001 §7 test 6
    #[test]
    fn path_escapes_rejected() {
        let (t, g) = setup();
        assert_eq!(g.check("@worktree/../local/ratified.lock", Access::Read).unwrap_err(), Denied::Escapes);
        assert_eq!(g.check("/etc/passwd", Access::Read).unwrap_err(), Denied::NotLogical("/etc/passwd".into()));
        symlink(t.path().join("local/memory/agents/red-team/lessons.md"), t.path().join("wt/src/link.md")).unwrap();
        assert_eq!(g.check("@worktree/src/link.md", Access::Read).unwrap_err(), Denied::Symlink);
        assert!(matches!(g.check("Memory/agents/builder/x", Access::Write), Err(Denied::NotInScope) | Err(Denied::NoZone)));
    }

    // specs/001 §7 test 7
    #[test]
    fn event_log_unreadable() {
        let (_t, g) = setup();
        assert!(matches!(g.check("missions/m1/events.jsonl", Access::Read), Err(Denied::NeverReadable(_))));
    }

    // specs/001 §7 test 11
    #[test]
    fn git_segments_rejected() {
        let (_t, g) = setup();
        assert_eq!(g.check("@worktree/.git", Access::Write).unwrap_err(), Denied::GitSegment);
        assert_eq!(g.check("@worktree/sub/.git/config", Access::Write).unwrap_err(), Denied::GitSegment);
        assert_eq!(g.check("@worktree/.GIT/hooks/x", Access::Write).unwrap_err(), Denied::GitSegment);
    }

    // specs/001 §7 test 12
    #[test]
    fn hard_links_and_protected_inodes_rejected() {
        let (t, g) = setup();
        fs::hard_link(t.path().join("local/ratified.lock"), t.path().join("wt/src/lock.json")).unwrap();
        let e = g.check("@worktree/src/lock.json", Access::Read).unwrap_err();
        assert!(e == Denied::ProtectedInode || e == Denied::HardLink, "{:?}", e);
    }

    #[test]
    fn other_roles_memory_and_operator_unreadable() {
        let (_t, g) = setup();
        assert!(matches!(g.check("memory/agents/red-team/lessons.md", Access::Read), Err(Denied::NeverReadable(_))));
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
    }
}
