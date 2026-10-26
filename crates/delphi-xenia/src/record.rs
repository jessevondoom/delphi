//! The record envelope, the three Phase 1 kinds, and the validation pipeline (spec 012 §2, §5, §6).
//!
//! [`validate_record`] is spec 012 §6 as code, in the spec's order, and **the only way to obtain a
//! [`Validated`]** (its fields are private): nothing from a record reaches a caller, let alone a model
//! (step 11), until every earlier step has passed.
//!
//! | step | here |
//! | :-: | :--- |
//! | 1-2 | [`crate::sidecar::verify_record`]: size, sidecar, path (raw bytes, no parsing of the record) |
//! | 3 | the same call (hash, strict signature), then the caller's `standing` check |
//! | 4 | [`frontmatter::parse`], the first and only place the record's text is looked at |
//! | 5 | schema: version, rejected trust keys, closed enums, required and unknown keys ([`Record::from_map`]) |
//! | 6 | [`check_cross`]: `id`/`kind` equal the path, `org`/`store` the pinned values, `author` the `kid` |
//! | 7 | [`check_strings`]: plain-text fields carry no Markdown links or HTML (NFC, controls, bidi and tag characters are screened by the parser for the whole file) |
//! | 8 | [`check_counts`]: lengths and counts |
//! | 9 | [`Pointer::check_publishable`] for `pointers` and `evidence` |
//! | 10 | [`check_dates`]; "in the future at publish" is [`Record::check_future_dates`], which needs a clock the caller injects |
//!
//! The record's claims about itself are display copies: no field here states a trust tier (the six keys that
//! would are rejected wherever they appear), and nothing in this module decides trust.

use crate::frontmatter::{self, bad, pick, Bad, Map};
use crate::layout::Kind;
use crate::limits::*;
use crate::pointer::Pointer;
use crate::redaction;
use crate::sidecar::{self, Sidecar};
use crate::{did, hash, is_slug, is_utc_timestamp, ulid, Error};

/// Keys that would state a trust tier. Rejected at any depth, including inside `pointers` and `applies_to`
/// (spec 012 §2.1, §6 step 5). Trust is computed locally from the roster, endorsements and freshness.
pub const TRUST_KEYS: [&str; 6] = ["tier", "trust", "trust_class", "source_type", "endorsed", "verified_by_org"];

/// The install's own pinned configuration, the only source of `org` and `store` (spec 012 §3).
#[derive(Debug, Clone, Copy)]
pub struct Pinned<'a> {
    pub org: &'a str,
    pub store: &'a str,
}

/// Why a record was refused, and at which step. [`Reject::reason`] is the quarantine slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    /// Steps 1-3 on raw bytes: size, sidecar, path, hash, signature.
    Verify(Error),
    /// Step 3: the signer is not `active` at the current epoch.
    Standing(&'static str),
    /// Step 4.
    Frontmatter(&'static str),
    /// `xenia:` names a major version this client does not understand. Not an attack (spec 012 §9).
    UnsupportedSchema,
    /// Step 5: one of [`TRUST_KEYS`] appeared.
    TrustKey(&'static str),
    /// Step 5.
    Schema(Bad),
    /// Step 1, checked after the frontmatter parses.
    BodyTooLarge,
    /// Step 6.
    Mismatch(&'static str),
    /// Step 7.
    Text(Bad),
    /// Step 8.
    Count(Bad),
    /// Step 9.
    Pointer(Bad),
    /// Step 10.
    Date(&'static str),
}

impl Reject {
    pub fn reason(&self) -> &'static str {
        match self {
            Reject::Verify(e) => e.reason(),
            Reject::Standing(_) => "not-active",
            Reject::Frontmatter(_) => "bad-frontmatter",
            Reject::UnsupportedSchema => "unsupported-schema",
            Reject::TrustKey(_) => "forbidden-key",
            Reject::Schema(_) => "bad-schema",
            Reject::BodyTooLarge => "oversize",
            Reject::Mismatch(_) => "mismatch",
            Reject::Text(_) => "bad-text",
            Reject::Count(_) => "over-limit",
            Reject::Pointer(_) => "bad-pointer",
            Reject::Date(_) => "bad-date",
        }
    }
}

impl From<Error> for Reject {
    fn from(e: Error) -> Reject {
        Reject::Verify(e)
    }
}

macro_rules! closed_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($var),+ }
        impl $name {
            pub const TABLE: &'static [(&'static str, $name)] = &[$(($s, $name::$var)),+];
            pub fn as_str(self) -> &'static str {
                match self { $($name::$var => $s),+ }
            }
        }
    };
}

closed_enum!(
    /// Canonical role ids (D-17); `operator` is the human writing by hand.
    Role { Operator = "operator", Architect = "architect", Builder = "builder", Investigator = "investigator", Scout = "scout", Strategist = "strategist", RedTeam = "red-team", Writer = "writer", Integrator = "integrator", Orchestrator = "orchestrator" }
);
closed_enum!(
    /// The local lattice's enum (spec 001 §5); the receiver clamps to its own ceiling regardless.
    SourceCeiling { UserRatified = "user_ratified", InternalEmpirical = "internal_empirical", InternalGenerated = "internal_generated", FederatedVerified = "federated_verified", FederatedPeer = "federated_peer", ExternalUntrusted = "external_untrusted" }
);
closed_enum!(Confidence { Low = "low", Medium = "medium", High = "high", Verified = "verified" });
closed_enum!(AuthorStatus { Active = "active", Superseded = "superseded", Withdrawn = "withdrawn" });
closed_enum!(BodyKind { Markdown = "markdown", None = "none" });
closed_enum!(MemoryType { Finding = "finding", Decision = "decision", Gotcha = "gotcha", HowTo = "how-to" });
closed_enum!(VerifiedBy { None = "none", Review = "review", Test = "test", Ci = "ci" });
closed_enum!(Verdict { Endorse = "endorse", Dispute = "dispute", Withdraw = "withdraw" });
closed_enum!(Check { TargetOpen = "target-open", EvidenceOpen = "evidence-open", SummaryMatches = "summary-matches" });
closed_enum!(TombstoneAction { Withdraw = "withdraw", Restore = "restore" });
closed_enum!(TombstoneReason { Superseded = "superseded", Mistake = "mistake", Sensitive = "sensitive", Departed = "departed", Other = "other" });

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    pub scanner: String,
    pub scanner_version: String,
    /// `blake3:` of body + pointers as scanned: the recipe of [`crate::redaction`], recomputed from the file's own
    /// bytes by [`check_content_hash`] (last step of [`check_parsed`]).
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliesTo {
    pub repos: Vec<String>,
    pub tech: Vec<String>,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFields {
    pub memory_type: MemoryType,
    pub applies_to: AppliesTo,
    pub verified_by: VerifiedBy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndorsementFields {
    pub target_id: String,
    pub target_rev: u64,
    pub target_hash: String,
    pub verdict: Verdict,
    pub scope: Vec<String>,
    pub note: Option<String>,
    pub checked: Vec<Check>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TombstoneFields {
    pub target: String,
    pub action: TombstoneAction,
    pub reason: TombstoneReason,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindFields {
    Memory(MemoryFields),
    Endorsement(EndorsementFields),
    Tombstone(TombstoneFields),
}

/// A parsed record. Every field is the author's claim; none is trusted by being here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    pub rev: u64,
    pub org: String,
    pub store: String,
    pub author: String,
    pub author_handle: String,
    pub author_role: Role,
    pub title: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub created: String,
    pub updated: String,
    pub source_ceiling: SourceCeiling,
    pub confidence: Confidence,
    pub status: AuthorStatus,
    /// `(id, rev)` this record replaces.
    pub supersedes: Option<(String, u64)>,
    pub review_by: Option<String>,
    pub expires: Option<String>,
    pub pointers: Vec<Pointer>,
    pub evidence: Vec<Pointer>,
    pub related: Vec<String>,
    pub redaction: Redaction,
    pub body_kind: BodyKind,
    pub kind_fields: KindFields,
    /// Data, never instructions.
    pub body: String,
}

impl Record {
    pub fn kind(&self) -> Kind {
        match self.kind_fields {
            KindFields::Memory(_) => Kind::Memory,
            KindFields::Endorsement(_) => Kind::Endorsement,
            KindFields::Tombstone(_) => Kind::Tombstone,
        }
    }

    /// Step 5. Structure only: required keys, closed enums, spellings. Limits and pointer policy come later.
    /// Consumes `front`; any key left over is an error, because an ignored field is a smuggling channel.
    pub fn from_map(mut m: Map, body: String) -> Result<Record, Bad> {
        if m.req_int("xenia")? != 1 {
            return Err(bad("xenia", "not schema major 1"));
        }
        let id = m.req_str("id")?;
        if !ulid::is_valid(&id) {
            return Err(bad("id", "not a ULID"));
        }
        let kind = Kind::parse(&m.req_str("kind")?).ok_or(bad("kind", "not memory, endorsement or tombstone"))?;
        let rev = m.req_int("rev")?;
        if !(1..=MAX_REV).contains(&rev) {
            return Err(bad("rev", "out of range"));
        }
        let org = slug(&mut m, "org", MAX_SLUG_LEN)?;
        let store = slug(&mut m, "store", MAX_SLUG_LEN)?;
        let author = m.req_str("author")?;
        if !did::is_valid(&author) {
            return Err(bad("author", "not a did:key"));
        }
        let author_handle = m.req_str("author_handle")?;
        if author_handle.is_empty() || !author_handle.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            return Err(bad("author_handle", "not [A-Za-z0-9._-]"));
        }
        let author_role = req_enum(&mut m, "author_role", Role::TABLE)?;
        let title = non_empty(m.req_str("title")?, "title")?;
        let summary = non_empty(m.req_str("summary")?, "summary")?;
        let tags = slug_list(m.req_list("tags")?, "tags", MAX_TAG_LEN)?;
        if m.req_str("visibility")? != "org" {
            return Err(bad("visibility", "Phase 1 allows `org` only"));
        }
        let created = m.req_str("created")?;
        let updated = m.req_str("updated")?;
        let source_ceiling = req_enum(&mut m, "source_ceiling", SourceCeiling::TABLE)?;
        let confidence = req_enum(&mut m, "confidence", Confidence::TABLE)?;
        let status = req_enum(&mut m, "status", AuthorStatus::TABLE)?;
        let supersedes = m.opt_nullable_str("supersedes")?.map(|s| parse_id_rev(&s).ok_or(bad("supersedes", "not <id>@<rev>"))).transpose()?;
        let review_by = m.opt_str("review_by")?;
        let expires = m.opt_str("expires")?;
        let pointers = pointers(m.opt_seq("pointers")?.ok_or(bad("pointers", "missing"))?)?;
        let evidence = pointers_opt(m.opt_seq("evidence")?)?;
        let related = m.opt_list("related")?.unwrap_or_default();
        if !related.iter().all(|r| ulid::is_valid(r)) {
            return Err(bad("related", "not ULIDs"));
        }
        let redaction = redaction(m.req_map("redaction")?)?;
        let body_kind = req_enum(&mut m, "body_kind", BodyKind::TABLE)?;
        match (body_kind, body.is_empty()) {
            (BodyKind::None, false) => return Err(bad("body_kind", "none, but there is a body")),
            (BodyKind::Markdown, true) => return Err(bad("body_kind", "markdown, but the body is empty")),
            _ => {}
        }
        let kind_fields = match kind {
            Kind::Memory => {
                if review_by.is_none() {
                    return Err(bad("review_by", "required for memory"));
                }
                KindFields::Memory(memory_fields(&mut m)?)
            }
            Kind::Endorsement => KindFields::Endorsement(endorsement_fields(&mut m)?),
            Kind::Tombstone => KindFields::Tombstone(tombstone_fields(&mut m)?),
        };
        m.finish().map_err(|_| bad("record", "unknown key"))?;
        Ok(Record {
            id, rev, org, store, author, author_handle, author_role, title, summary, tags, created, updated, source_ceiling, confidence, status, supersedes, review_by, expires,
            pointers, evidence, related, redaction, body_kind, kind_fields, body,
        })
    }

    /// Step 10 at publish: `review_by` and `expires` must be after `today` (`YYYY-MM-DD`, from the caller's
    /// injected clock). Ingest deliberately does not call this: a record past its review date is shown as
    /// stale, not refused.
    pub fn check_future_dates(&self, today: &str) -> Result<(), Reject> {
        if !is_date(today) {
            return Err(Reject::Date("today is not a date"));
        }
        for d in [&self.review_by, &self.expires].into_iter().flatten() {
            if d.as_str() <= today {
                return Err(Reject::Date("review_by or expires is not in the future"));
            }
        }
        Ok(())
    }
}

fn slug(m: &mut Map, f: &'static str, max: usize) -> Result<String, Bad> {
    let s = m.req_str(f)?;
    if is_slug(&s, max) { Ok(s) } else { Err(bad(f, "not [a-z0-9-]{1,32}")) }
}

fn slug_list(items: Vec<String>, f: &'static str, max: usize) -> Result<Vec<String>, Bad> {
    if !items.iter().all(|t| is_slug(t, max)) {
        return Err(bad(f, "an item is not [a-z0-9-]{1,32}"));
    }
    no_duplicates(items, f)
}

fn no_duplicates<T: PartialEq>(items: Vec<T>, f: &'static str) -> Result<Vec<T>, Bad> {
    for (i, a) in items.iter().enumerate() {
        if items[..i].contains(a) {
            return Err(bad(f, "duplicate item"));
        }
    }
    Ok(items)
}

fn non_empty(s: String, f: &'static str) -> Result<String, Bad> {
    if s.is_empty() { Err(bad(f, "empty")) } else { Ok(s) }
}

fn req_enum<T: Copy>(m: &mut Map, f: &'static str, table: &[(&str, T)]) -> Result<T, Bad> {
    pick(&m.req_str(f)?, table).ok_or(bad(f, "not one of the allowed values"))
}

/// `<id>@<rev>`: a ULID, `@`, a canonical integer of 1 or more.
pub fn parse_id_rev(s: &str) -> Option<(String, u64)> {
    let (id, rev) = s.split_once('@')?;
    if !ulid::is_valid(id) {
        return None;
    }
    let rev = frontmatter::Scalar { text: rev.to_string(), quoted: false }.as_int()?;
    (1..=MAX_REV).contains(&rev).then(|| (id.to_string(), rev))
}

fn pointers(items: Vec<Map>) -> Result<Vec<Pointer>, Bad> {
    items.into_iter().map(Pointer::from_map).collect()
}

fn pointers_opt(items: Option<Vec<Map>>) -> Result<Vec<Pointer>, Bad> {
    pointers(items.unwrap_or_default())
}

fn redaction(mut m: Map) -> Result<Redaction, Bad> {
    let scanner = m.req_str("scanner")?;
    let scanner_version = m.req_str("scanner_version")?;
    if m.req_str("result")? != "passed" {
        return Err(bad("redaction.result", "only `passed` records are publishable"));
    }
    let content_hash = m.req_str("content_hash")?;
    m.finish().map_err(|_| bad("redaction", "unknown key"))?;
    let name_ok = |s: &str, max: usize| !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'));
    if !name_ok(&scanner, MAX_SCANNER_CHARS) || !name_ok(&scanner_version, MAX_SCANNER_VERSION_CHARS) {
        return Err(bad("redaction", "scanner or version is not [A-Za-z0-9.-]"));
    }
    if !hash::is_valid(&content_hash) {
        return Err(bad("redaction.content_hash", "not blake3:<64 lowercase hex>"));
    }
    Ok(Redaction { scanner, scanner_version, content_hash })
}

fn applies_list(m: &mut Map, f: &'static str, ok: fn(u8) -> bool) -> Result<Vec<String>, Bad> {
    let items = m.opt_list(f)?.ok_or(bad(f, "missing"))?;
    if !items.iter().all(|s| !s.is_empty() && s.bytes().all(ok)) {
        return Err(bad(f, "an item has characters outside its allowed set"));
    }
    no_duplicates(items, f)
}

fn memory_fields(m: &mut Map) -> Result<MemoryFields, Bad> {
    let memory_type = req_enum(m, "memory_type", MemoryType::TABLE)?;
    let mut a = m.req_map("applies_to")?;
    // Logical names, never local paths: no `/`, `:` or leading dot, so `../x`, `/etc` and `C:` cannot appear.
    let name = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-');
    let tech = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+');
    let repos = applies_list(&mut a, "repos", name)?;
    let techs = applies_list(&mut a, "tech", tech)?;
    // Repo-relative globs.
    let glob = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/' | b'*' | b'?');
    let paths = if a.contains("paths") { applies_list(&mut a, "paths", glob)? } else { Vec::new() };
    a.finish().map_err(|_| bad("applies_to", "unknown key"))?;
    if repos.iter().any(|r| r.starts_with('.')) || paths.iter().any(|p| p.starts_with('/') || p.split('/').any(|s| s == ".." || s == ".")) {
        return Err(bad("applies_to", "a repo name or path looks like a local path"));
    }
    let verified_by = req_enum(m, "verified_by", VerifiedBy::TABLE)?;
    Ok(MemoryFields { memory_type, applies_to: AppliesTo { repos, tech: techs, paths }, verified_by })
}

fn endorsement_fields(m: &mut Map) -> Result<EndorsementFields, Bad> {
    let (target_id, target_rev) = parse_id_rev(&m.req_str("target")?).ok_or(bad("target", "not <id>@<rev>"))?;
    let target_hash = m.req_str("target_hash")?;
    if !hash::is_valid(&target_hash) {
        return Err(bad("target_hash", "not blake3:<64 lowercase hex>"));
    }
    let verdict = req_enum(m, "verdict", Verdict::TABLE)?;
    let scope = slug_list(m.req_list("scope")?, "scope", MAX_TAG_LEN)?;
    if verdict == Verdict::Endorse && scope.is_empty() {
        return Err(bad("scope", "an endorsement must name the areas it covers"));
    }
    let note = m.opt_str("note")?;
    if verdict == Verdict::Dispute && note.as_deref().is_none_or(str::is_empty) {
        return Err(bad("note", "required for dispute"));
    }
    let checked = m
        .req_list("checked")?
        .iter()
        .map(|c| pick(c, Check::TABLE).ok_or(bad("checked", "not one of target-open, evidence-open, summary-matches")))
        .collect::<Result<Vec<_>, _>>()?;
    let checked = no_duplicates(checked, "checked")?;
    Ok(EndorsementFields { target_id, target_rev, target_hash, verdict, scope, note, checked })
}

fn tombstone_fields(m: &mut Map) -> Result<TombstoneFields, Bad> {
    let target = m.req_str("target")?;
    if !ulid::is_valid(&target) {
        return Err(bad("target", "not a ULID"));
    }
    let action = req_enum(m, "action", TombstoneAction::TABLE)?;
    let reason = req_enum(m, "reason", TombstoneReason::TABLE)?;
    let note = m.opt_str("note")?;
    Ok(TombstoneFields { target, action, reason, note })
}

/// What a fully validated record is: the record, and the sidecar whose signature covers its exact bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated {
    record: Record,
    sidecar: Sidecar,
}

impl Validated {
    pub fn record(&self) -> &Record {
        &self.record
    }

    pub fn sidecar(&self) -> &Sidecar {
        &self.sidecar
    }
}

/// Spec 012 §6, steps 1 to 11, in order. `standing` is step 3's roster check (the signer is `active` at the
/// current epoch); the roster chain is milestone X3, so the caller supplies it, and it runs **before** the
/// frontmatter parser, like the signature does. A failure at any step returns before the next step runs.
pub fn validate_record(
    pinned: Pinned<'_>,
    path: &str,
    record_bytes: &[u8],
    sidecar_bytes: &[u8],
    standing: &dyn Fn(&Sidecar) -> Result<(), &'static str>,
) -> Result<Validated, Reject> {
    // Steps 1-3: raw bytes only; the record is hashed and nothing else.
    let car = sidecar::verify_record(pinned.org, pinned.store, path, record_bytes, sidecar_bytes)?;
    standing(&car).map_err(Reject::Standing)?;
    // The path was valid or verify_record would have failed.
    let at = crate::layout::RecordPath::parse(path)?;
    let record = check_parsed(pinned, &at, &car, record_bytes)?;
    // Step 11: only now is there a `Validated` for anything downstream to hold.
    Ok(Validated { record, sidecar: car })
}

/// Steps 4 to 10 on bytes whose steps 1 to 3 have already passed. Public so the fuzz tests can drive the
/// exact code the pipeline runs, without a signature per input; calling it on unverified bytes gives a
/// parsed [`Record`], never a [`Validated`], so it cannot be mistaken for verification.
pub fn check_parsed(pinned: Pinned<'_>, at: &crate::layout::RecordPath, car: &Sidecar, record_bytes: &[u8]) -> Result<Record, Reject> {
    // Step 4.
    let doc = frontmatter::parse(record_bytes).map_err(|e| Reject::Frontmatter(e.0))?;
    let front = doc.front;
    // The version is read first so that a future schema is "unsupported", not "malformed".
    match front.get("xenia") {
        Some(frontmatter::Value::Scalar(s)) => match s.as_int() {
            Some(1) => {}
            Some(n) if n > 1 => return Err(Reject::UnsupportedSchema),
            _ => return Err(Reject::Schema(bad("xenia", "not a schema major"))),
        },
        _ => return Err(Reject::Schema(bad("xenia", "missing or not a number"))),
    }
    // Step 5.
    if let Some(k) = front.find_key_deep(&TRUST_KEYS) {
        return Err(Reject::TrustKey(TRUST_KEYS.iter().find(|t| **t == k).copied().unwrap_or("trust")));
    }
    let record = Record::from_map(front, doc.body).map_err(Reject::Schema)?;
    if record.body.len() > MAX_BODY_BYTES {
        return Err(Reject::BodyTooLarge);
    }
    // Steps 6-10.
    check_cross(&record, pinned, at, car)?;
    check_strings(&record)?;
    check_counts(&record)?;
    for p in record.pointers.iter().chain(&record.evidence) {
        p.check_publishable().map_err(Reject::Pointer)?;
    }
    check_dates(&record)?;
    // Last on purpose: a record that is wrong in a specific way (a bad pointer, a bad date) is reported for that,
    // and the hash is the final statement that the scanned text is the text in the file.
    check_content_hash(&record, record_bytes)?;
    Ok(record)
}

/// Step 6. Compares the record's claims to the path, the pinned configuration and the sidecar's `kid`, which
/// are the authoritative sources (spec 011 §3.4).
pub fn check_cross(r: &Record, pinned: Pinned<'_>, at: &crate::layout::RecordPath, car: &Sidecar) -> Result<(), Reject> {
    if r.id != at.id {
        return Err(Reject::Mismatch("id differs from the file name"));
    }
    if r.kind() != at.kind {
        return Err(Reject::Mismatch("kind differs from the directory"));
    }
    if r.org != pinned.org {
        return Err(Reject::Mismatch("org differs from the pinned org"));
    }
    if r.store != pinned.store {
        return Err(Reject::Mismatch("store differs from the pinned store"));
    }
    if r.author != car.kid {
        return Err(Reject::Mismatch("author differs from the sidecar kid"));
    }
    let own = r.id.as_str();
    let refers_to_self = r.supersedes.as_ref().is_some_and(|(id, _)| id == own)
        || r.related.iter().any(|x| x == own)
        || match &r.kind_fields {
            KindFields::Endorsement(e) => e.target_id == own,
            KindFields::Tombstone(t) => t.target == own,
            KindFields::Memory(_) => false,
        };
    if refers_to_self {
        return Err(Reject::Mismatch("a record refers to itself"));
    }
    Ok(())
}

/// `redaction.content_hash` is the recipe of [`crate::redaction`] over the bytes this verifier holds: the exact
/// body and the exact `pointers` and `evidence` text. The publish gate scans those bytes and writes the hash;
/// recomputing it here means `result: passed` cannot be claimed for content other than what was scanned.
/// Reported as a cross-check failure (`mismatch`): the record disagrees with itself.
pub fn check_content_hash(r: &Record, record_bytes: &[u8]) -> Result<(), Reject> {
    match redaction::content_hash(record_bytes) {
        Some(h) if h == r.redaction.content_hash => Ok(()),
        _ => Err(Reject::Mismatch("redaction.content_hash is not the hash of the body and pointers")),
    }
}

/// A plain-text field: one line, NFC, no controls, bidi or tag characters, no Markdown links, no HTML, no
/// character entities, no surrounding space.
fn plain_text(s: &str, f: &'static str) -> Result<(), Bad> {
    frontmatter::check_text(s).map_err(|e| bad(f, e.0))?;
    if s.contains('\n') {
        return Err(bad(f, "must be one line"));
    }
    if s != s.trim() {
        return Err(bad(f, "leading or trailing space"));
    }
    if s.contains("](") || s.contains("][") || s.contains("]:") {
        return Err(bad(f, "contains a Markdown link"));
    }
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'<' && b.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, b'/' | b'!' | b'?')) {
            return Err(bad(f, "contains HTML"));
        }
        if c == b'&' {
            let tail = &b[i + 1..b.len().min(i + 12)];
            if let Some(end) = tail.iter().position(|&x| x == b';') {
                if end > 0 && tail[..end].iter().all(|x| x.is_ascii_alphanumeric() || *x == b'#') {
                    return Err(bad(f, "contains a character entity"));
                }
            }
        }
    }
    Ok(())
}

/// Step 7.
pub fn check_strings(r: &Record) -> Result<(), Reject> {
    let t = Reject::Text;
    plain_text(&r.title, "title").map_err(t)?;
    plain_text(&r.summary, "summary").map_err(t)?;
    for p in r.pointers.iter().chain(&r.evidence) {
        plain_text(&p.label, "label").map_err(t)?;
        if let Some(h) = &p.access_hint {
            plain_text(h, "access_hint").map_err(t)?;
        }
    }
    match &r.kind_fields {
        KindFields::Endorsement(EndorsementFields { note: Some(n), .. }) | KindFields::Tombstone(TombstoneFields { note: Some(n), .. }) => plain_text(n, "note").map_err(t)?,
        KindFields::Memory(m) => {
            // Names and globs are already restricted to a safe ASCII set at the schema step; checking them
            // here too keeps "every string field passes the same screen" true without a special case.
            for s in m.applies_to.repos.iter().chain(&m.applies_to.tech).chain(&m.applies_to.paths) {
                plain_text(s, "applies_to").map_err(t)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn over(n: usize, max: usize, f: &'static str) -> Result<(), Reject> {
    if n > max { Err(Reject::Count(bad(f, "over the limit"))) } else { Ok(()) }
}

/// Step 8. Characters, not bytes, for text; item counts for lists.
pub fn check_counts(r: &Record) -> Result<(), Reject> {
    over(r.title.chars().count(), MAX_TITLE_CHARS, "title")?;
    over(r.summary.chars().count(), MAX_SUMMARY_CHARS, "summary")?;
    over(r.author_handle.len(), MAX_HANDLE_CHARS, "author_handle")?;
    over(r.tags.len(), MAX_TAGS, "tags")?;
    over(r.pointers.len(), MAX_POINTERS, "pointers")?;
    over(r.evidence.len(), MAX_EVIDENCE, "evidence")?;
    over(r.related.len(), MAX_RELATED, "related")?;
    for p in r.pointers.iter().chain(&r.evidence) {
        over(p.label.chars().count(), MAX_LABEL_CHARS, "label")?;
        over(p.access_hint.as_deref().map_or(0, |h| h.chars().count()), MAX_ACCESS_HINT_CHARS, "access_hint")?;
    }
    match &r.kind_fields {
        KindFields::Memory(m) => {
            let a = &m.applies_to;
            for l in [&a.repos, &a.tech, &a.paths] {
                over(l.len(), MAX_APPLIES_ITEMS, "applies_to")?;
                for s in l {
                    over(s.chars().count(), MAX_APPLIES_ITEM_CHARS, "applies_to")?;
                }
            }
        }
        KindFields::Endorsement(e) => {
            over(e.scope.len(), MAX_SCOPE, "scope")?;
            over(e.note.as_deref().map_or(0, |n| n.chars().count()), MAX_ENDORSEMENT_NOTE_CHARS, "note")?;
        }
        KindFields::Tombstone(t) => over(t.note.as_deref().map_or(0, |n| n.chars().count()), MAX_TOMBSTONE_NOTE_CHARS, "note")?,
    }
    Ok(())
}

/// `YYYY-MM-DD` that exists on the calendar.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || !b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()) {
        return false;
    }
    let n = |r: std::ops::Range<usize>| s[r].parse::<u32>().unwrap_or(0);
    valid_ymd(n(0..4), n(5..7), n(8..10))
}

fn valid_ymd(y: u32, m: u32, d: u32) -> bool {
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 28 + u32::from(leap),
        _ => return false,
    };
    y >= 1 && (1..=days).contains(&d)
}

/// A timestamp that is both the right shape ([`is_utc_timestamp`]) and a real calendar day.
fn is_real_timestamp(s: &str) -> bool {
    is_utc_timestamp(s) && is_date(&s[..10])
}

/// Step 10 at ingest: spellings, real calendar days, `updated >= created`. Whether `review_by`/`expires` are in
/// the future is a publish-time check ([`Record::check_future_dates`]).
pub fn check_dates(r: &Record) -> Result<(), Reject> {
    if !is_real_timestamp(&r.created) || !is_real_timestamp(&r.updated) {
        return Err(Reject::Date("created or updated is not a UTC timestamp"));
    }
    // One fixed-width spelling, so string order is time order.
    if r.updated < r.created {
        return Err(Reject::Date("updated is before created"));
    }
    for d in [&r.review_by, &r.expires].into_iter().flatten() {
        if !is_date(d) {
            return Err(Reject::Date("review_by or expires is not a YYYY-MM-DD date"));
        }
    }
    Ok(())
}
