//! Step 8 of `specs/023` §10: the `[REV-SEC-05]` secret and path scan over every text file, plus the identifiers a
//! bundle should not carry.
//!
//! One scanner serves three callers, so that what `validate` fails on, what the redaction report lists and what
//! `distill` masks before a model reads a source are the same classes (`specs/027` §6.2): [`scan_text`] finds, and
//! [`mask_text`] replaces. A hit never carries the value it found; [`Hit::found`] is a masked description.

use crate::manifest::Manifest;
use crate::tree::{in_vectors, Tree};
use crate::{Host, Report};
use delphi_core::redact::Redactor;
use regex::Regex;

/// What the scan knows about the place a text is going: the one e-mail address it may carry, and the names of the
/// workspace it came from.
#[derive(Debug, Default, Clone)]
pub struct Context {
    /// `publisher.contact`, the only address allowed to appear.
    pub contact: Option<String>,
    /// The workspace's `[privacy] identifiers`.
    pub identifiers: Vec<String>,
    /// Work handles of people (the roster, once it exists, and `[privacy] handles`).
    pub handles: Vec<String>,
}

/// One finding. `fail` is false for the classes `specs/027` §6.2 makes warnings (a decision record may legitimately
/// name its author by handle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub line: usize,
    /// `secret`, `token`, `private-key`, `local-path`, `e-mail`, `person-name`, `identifier` or `transcript`: the classes of
    /// `skills/org-bundle-authoring/templates/redaction-report.md`.
    pub class: &'static str,
    /// What was found, with anything secret left out: a length, a domain, a path prefix.
    pub found: String,
    pub fail: bool,
}

struct Patterns {
    redactor: Redactor,
    home: Regex,
    email: Regex,
    key_header: Regex,
    log: Regex,
}

fn patterns() -> Patterns {
    Patterns {
        redactor: Redactor::new(),
        home: Regex::new(r"(/Users/[A-Za-z0-9._-]+/|/home/[A-Za-z0-9._-]+/|[A-Za-z]:\\Users\\[A-Za-z0-9._-]+)").unwrap(),
        email: Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+").unwrap(),
        key_header: Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----").unwrap(),
        // What a mission's own record looks like. A knowledge file points to a mission by file and commit and never carries
        // its log or transcript (`specs/027` §6.4); the event log is unreadable to every role.
        log: Regex::new(r#"events\.jsonl|local/(?:missions|sessions)/|/workspaces/[A-Za-z0-9._-]+/local/|"type"\s*:\s*"(?:task|tool|handoff|approval)\.[a-z_]+""#).unwrap(),
    }
}

/// Every finding in `text`, in line order.
pub fn scan_text(text: &str, ctx: &Context) -> Vec<Hit> {
    let p = patterns();
    let mut hits = Vec::new();
    let mut key_lines: Vec<usize> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if p.key_header.is_match(line) {
            key_lines.push(n);
            hits.push(Hit { line: n, class: "private-key", found: "a private key block".into(), fail: true });
        } else {
            let (masked, changed) = p.redactor.redact_str(line);
            if changed {
                let class = if line.contains("PRIVATE KEY") {
                    "private-key"
                } else if ["token", "TOKEN", "ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_", "xox"].iter().any(|k| line.contains(k)) {
                    "token"
                } else {
                    "secret"
                };
                hits.push(Hit { line: n, class, found: format!("a {} ({} characters changed by redaction)", class, line.chars().count().abs_diff(masked.chars().count()).max(1)), fail: true });
            }
        }
        for m in p.home.find_iter(line) {
            // The user name is personal; the prefix is what makes it a path worth reporting.
            let root = m.as_str().trim_start_matches('/').split(['/', '\\']).next().unwrap_or("");
            hits.push(Hit { line: n, class: "local-path", found: format!("a path under /{}/…", root), fail: true });
        }
        for m in p.email.find_iter(line) {
            if ctx.contact.as_deref() != Some(m.as_str()) {
                let domain = m.as_str().rsplit_once('@').map(|(_, d)| d).unwrap_or("");
                hits.push(Hit { line: n, class: "e-mail", found: format!("an address at {}", domain), fail: true });
            }
        }
        if p.log.is_match(line) {
            hits.push(Hit { line: n, class: "transcript", found: "a reference to a mission's event log or record".into(), fail: false });
        }
        for id in ctx.identifiers.iter().filter(|i| !i.is_empty()) {
            if line.contains(id.as_str()) {
                hits.push(Hit { line: n, class: "identifier", found: format!("the workspace identifier `{}`", id), fail: false });
            }
        }
        for h in ctx.handles.iter().filter(|h| !h.is_empty()) {
            if line.contains(h.as_str()) {
                hits.push(Hit { line: n, class: "person-name", found: format!("the handle `{}`", h), fail: false });
            }
        }
    }
    // A key spread over lines is redacted as one block, which a line-by-line pass cannot see; the whole text decides.
    if key_lines.is_empty() && p.redactor.redact_str(text).1 && hits.iter().all(|h| !h.fail) {
        hits.push(Hit { line: 1, class: "secret", found: "a secret spread over several lines".into(), fail: true });
    }
    hits
}

/// `text` with every failing finding replaced by a marker, for a source a model is about to read. Identifiers and
/// handles are masked too: a distilled draft should not carry the workspace's own names forward.
pub fn mask_text(text: &str, ctx: &Context) -> String {
    let p = patterns();
    let (mut out, _) = p.redactor.redact_str(text);
    out = p.home.replace_all(&out, "<local-path>/").into_owned();
    let contact = ctx.contact.clone();
    out = p.email.replace_all(&out, |c: &regex::Captures| if contact.as_deref() == Some(&c[0]) { c[0].to_string() } else { "<e-mail>".to_string() }).into_owned();
    for id in ctx.identifiers.iter().filter(|i| !i.is_empty()) {
        out = out.replace(id.as_str(), "<identifier>");
    }
    for h in ctx.handles.iter().filter(|h| !h.is_empty()) {
        out = out.replace(h.as_str(), "<handle>");
    }
    out
}

/// A scanned bundle file: every hit with its path, for [`report`].
#[derive(Debug, Clone)]
pub struct FileHit {
    pub path: String,
    pub hit: Hit,
}

/// The files step 8 looks at: every text file outside `vectors/` and the machine-written `.delphi-bundle/`.
fn scanned<'a>(tree: &'a Tree) -> impl Iterator<Item = &'a crate::tree::Entry> + 'a {
    tree.entries.iter().filter(|e| !in_vectors(&e.path) && !e.path.starts_with(".delphi-bundle/"))
}

/// Every hit in the tree, for `validate --report`.
pub fn report(tree: &Tree, m: Option<&Manifest>, host: &dyn Host) -> (usize, Vec<FileHit>) {
    let ctx = Context { contact: m.and_then(|m| m.publisher_contact.clone()), identifiers: host.identifiers(), handles: host.roster_handles() };
    let mut files = 0;
    let mut out = Vec::new();
    for e in scanned(tree) {
        let Some(text) = tree.text(&e.path) else { continue };
        files += 1;
        out.extend(scan_text(&text, &ctx).into_iter().map(|hit| FileHit { path: e.path.clone(), hit }));
    }
    (files, out)
}

pub fn check(tree: &Tree, m: Option<&Manifest>, host: &dyn Host, r: &mut Report) {
    let ctx = Context { contact: m.and_then(|m| m.publisher_contact.clone()), identifiers: host.identifiers(), handles: host.roster_handles() };
    for e in scanned(tree) {
        // A staging directory holds drafts and a redaction report that have not been reviewed; it must never be packaged.
        if e.path.split('/').any(|s| s == ".distill") {
            r.fail(8, &e.path, "is a distill staging file; drafts and reports stay outside the bundle");
        }
        let Some(text) = tree.text(&e.path) else { continue };
        let hits = scan_text(&text, &ctx);
        // One finding per file and class, as before; the report lists every line.
        let mut seen: Vec<&'static str> = Vec::new();
        for h in &hits {
            if seen.contains(&h.class) {
                continue;
            }
            seen.push(h.class);
            let msg = match h.class {
                "secret" | "token" | "private-key" => "contains a secret (an API key, token or private key)".to_string(),
                "local-path" => format!("contains a local user path (`{}`)", h.found),
                "e-mail" => "contains an e-mail address other than publisher.contact".to_string(),
                "identifier" => format!("contains {}", h.found),
                "transcript" => format!("contains {}", h.found),
                _ => format!("names {}", h.found),
            };
            if h.fail {
                r.fail(8, &e.path, msg);
            } else {
                r.warn(8, &e.path, msg);
            }
        }
    }
}
