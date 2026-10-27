//! Test builders for signed records and freshness statements (companion of `testkit`, same rules: test-only,
//! keys in memory, signs whatever it is told to, never enabled in a release build).

use crate::identity::Identity;
use crate::layout::Kind;
use crate::sidecar::Sidecar;
use crate::sig::Domain;
use crate::testkit::{TestOrg, ORG, STORE, TS};
use crate::hash;
use serde_json::{json, Value};

/// A record file and its sidecar, signed, at their store paths.
#[derive(Debug, Clone)]
pub struct SignedRecord {
    pub kind: Kind,
    pub id: String,
    pub path: String,
    pub md: Vec<u8>,
    pub sidecar_path: String,
    pub sidecar: Vec<u8>,
}

impl SignedRecord {
    /// `(path, bytes)` for both files, ready to be written into a store.
    pub fn files(&self) -> [(&str, &[u8]); 2] {
        [(self.path.as_str(), self.md.as_slice()), (self.sidecar_path.as_str(), self.sidecar.as_slice())]
    }
}

/// Rewrite the `redaction.content_hash` line of a record file so it is the real recipe hash of that file's own
/// body and pointer text (what the publish gate writes and the verifier recomputes). Text without such a line
/// is returned unchanged.
pub fn seal_text(text: &str) -> String {
    let Some(real) = crate::redaction::content_hash(text.as_bytes()) else { return text.to_string() };
    let Some(at) = text.find("  content_hash: \"") else { return text.to_string() };
    let from = at + "  content_hash: \"".len();
    let Some(len) = text[from..].find('"') else { return text.to_string() };
    format!("{}{}{}", &text[..from], real, &text[from + len..])
}

/// Sign `md` as record `kind`/`id` with `who` (the sidecar the publisher would write), after setting its
/// `content_hash` to the real one ([`seal_text`]); use [`sign_record_raw`] to sign the bytes exactly as given.
/// `roster_epoch` and `signed_at` are informational, so tests pass anything.
pub fn sign_record(who: &Identity, kind: Kind, id: &str, md: Vec<u8>) -> SignedRecord {
    let md = match String::from_utf8(md) {
        Ok(t) => seal_text(&t).into_bytes(),
        Err(e) => e.into_bytes(),
    };
    sign_record_raw(who, kind, id, md)
}

/// [`sign_record`] with the bytes exactly as given.
pub fn sign_record_raw(who: &Identity, kind: Kind, id: &str, md: Vec<u8>) -> SignedRecord {
    let h = hash::of(&md);
    let d = Domain::record(ORG, STORE, kind, id, &who.did(), &h, 1, TS).unwrap();
    let car = Sidecar { kid: who.did(), signed_at: TS.into(), roster_epoch: 1, hash: h, sig: who.sign(&d) };
    SignedRecord {
        kind,
        id: id.into(),
        path: format!("records/{}/{id}.md", kind.as_str()),
        md,
        sidecar_path: format!("records/{}/{id}.sig.json", kind.as_str()),
        sidecar: car.to_json().into_bytes(),
    }
}

fn common(id: &str, kind: &str, rev: u64, who: &Identity, status: &str) -> String {
    common_with(id, kind, rev, who, status, "payments", "internal_empirical")
}

fn common_with(id: &str, kind: &str, rev: u64, who: &Identity, status: &str, tags: &str, ceiling: &str) -> String {
    format!(
        "---
xenia: 1
id: {id}
kind: {kind}
rev: {rev}
org: {ORG}
store: {STORE}
author: \"{}\"
author_handle: tester
author_role: builder
title: A note about {id} at revision {rev}
summary: A short summary that is long enough to be a summary of a test record.
tags: [{tags}]
visibility: org
created: \"2026-10-24T09:00:00Z\"
updated: \"2026-10-24T09:00:00Z\"
source_ceiling: {ceiling}
confidence: medium
status: {status}
pointers: []
redaction:
  scanner: rev-sec-05
  scanner_version: \"1\"
  result: passed
  content_hash: \"{}\"
",
        who.did(),
        hash::of(b"scanned")
    )
}

/// A valid `memory` record. `status` is `active` or `withdrawn`; `variant` goes into the body so two records
/// at one `rev` can have different bytes.
pub fn memory(who: &Identity, id: &str, rev: u64, status: &str, variant: &str) -> SignedRecord {
    let mut t = common(id, "memory", rev, who, status);
    t.push_str(&format!(
        "supersedes: null\nreview_by: \"2027-01-24\"\nevidence: []\nrelated: []\nbody_kind: markdown\nmemory_type: gotcha\napplies_to:\n  repos: [payments]\n  tech: [stripe]\n  paths: []\nverified_by: none\n---\nBody {variant}.\n"
    ));
    sign_record(who, Kind::Memory, id, t.into_bytes())
}

/// A valid `tombstone` of `target` (an `id`) with `action` `withdraw` or `restore`.
pub fn tombstone(who: &Identity, id: &str, rev: u64, target: &str, action: &str) -> SignedRecord {
    let mut t = common(id, "tombstone", rev, who, "active");
    t.push_str(&format!("body_kind: none\ntarget: {target}\naction: {action}\nreason: mistake\nnote: A test tombstone.\n---\n"));
    sign_record(who, Kind::Tombstone, id, t.into_bytes())
}

/// A valid `endorsement` of `target@target_rev` with `verdict` `endorse`, `dispute` or `withdraw`.
pub fn endorsement(who: &Identity, id: &str, rev: u64, target: &str, target_rev: u64, verdict: &str) -> SignedRecord {
    let mut t = common(id, "endorsement", rev, who, "active");
    t.push_str(&format!(
        "body_kind: none\ntarget: \"{target}@{target_rev}\"\ntarget_hash: \"{}\"\nverdict: {verdict}\nnote: A test note.\nscope: [payments]\nchecked: [target-open]\n---\n",
        hash::of(b"target bytes")
    ));
    sign_record(who, Kind::Endorsement, id, t.into_bytes())
}

/// A freshness statement as a test writes it; the fields are plain data so any attack is one assignment.
#[derive(Debug, Clone)]
pub struct FreshSpec {
    pub org: String,
    pub store: String,
    pub epoch: u64,
    pub epoch_hash: String,
    pub store_head: String,
    pub issued_at: String,
    pub expires_at: String,
    pub rewrite_of: Option<String>,
}

impl FreshSpec {
    /// A statement for `org`'s head epoch naming `store_head`, issued at `issued` (Unix seconds) and valid for
    /// `life_secs`.
    pub fn for_head(org: &TestOrg, store_head: &str, issued: i64, life_secs: i64) -> FreshSpec {
        let (epoch, epoch_hash) = org.head_pair();
        FreshSpec {
            org: org.org.clone(),
            store: org.store.clone(),
            epoch,
            epoch_hash,
            store_head: store_head.into(),
            issued_at: crate::time::format_utc(issued),
            expires_at: crate::time::format_utc(issued + life_secs),
            rewrite_of: None,
        }
    }

    pub fn to_json(&self) -> String {
        let mut v = json!({
            "org": self.org, "store": self.store, "epoch": self.epoch, "epoch_hash": self.epoch_hash,
            "store_head": self.store_head, "issued_at": self.issued_at, "expires_at": self.expires_at,
        });
        if let Some(r) = &self.rewrite_of {
            v.as_object_mut().unwrap().insert("rewrite_of".into(), json!(r));
        }
        serde_json::to_string_pretty(&v).unwrap() + "\n"
    }

    /// `(freshness.json, freshness.sigs.json)` signed by exactly `signers`.
    pub fn sign(&self, signers: &[&Identity]) -> (Vec<u8>, Vec<u8>) {
        let body = self.to_json().into_bytes();
        let domain = Domain::freshness(&self.org, &self.store, &hash::of(&body)).unwrap();
        let list: Vec<Value> = signers.iter().map(|s| json!({ "kid": s.did(), "sig": s.sign(&domain) })).collect();
        let sigs = serde_json::to_string_pretty(&json!({ "signatures": list })).unwrap() + "\n";
        (body, sigs.into_bytes())
    }
}

impl SignedRecord {
    /// `blake3:` of the record file: what an endorsement's `target_hash` must equal.
    pub fn hash(&self) -> String {
        hash::of(&self.md)
    }
}

/// [`memory`] with chosen `tags` (comma-separated, may be empty) and `source_ceiling`.
pub fn memory_with(who: &Identity, id: &str, rev: u64, status: &str, variant: &str, tags: &str, ceiling: &str) -> SignedRecord {
    let mut t = common_with(id, "memory", rev, who, status, tags, ceiling);
    t.push_str(&format!(
        "supersedes: null\nreview_by: \"2027-01-24\"\nevidence: []\nrelated: []\nbody_kind: markdown\nmemory_type: gotcha\napplies_to:\n  repos: [payments]\n  tech: [stripe]\n  paths: []\nverified_by: none\n---\nBody {variant}.\n"
    ));
    sign_record(who, Kind::Memory, id, t.into_bytes())
}

/// [`endorsement`] with a chosen `target_hash`, `scope` (comma-separated) and the endorsement's own `tags`.
#[allow(clippy::too_many_arguments)]
pub fn endorsement_with(who: &Identity, id: &str, rev: u64, target: &str, target_rev: u64, target_hash: &str, verdict: &str, scope: &str) -> SignedRecord {
    let mut t = common(id, "endorsement", rev, who, "active");
    t.push_str(&format!(
        "body_kind: none\ntarget: \"{target}@{target_rev}\"\ntarget_hash: \"{target_hash}\"\nverdict: {verdict}\nnote: A test note.\nscope: [{scope}]\nchecked: [target-open]\n---\n"
    ));
    sign_record(who, Kind::Endorsement, id, t.into_bytes())
}

/// [`tombstone`] with a chosen `reason` (`superseded`, `mistake`, `sensitive`, `departed`, `other`).
pub fn tombstone_with(who: &Identity, id: &str, rev: u64, target: &str, action: &str, reason: &str) -> SignedRecord {
    let mut t = common(id, "tombstone", rev, who, "active");
    t.push_str(&format!("body_kind: none\ntarget: {target}\naction: {action}\nreason: {reason}\nnote: A test tombstone.\n---\n"));
    sign_record(who, Kind::Tombstone, id, t.into_bytes())
}

/// The knobs of a test `bundle` record. Everything is plain text so any attack is one string replacement.
#[derive(Debug, Clone)]
pub struct BundleSpec {
    pub name: String,
    pub version: String,
    pub bundle_hash: String,
    pub tags: String,
    pub ceiling: String,
    pub status: String,
    /// The lines under `tree:` (each indented two spaces).
    pub tree: String,
    /// The text of `touches.roles` and `touches.connectors` (YAML flow lists) and of `compat.delphi`.
    pub roles: String,
    pub connectors: String,
    pub compat: String,
}

/// A valid tree block: `https` git pointer to the repository root, pinned by this tree id.
pub fn tree_block(locator: &str, pin_kind: &str, pin: &str) -> String {
    format!("  scheme: git\n  locator: \"{locator}\"\n  pin:\n    kind: {pin_kind}\n    value: \"{pin}\"\n  label: The bundle tree\n  content_type: other\n")
}

impl BundleSpec {
    pub fn new(name: &str, version: &str, bundle_hash: &str) -> BundleSpec {
        BundleSpec {
            name: name.into(),
            version: version.into(),
            bundle_hash: bundle_hash.into(),
            tags: "payments".into(),
            ceiling: "internal_empirical".into(),
            status: "active".into(),
            tree: tree_block("https://git.acme.example/platform/bundle.git#.", "git-tree", "0123456789abcdef0123456789abcdef01234567"),
            roles: "[\"strategist\", \"builder\"]".into(),
            connectors: "[\"slack\"]".into(),
            compat: ">=0.3.0".into(),
        }
    }
}

/// A valid `bundle` record (spec 027 §4.3) at `rev`.
pub fn bundle(who: &Identity, id: &str, rev: u64, spec: &BundleSpec) -> SignedRecord {
    let mut t = common_with(id, "bundle", rev, who, &spec.status, &spec.tags, &spec.ceiling);
    t.push_str(&format!(
        "supersedes: null\nevidence: []\nrelated: []\nbody_kind: none\nbundle_name: \"{}\"\nbundle_version: \"{}\"\nbundle_hash: \"{}\"\ntree:\n{}touches:\n  roles: {}\n  connectors: {}\ncompat:\n  delphi: \"{}\"\n  bundle_schema: 1\n---\n",
        spec.name, spec.version, spec.bundle_hash, spec.tree, spec.roles, spec.connectors, spec.compat
    ));
    sign_record(who, Kind::Bundle, id, t.into_bytes())
}
