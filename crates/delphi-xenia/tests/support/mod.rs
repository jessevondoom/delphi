//! Shared fixtures for the record tests: one valid record of each kind, signing, and the pinned scope.
#![allow(dead_code)]

use delphi_xenia::identity::Identity;
use delphi_xenia::layout::Kind;
use delphi_xenia::record::{validate_record, Pinned, Reject, Validated};
use delphi_xenia::sidecar::Sidecar;
use delphi_xenia::sig::Domain;
use delphi_xenia::hash;

pub const ORG: &str = "acme";
pub const STORE: &str = "acme-main";
pub const ID: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RA";
pub const OTHER_ID: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RB";
pub const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

pub fn pinned() -> Pinned<'static> {
    Pinned { org: ORG, store: STORE }
}

pub fn author() -> Identity {
    Identity::from_seed([9u8; 32])
}

/// The standard author's did, computed once: deriving a key in a debug build is slow enough to matter in a fuzz loop.
pub fn author_did() -> &'static str {
    static DID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DID.get_or_init(|| author().did())
}

pub fn path(kind: Kind, id: &str) -> String {
    format!("records/{}/{id}.md", kind.as_str())
}

/// A valid sidecar for these exact bytes at this exact path, signed by `who`.
pub fn sidecar_for(who: &Identity, kind: Kind, id: &str, bytes: &[u8]) -> Vec<u8> {
    let h = hash::of(bytes);
    let at = "2026-10-24T09:00:03Z";
    let d = Domain::record(ORG, STORE, kind, id, &who.did(), &h, 7, at).unwrap();
    Sidecar { kid: who.did(), signed_at: at.into(), roster_epoch: 7, hash: h, sig: who.sign(&d) }.to_json().into_bytes()
}

/// Run the whole pipeline on `bytes` at the path of `kind`/`id`, signed by the standard author, signer active.
pub fn validate(kind: Kind, id: &str, bytes: &[u8]) -> Result<Validated, Reject> {
    let who = author();
    validate_signed_by(&who, kind, id, bytes)
}

pub fn validate_signed_by(who: &Identity, kind: Kind, id: &str, bytes: &[u8]) -> Result<Validated, Reject> {
    validate_record(pinned(), &path(kind, id), bytes, &sidecar_for(who, kind, id, bytes), &|_| Ok(()))
}

pub fn memory_text() -> String {
    memory_with_author(&author().did())
}

pub fn memory_with_author(did: &str) -> String {
    format!(
        "---
xenia: 1
id: {ID}
kind: memory
rev: 3
org: {ORG}
store: {STORE}
author: \"{did}\"
author_handle: jlee
author_role: builder
title: Stripe webhooks retry for three days
summary: Deliveries are retried with backoff for up to three days; handlers must be idempotent.
tags: [payments, stripe-webhooks]
visibility: org
created: \"2026-10-24T09:00:00Z\"
updated: \"2026-10-24T09:00:00Z\"
source_ceiling: internal_empirical
confidence: medium
status: active
supersedes: null
review_by: \"2027-01-24\"
expires: \"2027-06-24\"
pointers:
  - scheme: git
    locator: \"https://github.com/acme/payments.git#docs/webhooks.md\"
    pin:
      kind: git-commit
      value: \"{SHA}\"
    range: L40-L120
    label: Webhook handler notes
    content_type: text/markdown
    bytes: 18234
    access_hint: payments-eng Drive group
evidence: []
related: []
redaction:
  scanner: rev-sec-05
  scanner_version: \"1\"
  result: passed
  content_hash: \"{}\"
body_kind: markdown
memory_type: gotcha
applies_to:
  repos: [payments]
  tech: [stripe-webhooks, postgres]
  paths: [\"src/webhooks/*\"]
verified_by: test
---
Handlers must dedupe on the event id.
",
        hash::of(b"scanned")
    )
}

pub fn endorsement_text() -> String {
    format!(
        "---
xenia: 1
id: {OTHER_ID}
kind: endorsement
rev: 1
org: {ORG}
store: {STORE}
author: \"{}\"
author_handle: msmith
author_role: operator
title: Endorse the webhook retry note
summary: Reviewed the target and the handler notes; the retry window matches the provider docs.
tags: [payments]
visibility: org
created: \"2026-10-25T09:00:00Z\"
updated: \"2026-10-25T09:00:00Z\"
source_ceiling: user_ratified
confidence: high
status: active
pointers: []
redaction:
  scanner: rev-sec-05
  scanner_version: \"1\"
  result: passed
  content_hash: \"{}\"
body_kind: none
target: \"{ID}@3\"
target_hash: \"{}\"
verdict: endorse
scope: [payments]
checked: [target-open, summary-matches]
---
",
        author().did(),
        hash::of(b"scanned"),
        hash::of(b"the target bytes")
    )
}

pub fn tombstone_text() -> String {
    format!(
        "---
xenia: 1
id: {OTHER_ID}
kind: tombstone
rev: 1
org: {ORG}
store: {STORE}
author: \"{}\"
author_handle: msmith
author_role: operator
title: Withdraw the webhook retry note
summary: The note was written against a deprecated API version and should be hidden.
tags: [payments]
visibility: org
created: \"2026-10-25T09:00:00Z\"
updated: \"2026-10-25T09:00:00Z\"
source_ceiling: user_ratified
confidence: high
status: active
pointers: []
redaction:
  scanner: rev-sec-05
  scanner_version: \"1\"
  result: passed
  content_hash: \"{}\"
body_kind: none
target: {ID}
action: withdraw
reason: mistake
note: Written against API version 2019.
---
",
        author().did(),
        hash::of(b"scanned")
    )
}

/// Replace exactly one occurrence, panicking if the fixture drifted, so a mutation can never silently no-op.
pub fn swap(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "fixture must contain {from:?} exactly once");
    text.replacen(from, to, 1)
}

pub fn reason(r: Result<Validated, Reject>) -> &'static str {
    match r {
        Ok(_) => "accepted",
        Err(e) => e.reason(),
    }
}

pub fn memory_reason(text: &str) -> &'static str {
    reason(validate(Kind::Memory, ID, text.as_bytes()))
}
