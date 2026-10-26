//! `xenia-testkit`: builds signed rosters for tests (here, and in other crates through the `test-support`
//! feature). Test-only: it holds private keys in plain memory and will sign anything it is told to, which is why
//! it is not compiled into a normal build.
//!
//! The pieces, smallest first:
//!
//! - [`key`] a deterministic [`Identity`] from a one-byte seed.
//! - [`MemberSpec`] / [`EpochSpec`] describe a roster entry / epoch file; `to_json` renders the exact bytes and
//!   [`EpochSpec::sign`] produces the [`EpochFile`] (bytes + detached signatures) any chosen keys would produce.
//!   Because specs are plain data you can mutate them into any attack: wrong `prev_hash`, dropped entry, changed
//!   `person`, an admin set that approves itself.
//! - [`TestOrg`] is a named cast of keys plus a growing chain. [`TestOrg::three_admins`] is the standard fixture
//!   (alice, bob, carol: admins, 2-of-3; dave: steward of `rust`; erin: plain member). [`TestOrg::advance`] adds an
//!   epoch from a closure and a list of signer names; [`TestOrg::rotate`], [`TestOrg::fork_at`] and
//!   [`TestOrg::takeover_with`] build rotations, forks and takeovers. [`TestOrg::pin_head`] / [`TestOrg::anchor_all`]
//!   give the trust anchors a joiner or an existing install would hold.
//!
//! Nothing here is verified by the builder: it will happily produce an invalid chain. That is the point.

use crate::identity::Identity;
use crate::roster::{Anchor, EpochFile, VerifiedRoster};
use crate::sig::Domain;
use crate::{did, hash, roster};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const ORG: &str = "acme";
pub const STORE: &str = "acme-main";
pub const TS: &str = "2026-10-24T09:00:00Z";

/// A deterministic identity: the same seed byte always gives the same DID.
pub fn key(seed: u8) -> Identity {
    Identity::from_seed([seed; 32])
}

/// One roster entry as a test writes it. Defaults: an `active` plain member.
#[derive(Debug, Clone)]
pub struct MemberSpec {
    pub did: String,
    pub person: String,
    pub handle: String,
    pub roles: Vec<&'static str>,
    pub steward_areas: Vec<String>,
    pub added_at: String,
    pub status: &'static str,
    pub successor: Option<String>,
    pub removed_at_epoch: Option<u64>,
    pub voids_tombstones: Option<bool>,
}

impl MemberSpec {
    pub fn new(id: &Identity, person: &str) -> MemberSpec {
        MemberSpec {
            did: id.did(),
            person: person.to_string(),
            handle: person.to_string(),
            roles: vec!["member"],
            steward_areas: vec![],
            added_at: TS.to_string(),
            status: "active",
            successor: None,
            removed_at_epoch: None,
            voids_tombstones: None,
        }
    }
    pub fn admin(mut self) -> MemberSpec {
        self.roles = vec!["member", "admin"];
        self
    }
    pub fn steward(mut self, areas: &[&str]) -> MemberSpec {
        self.steward_areas = areas.iter().map(|a| a.to_string()).collect();
        self
    }
    pub fn to_json(&self) -> Value {
        let mut m = json!({
            "did": self.did, "person": self.person, "handle": self.handle, "roles": self.roles,
            "steward_areas": self.steward_areas, "added_at": self.added_at, "status": self.status,
        });
        let o = m.as_object_mut().unwrap();
        if let Some(s) = &self.successor {
            o.insert("successor".into(), json!(s));
        }
        if let Some(n) = self.removed_at_epoch {
            o.insert("removed_at_epoch".into(), json!(n));
        }
        if let Some(v) = self.voids_tombstones {
            o.insert("voids_tombstones".into(), json!(v));
        }
        m
    }
}

/// One epoch file as a test writes it.
#[derive(Debug, Clone)]
pub struct EpochSpec {
    pub org: String,
    pub store: String,
    pub epoch: u64,
    pub prev_hash: Option<String>,
    pub members: Vec<MemberSpec>,
}

impl EpochSpec {
    pub fn to_json(&self) -> String {
        let v = json!({
            "org": self.org, "store": self.store, "epoch": self.epoch, "prev_hash": self.prev_hash,
            "members": self.members.iter().map(MemberSpec::to_json).collect::<Vec<_>>(),
        });
        let mut s = serde_json::to_string_pretty(&v).unwrap();
        s.push('\n');
        s
    }

    pub fn hash(&self) -> String {
        hash::of(self.to_json().as_bytes())
    }

    pub fn member_mut(&mut self, id: &Identity) -> &mut MemberSpec {
        let d = id.did();
        self.members.iter_mut().find(|m| m.did == d).expect("no such member in the draft")
    }

    /// The file plus a signatures file signed by exactly `signers`, over this epoch's own bytes.
    pub fn sign(&self, signers: &[&Identity]) -> EpochFile {
        sign_bytes(&self.org, &self.store, self.epoch, self.to_json().as_bytes(), signers)
    }
}

/// Sign arbitrary epoch bytes (for tests that edit the JSON text directly). `epoch` goes in the domain and in
/// the signatures file.
pub fn sign_bytes(org: &str, store: &str, epoch: u64, bytes: &[u8], signers: &[&Identity]) -> EpochFile {
    let domain = Domain::roster(org, store, epoch, &hash::of(bytes)).unwrap();
    let sigs: Vec<(String, String)> = signers.iter().map(|s| (s.did(), s.sign(&domain))).collect();
    EpochFile { epoch_json: bytes.to_vec(), sigs_json: sigs_json(epoch, &sigs).into_bytes() }
}

pub fn sigs_json(epoch: u64, sigs: &[(String, String)]) -> String {
    let list: Vec<Value> = sigs.iter().map(|(k, s)| json!({ "kid": k, "sig": s })).collect();
    serde_json::to_string_pretty(&json!({ "epoch": epoch, "signatures": list })).unwrap() + "\n"
}

/// A named cast of keys and the chain built so far. Names map to a fixed seed, so a [`TestOrg`] and its clones
/// share identities.
#[derive(Debug, Clone)]
pub struct TestOrg {
    pub org: String,
    pub store: String,
    seeds: BTreeMap<String, u8>,
    /// The spec of every epoch, `specs[i]` is epoch `i + 1`.
    pub specs: Vec<EpochSpec>,
    /// The signed files, parallel to `specs`: this is what a store would hold and what verification takes.
    pub files: Vec<EpochFile>,
}

impl TestOrg {
    /// Empty cast, no chain yet.
    pub fn empty() -> TestOrg {
        TestOrg { org: ORG.into(), store: STORE.into(), seeds: BTreeMap::new(), specs: vec![], files: vec![] }
    }

    /// Register (or look up) a key by name.
    pub fn who(&mut self, name: &str) -> Identity {
        let next = self.seeds.len() as u8 + 1;
        key(*self.seeds.entry(name.to_string()).or_insert(next))
    }

    /// A key registered earlier. Panics on an unknown name so a typo in a test cannot silently mint a stranger.
    pub fn key(&self, name: &str) -> Identity {
        key(*self.seeds.get(name).unwrap_or_else(|| panic!("unknown key {name}")))
    }

    pub fn did(&self, name: &str) -> String {
        self.key(name).did()
    }

    pub fn fingerprint(&self, name: &str) -> String {
        did::fingerprint(&self.did(name)).unwrap()
    }

    /// The standard fixture: genesis with admins alice, bob, carol (people `alice`, `bob`, `carol`; 2-of-3),
    /// dave (steward of `rust`) and erin (plain member), signed by all three founding admins.
    pub fn three_admins() -> TestOrg {
        let mut t = TestOrg::empty();
        let (a, b, c, d, e) = (t.who("alice"), t.who("bob"), t.who("carol"), t.who("dave"), t.who("erin"));
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
            ],
        };
        t.push_signed(genesis, &["alice", "bob", "carol"]);
        t
    }

    pub fn head_spec(&self) -> &EpochSpec {
        self.specs.last().expect("no epochs yet")
    }

    /// A copy of the head with the next number and `prev_hash` linked: edit it, then [`TestOrg::push_signed`].
    pub fn draft(&self) -> EpochSpec {
        let head = self.head_spec();
        EpochSpec { epoch: head.epoch + 1, prev_hash: Some(head.hash()), ..head.clone() }
    }

    /// Append `spec`, signed by the named keys.
    pub fn push_signed(&mut self, spec: EpochSpec, signers: &[&str]) {
        let keys: Vec<Identity> = signers.iter().map(|n| self.key(n)).collect();
        let refs: Vec<&Identity> = keys.iter().collect();
        self.files.push(spec.sign(&refs));
        self.specs.push(spec);
    }

    /// The next epoch: `edit` changes a draft of the head, the named keys sign it.
    pub fn advance(&mut self, signers: &[&str], edit: impl FnOnce(&mut TestOrg, &mut EpochSpec)) {
        let mut spec = self.draft();
        edit(self, &mut spec);
        self.push_signed(spec, signers);
    }

    /// In `spec`, retire `old` with `successor = new` and add `new` as an `active` entry with the same person,
    /// roles and areas. `compromised` sets `voids_tombstones` (mandatory for a compromise, spec 011 §8.2).
    pub fn rotate(&mut self, spec: &mut EpochSpec, old: &str, new: &str, compromised: bool) {
        let new_id = self.who(new);
        let old_did = self.did(old);
        let o = spec.members.iter_mut().find(|m| m.did == old_did).expect("old key is not in the draft");
        o.status = "retired";
        o.successor = Some(new_id.did());
        if compromised {
            o.voids_tombstones = Some(true);
        }
        let mut fresh = o.clone();
        fresh.did = new_id.did();
        fresh.status = "active";
        fresh.successor = None;
        fresh.voids_tombstones = None;
        fresh.added_at = TS.to_string();
        spec.members.push(fresh);
    }

    /// The first `n` epochs only: the state of the store (or an install's view) at epoch `n`.
    pub fn upto(&self, n: usize) -> TestOrg {
        let mut t = self.clone();
        t.specs.truncate(n);
        t.files.truncate(n);
        t
    }

    /// A fork at epoch `n`: the chain up to `n - 1`, then `edit` builds a *different* epoch `n` signed by
    /// `signers`. The original and the fork share every epoch before `n`.
    pub fn fork_at(&self, n: usize, signers: &[&str], edit: impl FnOnce(&mut TestOrg, &mut EpochSpec)) -> TestOrg {
        let mut t = self.upto(n - 1);
        t.advance(signers, edit);
        t
    }

    /// A takeover: build an alternative epoch `n` on top of the same earlier chain, signed by keys that were
    /// `active` admins earlier but are not in the real chain's epoch `n - 1` (for example retired founders).
    /// Same as [`TestOrg::fork_at`] but named for what the test models.
    pub fn takeover_with(&self, n: usize, retired_signers: &[&str], edit: impl FnOnce(&mut TestOrg, &mut EpochSpec)) -> TestOrg {
        self.fork_at(n, retired_signers, edit)
    }

    /// Anchor holding the head pair only: what a joiner is handed.
    pub fn pin_head(&self) -> Anchor {
        let h = self.head_spec();
        Anchor::new([(h.epoch, h.hash())]).unwrap()
    }

    /// The `(epoch, hash)` pair an existing member reads out to a joiner.
    pub fn head_pair(&self) -> (u64, String) {
        let h = self.head_spec();
        (h.epoch, h.hash())
    }

    /// Anchor holding every epoch: what an install that has accepted the whole chain holds.
    pub fn anchor_all(&self) -> Anchor {
        Anchor::new(self.specs.iter().map(|s| (s.epoch, s.hash()))).unwrap()
    }

    /// Verify the files against the head pair.
    pub fn verify(&self) -> roster::RResult<VerifiedRoster> {
        roster::verify_chain(&self.org, &self.store, &self.files, &self.pin_head())
    }
}
