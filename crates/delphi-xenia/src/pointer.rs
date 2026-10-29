//! Pointers (spec 012 §4): where a record's claim came from, shown as text and never fetched in Phase 1.
//!
//! Because nothing is fetched there is no SSRF surface, but a pointer is still attacker text that a human
//! will copy into a browser or `git clone`, so the rules are about what that text can do to the person:
//!
//! * the **scheme allowlist** is `git`, `https`, `xenia`; `drive` parses (the schema accepts it) and is then
//!   refused for Phase 1 ([`Pointer::check_publishable`]);
//! * locators are **ASCII only** (no IDN or look-alike hosts), lowercase hosts, no userinfo of any kind
//!   (including `git@`), no query, no fragment (except `git`'s one `#path` separator), no `;`, `=`, `&`,
//!   no `%`-encoded dots, slashes, `@` or controls, no `..` segments;
//! * `file:`, relative and absolute local paths fail simply by not being `https://` or `ssh://` URLs (git),
//!   `https://` URLs (https), `xenia://<kind>/<id>[@rev]` (xenia) or a bare Drive id (drive); `delphi://` is
//!   refused by name (the scheme is `xenia://`), and `xenia://cap/...` is reserved for the unbuilt Plane 2;
//! * well-known credential shapes (GitHub, GitLab, Slack, AWS, JWT) anywhere in a locator are refused.
//!
//! Spec 012 §4 says "URL without userinfo or query tokens". A denylist of token parameter names would
//! always miss one, so any query string is refused instead (stricter than the spec; recorded in the plan).
//! A pin proves integrity, not trust, and no locator makes a target trusted; nothing here grants anything.

use crate::frontmatter::{bad, pick, Bad, Map};
use crate::layout::Kind;
use crate::limits::*;
use crate::{hash, ulid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Git,
    Https,
    Xenia,
    Drive,
}

impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Git => "git",
            Scheme::Https => "https",
            Scheme::Xenia => "xenia",
            Scheme::Drive => "drive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinKind {
    GitCommit,
    GitTree,
    ContentHash,
    DriveRevision,
    None,
}

impl PinKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PinKind::GitCommit => "git-commit",
            PinKind::GitTree => "git-tree",
            PinKind::ContentHash => "content-hash",
            PinKind::DriveRevision => "drive-revision",
            PinKind::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Markdown,
    Plain,
    Pdf,
    Json,
    Png,
    Other,
}

impl ContentType {
    pub fn as_str(self) -> &'static str {
        match self {
            ContentType::Markdown => "text/markdown",
            ContentType::Plain => "text/plain",
            ContentType::Pdf => "application/pdf",
            ContentType::Json => "application/json",
            ContentType::Png => "image/png",
            ContentType::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub kind: PinKind,
    /// Absent exactly when `kind` is `none`.
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pointer {
    pub scheme: Scheme,
    pub locator: String,
    pub pin: Pin,
    pub hash: Option<String>,
    /// `L40-L120`, inclusive, as numbers.
    pub range: Option<(u64, u64)>,
    pub label: String,
    pub content_type: ContentType,
    pub bytes: Option<u64>,
    pub access_hint: Option<String>,
}

const SCHEMES: [(&str, Scheme); 4] = [("git", Scheme::Git), ("https", Scheme::Https), ("xenia", Scheme::Xenia), ("drive", Scheme::Drive)];
const PIN_KINDS: [(&str, PinKind); 5] =
    [("git-commit", PinKind::GitCommit), ("git-tree", PinKind::GitTree), ("content-hash", PinKind::ContentHash), ("drive-revision", PinKind::DriveRevision), ("none", PinKind::None)];
const CONTENT_TYPES: [(&str, ContentType); 6] = [
    ("text/markdown", ContentType::Markdown),
    ("text/plain", ContentType::Plain),
    ("application/pdf", ContentType::Pdf),
    ("application/json", ContentType::Json),
    ("image/png", ContentType::Png),
    ("other", ContentType::Other),
];

impl Pointer {
    /// Structure only (spec 012 §6 step 5): required fields, types, closed enums, the integer and range
    /// spellings. What a locator may say is [`Pointer::check_publishable`] (step 9).
    pub fn from_map(mut m: Map) -> Result<Pointer, Bad> {
        let scheme = pick(&m.req_str("scheme")?, &SCHEMES).ok_or(bad("scheme", "not one of git, https, xenia, drive"))?;
        let locator = m.req_str("locator")?;
        let mut pin_map = m.req_map("pin")?;
        let kind = pick(&pin_map.req_str("kind")?, &PIN_KINDS).ok_or(bad("pin.kind", "unknown pin kind"))?;
        let value = pin_map.opt_str("value")?;
        pin_map.finish().map_err(|_| bad("pin", "unknown key"))?;
        match (kind, &value) {
            (PinKind::None, Some(_)) => return Err(bad("pin.value", "a pin of kind none has no value")),
            (PinKind::None, None) => {}
            (_, None) => return Err(bad("pin.value", "missing")),
            (_, Some(v)) if v.is_empty() || v.chars().count() > MAX_PIN_VALUE_CHARS => return Err(bad("pin.value", "empty or too long")),
            _ => {}
        }
        let hash = m.opt_str("hash")?;
        if let Some(h) = &hash {
            if !hash::is_valid(h) {
                return Err(bad("hash", "not blake3:<64 lowercase hex>"));
            }
        }
        let range = m.opt_str("range")?.map(|r| parse_range(&r)).transpose()?;
        let label = m.req_str("label")?;
        let content_type = pick(&m.req_str("content_type")?, &CONTENT_TYPES).ok_or(bad("content_type", "not in the closed list"))?;
        let bytes = m.opt_int("bytes")?;
        if bytes.is_some_and(|b| b > MAX_POINTER_BYTES) {
            return Err(bad("bytes", "implausibly large"));
        }
        let access_hint = m.opt_str("access_hint")?;
        m.finish().map_err(|_| bad("pointer", "unknown key"))?;
        Ok(Pointer { scheme, locator, pin: Pin { kind, value }, hash, range, label, content_type, bytes, access_hint })
    }

    /// Spec 012 §6 step 9 for Phase 1: the scheme allowlist, the locator rules per scheme, the pin kinds the
    /// scheme allows, and no credentials. Run at publish and again at ingest.
    pub fn check_publishable(&self) -> Result<(), Bad> {
        check_locator_common(&self.locator)?;
        match self.scheme {
            Scheme::Git => {
                check_git_locator(&self.locator)?;
                match (self.pin.kind, self.pin.value.as_deref()) {
                    (PinKind::GitCommit | PinKind::GitTree, Some(v)) if is_git_object_id(v) => {}
                    _ => return Err(bad("pin", "a git pointer needs a git-commit or git-tree pin with a full lowercase object id")),
                }
            }
            Scheme::Https => {
                check_https_locator(&self.locator)?;
                match (self.pin.kind, self.pin.value.as_deref()) {
                    (PinKind::None, None) => {}
                    (PinKind::ContentHash, Some(v)) if hash::is_valid(v) => {}
                    _ => return Err(bad("pin", "an https pointer needs a content-hash pin or none")),
                }
            }
            Scheme::Xenia => {
                check_xenia_locator(&self.locator)?;
                if self.pin.kind != PinKind::None {
                    return Err(bad("pin", "a xenia pointer needs no pin"));
                }
            }
            Scheme::Drive => {
                // The schema accepts `drive`; it is not publishable until Phase 3, so it is refused after the
                // same checks a Phase 3 pointer would face.
                check_drive_id(&self.locator)?;
                match (self.pin.kind, self.pin.value.as_deref()) {
                    (PinKind::DriveRevision, Some(v)) if is_token(v, MAX_PIN_VALUE_CHARS) => {}
                    (PinKind::ContentHash, Some(v)) if hash::is_valid(v) => {}
                    _ => return Err(bad("pin", "a drive pointer needs a drive-revision or content-hash pin")),
                }
                return Err(bad("scheme", "drive pointers are not publishable in Phase 1"));
            }
        }
        if self.range.is_some() && !matches!(self.scheme, Scheme::Git | Scheme::Https) {
            return Err(bad("range", "line ranges apply to git and https text only"));
        }
        if self.range.is_some() && !matches!(self.content_type, ContentType::Markdown | ContentType::Plain | ContentType::Json | ContentType::Other) {
            return Err(bad("range", "line ranges apply to text only"));
        }
        Ok(())
    }
}

fn parse_range(r: &str) -> Result<(u64, u64), Bad> {
    let err = || bad("range", "not L<start>-L<end>");
    let (a, b) = r.strip_prefix('L').and_then(|t| t.split_once("-L")).ok_or_else(err)?;
    let num = |s: &str| -> Result<u64, Bad> {
        let ok = !s.is_empty() && s.len() <= 7 && s.bytes().all(|c| c.is_ascii_digit()) && !s.starts_with('0');
        if ok { s.parse().map_err(|_| err()) } else { Err(err()) }
    };
    let (a, b) = (num(a)?, num(b)?);
    if a > b || b > MAX_RANGE_LINE {
        return Err(bad("range", "empty or out of bounds"));
    }
    Ok((a, b))
}

fn is_git_object_id(v: &str) -> bool {
    matches!(v.len(), 40 | 64) && v.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `[A-Za-z0-9._-]{1,max}`, not starting with a dot: file ids and revision ids.
fn is_token(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.starts_with('.') && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn check_drive_id(id: &str) -> Result<(), Bad> {
    if (10..=128).contains(&id.len()) && is_token(id, 128) && !id.contains('.') {
        Ok(())
    } else {
        Err(bad("locator", "not a bare Drive file id"))
    }
}

/// Rules every locator shares, applied before the scheme-specific shape.
fn check_locator_common(l: &str) -> Result<(), Bad> {
    if l.is_empty() || l.len() > MAX_LOCATOR_CHARS {
        return Err(bad("locator", "empty or too long"));
    }
    // Printable ASCII only: no spaces, controls, backslashes, or look-alike letters in a host.
    if !l.bytes().all(|b| (0x21..=0x7e).contains(&b)) || l.contains('\\') {
        return Err(bad("locator", "must be printable ASCII without backslashes"));
    }
    // Query and parameter syntax is where signed-URL and token parameters live.
    if l.contains(['?', ';', '=', '&']) {
        return Err(bad("locator", "query or parameter syntax is not allowed"));
    }
    let lower = l.to_ascii_lowercase();
    if lower.starts_with("delphi:") {
        return Err(bad("locator", "delphi:// is not a pointer scheme; use xenia://"));
    }
    if lower == "xenia://cap" || lower.starts_with("xenia://cap/") {
        return Err(bad("locator", "xenia://cap/ is reserved for capability URIs (spec 007 Plane 2), not built"));
    }
    if looks_like_credential(l) {
        return Err(bad("locator", "contains something shaped like a credential"));
    }
    Ok(())
}

fn looks_like_credential(l: &str) -> bool {
    const PREFIXES: [&str; 13] = ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_", "glpat-", "xoxb-", "xoxp-", "xoxa-", "sk_live_", "sk-ant-", "pat_"];
    l.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')).any(|t| {
        PREFIXES.iter().any(|p| t.starts_with(p)) || (t.len() >= 16 && t.starts_with("AKIA")) || (t.len() >= 20 && t.starts_with("eyJ"))
    })
}

/// `https://host[:port]` + optional path (everything after `host[:port]`), no userinfo.
fn split_authority<'a>(url: &'a str, scheme: &str) -> Result<&'a str, Bad> {
    let rest = url.strip_prefix(scheme).and_then(|r| r.strip_prefix("://")).ok_or(bad("locator", "wrong URL scheme"))?;
    let end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    if authority.contains(['@', '%', '[', ']']) {
        return Err(bad("locator", "userinfo, escapes and IP literals are not allowed in the host"));
    }
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && !host.starts_with(['.', '-'])
        && !host.ends_with(['.', '-'])
        && !host.contains("..")
        && host.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'));
    if !host_ok {
        return Err(bad("locator", "host must be lowercase ASCII letters, digits, dots and hyphens"));
    }
    if let Some(p) = port {
        let ok = !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()) && !p.starts_with('0') && p.parse::<u32>().is_ok_and(|n| n <= 65535);
        if !ok {
            return Err(bad("locator", "bad port"));
        }
    }
    Ok(path)
}

/// Path segments: no `.` or `..`, no empty segment in the middle, `%` only as an escape of something harmless.
fn check_path_segments(path: &str, allow_trailing_slash: bool) -> Result<(), Bad> {
    let body = path.strip_prefix('/').unwrap_or(path);
    let body = if allow_trailing_slash { body.strip_suffix('/').unwrap_or(body) } else { body };
    if body.is_empty() {
        return Ok(());
    }
    for seg in body.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(bad("locator", "empty, `.` or `..` path segment"));
        }
        check_escapes(seg)?;
    }
    Ok(())
}

fn check_escapes(seg: &str) -> Result<(), Bad> {
    let b = seg.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = b.get(i + 1..i + 3).ok_or(bad("locator", "truncated % escape"))?;
            let v = std::str::from_utf8(hex).ok().and_then(|h| u8::from_str_radix(h, 16).ok()).ok_or(bad("locator", "bad % escape"))?;
            // Encoded dots, slashes, backslashes, `@`, `%`, controls and DEL can hide a traversal or an authority.
            if v < 0x20 || v == 0x7f || matches!(v, b'.' | b'/' | b'\\' | b'@' | b'%' | b'?' | b'#' | b':' | b';' | b'=' | b'&') {
                return Err(bad("locator", "a % escape hides a reserved character"));
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    Ok(())
}

fn check_https_locator(l: &str) -> Result<(), Bad> {
    if l.contains('#') {
        return Err(bad("locator", "no fragment in an https locator"));
    }
    let path = split_authority(l, "https")?;
    check_path_segments(path, true)
}

/// `<remote-url>#<path>`: remote is `https://` or `ssh://` with no userinfo (so not `ssh://git@host/...`:
/// spec 012 §4 says "no embedded credentials" and a username is the part of an ssh URL that is one).
fn check_git_locator(l: &str) -> Result<(), Bad> {
    let (remote, path) = l.split_once('#').ok_or(bad("locator", "a git locator is <remote>#<path>"))?;
    let scheme = if remote.starts_with("https://") {
        "https"
    } else if remote.starts_with("ssh://") {
        "ssh"
    } else {
        return Err(bad("locator", "git remote must be https:// or ssh://"));
    };
    let remote_path = split_authority(remote, scheme)?;
    check_path_segments(remote_path, true)?;
    if path.is_empty() || path.starts_with('/') || path.contains(['#', ':']) {
        return Err(bad("locator", "git path must be a non-empty repository-relative path"));
    }
    check_path_segments(path, false)
}

/// `xenia://<kind>/<id>[@rev]`, another record in this store. The authority is the record kind; `cap` is
/// reserved for Plane 2 capability URIs and is never a kind.
fn check_xenia_locator(l: &str) -> Result<(), Bad> {
    let path = l.strip_prefix("xenia://").ok_or(bad("locator", "a xenia locator is xenia://<kind>/<id>[@rev]"))?;
    let (kind, rest) = path.split_once('/').ok_or(bad("locator", "a xenia locator is xenia://<kind>/<id>[@rev]"))?;
    if Kind::parse(kind).is_none() {
        return Err(bad("locator", "unknown record kind"));
    }
    let (id, rev) = match rest.split_once('@') {
        Some((i, r)) => (i, Some(r)),
        None => (rest, None),
    };
    if !ulid::is_valid(id) {
        return Err(bad("locator", "not a ULID"));
    }
    if let Some(r) = rev {
        let ok = !r.is_empty() && r.len() <= 9 && r.bytes().all(|b| b.is_ascii_digit()) && !r.starts_with('0');
        if !ok {
            return Err(bad("locator", "revision is not a canonical integer of 1 or more"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::{self, Scalar, Value};

    const ID: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RA";
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn sc(t: &str) -> Value {
        Value::Scalar(Scalar { text: t.to_string(), quoted: true })
    }

    /// Built as a map, not parsed from text, so hostile locators reach `from_map` byte for byte.
    fn ptr(scheme: &str, locator: &str, pin_kind: &str, pin_value: Option<&str>) -> Result<Pointer, Bad> {
        let mut pin = vec![("kind".to_string(), sc(pin_kind))];
        if let Some(v) = pin_value {
            pin.push(("value".to_string(), sc(v)));
        }
        let entries = vec![
            ("scheme".to_string(), sc(scheme)),
            ("locator".to_string(), sc(locator)),
            ("pin".to_string(), Value::Map(Map { entries: pin })),
            ("label".to_string(), sc("A label")),
            ("content_type".to_string(), sc("text/markdown")),
        ];
        Pointer::from_map(Map { entries })
    }

    fn publishable(scheme: &str, locator: &str, pin_kind: &str, pin_value: Option<&str>) -> Result<(), Bad> {
        ptr(scheme, locator, pin_kind, pin_value)?.check_publishable()
    }

    fn git(locator: &str) -> Result<(), Bad> {
        publishable("git", locator, "git-commit", Some(SHA))
    }

    fn web(locator: &str) -> Result<(), Bad> {
        publishable("https", locator, "none", None)
    }

    #[test]
    fn good_pointers_of_each_scheme_pass() {
        git("https://github.com/acme/payments.git#docs/adr/0007.md").unwrap();
        git("ssh://git.acme.example:2222/acme/payments.git#README.md").unwrap();
        web("https://docs.acme.example/runbooks/payments").unwrap();
        web("https://docs.acme.example:8443/a%20b/").unwrap();
        publishable("https", "https://docs.acme.example/x", "content-hash", Some(&hash::of(b"x"))).unwrap();
        publishable("xenia", &format!("xenia://memory/{ID}"), "none", None).unwrap();
        publishable("xenia", &format!("xenia://memory/{ID}@3"), "none", None).unwrap();
        publishable("git", "https://github.com/acme/p#src/lib.rs", "git-tree", Some(&"a".repeat(64))).unwrap();
    }

    #[test]
    fn schemes_outside_the_allowlist_are_refused() {
        for s in ["file", "ftp", "http", "ssh", "delphi", "javascript", "data", "HTTPS", "Git", ""] {
            assert!(ptr(s, "https://x.example/y", "none", None).is_err(), "{s:?}");
        }
    }

    #[test]
    fn delphi_uris_are_refused_with_a_pointer_to_xenia() {
        for l in ["delphi://memory/01JBX3Q8T2M9Z4K7WYH5N6D0RA", "DELPHI://cap/abc", "delphi://bob.tailnet/p/a.md"] {
            for scheme in ["xenia", "https", "git"] {
                let e = publishable(scheme, l, "none", None).unwrap_err();
                assert_eq!((e.field, e.why), ("locator", "delphi:// is not a pointer scheme; use xenia://"), "{scheme} {l:?}");
            }
        }
    }

    #[test]
    fn xenia_cap_is_reserved_and_never_a_record_kind() {
        for l in ["xenia://cap/abc", "xenia://cap", "XENIA://CAP/x"] {
            let e = publishable("xenia", l, "none", None).unwrap_err();
            assert_eq!(e.why, "xenia://cap/ is reserved for capability URIs (spec 007 Plane 2), not built", "{l:?}");
        }
        assert!(Kind::parse("cap").is_none());
        assert!(publishable("xenia", &format!("xenia://cap/{ID}"), "none", None).is_err());
    }

    #[test]
    fn drive_parses_but_is_not_publishable_in_phase_1() {
        let p = ptr("drive", "1AbCdEfGhIjKlMnOpQrStUv", "drive-revision", Some("12345")).unwrap();
        assert_eq!(p.scheme, Scheme::Drive);
        assert_eq!(p.check_publishable(), Err(bad("scheme", "drive pointers are not publishable in Phase 1")));
        // A share URL with a token is refused for its shape before the phase gate is reached.
        let shape = publishable("drive", "https://drive.google.com/file/d/ABC/view", "drive-revision", Some("1")).unwrap_err();
        assert_eq!((shape.field, shape.why), ("locator", "not a bare Drive file id"));
    }

    #[test]
    fn local_paths_file_urls_and_delphi_uris_are_refused() {
        for l in [
            "file:///etc/passwd", "/etc/passwd", "../secrets.md", "./notes.md", "~/notes.md", "D:/data/notes.md", "docs/readme.md", "delphi://cap/abc", "DELPHI://x", "memory/01JBX3Q8T2M9Z4K7WYH5N6D0RA", "xenia:memory/01JBX3Q8T2M9Z4K7WYH5N6D0RA",
            "https:///etc/passwd", "https://", "https:/x.example", "http://x.example/y",
        ] {
            assert!(web(l).is_err(), "https {l:?}");
            assert!(git(&format!("{l}#a.md")).is_err(), "git {l:?}");
            assert!(publishable("xenia", l, "none", None).is_err(), "xenia {l:?}");
        }
        assert!(git("file:///srv/repo.git#a.md").is_err());
        assert!(git("https://github.com/acme/p#/etc/passwd").is_err());
        assert!(git("https://github.com/acme/p#../../etc/passwd").is_err());
        assert!(git("https://github.com/acme/p#a/../b").is_err());
        assert!(git("https://github.com/acme/p#").is_err());
        assert!(git("https://github.com/acme/p").is_err(), "no #path");
    }

    #[test]
    fn credentials_tokens_and_signed_url_parameters_are_refused() {
        for l in [
            "https://user:pw@docs.acme.example/x", "https://user@docs.acme.example/x", "https://@docs.acme.example/x", "https://docs.acme.example@evil.example/x",
            "https://docs.acme.example/x?token=abc", "https://docs.acme.example/x?X-Amz-Signature=abc", "https://docs.acme.example/x?sig=abc&se=2027", "https://docs.acme.example/x?",
            "https://docs.acme.example/x#access_token=abc", "https://docs.acme.example/x;jsessionid=1", "https://docs.acme.example/ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "https://docs.acme.example/s/github_pat_11AAAA", "https://docs.acme.example/AKIAIOSFODNN7EXAMPLE", "https://docs.acme.example/eyJhbGciOiJIUzI1NiJ9.e30.abc",
            "https://docs.acme.example/xoxb-123-456", "https://docs.acme.example/glpat-abcdefghij",
        ] {
            assert!(web(l).is_err(), "{l:?}");
        }
        for r in ["ssh://git@github.com/acme/p.git", "https://u:p@github.com/acme/p.git", "https://github.com/acme/p.git?token=1", "ssh://github.com/acme/p.git?x=1"] {
            assert!(git(&format!("{r}#a.md")).is_err(), "{r:?}");
        }
        // The same URL without the credential is fine, so the refusals above are about the credential.
        web("https://docs.acme.example/x").unwrap();
        git("ssh://github.com/acme/p.git#a.md").unwrap();
    }

    #[test]
    fn host_tricks_and_encodings_are_refused() {
        for l in [
            "https://Docs.acme.example/x", "https://docs.acme.example./x", "https://-docs.example/x", "https://do..cs.example/x", "https://[::1]/x", "https://docs.acme.example:0/x",
            "https://docs.acme.example:99999/x", "https://docs.acme.example:/x", "https://docs.acme.example:80:81/x", "https://d\u{43e}cs.acme.example/x", "https://docs.acme.example/x y",
            "https://docs.acme.example/a/../b", "https://docs.acme.example/a/./b", "https://docs.acme.example/a//b", "https://docs.acme.example/%2e%2e/b", "https://docs.acme.example/a%2Fb",
            "https://docs.acme.example/a%40b", "https://docs.acme.example/%00", "https://docs.acme.example/%zz", "https://docs.acme.example/a%2", "https://docs.acme%2Eexample/x",
            "https://docs.acme.example/a\\b", "https://docs.acme.example/x\n", "https://docs.acme.example/\u{202e}x",
        ] {
            assert!(web(l).is_err(), "{l:?}");
        }
    }

    #[test]
    fn pins_must_match_the_scheme() {
        assert!(publishable("git", "https://h.example/r#a", "content-hash", Some(&hash::of(b"x"))).is_err());
        assert!(publishable("git", "https://h.example/r#a", "none", None).is_err());
        assert!(publishable("git", "https://h.example/r#a", "git-commit", Some("abc123")).is_err(), "short sha");
        assert!(publishable("git", "https://h.example/r#a", "git-commit", Some(&SHA.to_uppercase())).is_err());
        assert!(publishable("https", "https://h.example/r", "git-commit", Some(SHA)).is_err());
        assert!(publishable("https", "https://h.example/r", "content-hash", Some("blake3:xx")).is_err());
        assert!(publishable("xenia", &format!("xenia://memory/{ID}"), "content-hash", Some(&hash::of(b"x"))).is_err());
        assert!(publishable("xenia", &format!("xenia://skill/{ID}"), "none", None).is_err());
        assert!(publishable("xenia", "xenia://memory/not-a-ulid", "none", None).is_err());
        assert!(publishable("xenia", &format!("xenia://memory/{ID}@0"), "none", None).is_err());
        assert!(publishable("xenia", &format!("xenia://memory/{ID}@03"), "none", None).is_err());
        assert!(publishable("xenia", &format!("xenia://memory/{ID}@1@2"), "none", None).is_err());
    }

    #[test]
    fn structure_is_strict() {
        assert!(ptr("https", "https://h.example/r", "none", Some("x")).is_err(), "none carries no value");
        assert!(ptr("https", "https://h.example/r", "content-hash", None).is_err(), "other pins need a value");
        let ok = "scheme: https\nlocator: \"https://h.example/r\"\npin:\n  kind: none\nlabel: L\ncontent_type: text/plain\n";
        let parse = |extra: &str| Pointer::from_map(frontmatter::parse(format!("---\n{ok}{extra}---\n").as_bytes()).unwrap().front);
        assert!(parse("").is_ok());
        assert!(parse("range: L40-L120\n").is_ok());
        for r in ["L40", "40-120", "L0-L5", "L5-L4", "L01-L5", "L1-L1000001", "l1-l2", "L1-L2 ", "L-1-L2"] {
            assert!(parse(&format!("range: \"{r}\"\n")).is_err(), "{r}");
        }
        assert!(parse("range: L1-L1000000\n").is_ok());
        assert!(parse("bytes: 18234\n").is_ok());
        assert!(parse("bytes: \"18234\"\n").is_err());
        assert!(parse(&format!("bytes: {}\n", MAX_POINTER_BYTES + 1)).is_err());
        assert!(parse("tier: 1\n").is_err(), "unknown key");
        assert!(parse("pin2: 1\n").is_err());
        assert!(parse(&format!("hash: \"{}\"\n", hash::of(b"x"))).is_ok());
        assert!(parse("hash: \"blake3:00\"\n").is_err());
        assert!(parse("content_type2: x\n").is_err());
        let wrong_type = ok.replace("text/plain", "text/html");
        assert!(Pointer::from_map(frontmatter::parse(format!("---\n{wrong_type}---\n").as_bytes()).unwrap().front).is_err());
        // Ranges apply to text in git and https only.
        let mut p = parse("range: L1-L2\n").unwrap();
        p.check_publishable().unwrap();
        p.content_type = ContentType::Png;
        assert!(p.check_publishable().is_err());
    }

    #[test]
    fn locator_length_boundary() {
        let base = "https://h.example/";
        let at = format!("{base}{}", "a".repeat(MAX_LOCATOR_CHARS - base.len()));
        assert_eq!(at.len(), MAX_LOCATOR_CHARS);
        web(&at).unwrap();
        assert!(web(&format!("{at}a")).is_err());
    }
}
