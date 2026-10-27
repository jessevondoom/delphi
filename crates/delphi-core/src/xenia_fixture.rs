//! A whole Xenia install for tests, shared by `delphi-core`'s and `delphi-engine`'s tests: a signed org (the
//! standard three-admin cast), a git remote standing in for the store host, an install joined to it, and builders for
//! records whose text a test controls. Compiled for tests and under `test-support` only; never enable it in a release.
//!
//! Cast of [`TestOrg::three_admins`]: `alice`, `bob`, `carol` admins; `dave` steward of `rust`; `erin` a plain member.
//! [`Fx::with_cast`] adds `sam` (steward of `payments`), `wes` (steward of `*`) and `mia` (a member).

use crate::xenia_store::Store;
use crate::xenia_sync::{VerifiedSync, VerifyConfig};
use crate::xenia_testkit::{TestRemote, GOOD_GITATTRIBUTES};
use delphi_xenia::hash;
use delphi_xenia::identity::Identity;
use delphi_xenia::pins::{self, JoinRequest};
use delphi_xenia::testkit::{EpochSpec, MemberSpec, TestOrg, ORG, STORE};
use delphi_xenia::testkit_records::{self as rk, FreshSpec};
use delphi_xenia::time::parse_utc;

// For other crates' tests, which do not depend on `delphi-xenia` themselves.
pub use delphi_xenia::testkit_records as records;
pub use delphi_xenia::testkit::TestOrg as Org;
pub use delphi_xenia::testkit_records::SignedRecord;

pub const DAY: i64 = 24 * 3600;

/// The fixed test clock: 2026-10-26T12:00:00Z.
pub fn now() -> i64 {
    parse_utc("2026-10-26T12:00:00Z").unwrap()
}

pub fn cfg() -> VerifyConfig {
    VerifyConfig::at(now())
}

/// A ULID-shaped id: `u(0)` sorts before `u(1)`.
pub fn u(n: usize) -> String {
    let alphabet = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    format!("01JBX3Q8T2M9Z4K7WYH5N6D0R{}", alphabet[n] as char)
}

pub struct Fx {
    pub dir: std::path::PathBuf,
    pub remote: TestRemote,
    pub store: Store,
    pub org: TestOrg,
}

impl Fx {
    pub fn new(dir: &std::path::Path, org: TestOrg) -> Fx {
        let tmp = dir.to_path_buf();
        let home = tmp.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let remote = TestRemote::new(&tmp.join("host"));
        remote.write(".gitattributes", GOOD_GITATTRIBUTES.as_bytes());
        remote.write("org.toml", format!("org = \"{ORG}\"\nstore = \"{STORE}\"\n").as_bytes());
        let store = Store::open(&home, ORG, STORE, &remote.url(), true).unwrap();
        let fx = Fx { dir: tmp, remote, store, org };
        fx.roster();
        fx
    }

    pub fn standard(dir: &std::path::Path) -> Fx {
        Fx::new(dir, TestOrg::three_admins())
    }

    /// The standard cast plus `sam` (steward of `payments`), `wes` (steward of `*`) and `mia` (member), one epoch.
    pub fn with_cast(dir: &std::path::Path) -> Fx {
        let mut t = TestOrg::empty();
        let k = |t: &mut TestOrg, n: &str| t.who(n);
        let (a, b, c, d, e) = (k(&mut t, "alice"), k(&mut t, "bob"), k(&mut t, "carol"), k(&mut t, "dave"), k(&mut t, "erin"));
        let (sam, wes, mia) = (k(&mut t, "sam"), k(&mut t, "wes"), k(&mut t, "mia"));
        let genesis = EpochSpec {
            org: ORG.into(),
            store: STORE.into(),
            epoch: 1,
            prev_hash: None,
            members: vec![
                MemberSpec::new(&a, "alice").admin(),
                MemberSpec::new(&b, "bob").admin(),
                MemberSpec::new(&c, "carol").admin(),
                MemberSpec::new(&d, "dave").steward(&["rust"]),
                MemberSpec::new(&e, "erin"),
                MemberSpec::new(&sam, "sam").steward(&["payments"]),
                MemberSpec::new(&wes, "wes").steward(&["*"]),
                MemberSpec::new(&mia, "mia"),
            ],
        };
        t.push_signed(genesis, &["alice", "bob", "carol"]);
        Fx::new(dir, t)
    }

    /// Write every epoch file of `self.org` (unchanged files make no diff).
    pub fn roster(&self) {
        for (i, f) in self.org.files.iter().enumerate() {
            let n = i + 1;
            self.remote.write(&format!("roster/epochs/{n}.json"), &f.epoch_json);
            self.remote.write(&format!("roster/epochs/{n}.sigs.json"), &f.sigs_json);
        }
    }

    pub fn id(&self, who: &str) -> Identity {
        self.org.key(who)
    }

    pub fn put(&self, r: &SignedRecord) {
        for (p, b) in r.files() {
            self.remote.write(p, b);
        }
    }

    pub fn commit(&self, m: &str) -> String {
        self.remote.commit(m)
    }

    pub fn put_commit(&self, r: &SignedRecord, m: &str) -> String {
        self.put(r);
        self.commit(m)
    }

    /// A statement for the head epoch naming `store_head`, issued `age` seconds before the test clock, valid `life`.
    pub fn fresh_at(&self, store_head: &str, issued: i64, life: i64) -> String {
        let (stmt, sigs) = FreshSpec::for_head(&self.org, store_head, issued, life).sign(&[&self.id("alice")]);
        self.remote.write("roster/freshness.json", &stmt);
        self.remote.write("roster/freshness.sigs.json", &sigs);
        self.commit("freshness")
    }

    /// Issued an hour ago, good for a week.
    pub fn fresh(&self, store_head: &str) -> String {
        self.fresh_at(store_head, now() - 3600, 7 * DAY)
    }

    pub fn join_store(&self, store: &Store) {
        let (epoch, h) = self.org.head_pair();
        let req = JoinRequest { url: self.remote.url(), admin_fingerprint: self.org.fingerprint("alice"), head_epoch: epoch, head_hash: h };
        pins::join(&store.pin_storage(), &req, &self.org.files).unwrap();
    }

    pub fn join(&self) {
        self.join_store(&self.store);
    }

    pub fn sync(&self) -> VerifiedSync {
        self.store.sync_verified(&cfg()).unwrap()
    }

    pub fn sync_at(&self, now: i64) -> VerifiedSync {
        self.store.sync_verified(&VerifyConfig::at(now)).unwrap()
    }

    /// An install that has never synced, joined at the org's current head.
    pub fn fresh_install(&self, name: &str) -> Store {
        let home = self.dir.join(name);
        std::fs::create_dir_all(&home).unwrap();
        let s = Store::open(&home, ORG, STORE, &self.remote.url(), true).unwrap();
        self.join_store(&s);
        s
    }

    /// The baseline most tests start from: `records` and the roster in one commit, a fresh statement in the next, the
    /// install joined and synced to that. Returns the fixture and the commit the records are in.
    pub fn installed(dir: &std::path::Path, records: &[&SignedRecord]) -> (Fx, String) {
        Fx::installed_on(Fx::with_cast(dir), records)
    }

    pub fn installed_on(fx: Fx, records: &[&SignedRecord]) -> (Fx, String) {
        for r in records {
            fx.put(r);
        }
        let c1 = fx.commit("c1");
        fx.fresh(&c1);
        fx.join();
        let r = fx.sync();
        assert!(matches!(r, VerifiedSync::Accepted { .. }), "baseline sync: {r:?}");
        (fx, c1)
    }

    /// Commit a new freshness statement (for the commit before it) and sync it.
    pub fn refresh_and_sync(&self) -> VerifiedSync {
        let head = self.remote.commit("noop");
        self.fresh(&head);
        self.sync()
    }
}

/// A `memory` record whose text a test controls. `pointers` is the YAML list item text after `pointers:` (for example
/// `[]`). Everything else is as [`rk::memory_with`]; the record is really signed, so it still has to pass X2 to count.
#[allow(clippy::too_many_arguments)]
pub fn memory_text(who: &Identity, id: &str, rev: u64, title: &str, summary: &str, body: &str, tags: &str, ceiling: &str, pointers: &str, status: &str, supersedes: &str) -> SignedRecord {
    let t = format!(
        "---
xenia: 1
id: {id}
kind: memory
rev: {rev}
org: {ORG}
store: {STORE}
author: \"{}\"
author_handle: tester
author_role: builder
title: {title}
summary: {summary}
tags: [{tags}]
visibility: org
created: \"2026-10-24T09:00:00Z\"
updated: \"2026-10-24T09:00:00Z\"
source_ceiling: {ceiling}
confidence: high
status: {status}
pointers:{pointers}
redaction:
  scanner: rev-sec-05
  scanner_version: \"1\"
  result: passed
  content_hash: \"{}\"
supersedes: {supersedes}
review_by: \"2027-01-24\"
evidence: []
related: []
body_kind: markdown
memory_type: gotcha
applies_to:
  repos: [payments]
  tech: [stripe]
  paths: []
verified_by: none
---
{body}
",
        who.did(),
        hash::of(b"scanned")
    );
    rk::sign_record(who, delphi_xenia::layout::Kind::Memory, id, t.into_bytes())
}

/// An endorsement of exactly `target`'s bytes by `who`, scoped to `scope` (comma-separated).
pub fn endorse(who: &Identity, eid: usize, target: &SignedRecord, target_rev: u64, scope: &str) -> SignedRecord {
    rk::endorsement_with(who, &u(eid), 1, &target.id, target_rev, &target.hash(), "endorse", scope)
}
