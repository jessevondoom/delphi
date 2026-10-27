//! Logical path guard (`specs/001` §2.2, amendments 001-A1 and 001-A2). Step 1 ships it as a tested
//! library; the daemon's file tools use it from Step 2.
//!
//! Order of operations: canonicalize → apply policy on the **case-folded** path (macOS volumes are
//! case-insensitive, so `Events.jsonl` is `events.jsonl`) → map to exactly one zone root → open each
//! component with `openat(O_NOFOLLOW)`, rejecting non-regular files, hard links and protected inodes.
//! The caller receives open descriptors, never a path to re-open, so nothing can be swapped in between.

use std::collections::HashSet;
use std::io::Write;
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
    /// Added bundles by `org/name`, each the active hash's `knowledge/` directory, read-only through
    /// `@bundle/<org>/<name>/knowledge/**` (`specs/024` §4.1). Nothing else of a bundle is reachable.
    pub bundles: Vec<(String, PathBuf)>,
    /// The connector payload store, readable only as `@payload/<64 hex>` (`specs/010` §8.2).
    pub payload: Option<PathBuf>,
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
/// the error log (`logs/`, `specs/039`: it holds hostile text echoed from tools and stores), staged proposals, ratification and provenance state, other roles' memory, the operator home.
fn never_readable(key: &str, role: &str) -> Option<String> {
    // `missions/<id>/events…` is the whole segmented log: segments, rotation files and the writer lock.
    if key.ends_with("events.jsonl") || key.starts_with("missions/") && segments(key).nth(2) == Some("events") {
        return Some("event log".into());
    }
    if segments(key).any(|s| s == ".barrier") {
        return Some("Council barrier".into());
    }
    for p in ["run/", "state.db", "local/", "proposals/", "operator/", "ratified", "provenance", "memory/operator/", "config.toml", "logs/"] {
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
    if key.starts_with("@bundle/") {
        return Some("added bundles are read-only (they change only by `delphi bundle add|update|remove`)".into());
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
        if key.starts_with("@payload/") {
            // Read-only, one file by its exact blake3 name: no directory, no listing, no traversal.
            let name = &path["@payload/".len()..];
            let hex = name.len() == 64 && name.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'));
            return if access == Access::Read && hex { self.zones.payload.clone().map(|p| (p, name.to_string())) } else { None };
        }
        if let Some(rest) = key.strip_prefix("@project/") {
            let name = rest.split('/').next()?;
            let (_, root) = self.zones.projects.iter().find(|(n, _)| n.to_lowercase() == name)?;
            let inner = path.splitn(3, '/').nth(2)?.to_string();
            return Some((root.clone(), inner));
        }
        if let Some(rest) = key.strip_prefix("@bundle/") {
            // Read-only, and only a bundle's knowledge: skills, agents, behavior and vectors reach a prompt as
            // what they are (a skill, a guidance block), never as files an agent can open (`specs/024` §4.1).
            if access != Access::Read {
                return None;
            }
            let mut seg = rest.split('/');
            let (org, name, section) = (seg.next()?, seg.next()?, seg.next()?);
            let id = format!("{}/{}", org, name);
            let (_, root) = self.zones.bundles.iter().find(|(n, _)| *n == id)?;
            if section == "skills" {
                // A skill's `references/` is text to retrieve (`specs/025` §3): `skills/<skill>/references/**` and nothing
                // else of `skills/` (no SKILL.md, no scripts, no templates).
                let (skill, dir) = (seg.next()?, seg.next()?);
                if skill.is_empty() || dir != "references" {
                    return None;
                }
                let bundle_dir = root.parent()?;
                let inner = path.splitn(4, '/').nth(3).unwrap_or("").to_string();
                return Some((bundle_dir.to_path_buf(), inner));
            }
            if section != "knowledge" {
                return None;
            }
            let inner = path.splitn(5, '/').nth(4).unwrap_or("").to_string();
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

    /// Policy shared by every operation: canonicalize, fold case, apply the deny lists and the role's
    /// globs, and map to exactly one zone root. Returns (root, inner path in that zone).
    fn plan(&self, logical: &str, access: Access, is_dir: bool) -> Result<(PathBuf, String), Denied> {
        let path = canonical(logical)?;
        let key = path.to_lowercase();
        // The deny lists are written for paths *under* a zone (`operator/…`), so a directory is checked
        // both as itself and with a trailing slash; otherwise `operator` alone would slip through.
        let denied = never_readable(&key, &self.role).or_else(|| if is_dir { never_readable(&format!("{}/", key), &self.role) } else { None });
        if let Some(why) = denied {
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
        if is_dir {
            // Zone lookup works on prefixes ("specs/"), so a directory is looked up with a trailing slash.
            let (root, inner) = self.zone_of(&format!("{}/", path), &format!("{}/", key), access).ok_or(Denied::NoZone)?;
            Ok((root, inner.trim_end_matches('/').to_string()))
        } else {
            self.zone_of(&path, &key, access).ok_or(Denied::NoZone)
        }
    }

    /// Policy only, no filesystem access: would this logical path be allowed for `access`? Used to refuse
    /// an action before the operator is asked about it.
    pub fn check_policy(&self, logical: &str, access: Access) -> Result<(), Denied> {
        self.plan(logical, access, false).map(|_| ())
    }

    /// Open a directory for reading under the same policy as [`Guard::check`].
    pub fn check_dir(&self, logical: &str) -> Result<OwnedFd, Denied> {
        let (root, inner) = self.plan(logical, Access::Read, true)?;
        let mut dir = {
            let c = CString::new(root.as_os_str().as_bytes()).map_err(|_| Denied::Io("root".into()))?;
            let raw = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
            if raw < 0 {
                return Err(Denied::Io(format!("zone root {}: {}", root.display(), std::io::Error::last_os_error())));
            }
            unsafe { OwnedFd::from_raw_fd(raw) }
        };
        use std::os::fd::AsRawFd;
        for comp in inner.split('/').filter(|c| !c.is_empty()) {
            dir = openat_nofollow(dir.as_raw_fd(), comp, libc::O_RDONLY | libc::O_DIRECTORY)?;
        }
        Ok(dir)
    }

    /// List a directory, hiding `.git` and anything the role may never read.
    pub fn list(&self, logical: &str) -> Result<Vec<DirEntry>, Denied> {
        use std::os::fd::AsRawFd;
        let dir = self.check_dir(logical)?;
        let base = canonical(logical)?;
        Ok(list_fd(dir.as_raw_fd())?
            .into_iter()
            .filter(|e| e.name.to_lowercase() != ".git" && never_readable(&format!("{}/{}", base, e.name).to_lowercase(), &self.role).is_none())
            .collect())
    }

    /// Read a file through the guard (at most `max` bytes).
    pub fn read_file(&self, logical: &str, max: usize) -> Result<(Vec<u8>, bool), Denied> {
        let r = self.check(logical, Access::Read)?;
        r.read(max).map_err(|e| Denied::Io(e.to_string()))
    }

    /// Create or replace a file atomically: write a temp file in the target directory (creating missing
    /// parents), fsync it, rename it over the target, fsync the directory. Every guard check applies.
    /// Returns whether the file was newly created.
    pub fn write_atomic(&self, logical: &str, bytes: &[u8]) -> Result<bool, Denied> {
        use std::os::fd::AsRawFd;
        let (root, inner) = self.plan(logical, Access::Write, false)?;
        let r = resolve_nofollow_opts(&root, &inner, Access::Write, &self.protected, true)?;
        let created = r.file.is_none();
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let tmp = format!(".{}.delphi-tmp-{}-{}", r.name, std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        let tmpc = CString::new(tmp.as_bytes()).map_err(|_| Denied::NotLogical("nul".into()))?;
        let fd = unsafe { libc::openat(r.dir.as_raw_fd(), tmpc.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o644 as libc::c_uint) };
        if fd < 0 {
            return Err(Denied::Io(std::io::Error::last_os_error().to_string()));
        }
        let mut f = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        let cleanup = |dirfd: RawFd| unsafe {
            libc::unlinkat(dirfd, tmpc.as_ptr(), 0);
        };
        let write = (|| -> std::io::Result<()> {
            f.write_all(bytes)?;
            f.sync_all()
        })();
        if let Err(e) = write {
            cleanup(r.dir.as_raw_fd());
            return Err(Denied::Io(e.to_string()));
        }
        drop(f);
        let namec = CString::new(r.name.as_bytes()).map_err(|_| Denied::NotLogical("nul".into()))?;
        if unsafe { libc::renameat(r.dir.as_raw_fd(), tmpc.as_ptr(), r.dir.as_raw_fd(), namec.as_ptr()) } != 0 {
            let e = std::io::Error::last_os_error();
            cleanup(r.dir.as_raw_fd());
            return Err(Denied::Io(e.to_string()));
        }
        unsafe { libc::fsync(r.dir.as_raw_fd()) };
        Ok(created)
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

/// What a directory entry is, from `d_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub kind: EntryKind,
}

/// List a directory through an open descriptor (no path is re-resolved).
fn list_fd(fd: RawFd) -> Result<Vec<DirEntry>, Denied> {
    let dup = unsafe { libc::dup(fd) };
    if dup < 0 {
        return Err(Denied::Io(std::io::Error::last_os_error().to_string()));
    }
    let dirp = unsafe { libc::fdopendir(dup) };
    if dirp.is_null() {
        unsafe { libc::close(dup) };
        return Err(Denied::Io(std::io::Error::last_os_error().to_string()));
    }
    let mut out = Vec::new();
    loop {
        let ent = unsafe { libc::readdir(dirp) };
        if ent.is_null() {
            break;
        }
        let name = unsafe { std::ffi::CStr::from_ptr((*ent).d_name.as_ptr()) }.to_string_lossy().into_owned();
        if name == "." || name == ".." {
            continue;
        }
        let kind = match unsafe { (*ent).d_type } {
            libc::DT_REG => EntryKind::File,
            libc::DT_DIR => EntryKind::Dir,
            _ => EntryKind::Other, // symlinks, sockets, FIFOs: shown as unreadable
        };
        out.push(DirEntry { name, kind });
    }
    unsafe { libc::closedir(dirp) };
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

impl Resolved {
    /// Read the opened file (at most `max` bytes; `truncated` reports whether more existed).
    pub fn read(&self, max: usize) -> std::io::Result<(Vec<u8>, bool)> {
        use std::io::Read;
        let fd = self.file.as_ref().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"))?;
        let mut f = std::fs::File::from(fd.try_clone()?);
        let mut buf = Vec::new();
        Read::take(&mut f, max as u64 + 1).read_to_end(&mut buf)?;
        let truncated = buf.len() > max;
        buf.truncate(max);
        Ok((buf, truncated))
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
    resolve_nofollow_opts(root, inner, access, protected, false)
}

fn mkdirat(dirfd: RawFd, name: &str) -> Result<(), Denied> {
    let c = CString::new(name.as_bytes()).map_err(|_| Denied::NotLogical("nul".into()))?;
    let rc = unsafe { libc::mkdirat(dirfd, c.as_ptr(), 0o755) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EEXIST) {
            return Err(Denied::Io(err.to_string()));
        }
    }
    Ok(())
}

/// As [`resolve_nofollow`]; with `create_parents`, missing intermediate directories are created
/// one component at a time with `mkdirat` and then opened with `O_NOFOLLOW`, so a racing symlink
/// can't redirect the walk.
pub fn resolve_nofollow_opts(root: &Path, inner: &str, access: Access, protected: &HashSet<(u64, u64)>, create_parents: bool) -> Result<Resolved, Denied> {
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
        dir = match openat_nofollow(dir.as_raw_fd(), comp, libc::O_RDONLY | libc::O_DIRECTORY) {
            Err(Denied::NotFound) if create_parents => {
                mkdirat(dir.as_raw_fd(), comp)?;
                openat_nofollow(dir.as_raw_fd(), comp, libc::O_RDONLY | libc::O_DIRECTORY)?
            }
            other => other?,
        };
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
    #[test]
    fn the_whole_event_log_directory_is_off_limits() {
        for k in ["missions/m1/events/.lock", "missions/m1/events/000001.jsonl", "missions/m1/events", "missions/m1/events.jsonl"] {
            assert!(never_readable(k, "builder").is_some() && never_writable(k).is_some(), "{}", k);
        }
        assert!(never_readable("missions/m1/notes/events-plan.md", "builder").is_none());
    }

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
        for d in ["local/missions/m1/councils/.barrier", "local/memory/agents/red-team", "team/context", "base/agents/builder", "ratified/memory/agents/builder", "wt/src", "proj/src", "cache/knowledge/runbooks", "cache/behavior", "cache/skills/s", "cache/vectors"] {
            fs::create_dir_all(ws.join(d)).unwrap();
        }
        fs::write(ws.join("cache/knowledge/runbooks/deploy.md"), "deploy").unwrap();
        fs::write(ws.join("cache/behavior/goals.md"), "goals").unwrap();
        fs::write(ws.join("cache/skills/s/SKILL.md"), "skill").unwrap();
        fs::write(ws.join("cache/vectors/index.json"), "{}").unwrap();
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
                bundles: vec![("acme/a".into(), ws.join("cache/knowledge"))],
                payload: Some(ws.join("payloads")),
            },
            role: "builder".into(),
            read_globs: vec!["*".into()],
            write_globs: vec!["@worktree/**".into(), "missions/**".into()],
            protected,
        };
        (t, g)
    }

    // specs/010 §8.2: a stored connector result is reachable only by its exact name, read-only.
    #[test]
    fn payloads_are_readable_only_by_their_exact_hash_name() {
        let (t, g) = setup();
        let hash = "ab".repeat(32);
        fs::create_dir_all(t.path().join("payloads")).unwrap();
        fs::write(t.path().join("payloads").join(&hash), "private mail").unwrap();
        fs::write(t.path().join("payloads/notes.txt"), "not a payload").unwrap();
        let r = g.check(&format!("@payload/{}", hash), Access::Read).unwrap();
        assert_eq!(r.read(100).unwrap().0, b"private mail");
        for bad in [format!("@payload/{}", hash.to_uppercase()), format!("@payload/{}/", &hash[..63]), "@payload/notes.txt".into(), "@payload".into(), "@payload/".into(), format!("@payload/../{}", hash), format!("@payload/x/{}", hash)] {
            assert!(g.check(&bad, Access::Read).is_err(), "{}", bad);
        }
        assert!(g.check(&format!("@payload/{}", hash), Access::Write).is_err(), "never writable");
        assert!(g.list("@payload").is_err(), "never listable");
        // With no payload store configured, the scheme names nothing.
        let (_t2, mut g2) = setup();
        g2.zones.payload = None;
        assert!(g2.check(&format!("@payload/{}", hash), Access::Read).is_err());
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
    fn directories_list_without_leaking_hidden_or_forbidden_entries() {
        let (t, mut g) = setup();
        fs::write(t.path().join("local/missions/m1/plan.md"), "p").unwrap();
        let names = |l: Vec<DirEntry>| l.into_iter().map(|e| e.name).collect::<Vec<_>>();
        // events.jsonl never appears; the councils directory does, but its .barrier is hidden inside.
        assert_eq!(names(g.list("missions/m1").unwrap()), vec!["councils", "notes.md", "plan.md"]);
        assert!(g.list("missions/m1/councils").unwrap().is_empty());
        // .git is hidden even in a worktree that has one.
        assert_eq!(names(g.list("@worktree").unwrap()), vec!["plain.txt", "src"]);
        assert!(matches!(g.list("missions/m1/councils/.barrier"), Err(Denied::NeverReadable(_))));
        for d in ["operator", "Operator", "local", "run", "proposals", "memory/operator"] {
            assert!(matches!(g.list(d), Err(Denied::NeverReadable(_))), "{} must not be listable", d);
        }
        assert!(matches!(g.list("memory/agents/red-team"), Err(Denied::NeverReadable(_))), "another role's memory directory");
        assert_eq!(g.list("@project/app/src").unwrap()[0].name, "main.rs");
        g.read_globs = vec!["specs/**".into()];
        assert!(matches!(g.list("missions/m1"), Err(Denied::NotInScope)));
        assert!(g.list("@worktree/src/lib.rs").is_err(), "a file is not a directory");
    }

    #[test]
    fn atomic_writes_create_parents_and_replace_files_safely() {
        let (t, g) = setup();
        assert!(g.write_atomic("missions/m2/deep/notes.md", b"hello").unwrap(), "created");
        assert_eq!(fs::read_to_string(t.path().join("local/missions/m2/deep/notes.md")).unwrap(), "hello");
        assert!(!g.write_atomic("missions/m2/deep/notes.md", b"replaced").unwrap(), "replaced");
        assert_eq!(g.read_file("missions/m2/deep/notes.md", 100).unwrap(), (b"replaced".to_vec(), false));
        let leftovers: Vec<_> = fs::read_dir(t.path().join("local/missions/m2/deep")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert_eq!(leftovers, vec!["notes.md"], "no temp files remain");
        // The deny lists and scopes still apply to writes.
        assert!(matches!(g.write_atomic("missions/m1/events.jsonl", b"x"), Err(Denied::NeverReadable(_))));
        assert!(matches!(g.write_atomic("@project/app/src/main.rs", b"x"), Err(Denied::NeverWritable(_))));
        assert!(matches!(g.write_atomic("GOVERNANCE.md", b"x"), Err(Denied::NeverWritable(_))));
        assert_eq!(g.write_atomic("missions/../x", b"x").unwrap_err(), Denied::Escapes);
        // A symlinked parent is refused rather than followed.
        symlink(t.path().join("wt"), t.path().join("local/missions/m3")).unwrap();
        assert_eq!(g.write_atomic("missions/m3/x.md", b"x").unwrap_err(), Denied::Symlink);
        assert!(!t.path().join("wt/x.md").exists());
    }

    #[test]
    fn reads_are_capped_and_report_truncation() {
        let (t, g) = setup();
        fs::write(t.path().join("local/missions/m1/big.md"), "0123456789").unwrap();
        assert_eq!(g.read_file("missions/m1/big.md", 4).unwrap(), (b"0123".to_vec(), true));
        assert_eq!(g.read_file("missions/m1/big.md", 10).unwrap(), (b"0123456789".to_vec(), false));
    }

    #[test]
    fn project_root_is_read_only() {
        let (_t, g) = setup();
        assert!(g.check("@project/app/src/main.rs", Access::Read).unwrap().file.is_some());
        assert!(matches!(g.check("@project/app/src/main.rs", Access::Write), Err(Denied::NeverWritable(_))));
        assert_eq!(g.check("@project/app/.git/config", Access::Read).unwrap_err(), Denied::GitSegment);
    }

    // specs/024 AC-9: a bundle's knowledge is readable, read-only; every other directory of it is denied.
    #[test]
    fn a_bundle_is_reachable_only_as_read_only_knowledge() {
        let (t, g) = setup();
        let k = g.check("@bundle/acme/a/knowledge/runbooks/deploy.md", Access::Read).unwrap();
        assert!(k.file.is_some());
        let names = |v: Vec<DirEntry>| v.into_iter().map(|e| e.name).collect::<Vec<_>>();
        assert_eq!(names(g.list("@bundle/acme/a/knowledge").unwrap()), vec!["runbooks"]);
        assert_eq!(names(g.list("@bundle/acme/a/knowledge/runbooks").unwrap()), vec!["deploy.md"]);
        for p in ["behavior/goals.md", "skills/s/SKILL.md", "agents/x.md", "vectors/index.json", ".delphi-bundle/files.json", "bundle.toml"] {
            assert!(g.check(&format!("@bundle/acme/a/{}", p), Access::Read).is_err(), "{} must be denied", p);
        }
        assert!(g.list("@bundle/acme/a").is_err() && g.list("@bundle/acme").is_err() && g.list("@bundle").is_err(), "no listing above knowledge/");
        assert!(g.check("@bundle/acme/other/knowledge/x.md", Access::Read).is_err(), "a bundle that is not added");
        assert!(g.check("@bundle/acme/a/knowledge/../behavior/goals.md", Access::Read).is_err(), "no traversal out of knowledge/");
        assert!(matches!(g.check("@bundle/acme/a/knowledge/runbooks/deploy.md", Access::Write), Err(Denied::NeverWritable(_))));
        assert!(matches!(g.write_atomic("@bundle/acme/a/knowledge/new.md", b"x"), Err(Denied::NeverWritable(_))));
        // a symlink planted inside knowledge/ is never followed
        std::os::unix::fs::symlink("../behavior/goals.md", t.path().join("cache/knowledge/link.md")).unwrap();
        assert!(g.check("@bundle/acme/a/knowledge/link.md", Access::Read).is_err());
        // the lock is daemon state under local/, which agents never read
        assert!(matches!(g.check("local/bundles.lock", Access::Read), Err(Denied::NeverReadable(_))));
    }

    // X10 gap: `local/bundles.lock` holds the install-owned trust fields (the store-endorsed level, the trusted-source
    // grant). No agent may read or write it by any spelling, whatever its globs, and a write never creates it.
    #[test]
    fn bundles_lock_is_neither_readable_nor_writable_by_any_spelling() {
        let (t, g) = setup();
        for p in ["local/bundles.lock", "Local/Bundles.Lock", "local//bundles.lock", "./local/bundles.lock", "local/bundles.lock.lck", "local/./bundles.lock"] {
            assert!(g.check(p, Access::Read).is_err(), "{p} must not be readable");
            assert!(matches!(g.check(p, Access::Write), Err(Denied::NeverReadable(_)) | Err(Denied::NeverWritable(_))), "{p} must not be writable");
            assert!(g.write_atomic(p, b"[[bundle]]\ntrust = \"trusted-source\"\n").is_err(), "{p}");
        }
        for p in ["local/x/../bundles.lock", "@worktree/../local/bundles.lock"] {
            assert!(g.check(p, Access::Write).is_err(), "{p}");
        }
        assert!(never_writable("local/bundles.lock").is_some() && never_readable("local/bundles.lock", "builder").is_some());
        assert!(!t.path().join("local/bundles.lock").exists(), "a refused write creates nothing");
    }

    // specs/025 AC-5: a skill's references/ is readable text; the rest of the skill (SKILL.md, scripts, templates) is not.
    #[test]
    fn a_skills_references_are_readable_and_nothing_else_of_the_skill() {
        let (t, g) = setup();
        for d in ["cache/skills/s/references/deep", "cache/skills/s/scripts", "cache/skills/s/templates"] {
            fs::create_dir_all(t.path().join(d)).unwrap();
        }
        fs::write(t.path().join("cache/skills/s/references/guide.md"), "guide").unwrap();
        fs::write(t.path().join("cache/skills/s/references/deep/more.md"), "more").unwrap();
        fs::write(t.path().join("cache/skills/s/scripts/run.sh"), "rm -rf /").unwrap();
        fs::write(t.path().join("cache/skills/s/templates/t.md"), "t").unwrap();
        assert!(g.check("@bundle/acme/a/skills/s/references/guide.md", Access::Read).unwrap().file.is_some());
        assert!(g.check("@bundle/acme/a/skills/s/references/deep/more.md", Access::Read).unwrap().file.is_some());
        assert_eq!(g.list("@bundle/acme/a/skills/s/references").unwrap().len(), 2);
        for p in ["skills/s/SKILL.md", "skills/s/scripts/run.sh", "skills/s/templates/t.md", "skills/s", "skills", "skills/s/references/../SKILL.md", "skills/s/references/../scripts/run.sh", "skills//references/guide.md", "skills/../behavior/goals.md", "SKILLS/S/Scripts/run.sh"] {
            assert!(g.check(&format!("@bundle/acme/a/{}", p), Access::Read).is_err(), "{} must be denied", p);
        }
        assert!(g.list("@bundle/acme/a/skills").is_err() && g.list("@bundle/acme/a/skills/s").is_err());
        assert!(g.check("@bundle/acme/other/skills/s/references/guide.md", Access::Read).is_err(), "a bundle that is not added");
        assert!(matches!(g.check("@bundle/acme/a/skills/s/references/guide.md", Access::Write), Err(Denied::NeverWritable(_))));
        // a symlink planted in references/ is never followed, and neither is a linked references directory
        symlink("../SKILL.md", t.path().join("cache/skills/s/references/link.md")).unwrap();
        assert!(g.check("@bundle/acme/a/skills/s/references/link.md", Access::Read).is_err());
        fs::create_dir_all(t.path().join("cache/skills/t")).unwrap();
        symlink("../../behavior", t.path().join("cache/skills/t/references")).unwrap();
        assert!(g.check("@bundle/acme/a/skills/t/references/goals.md", Access::Read).is_err());
    }

    // specs/039: the central error log lives at `$DELPHI_HOME/logs/`, outside every zone, and no agent tool may reach it.
    #[test]
    fn the_error_log_is_unreachable_by_any_agent_tool() {
        let (t, mut g) = setup();
        let home = t.path();
        fs::create_dir_all(home.join("logs")).unwrap();
        fs::write(home.join("logs/delphi.log.jsonl"), "{\"slug\":\"x\"}\n").unwrap();
        // Even a copy placed in the local zone is denied (the `local/` rule).
        fs::create_dir_all(home.join("local/logs")).unwrap();
        fs::write(home.join("local/logs/delphi.log.jsonl"), "x").unwrap();
        symlink(home.join("logs"), home.join("wt/loglink")).unwrap();
        symlink(home.join("logs/delphi.log.jsonl"), home.join("wt/filelink")).unwrap();
        g.read_globs = vec!["*".into(), "**".into()];
        g.write_globs = vec!["**".into(), "@worktree/**".into(), "missions/**".into()];
        for spelling in [
            "logs/delphi.log.jsonl", "LOGS/delphi.log.jsonl", "Logs/Delphi.Log.Jsonl", "./logs/delphi.log.jsonl", "logs//delphi.log.jsonl", "logs/./delphi.log.jsonl",
            "logs/delphi.log.jsonl.1", "logs/delphi.log.jsonl.lock", "logs", "logs/", "local/logs/delphi.log.jsonl",
            "../logs/delphi.log.jsonl", "@worktree/../logs/delphi.log.jsonl", "@project/app/../../logs/delphi.log.jsonl", "/tmp/anything/logs/delphi.log.jsonl", "~/.delphi/logs/delphi.log.jsonl",
            "@worktree/loglink/delphi.log.jsonl", "@worktree/filelink", "@worktree/loglink",
        ] {
            assert!(g.check(spelling, Access::Read).is_err(), "`{}` must not be readable", spelling);
            assert!(g.read_file(spelling, 100).is_err(), "`{}` must not be readable", spelling);
            assert!(g.write_atomic(spelling, b"x").is_err(), "`{}` must not be writable", spelling);
        }
        assert!(matches!(g.list("logs"), Err(Denied::NeverReadable(_))));
        assert!(matches!(g.list("LOGS"), Err(Denied::NeverReadable(_))));
        assert!(g.list("@worktree/loglink").is_err());
        assert_eq!(fs::read_to_string(home.join("logs/delphi.log.jsonl")).unwrap(), "{\"slug\":\"x\"}\n", "nothing was written");
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
