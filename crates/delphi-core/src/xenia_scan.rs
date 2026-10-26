//! The publish gate's scan (spec 011 §7.1 step 2; plan milestone X7): the `[REV-SEC-05]` path and secret scan over
//! every piece of text a record will carry, and the flags the preview shows.
//!
//! **Blocks** stop the publish. They are the `[REV-SEC-05]` classes: a secret or token (the pattern set of
//! [`Redactor`], the same one that masks events and that the bundle `validate` step 8 scan uses), a private-key
//! block, an operator-registered credential literal in raw, base64 or URL-encoded form, a local user path
//! (`/Users/<name>/`, `/home/<name>/`, `C:\Users\<name>`) and a reference to a mission's own record (`events.jsonl`,
//! `local/missions/`, a `"type":"task.*"` event line), which is what an "unredacted session trace" looks like.
//!
//! **Flags** do not block (spec 011 §7.1: "flags (does not block) URLs, long high-entropy strings and
//! customer-identifier patterns"); they are shown beside the exact bytes and the operator's approval has to
//! acknowledge them ([`crate::xenia_publish::Approval`]). Classes: `url`, `high-entropy`, `email`, `identifier` (the
//! workspace's `[privacy] identifiers`) and `customer-id` (a payment-processor customer or account id, a long digit
//! run).
//!
//! **Where the patterns live.** `delphi-bundle` depends on this crate, so its step 8 scanner cannot be imported here.
//! The secret patterns are shared through [`Redactor`] (one list for events, bundles and records); the three small
//! expressions that exist only in the bundle scanner (home path, key header, mission-record reference) are repeated
//! below, and a test pins that both scanners agree on a canary of every class.
//!
//! **A finding never carries the value it found.** [`Block::found`] and [`Flag::found`] are masked descriptions
//! (a class, a length, a host), because the report is shown to the operator and logged.

use crate::redact::Redactor;
use delphi_xenia::record::{KindFields, Record};
use regex::Regex;

/// What the scan knows about the place the text is going.
#[derive(Debug, Default, Clone)]
pub struct ScanContext {
    /// The workspace's `[privacy] identifiers` (customer and project names that should not leave).
    pub identifiers: Vec<String>,
    /// Credential values the daemon resolved at runtime; matched raw, base64 and URL-encoded.
    pub secret_literals: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// `secret`, `token`, `private-key`, `local-path` or `session-trace`.
    pub class: &'static str,
    /// The record field, for example `summary`, `body` or `pointers[0].label`.
    pub field: String,
    pub line: usize,
    pub found: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    /// `url`, `high-entropy`, `email`, `identifier` or `customer-id`.
    pub class: &'static str,
    pub field: String,
    pub line: usize,
    pub found: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScanReport {
    pub blocks: Vec<Block>,
    pub flags: Vec<Flag>,
}

impl ScanReport {
    pub fn passed(&self) -> bool {
        self.blocks.is_empty()
    }
}

struct Patterns {
    redactor: Redactor,
    home: Regex,
    key_header: Regex,
    trace: Regex,
    url: Regex,
    email: Regex,
    customer: Regex,
    digits: Regex,
    word: Regex,
}

fn patterns(ctx: &ScanContext) -> Patterns {
    let mut redactor = Redactor::new();
    for l in &ctx.secret_literals {
        redactor.register_literal(l);
    }
    Patterns {
        redactor,
        home: Regex::new(r"(/Users/[A-Za-z0-9._-]+/|/home/[A-Za-z0-9._-]+/|[A-Za-z]:\\Users\\[A-Za-z0-9._-]+)").unwrap(),
        key_header: Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----").unwrap(),
        trace: Regex::new(r#"events\.jsonl|local/missions/|/workspaces/[A-Za-z0-9._-]+/local/|"type"\s*:\s*"(?:task|tool|handoff|approval)\.[a-z_]+""#).unwrap(),
        url: Regex::new(r#"(?i)\b(?:https?|ssh|ftp)://[^\s<>")\]]+"#).unwrap(),
        email: Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+").unwrap(),
        customer: Regex::new(r"\b(?:cus|acct|sub|in|pi|ch|cust|customer|account)[_-][A-Za-z0-9]{6,}\b").unwrap(),
        digits: Regex::new(r"\b\d{12,}\b").unwrap(),
        word: Regex::new(r"[A-Za-z0-9+/_=-]{24,}").unwrap(),
    }
}

/// Shannon entropy in bits per character.
fn entropy(s: &str) -> f64 {
    let mut counts = std::collections::BTreeMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0usize) += 1;
    }
    let n = s.chars().count() as f64;
    counts.values().map(|&c| { let p = c as f64 / n; -p * p.log2() }).sum()
}

/// A long run of mixed characters that looks random: at least 24 characters, letters and digits both, and an
/// entropy above what words and paths reach. Tuned to flag keys and hashes and to leave ordinary prose alone.
fn high_entropy(token: &str) -> bool {
    token.len() >= 24
        && token.bytes().any(|b| b.is_ascii_digit())
        && token.bytes().any(|b| b.is_ascii_alphabetic())
        && entropy(token) >= 3.6
}

fn scan_field(p: &Patterns, ctx: &ScanContext, field: &str, text: &str, free: bool, out: &mut ScanReport) {
    let mut key_block = false;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if p.key_header.is_match(line) {
            key_block = true;
            out.blocks.push(Block { class: "private-key", field: field.into(), line: n, found: "a private key block".into() });
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
                out.blocks.push(Block { class, field: field.into(), line: n, found: format!("a {class} ({} characters changed by redaction)", line.chars().count().abs_diff(masked.chars().count()).max(1)) });
            }
        }
        for m in p.home.find_iter(line) {
            let root = m.as_str().trim_start_matches('/').split(['/', '\\']).next().unwrap_or("");
            out.blocks.push(Block { class: "local-path", field: field.into(), line: n, found: format!("a path under /{root}/…") });
        }
        if p.trace.is_match(line) {
            out.blocks.push(Block { class: "session-trace", field: field.into(), line: n, found: "a reference to a mission's event log or record".into() });
        }
        if free {
            for m in p.url.find_iter(line) {
                let host = m.as_str().split("://").nth(1).unwrap_or("").split(['/', ':']).next().unwrap_or("");
                out.flags.push(Flag { class: "url", field: field.into(), line: n, found: format!("a link to {}", host.chars().take(80).collect::<String>()) });
            }
        }
        for m in p.email.find_iter(line) {
            let domain = m.as_str().rsplit_once('@').map(|(_, d)| d).unwrap_or("");
            out.flags.push(Flag { class: "email", field: field.into(), line: n, found: format!("an address at {domain}") });
        }
        for w in p.word.find_iter(line) {
            if high_entropy(w.as_str()) {
                out.flags.push(Flag { class: "high-entropy", field: field.into(), line: n, found: format!("a {}-character string that looks random", w.as_str().len()) });
            }
        }
        if p.customer.is_match(line) || p.digits.is_match(line) {
            out.flags.push(Flag { class: "customer-id", field: field.into(), line: n, found: "something shaped like a customer or account id".into() });
        }
        for id in ctx.identifiers.iter().filter(|i| !i.is_empty()) {
            if line.contains(id.as_str()) {
                out.flags.push(Flag { class: "identifier", field: field.into(), line: n, found: format!("the workspace identifier `{id}`") });
            }
        }
    }
    // A key spread over lines is redacted as one block, which a line-by-line pass cannot see; the whole text decides.
    if !key_block && out.blocks.iter().all(|b| b.field != field) && p.redactor.redact_str(text).1 {
        out.blocks.push(Block { class: "secret", field: field.into(), line: 1, found: "a secret spread over several lines".into() });
    }
}

/// The text fields of a record, by name. `free` marks the fields a person wrote as prose (URL flags apply to those;
/// pointer locators are URLs by nature and are shown in the preview as they are).
fn fields(r: &Record) -> Vec<(String, String, bool)> {
    let mut v: Vec<(String, String, bool)> = vec![("title".into(), r.title.clone(), true), ("summary".into(), r.summary.clone(), true), ("body".into(), r.body.clone(), true)];
    v.push(("author_handle".into(), r.author_handle.clone(), false));
    for (i, t) in r.tags.iter().enumerate() {
        v.push((format!("tags[{i}]"), t.clone(), false));
    }
    for (name, list) in [("pointers", &r.pointers), ("evidence", &r.evidence)] {
        for (i, p) in list.iter().enumerate() {
            v.push((format!("{name}[{i}].locator"), p.locator.clone(), false));
            v.push((format!("{name}[{i}].label"), p.label.clone(), true));
            if let Some(h) = &p.access_hint {
                v.push((format!("{name}[{i}].access_hint"), h.clone(), true));
            }
            if let Some(val) = &p.pin.value {
                v.push((format!("{name}[{i}].pin"), val.clone(), false));
            }
        }
    }
    match &r.kind_fields {
        KindFields::Memory(m) => {
            for (name, list) in [("repos", &m.applies_to.repos), ("tech", &m.applies_to.tech), ("paths", &m.applies_to.paths)] {
                for (i, s) in list.iter().enumerate() {
                    v.push((format!("applies_to.{name}[{i}]"), s.clone(), false));
                }
            }
        }
        KindFields::Endorsement(e) => {
            if let Some(n) = &e.note {
                v.push(("note".into(), n.clone(), true));
            }
        }
        KindFields::Tombstone(t) => {
            if let Some(n) = &t.note {
                v.push(("note".into(), n.clone(), true));
            }
        }
    }
    v
}

/// Scan every text field of `r`, then the serialized file as a whole for blocks (defense in depth: a field this
/// function forgot to list still cannot carry a secret into the signed bytes).
pub fn scan_record(r: &Record, file: Option<&str>, ctx: &ScanContext) -> ScanReport {
    let p = patterns(ctx);
    let mut report = ScanReport::default();
    for (name, text, free) in fields(r) {
        scan_field(&p, ctx, &name, &text, free, &mut report);
    }
    if let Some(file) = file {
        let mut whole = ScanReport::default();
        scan_field(&p, ctx, "file", file, false, &mut whole);
        // Only what the per-field pass did not already report, so a finding is listed once.
        for b in whole.blocks {
            if !report.blocks.iter().any(|x| x.class == b.class) {
                report.blocks.push(b);
            }
        }
    }
    report
}

/// Scan loose text (for example a note typed into the console before it becomes a record).
pub fn scan_text(field: &str, text: &str, ctx: &ScanContext) -> ScanReport {
    let p = patterns(ctx);
    let mut report = ScanReport::default();
    scan_field(&p, ctx, field, text, true, &mut report);
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(r: &ScanReport) -> Vec<&'static str> {
        let mut v: Vec<_> = r.blocks.iter().map(|b| b.class).collect();
        v.sort();
        v.dedup();
        v
    }

    #[test]
    fn a_canary_of_every_block_class_is_found_and_none_is_echoed() {
        let ctx = ScanContext::default();
        let canaries = [
            ("secret", "the key is sk-ant-abcdefghijklmnopqrstuvwxyz0123456789"),
            ("token", "use ghp_abcdefghijklmnopqrstuvwxyz0123456789"),
            ("secret", "AKIAABCDEFGHIJKLMNOP is the access key"),
            ("private-key", "-----BEGIN OPENSSH PRIVATE KEY-----"),
            ("local-path", concat!("see /Us", "ers/jane/work/notes.md")),
            ("local-path", concat!("see /ho", "me/deploy/app")),
            ("local-path", r"see C:\Users\jane\notes"),
            ("session-trace", "from local/missions/m-12/events.jsonl"),
            ("secret", "export STRIPE_API_KEY=whatever123"),
        ];
        for (class, text) in canaries {
            let r = scan_text("summary", text, &ctx);
            assert!(r.blocks.iter().any(|b| b.class == class) || (class == "secret" && !r.blocks.is_empty()) || (class == "token" && !r.blocks.is_empty()), "{text}: {:?}", r.blocks);
            for b in &r.blocks {
                assert!(!b.found.contains("abcdefghijklmnop") && !b.found.contains("jane") && !b.found.contains("whatever"), "a finding echoed its value: {b:?}");
            }
        }
        assert!(scan_text("summary", "a plain sentence about retries and idempotency", &ctx).passed());
    }

    #[test]
    fn a_registered_literal_is_found_raw_and_encoded() {
        use base64::Engine;
        let secret = "correct-horse-battery-staple-9";
        let ctx = ScanContext { secret_literals: vec![secret.into()], ..Default::default() };
        assert!(!scan_text("body", &format!("password is {secret}"), &ctx).passed());
        let b64 = base64::engine::general_purpose::STANDARD.encode(secret);
        assert!(!scan_text("body", &format!("blob {b64}"), &ctx).passed());
        assert!(scan_text("body", "nothing here", &ctx).passed());
    }

    #[test]
    fn a_key_spread_over_lines_is_found() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nMIIBOgIBAAJBAKj34GkxFhD90vcNLYLInFEX6Ppy1tPf9Cnzj4p4WGeKLs1Pt8Qu\n-----END RSA PRIVATE KEY-----";
        assert!(classes(&scan_text("body", text, &ScanContext::default())).contains(&"private-key"));
    }

    #[test]
    fn flags_do_not_block_and_are_masked() {
        let ctx = ScanContext { identifiers: vec!["Initech".into()], ..Default::default() };
        let r = scan_text(
            "body",
            "See https://wiki.internal.example/page for Initech; ask jane@initech.example. Customer cus_9sdf8sdf8sd. Blob Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MGFiY2RlZg9z.",
            &ctx,
        );
        assert!(r.passed(), "{:?}", r.blocks);
        let flagged: Vec<_> = r.flags.iter().map(|f| f.class).collect();
        for want in ["url", "identifier", "email", "customer-id", "high-entropy"] {
            assert!(flagged.contains(&want), "{want} missing from {flagged:?}");
        }
        assert!(r.flags.iter().all(|f| !f.found.contains("cus_9sdf8sdf8sd") && !f.found.contains("Zm9vYmFy")));
    }

    #[test]
    fn prose_is_not_flagged_as_random() {
        let r = scan_text("summary", "Deliveries are retried with exponential backoff for up to three days; handlers must be idempotent.", &ScanContext::default());
        assert!(r.flags.is_empty() && r.blocks.is_empty(), "{r:?}");
        // A long path-like token with real words is not random.
        let r = scan_text("summary", "crates/delphi-core/src/xenia_publish.rs", &ScanContext::default());
        assert!(r.flags.is_empty(), "{r:?}");
    }
}
