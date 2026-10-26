//! The store clone, as pure rules (spec 011 §3.3.5, §3.5; spec 012 §7): where it lives, which URLs it may be
//! fetched from, which tree shapes are refused, what `.gitattributes` may say, and the size caps. The code
//! that runs `git` and touches the disk is `delphi-core::xenia_store`; everything a hostile repository can
//! put in front of it is decided here, over bytes and text, so it is testable without git.
//!
//! The posture: the daemon keeps a **bare** clone and reads objects with `git cat-file`. Nothing is ever
//! checked out, so hooks (`post-checkout`), filters, `.gitattributes` conversions, symlink creation and
//! submodule handling have nothing to run on. The checks below are the second line: a tree that carries any
//! of those things is refused outright instead of being trusted to be inert.

use crate::layout::{Kind, RecordPath};
use crate::sticky::Tracking;
use crate::{is_slug, Error, Result};
use std::path::{Path, PathBuf};

/// Where one store lives. The clone and the state are siblings, never nested: the state is install-owned and
/// must not sit inside a tree an attacker fills (spec 011 §3.5, T24).
///
/// ```text
/// $DELPHI_HOME/xenia/<org>/<store>/clone/   bare repository, 0700, objects only
///                                  state/   sticky.json (and X3's pins.toml), 0700
///                                  empty-hooks/  an empty directory used as core.hooksPath
///                                  empty-home/   an empty directory used as HOME
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePaths {
    pub root: PathBuf,
    pub clone: PathBuf,
    pub state: PathBuf,
    pub sticky: PathBuf,
    pub empty_hooks: PathBuf,
    /// An empty directory used as `HOME` and `XDG_CONFIG_HOME`, so no user configuration is found.
    pub empty_home: PathBuf,
}

impl StorePaths {
    /// `org` and `store` are the pinned values, validated as slugs so neither can carry a separator or `..`.
    pub fn new(home: &Path, org: &str, store: &str) -> Result<StorePaths> {
        if !is_slug(org, 32) {
            return Err(Error::BadField("org is not a slug of 1-32 characters"));
        }
        if !is_slug(store, 32) {
            return Err(Error::BadField("store is not a slug of 1-32 characters"));
        }
        let root = home.join("xenia").join(org).join(store);
        Ok(StorePaths { clone: root.join("clone"), state: root.join("state"), sticky: root.join("state").join("sticky.json"), empty_hooks: root.join("empty-hooks"), empty_home: root.join("empty-home"), root })
    }

    /// Spec 011 §3.3.5 and test 24: the clone is under `$DELPHI_HOME/xenia/` and under none of the workspace
    /// zones (`zone_roots`, for example `$DELPHI_HOME/team` and each workspace). Compared lexically on
    /// absolute paths; the caller passes canonical ones.
    pub fn is_outside_zones(&self, home: &Path, zone_roots: &[PathBuf]) -> bool {
        self.clone.starts_with(home.join("xenia")) && !zone_roots.iter().any(|z| self.clone.starts_with(z) || z.starts_with(&self.clone))
    }
}

/// The only transports a store URL may use. `git://` is unauthenticated and unencrypted and `http://` is
/// cleartext, so neither is accepted; `ext::`, `fd::` and other remote helpers run programs and are never.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Https,
    Ssh,
    /// A local path or `file://` URL. Off unless the caller asks for it (tests, and a store on a shared drive).
    File,
}

impl Scheme {
    /// The name git's `protocol.<name>.allow` uses.
    pub fn git_name(self) -> &'static str {
        match self {
            Scheme::Https => "https",
            Scheme::Ssh => "ssh",
            Scheme::File => "file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreUrl {
    pub scheme: Scheme,
    /// What is passed to `git fetch`, always with an explicit scheme (a bare path becomes `file://`).
    pub url: String,
}

impl StoreUrl {
    pub fn parse(s: &str, allow_file: bool) -> Result<StoreUrl> {
        if s.is_empty() || s.len() > 2048 {
            return Err(Error::BadField("store url is empty or too long"));
        }
        // Control characters, spaces and backslashes have no place in a URL and are how argument and
        // protocol-helper tricks get through.
        if s.bytes().any(|b| b <= b' ' || b == 0x7f || b == b'\\') || !s.is_ascii() {
            return Err(Error::BadField("store url has control, space, backslash or non-ASCII characters"));
        }
        if s.starts_with('-') {
            return Err(Error::BadField("store url looks like an option"));
        }
        if let Some(rest) = s.strip_prefix("https://") {
            return Self::checked_authority(rest, Scheme::Https, s);
        }
        if let Some(rest) = s.strip_prefix("ssh://") {
            return Self::checked_authority(rest, Scheme::Ssh, s);
        }
        if s.contains("::") {
            return Err(Error::BadField("store url uses a remote helper"));
        }
        if let Some(path) = s.strip_prefix("file://") {
            if !allow_file || !path.starts_with('/') || path.contains("/../") || path.ends_with("/..") {
                return Err(Error::BadField("file store urls are off or not absolute"));
            }
            return Ok(StoreUrl { scheme: Scheme::File, url: s.to_string() });
        }
        if s.starts_with('/') {
            if !allow_file || s.contains("/../") || s.ends_with("/..") {
                return Err(Error::BadField("local store paths are off or not clean"));
            }
            return Ok(StoreUrl { scheme: Scheme::File, url: format!("file://{s}") });
        }
        Err(Error::BadField("store url scheme is not https, ssh or an allowed local path"))
    }

    fn checked_authority(rest: &str, scheme: Scheme, whole: &str) -> Result<StoreUrl> {
        let host = rest.split('/').next().unwrap_or("");
        // `user:password@` would put a credential into argv and logs; `ssh://git@host/...` is fine.
        let userinfo = host.rsplit_once('@').map(|(u, _)| u).unwrap_or("");
        if host.is_empty() || host.starts_with('-') || host.starts_with('@') || userinfo.contains(':') || !rest.contains('/') {
            return Err(Error::BadField("store url has no host, an option-like host, or embedded credentials"));
        }
        Ok(StoreUrl { scheme, url: whole.to_string() })
    }
}

/// Size and shape caps. The spec gives none for objects and trees (its record limits are 16 KiB and 1 KiB,
/// spec 012 §6), so these are X4a's choices, generous for a store of small text files and far below
/// anything that could exhaust a machine. Configurable so tests can use small ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caps {
    /// Any single git object (blob, tree or commit) that is new in a fetch.
    pub max_object_bytes: u64,
    /// New objects in one fetch.
    pub max_new_objects: u64,
    /// Sum of the sizes of new objects.
    pub max_total_bytes: u64,
    /// Entries (files) in the tip tree.
    pub max_tree_entries: usize,
    /// New commits in one fetch (bounds the X4b history walk).
    pub max_new_commits: u64,
    /// Path length in bytes and depth in components.
    pub max_path_bytes: usize,
    pub max_path_depth: usize,
    /// Wall-clock for one git network call, in seconds.
    pub fetch_timeout_secs: u64,
}

impl Default for Caps {
    fn default() -> Caps {
        Caps {
            max_object_bytes: 512 * 1024,
            max_new_objects: 100_000,
            max_total_bytes: 64 * 1024 * 1024,
            max_tree_entries: 20_000,
            max_new_commits: 20_000,
            max_path_bytes: 255,
            max_path_depth: 6,
            fetch_timeout_secs: 120,
        }
    }
}

/// One line of `git ls-tree -r -z --long`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: u32,
    pub otype: String,
    pub sha: String,
    /// `None` for a gitlink (`-`).
    pub size: Option<u64>,
    pub path: Vec<u8>,
}

/// Parse the NUL-separated output of `git ls-tree -r -z --long <commit>`. Strict: a line that does not have
/// exactly the shape is an error, so a surprising output can never be half-read.
pub fn parse_ls_tree(out: &[u8], caps: &Caps) -> Result<Vec<TreeEntry>> {
    let mut entries = Vec::new();
    for rec in out.split(|b| *b == 0) {
        if rec.is_empty() {
            continue;
        }
        if entries.len() >= caps.max_tree_entries {
            return Err(Error::Oversize("tree has too many entries"));
        }
        let tab = rec.iter().position(|b| *b == b'\t').ok_or(Error::BadPath("ls-tree line has no tab"))?;
        let (meta, path) = (&rec[..tab], &rec[tab + 1..]);
        let meta = std::str::from_utf8(meta).map_err(|_| Error::BadPath("ls-tree metadata is not UTF-8"))?;
        let mut it = meta.split(' ').filter(|p| !p.is_empty());
        let (mode, otype, sha, size) = (it.next(), it.next(), it.next(), it.next());
        let (Some(mode), Some(otype), Some(sha), Some(size), None) = (mode, otype, sha, size, it.next()) else {
            return Err(Error::BadPath("ls-tree line has the wrong number of fields"));
        };
        let mode = u32::from_str_radix(mode, 8).map_err(|_| Error::BadPath("ls-tree mode"))?;
        let size = if size == "-" { None } else { Some(size.parse::<u64>().map_err(|_| Error::BadPath("ls-tree size"))?) };
        if !crate::sticky::is_commit_id(sha) {
            return Err(Error::BadPath("ls-tree object id"));
        }
        entries.push(TreeEntry { mode, otype: otype.to_string(), sha: sha.to_string(), size, path: path.to_vec() });
    }
    Ok(entries)
}

/// Why a tree entry is refused. The slug is safe to show; the path is shown lossily and bounded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hostile {
    pub path: String,
    pub reason: &'static str,
}

/// Check every entry of the tip tree. The result is empty only for a tree a Phase 1 store can legitimately
/// have: regular files (`100644`; `100755` is refused too, since nothing in a store is executable), no
/// symlinks, no gitlinks, no `.git*` names except the root `.gitattributes`, sane names and sizes.
pub fn check_tree(entries: &[TreeEntry], caps: &Caps) -> Vec<Hostile> {
    let mut bad = Vec::new();
    let mut flag = |e: &TreeEntry, reason: &'static str| bad.push(Hostile { path: show_path(&e.path), reason });
    for e in entries {
        match e.mode {
            0o100644 => {}
            0o120000 => {
                flag(e, "symlink");
                continue;
            }
            0o160000 => {
                flag(e, "submodule");
                continue;
            }
            0o100755 => {
                flag(e, "executable-file");
                continue;
            }
            _ => {
                flag(e, "unusual-mode");
                continue;
            }
        }
        if e.otype != "blob" {
            flag(e, "not-a-blob");
            continue;
        }
        if e.size.is_none_or(|s| s > caps.max_object_bytes) {
            flag(e, "oversize-file");
            continue;
        }
        if let Err(reason) = check_path(&e.path, caps) {
            flag(e, reason);
        }
    }
    bad
}

/// A path from a hostile tree. Rejects everything a checkout on any platform could turn into something else
/// (`.git` in any case, `..`, backslashes, drive or stream colons, control bytes, 8.3 short names) even
/// though we never check out: one strict rule is easier to trust than a list of platforms.
pub fn check_path(path: &[u8], caps: &Caps) -> std::result::Result<(), &'static str> {
    if path.is_empty() || path.len() > caps.max_path_bytes {
        return Err("bad-path-length");
    }
    let Ok(s) = std::str::from_utf8(path) else {
        return Err("non-utf8-path");
    };
    if s.starts_with('/') || s.ends_with('/') {
        return Err("bad-path");
    }
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() > caps.max_path_depth {
        return Err("path-too-deep");
    }
    for (i, c) in parts.iter().enumerate() {
        if c.is_empty() || *c == "." || *c == ".." {
            return Err("dot-component");
        }
        if c.bytes().any(|b| b < 0x20 || b == 0x7f || b == b'\\' || b == b':') {
            return Err("forbidden-character");
        }
        // Trailing dots and spaces are stripped by Windows, so `.git.` is `.git` there.
        let lower = c.to_ascii_lowercase();
        let trimmed = lower.trim_end_matches(['.', ' ']);
        if trimmed.starts_with(".git") && !(i == 0 && parts.len() == 1 && *c == ".gitattributes") {
            return Err("git-metadata-name");
        }
        if trimmed.starts_with("git~") {
            return Err("git-short-name");
        }
    }
    Ok(())
}

fn show_path(p: &[u8]) -> String {
    let s: String = String::from_utf8_lossy(&p[..p.len().min(120)]).chars().map(|c| if c.is_control() { '?' } else { c }).collect();
    s
}

/// Why a store's `.gitattributes` is refused (spec 012 §7, test 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrsRefusal {
    Missing,
    NotUtf8OrTooLarge,
    /// No `* -text` line: checkouts could convert line endings and break raw-byte hashes.
    NoDefaultNoText,
    /// A line that sets an attribute that converts, runs a program, or defines a macro.
    ForbiddenAttribute,
    /// A line that is not a comment, blank, or `<pattern> <attributes>` in the narrow allowed form.
    Malformed,
}

impl AttrsRefusal {
    pub fn reason(self) -> &'static str {
        match self {
            AttrsRefusal::Missing => "gitattributes-missing",
            AttrsRefusal::NotUtf8OrTooLarge => "gitattributes-unreadable",
            AttrsRefusal::NoDefaultNoText => "gitattributes-no-default-no-text",
            AttrsRefusal::ForbiddenAttribute => "gitattributes-forbidden-attribute",
            AttrsRefusal::Malformed => "gitattributes-malformed",
        }
    }
}

/// The root `.gitattributes` must keep every checkout byte-exact (`* -text`) and must not do anything else
/// that matters: no `filter=` (runs a program), no `text`/`eol`/`crlf`/`ident`/`working-tree-encoding`/
/// `export-subst` (rewrites bytes), no `diff=`/`merge=` drivers, no `[attr]` macros (which can hide any of
/// those behind a name). Only `-text`, `binary`, `-diff`, `-merge` and `linguist-*` are allowed after a
/// pattern, and a later line may therefore never turn conversion back on.
pub fn check_gitattributes(text: Option<&[u8]>) -> std::result::Result<(), AttrsRefusal> {
    let Some(bytes) = text else {
        return Err(AttrsRefusal::Missing);
    };
    if bytes.len() > 4096 {
        return Err(AttrsRefusal::NotUtf8OrTooLarge);
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(AttrsRefusal::NotUtf8OrTooLarge);
    };
    let mut default_no_text = false;
    for line in text.split('\n') {
        // CR is refused so a line cannot look different to git and to us.
        if line.contains('\r') || line.bytes().any(|b| b < 0x20 && b != b'\t') {
            return Err(AttrsRefusal::Malformed);
        }
        let line = line.trim_matches([' ', '\t']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') || line.starts_with('"') {
            return Err(AttrsRefusal::ForbiddenAttribute);
        }
        let mut toks = line.split([' ', '\t']).filter(|t| !t.is_empty());
        let pattern = toks.next().ok_or(AttrsRefusal::Malformed)?;
        let attrs: Vec<&str> = toks.collect();
        if attrs.is_empty() {
            return Err(AttrsRefusal::Malformed);
        }
        for a in &attrs {
            let allowed = matches!(*a, "-text" | "binary" | "-diff" | "-merge") || a.starts_with("linguist-");
            if !allowed {
                return Err(AttrsRefusal::ForbiddenAttribute);
            }
        }
        if pattern == "*" && attrs.contains(&"-text") {
            default_no_text = true;
        }
    }
    if default_no_text { Ok(()) } else { Err(AttrsRefusal::NoDefaultNoText) }
}

/// What a path under `records/` or `roster/` is, for sticky tracking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathClass {
    Record(RecordPath),
    Sidecar,
    EpochFile(u64),
    EpochSigs,
    Freshness,
    FreshnessSigs,
}

/// `None` for anything the layout does not know: it is quarantined, never tracked.
pub fn classify_path(path: &str) -> Option<PathClass> {
    use crate::layout::{FRESHNESS, FRESHNESS_SIGS};
    if path == FRESHNESS {
        return Some(PathClass::Freshness);
    }
    if path == FRESHNESS_SIGS {
        return Some(PathClass::FreshnessSigs);
    }
    if let Some(name) = path.strip_prefix("roster/epochs/") {
        if let Some(n) = name.strip_suffix(".sigs.json") {
            return epoch_number(n).map(|_| PathClass::EpochSigs);
        }
        return name.strip_suffix(".json").and_then(epoch_number).map(PathClass::EpochFile);
    }
    if let Some(rest) = path.strip_prefix("records/") {
        if let Some((kind, file)) = rest.split_once('/') {
            if Kind::parse(kind).is_some() && file.ends_with(".sig.json") {
                let id = file.trim_end_matches(".sig.json");
                if crate::ulid::is_valid(id) {
                    return Some(PathClass::Sidecar);
                }
            }
        }
        return RecordPath::parse(path).ok().map(PathClass::Record);
    }
    None
}

fn epoch_number(s: &str) -> Option<u64> {
    // Canonical decimal only: `01.json` and `1.json` must not both exist as "epoch 1".
    if s.is_empty() || s.len() > 9 || (s.len() > 1 && s.starts_with('0')) || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok().filter(|n| *n >= 1)
}

/// The sticky ordering for a path. `rev_of` reads a record's `rev`: it is X2's verified reader, never a
/// parse done here, because the signature must be checked before any `rev` is read from a file.
pub fn tracking_for(class: &PathClass, rev_of: impl FnOnce() -> std::result::Result<u64, &'static str>) -> std::result::Result<Tracking, &'static str> {
    match class {
        PathClass::Record(_) => {
            let rev = rev_of()?;
            if rev == 0 { Err("rev-is-zero") } else { Ok(Tracking::Rev(rev)) }
        }
        PathClass::EpochFile(_) => Ok(Tracking::Immutable),
        PathClass::Sidecar | PathClass::EpochSigs | PathClass::Freshness | PathClass::FreshnessSigs => Ok(Tracking::Present),
    }
}

/// Is this tip path one that sticky state tracks (spec 011 §3.5: every path under `records/` and `roster/`)?
pub fn is_tracked_area(path: &str) -> bool {
    path.starts_with("records/") || path.starts_with("roster/")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RA";
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn entry(mode: u32, otype: &str, size: Option<u64>, path: &str) -> TreeEntry {
        TreeEntry { mode, otype: otype.into(), sha: SHA.into(), size, path: path.as_bytes().to_vec() }
    }

    #[test]
    fn the_clone_and_the_state_are_siblings_under_xenia() {
        let home = Path::new("/h");
        let p = StorePaths::new(home, "acme", "main").unwrap();
        assert_eq!(p.clone, Path::new("/h/xenia/acme/main/clone"));
        assert_eq!(p.sticky, Path::new("/h/xenia/acme/main/state/sticky.json"));
        assert!(!p.state.starts_with(&p.clone) && !p.clone.starts_with(&p.state), "state must not sit inside the clone");
        assert!(p.is_outside_zones(home, &[home.join("team"), PathBuf::from("/work/space")]));
        assert!(!p.is_outside_zones(home, &[home.join("xenia")]));
        assert!(!p.is_outside_zones(Path::new("/other"), &[]));
    }

    #[test]
    fn org_and_store_cannot_escape_the_directory() {
        for bad in ["", "..", "a/b", "A", "a b", "a\n", "../x", &"a".repeat(33)] {
            assert!(StorePaths::new(Path::new("/h"), bad, "main").is_err(), "{bad:?}");
            assert!(StorePaths::new(Path::new("/h"), "acme", bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn store_urls_accept_https_ssh_and_opt_in_local_paths_only() {
        assert_eq!(StoreUrl::parse("https://git.example.com/acme/store.git", false).unwrap().scheme, Scheme::Https);
        assert_eq!(StoreUrl::parse("ssh://git@git.example.com/acme/store.git", false).unwrap().scheme, Scheme::Ssh);
        let local = StoreUrl::parse("/tmp/remote.git", true).unwrap();
        assert_eq!((local.scheme, local.url.as_str()), (Scheme::File, "file:///tmp/remote.git"));
        assert!(StoreUrl::parse("file:///tmp/remote.git", true).is_ok());
        for bad in [
            "ext::sh -c touch% /tmp/x",
            "ext::sh",
            "fd::3",
            "git://example.com/x.git",
            "http://example.com/x.git",
            "-oProxyCommand=touch /tmp/x",
            "--upload-pack=touch /tmp/x",
            "https://-oProxyCommand=x/y",
            "https://user:secret@example.com/x.git",
            "https://example.com",
            "https:///x",
            "https://example.com/a b",
            "https://example.com/\n",
            "ssh://-oProxyCommand=x/y",
            "git@example.com:acme/store.git",
            "transport::https://example.com/x",
            "relative/path",
            "",
        ] {
            assert!(StoreUrl::parse(bad, true).is_err(), "{bad:?}");
        }
        assert!(StoreUrl::parse("/tmp/remote.git", false).is_err());
        assert!(StoreUrl::parse("file:///tmp/remote.git", false).is_err());
        assert!(StoreUrl::parse("/tmp/../etc/x", true).is_err());
    }

    #[test]
    fn ls_tree_output_is_parsed_strictly() {
        let out = format!("100644 blob {SHA}     123\trecords/memory/{ID}.md\0160000 commit {SHA}       -\tsub\0");
        let e = parse_ls_tree(out.as_bytes(), &Caps::default()).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].mode, e[0].size), (0o100644, Some(123)));
        assert_eq!((e[1].mode, e[1].size), (0o160000, None));
        for bad in [
            format!("100644 blob {SHA} 1 records/x\0"),
            format!("100644 blob {SHA}\tx\0"),
            "100644 blob zz 5\tx\0".to_string(),
            format!("10064x blob {SHA} 5\tx\0"),
            format!("100644 blob {SHA} 5 6\tx\0"),
        ] {
            assert!(parse_ls_tree(bad.as_bytes(), &Caps::default()).is_err(), "{bad:?}");
        }
        let small = Caps { max_tree_entries: 1, ..Caps::default() };
        assert!(parse_ls_tree(out.as_bytes(), &small).is_err());
    }

    #[test]
    fn trees_refuse_symlinks_submodules_executables_and_git_names() {
        let caps = Caps::default();
        let ok = vec![entry(0o100644, "blob", Some(10), ".gitattributes"), entry(0o100644, "blob", Some(10), "org.toml"), entry(0o100644, "blob", Some(10), &format!("records/memory/{ID}.md"))];
        assert!(check_tree(&ok, &caps).is_empty());
        let reason = |e: TreeEntry| check_tree(&[e], &caps).first().map(|h| h.reason);
        assert_eq!(reason(entry(0o120000, "blob", Some(5), "records/link")), Some("symlink"));
        assert_eq!(reason(entry(0o160000, "commit", None, "vendor/sub")), Some("submodule"));
        assert_eq!(reason(entry(0o100755, "blob", Some(5), "run.sh")), Some("executable-file"));
        assert_eq!(reason(entry(0o100644, "blob", Some(caps.max_object_bytes + 1), "indexes/big")), Some("oversize-file"));
        assert_eq!(reason(entry(0o100644, "blob", Some(5), ".gitmodules")), Some("git-metadata-name"));
        assert_eq!(reason(entry(0o100644, "blob", Some(5), "records/.gitattributes")), Some("git-metadata-name"));
        assert_eq!(reason(entry(0o100644, "blob", Some(5), ".GITMODULES")), Some("git-metadata-name"));
        assert_eq!(reason(entry(0o100644, "blob", Some(5), ".git./config")), Some("git-metadata-name"));
        assert_eq!(reason(entry(0o100644, "blob", Some(5), "GIT~1/x")), Some("git-short-name"));
        assert_eq!(reason(entry(0o100644, "blob", Some(5), "a/../b")), Some("dot-component"));
        assert_eq!(reason(entry(0o100644, "blob", Some(5), "a\\b")), Some("forbidden-character"));
        assert_eq!(reason(entry(0o100644, "blob", Some(5), "a:stream")), Some("forbidden-character"));
        assert_eq!(reason(entry(0o100644, "blob", Some(5), "a\nb")), Some("forbidden-character"));
        assert_eq!(reason(entry(0o100644, "blob", Some(5), "a/b/c/d/e/f/g")), Some("path-too-deep"));
        let mut raw = entry(0o100644, "blob", Some(5), "x");
        raw.path = vec![b'a', 0xff];
        assert_eq!(reason(raw), Some("non-utf8-path"));
    }

    #[test]
    fn gitattributes_must_say_star_minus_text_and_nothing_that_converts_or_runs() {
        assert_eq!(check_gitattributes(Some(b"* -text\n")), Ok(()));
        assert_eq!(check_gitattributes(Some(b"# keep bytes exact\n\n* -text\n*.png binary\n")), Ok(()));
        assert_eq!(check_gitattributes(Some(b"*\t-text")), Ok(()));
        assert_eq!(check_gitattributes(None), Err(AttrsRefusal::Missing));
        assert_eq!(check_gitattributes(Some(b"")), Err(AttrsRefusal::NoDefaultNoText));
        assert_eq!(check_gitattributes(Some(b"*.md -text\n")), Err(AttrsRefusal::NoDefaultNoText));
        assert_eq!(check_gitattributes(Some(b"* text\n")), Err(AttrsRefusal::ForbiddenAttribute));
        for bad in [
            "* -text\n* filter=evil\n",
            "* -text filter=evil\n",
            "* -text\n*.md text\n",
            "* -text\n*.md text=auto\n",
            "* -text\n*.md eol=crlf\n",
            "* -text\n*.md crlf\n",
            "* -text\n*.md ident\n",
            "* -text\n*.md working-tree-encoding=UTF-16\n",
            "* -text\n*.md export-subst\n",
            "* -text\n*.md diff=evil\n",
            "* -text\n*.md merge=evil\n",
            "* -text\n[attr]x filter=evil\n*.md x\n",
            "* -text\n\"quoted\" -text\n",
            "* -text\n* !text\n",
            "* -text\n*.md -text -filter\n",
        ] {
            assert_eq!(check_gitattributes(Some(bad.as_bytes())), Err(AttrsRefusal::ForbiddenAttribute), "{bad:?}");
        }
        assert_eq!(check_gitattributes(Some(b"* -text\r\n")), Err(AttrsRefusal::Malformed));
        assert_eq!(check_gitattributes(Some(b"* -text\n*.md\n")), Err(AttrsRefusal::Malformed));
        assert_eq!(check_gitattributes(Some(&[b'*', b' ', b'-', b't', 0xff])), Err(AttrsRefusal::NotUtf8OrTooLarge));
        assert_eq!(check_gitattributes(Some(&vec![b'#'; 5000])), Err(AttrsRefusal::NotUtf8OrTooLarge));
    }

    #[test]
    fn paths_classify_to_exactly_the_layout() {
        assert_eq!(classify_path(&format!("records/memory/{ID}.md")), Some(PathClass::Record(RecordPath::parse(&format!("records/memory/{ID}.md")).unwrap())));
        assert_eq!(classify_path(&format!("records/endorsement/{ID}.sig.json")), Some(PathClass::Sidecar));
        assert_eq!(classify_path("roster/epochs/3.json"), Some(PathClass::EpochFile(3)));
        assert_eq!(classify_path("roster/epochs/3.sigs.json"), Some(PathClass::EpochSigs));
        assert_eq!(classify_path("roster/freshness.json"), Some(PathClass::Freshness));
        assert_eq!(classify_path("roster/freshness.sigs.json"), Some(PathClass::FreshnessSigs));
        for bad in ["roster/epochs/03.json", "roster/epochs/0.json", "roster/epochs/x.json", "roster/other.json", "records/skill/x.md", &format!("records/memory/{ID}.txt"), &format!("records/memory/{ID}.sig.json.bak"), "records/memory/nope.sig.json", "org.toml"] {
            assert_eq!(classify_path(bad), None, "{bad}");
        }
    }

    #[test]
    fn tracking_reads_a_rev_only_for_records() {
        let rec = classify_path(&format!("records/memory/{ID}.md")).unwrap();
        assert_eq!(tracking_for(&rec, || Ok(3)), Ok(Tracking::Rev(3)));
        assert_eq!(tracking_for(&rec, || Ok(0)), Err("rev-is-zero"));
        assert_eq!(tracking_for(&rec, || Err("unverified")), Err("unverified"));
        assert_eq!(tracking_for(&PathClass::EpochFile(1), || panic!("no rev read")), Ok(Tracking::Immutable));
        assert_eq!(tracking_for(&PathClass::Freshness, || panic!("no rev read")), Ok(Tracking::Present));
    }
}
