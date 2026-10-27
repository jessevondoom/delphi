//! The record serializer: the writing half of spec 012 §2 (X2 only read; milestone X7 publishes).
//!
//! [`serialize`] turns a [`Record`] into the **canonical bytes** that [`crate::record::validate_record`] accepts.
//! There is one spelling: spec 012's key order, every string double-quoted with only the `\"` and `\\` escapes
//! the parser knows, enums and integers bare, two-space indent, LF, `[]` for an empty list, `null` only for an
//! absent `supersedes`, the body verbatim after the closing `---`.
//!
//! **The output is checked, not trusted.** After writing, the bytes go back through the same parser and
//! structural steps the verifier runs ([`check_parsed`], spec 012 §6 steps 4-10) and must parse to a record equal to the
//! input. So anything this function returns validates and round-trips, whatever it was given, and anything it
//! refuses is refused for a stated reason ([`SerializeError::reason`]):
//!
//! * `unrepresentable`: a one-line field holds a line break or control character (no escape exists for it);
//! * `invalid`: the verifier's own rules refuse the record (the specific rule is in the error);
//! * `not-canonical`: the bytes parse to something other than the input (a defect here, never an attack);
//! * `too-large`: the file would exceed the 16 KiB record limit.
//!
//! The serializer does not decide anything about trust: it does not sign, scan or check standing.

use crate::layout::RecordPath;
use crate::limits::MAX_RECORD_BYTES;
use crate::pointer::Pointer;
use crate::record::{check_parsed, KindFields, Pinned, Record, Reject};
use crate::redaction;
use crate::sidecar::Sidecar;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SerializeError {
    /// A one-line field contains a line break or a control character; the format has no escape for it.
    Unrepresentable(&'static str),
    /// The verifier would refuse this record (the first rule it breaks).
    Invalid(Reject),
    /// The bytes parse back to a different record: a bug in this module, reported rather than published.
    NotCanonical,
    TooLarge,
}

impl SerializeError {
    pub fn reason(&self) -> &'static str {
        match self {
            SerializeError::Unrepresentable(_) => "unrepresentable",
            SerializeError::Invalid(_) => "invalid",
            SerializeError::NotCanonical => "not-canonical",
            SerializeError::TooLarge => "too-large",
        }
    }
}

impl std::fmt::Display for SerializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerializeError::Unrepresentable(w) => write!(f, "unrepresentable: {w}"),
            SerializeError::Invalid(r) => write!(f, "invalid: {}", r.reason()),
            other => write!(f, "{}", other.reason()),
        }
    }
}

impl std::error::Error for SerializeError {}

type Res<T> = Result<T, SerializeError>;

/// `"..."` with the two escapes the parser knows. A line break or control character has no spelling.
fn q(s: &str) -> Res<String> {
    if s.chars().any(|c| c.is_control()) {
        return Err(SerializeError::Unrepresentable("a line break or control character in a one-line field"));
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    Ok(out)
}

/// `["a", "b"]`, always quoted so that a tag spelled `null` is the word and not a null.
fn list<S: AsRef<str>>(items: &[S]) -> Res<String> {
    let parts: Res<Vec<String>> = items.iter().map(|s| q(s.as_ref())).collect();
    Ok(format!("[{}]", parts?.join(", ")))
}

fn pointer_block(key: &str, items: &[Pointer], out: &mut String) -> Res<()> {
    if items.is_empty() {
        out.push_str(&format!("{key}: []\n"));
        return Ok(());
    }
    out.push_str(&format!("{key}:\n"));
    for p in items {
        out.push_str(&format!("  - scheme: {}\n", p.scheme.as_str()));
        out.push_str(&format!("    locator: {}\n", q(&p.locator)?));
        out.push_str("    pin:\n");
        out.push_str(&format!("      kind: {}\n", p.pin.kind.as_str()));
        if let Some(v) = &p.pin.value {
            out.push_str(&format!("      value: {}\n", q(v)?));
        }
        if let Some(h) = &p.hash {
            out.push_str(&format!("    hash: {}\n", q(h)?));
        }
        if let Some((a, b)) = p.range {
            out.push_str(&format!("    range: \"L{a}-L{b}\"\n"));
        }
        out.push_str(&format!("    label: {}\n", q(&p.label)?));
        out.push_str(&format!("    content_type: {}\n", p.content_type.as_str()));
        if let Some(b) = p.bytes {
            out.push_str(&format!("    bytes: {b}\n"));
        }
        if let Some(h) = &p.access_hint {
            out.push_str(&format!("    access_hint: {}\n", q(h)?));
        }
    }
    Ok(())
}

/// One pointer as a map under `key` (the `bundle` kind's `tree`).
fn pointer_map(key: &str, p: &Pointer, out: &mut String) -> Res<()> {
    out.push_str(&format!("{key}:\n  scheme: {}\n  locator: {}\n  pin:\n    kind: {}\n", p.scheme.as_str(), q(&p.locator)?, p.pin.kind.as_str()));
    if let Some(v) = &p.pin.value {
        out.push_str(&format!("    value: {}\n", q(v)?));
    }
    if let Some(h) = &p.hash {
        out.push_str(&format!("  hash: {}\n", q(h)?));
    }
    if let Some((a, b)) = p.range {
        out.push_str(&format!("  range: \"L{a}-L{b}\"\n"));
    }
    out.push_str(&format!("  label: {}\n  content_type: {}\n", q(&p.label)?, p.content_type.as_str()));
    if let Some(b) = p.bytes {
        out.push_str(&format!("  bytes: {b}\n"));
    }
    if let Some(h) = &p.access_hint {
        out.push_str(&format!("  access_hint: {}\n", q(h)?));
    }
    Ok(())
}

/// Write `r` as a record file. See the module docs for what is guaranteed about the result.
pub fn serialize(r: &Record) -> Res<Vec<u8>> {
    let bytes = render(r)?.into_bytes();
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(SerializeError::TooLarge);
    }
    self_check(r, &bytes)?;
    Ok(bytes)
}

/// The text of the canonical spelling, with no checks.
fn render(r: &Record) -> Res<String> {
    let mut t = String::new();
    t.push_str("---\nxenia: 1\n");
    t.push_str(&format!("id: {}\nkind: {}\nrev: {}\n", q(&r.id)?, r.kind().as_str(), r.rev));
    t.push_str(&format!("org: {}\nstore: {}\nauthor: {}\nauthor_handle: {}\n", q(&r.org)?, q(&r.store)?, q(&r.author)?, q(&r.author_handle)?));
    t.push_str(&format!("author_role: {}\ntitle: {}\nsummary: {}\ntags: {}\nvisibility: org\n", r.author_role.as_str(), q(&r.title)?, q(&r.summary)?, list(&r.tags)?));
    t.push_str(&format!("created: {}\nupdated: {}\n", q(&r.created)?, q(&r.updated)?));
    t.push_str(&format!("source_ceiling: {}\nconfidence: {}\nstatus: {}\n", r.source_ceiling.as_str(), r.confidence.as_str(), r.status.as_str()));
    match &r.supersedes {
        Some((id, rev)) => t.push_str(&format!("supersedes: {}\n", q(&format!("{id}@{rev}"))?)),
        None => t.push_str("supersedes: null\n"),
    }
    if let Some(d) = &r.review_by {
        t.push_str(&format!("review_by: {}\n", q(d)?));
    }
    if let Some(d) = &r.expires {
        t.push_str(&format!("expires: {}\n", q(d)?));
    }
    pointer_block("pointers", &r.pointers, &mut t)?;
    pointer_block("evidence", &r.evidence, &mut t)?;
    t.push_str(&format!("related: {}\n", list(&r.related)?));
    t.push_str(&format!(
        "redaction:\n  scanner: {}\n  scanner_version: {}\n  result: passed\n  content_hash: {}\n",
        q(&r.redaction.scanner)?,
        q(&r.redaction.scanner_version)?,
        q(&r.redaction.content_hash)?
    ));
    t.push_str(&format!("body_kind: {}\n", r.body_kind.as_str()));
    match &r.kind_fields {
        KindFields::Memory(m) => {
            t.push_str(&format!("memory_type: {}\napplies_to:\n", m.memory_type.as_str()));
            t.push_str(&format!("  repos: {}\n  tech: {}\n  paths: {}\n", list(&m.applies_to.repos)?, list(&m.applies_to.tech)?, list(&m.applies_to.paths)?));
            t.push_str(&format!("verified_by: {}\n", m.verified_by.as_str()));
        }
        KindFields::Endorsement(e) => {
            t.push_str(&format!("target: {}\ntarget_hash: {}\nverdict: {}\n", q(&format!("{}@{}", e.target_id, e.target_rev))?, q(&e.target_hash)?, e.verdict.as_str()));
            t.push_str(&format!("scope: {}\n", list(&e.scope)?));
            if let Some(n) = &e.note {
                t.push_str(&format!("note: {}\n", q(n)?));
            }
            let checked: Vec<&str> = e.checked.iter().map(|c| c.as_str()).collect();
            t.push_str(&format!("checked: {}\n", list(&checked)?));
        }
        KindFields::Tombstone(s) => {
            t.push_str(&format!("target: {}\naction: {}\nreason: {}\n", q(&s.target)?, s.action.as_str(), s.reason.as_str()));
            if let Some(n) = &s.note {
                t.push_str(&format!("note: {}\n", q(n)?));
            }
        }
        KindFields::Bundle(b) => {
            t.push_str(&format!("bundle_name: {}\nbundle_version: {}\nbundle_hash: {}\n", q(&b.bundle_name)?, q(&b.bundle_version)?, q(&b.bundle_hash)?));
            pointer_map("tree", &b.tree, &mut t)?;
            let roles: Vec<&str> = b.touches_roles.iter().map(|r| r.as_str()).collect();
            t.push_str(&format!("touches:\n  roles: {}\n  connectors: {}\n", list(&roles)?, list(&b.touches_connectors)?));
            t.push_str("compat:\n");
            if let Some(d) = &b.compat_delphi {
                t.push_str(&format!("  delphi: {}\n", q(d)?));
            }
            t.push_str(&format!("  bundle_schema: {}\n", b.compat_schema));
        }
    }
    t.push_str("---\n");
    t.push_str(&r.body);
    Ok(t)
}

/// The output goes through the verifier's own structural steps and must come back as the same record.
fn self_check(r: &Record, bytes: &[u8]) -> Res<()> {
    let at = RecordPath { kind: r.kind(), id: r.id.clone() };
    // Only `kid` matters to the cross-check; the rest of the sidecar is not read by `check_parsed`.
    let car = Sidecar { kid: r.author.clone(), signed_at: "2000-01-01T00:00:00Z".into(), roster_epoch: 1, hash: crate::hash::of(bytes), sig: String::new() };
    let parsed = check_parsed(Pinned { org: &r.org, store: &r.store }, &at, &car, bytes).map_err(SerializeError::Invalid)?;
    if parsed == *r { Ok(()) } else { Err(SerializeError::NotCanonical) }
}

/// Write `r` with `redaction.content_hash` computed from its own body and pointer text (the publish gate's
/// step, [`crate::redaction`]). The scanner name and version are set to the ones the gate runs. Returns the
/// record as sealed and its bytes.
pub fn seal(r: &Record) -> Res<(Record, Vec<u8>)> {
    let mut sealed = r.clone();
    sealed.redaction.scanner = redaction::SCANNER.into();
    sealed.redaction.scanner_version = redaction::SCANNER_VERSION.into();
    // The regions do not contain the redaction block, so any well-formed placeholder gives the same regions.
    sealed.redaction.content_hash = crate::hash::of(b"placeholder");
    let draft = render(&sealed)?;
    sealed.redaction.content_hash = redaction::content_hash(draft.as_bytes()).ok_or(SerializeError::NotCanonical)?;
    let bytes = serialize(&sealed)?;
    Ok((sealed, bytes))
}

/// `records/<kind>/<id>.md` for a record.
pub fn path_of(r: &Record) -> String {
    format!("records/{}/{}.md", r.kind().as_str(), r.id)
}
