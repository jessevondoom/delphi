//! The `bundle` record kind on this install's side (spec 027 §4.3; slice B8): what a store's bundle records say at the
//! accepted head, how they stand, and how one is written for publishing. Nothing here fetches a tree (that is
//! `delphi bundle add --from-xenia`, which runs the same hardened fetch as a marketplace add) and nothing here
//! decides to add anything.
//!
//! * **Reading.** [`BundleSet::offline`] is `xenia_index::derive` over the accepted head: every record is verified
//!   again from its bytes, tracked by the sync, and judged by X5's pure `tier::compute` against the verified roster and
//!   the freshness statement. A record's own text never states a level: [`StoreBundle::level`] is `endorsed` only while
//!   a steward of another person's endorsement of exactly these bytes counts under a current statement, else
//!   `member-signed`, and `hidden` once an author or covering steward withdrew it (spec 024 §2.2).
//! * **Who may sign the tree.** A record is signed by a roster member. The bundle's own `sig.json` must be by that
//!   member or by a key in the member's `successor` chain, in either direction ([`StoreBundle::signer_dids`]);
//!   anything else is refused at add (spec 027 §4.3). The same chain is what lets a rotated publisher key continue the
//!   key the install pinned when it first added the bundle ([`StoreBundle::predecessors`]; spec 024 §2.3).
//! * **Writing.** [`BundleDraft`] is what a publisher supplies; the publish gate (`xenia_publish`) stamps the rest,
//!   scans, previews the exact bytes, asks once and signs. A bundle record has no content source, so it is published
//!   from a command, like an endorsement.

use crate::xenia_ingress::sanitize;
use crate::xenia_publish::{Item, Origin};
use crate::xenia_store::Store;
use crate::xenia_sync::{VerifiedSync, VerifyConfig};
use delphi_xenia::hash;
use delphi_xenia::pointer::Pointer;
use delphi_xenia::record::{AuthorStatus, BodyKind, BundleFields, Confidence, KindFields, Record, Redaction, Role, SourceCeiling};
use delphi_xenia::roster::VerifiedRoster;
use delphi_xenia::tier::{BundleLevel, MemoryState, Tier, VerifiedRecord, Visibility};
use std::path::Path;

/// One bundle record as the accepted head has it. Text fields have been through the ingress sanitizer.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreBundle {
    pub id: String,
    pub rev: u64,
    /// `blake3:` of the record file: what an endorsement binds to.
    pub record_hash: String,
    /// The DID that signed the record.
    pub signer: String,
    pub publisher_handle: String,
    pub name: String,
    pub version: String,
    /// `blake3:` of the bundle's `files.json`; the fetched tree must hash to exactly this.
    pub bundle_hash: String,
    pub tree: Pointer,
    pub roles: Vec<String>,
    pub connectors: Vec<String>,
    pub compat_delphi: Option<String>,
    pub title: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub level: BundleLevel,
    /// The tier after the endorsement rules and the author's `source_ceiling` (a bundle published from a span that read
    /// untrusted data cannot index above its ceiling, spec 024 §3).
    pub tier: Tier,
    pub clamped_by_ceiling: bool,
    /// Handles of the stewards whose endorsement counts.
    pub endorsed_by: Vec<String>,
    /// Handles who would count if the store's freshness statement were current.
    pub withheld_by_staleness: Vec<String>,
    pub disputed: bool,
    /// The slug of why it is not offered (`author-withdrawn`, `steward-tombstone`, ...), or `None` when visible.
    pub hidden: Option<&'static str>,
    /// Every DID whose `sig.json` the record's signer accepts: the signer and its `successor` chain in either direction.
    pub signer_dids: Vec<String>,
    /// DIDs the signer continues (the keys whose `successor` chain leads to the signer), so a pinned older key is
    /// continued by the signer without the bundle's manifest listing it.
    pub predecessors: Vec<String>,
    pub updated: String,
}

fn hidden_slug(h: &delphi_xenia::tier::HiddenBy) -> &'static str {
    use delphi_xenia::tier::HiddenBy::*;
    match h {
        AuthorSuperseded => "author-superseded",
        AuthorWithdrawn => "author-withdrawn",
        AuthorTombstone => "author-tombstone",
        StewardTombstone => "steward-tombstone",
        SensitiveHold => "sensitive-hold",
        Contested(_) => "contested",
    }
}

/// `did`'s relatives in the roster's `successor` chains: `(acceptable bundle signers, keys it continues)`.
fn relatives(roster: &VerifiedRoster, did: &str) -> (Vec<String>, Vec<String>) {
    let mine = roster.successor_chain(did);
    let mut ok: Vec<String> = mine.clone();
    let mut before = Vec::new();
    for m in &roster.head().epoch.members {
        if m.did != did && roster.successor_chain(&m.did).iter().any(|d| d == did) {
            before.push(m.did.clone());
        }
    }
    ok.extend(before.iter().cloned());
    ok.sort();
    ok.dedup();
    before.sort();
    (ok, before)
}

/// The view of a verified `bundle` record. `None` for any other kind.
pub fn store_bundle(roster: &VerifiedRoster, vr: &VerifiedRecord, st: &MemoryState) -> Option<StoreBundle> {
    let rec = vr.record();
    let KindFields::Bundle(b) = &rec.kind_fields else { return None };
    let level = match &st.visibility {
        Visibility::Hidden(_) => BundleLevel::Hidden,
        Visibility::Visible if !st.endorsed_by.is_empty() => BundleLevel::Endorsed,
        Visibility::Visible => BundleLevel::MemberSigned,
    };
    let handle = |did: &str| roster.member(did).map(|m| m.handle.clone()).unwrap_or_default();
    let (signer_dids, predecessors) = relatives(roster, vr.signer());
    let one = |s: &str| sanitize(s).split_whitespace().collect::<Vec<_>>().join(" ");
    Some(StoreBundle {
        id: rec.id.clone(),
        rev: rec.rev,
        record_hash: vr.hash().to_string(),
        signer: vr.signer().to_string(),
        publisher_handle: one(&handle(vr.signer())),
        name: b.bundle_name.clone(),
        version: b.bundle_version.clone(),
        bundle_hash: b.bundle_hash.clone(),
        tree: b.tree.clone(),
        roles: b.touches_roles.iter().map(|r| r.as_str().to_string()).collect(),
        connectors: b.touches_connectors.clone(),
        compat_delphi: b.compat_delphi.clone(),
        title: one(&rec.title),
        summary: one(&rec.summary),
        tags: rec.tags.iter().map(|t| one(t)).collect(),
        level,
        tier: st.tier,
        clamped_by_ceiling: st.clamped_by_ceiling,
        endorsed_by: st.endorsed_by.iter().map(|e| one(&handle(&e.did))).collect(),
        withheld_by_staleness: st.withheld_by_staleness.iter().map(|e| one(&handle(&e.did))).collect(),
        disputed: st.disputed(),
        hidden: match &st.visibility {
            Visibility::Hidden(h) => Some(hidden_slug(h)),
            Visibility::Visible => None,
        },
        signer_dids,
        predecessors,
        updated: rec.updated.clone(),
    })
}

/// Every bundle record of one store at its accepted head.
#[derive(Debug, Clone, PartialEq)]
pub struct BundleSet {
    pub org: String,
    pub store: String,
    pub head: String,
    /// False when no endorsement counts now (the freshness statement is missing, expired or replayed).
    pub fresh: bool,
    /// Sorted by id.
    pub bundles: Vec<StoreBundle>,
}

impl BundleSet {
    /// From the accepted head only (no network): the state the last verified sync left.
    pub fn offline(store: &Store, cfg: &VerifyConfig) -> Result<BundleSet, String> {
        let d = crate::xenia_index::derive(store, cfg).map_err(|e| e.to_string())?;
        let fresh = matches!(d.freshness, delphi_xenia::freshness::FreshnessState::Fresh { .. });
        Ok(BundleSet { org: d.org, store: d.store, head: d.head, fresh, bundles: d.bundles })
    }

    pub fn find(&self, id: &str) -> Option<&StoreBundle> {
        self.bundles.iter().find(|b| b.id == id)
    }

    /// The visible records that name a bundle `name` (more than one is worth a warning: names are not owned).
    pub fn named(&self, name: &str) -> Vec<&StoreBundle> {
        self.bundles.iter().filter(|b| b.name == name && b.hidden.is_none()).collect()
    }
}

/// How a verified sync ended, for a caller that must stop on tampering but may carry on offline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncNote {
    /// The store was reached and its head verified (or was already the accepted head).
    Verified,
    /// The store could not be reached; the last accepted state is what the caller has. The words say why.
    Offline(String),
}

/// A verified sync (spec 011 §3.4, §4): anything but an accepted or unchanged head, or an unreachable host, is a refusal
/// and changes nothing. Tampering is never softened into "offline".
pub fn sync(store: &Store, cfg: &VerifyConfig) -> Result<SyncNote, String> {
    match store.sync_verified(cfg).map_err(|e| e.to_string())? {
        VerifiedSync::Accepted { .. } | VerifiedSync::UpToDate { .. } => Ok(SyncNote::Verified),
        VerifiedSync::Unavailable(why) => Ok(SyncNote::Offline(why)),
        VerifiedSync::Tampering { findings, .. } => Err(format!("the store shows tampering in {} path(s); nothing was applied (run `delphi xenia status`)", findings.len())),
        VerifiedSync::Rejected { reason, .. } => Err(format!("the store was rejected ({reason}); nothing was applied")),
        VerifiedSync::NonFastForward(_) => Err("the store's history is not a continuation of what this install accepted; nothing was applied".into()),
        VerifiedSync::Refused(r) => Err(format!("the store was refused ({}); nothing was applied", r.reason())),
    }
}

/// The store `org/store` this install joined: its URL comes from the pins the operator verified at join, never from a
/// bundle, a lock or a record. A local-path store (a shared drive) is admitted exactly as `delphi xenia` admits it: it is the
/// URL the operator gave at join, with `--allow-local`.
pub fn open_joined(home: &Path, org: &str, store: &str) -> Result<Store, String> {
    let paths = delphi_xenia::store::StorePaths::new(home, org, store).map_err(|e| e.to_string())?;
    let pins = delphi_xenia::pins::load(&delphi_xenia::pins::FilePinStorage::new(paths.state.join("pins.toml"))).map_err(|e| e.reason().to_string())?.ok_or_else(|| format!("{org}/{store} has not been joined on this install"))?;
    if pins.org() != org || pins.store() != store {
        return Err("the pins belong to another store".into());
    }
    let url = pins.url();
    Store::open(home, org, store, url, !(url.starts_with("https://") || url.starts_with("ssh://"))).map_err(|e| e.to_string())
}

/// Where a bundle added from a store records its source in the lock: `xenia:<org>/<store>/<record id>@<rev>`.
pub fn source_path(org: &str, store: &str, id: &str, rev: u64) -> String {
    format!("xenia:{org}/{store}/{id}@{rev}")
}

/// The parts of a lock source path written by [`source_path`]; `None` for anything else.
pub fn parse_source_path(p: &str) -> Option<(String, String, String, u64)> {
    let rest = p.strip_prefix("xenia:")?;
    let (loc, rev) = rest.rsplit_once('@')?;
    let mut it = loc.split('/');
    let (org, store, id) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || !delphi_xenia::is_slug(org, 32) || !delphi_xenia::is_slug(store, 32) || !delphi_xenia::ulid::is_valid(id) {
        return None;
    }
    let rev: u64 = rev.parse().ok().filter(|r| (1..=1_000_000_000).contains(r) && rev.bytes().all(|b| b.is_ascii_digit()) && !rev.starts_with('0'))?;
    Some((org.to_string(), store.to_string(), id.to_string(), rev))
}

/// What a person supplies to publish a bundle (the rest is the gate's: org, store, author, times, id, rev, redaction).
#[derive(Debug, Clone)]
pub struct BundleDraft {
    /// Set to publish a new `rev` of a record this person already published.
    pub id: Option<String>,
    pub name: String,
    pub version: String,
    pub bundle_hash: String,
    pub tree: Pointer,
    pub roles: Vec<Role>,
    pub connectors: Vec<String>,
    pub compat_delphi: Option<String>,
    pub title: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub confidence: Confidence,
    /// Stamped from the publisher's span ceilings, never chosen by an agent.
    pub source_ceiling: SourceCeiling,
    pub author_role: Role,
    /// Release notes for the people deciding to add it; may be empty.
    pub body: String,
    pub review_by: Option<String>,
    pub expires: Option<String>,
    pub status: AuthorStatus,
}

impl BundleDraft {
    pub fn into_item(self) -> Item {
        let body = self.body.replace("\r\n", "\n");
        let record = Record {
            id: self.id.unwrap_or_default(),
            rev: 0,
            org: String::new(),
            store: String::new(),
            author: String::new(),
            author_handle: String::new(),
            author_role: self.author_role,
            title: self.title,
            summary: self.summary,
            tags: self.tags,
            created: String::new(),
            updated: String::new(),
            source_ceiling: self.source_ceiling,
            confidence: self.confidence,
            status: self.status,
            supersedes: None,
            review_by: self.review_by,
            expires: self.expires,
            pointers: Vec::new(),
            evidence: Vec::new(),
            related: Vec::new(),
            redaction: Redaction { scanner: String::new(), scanner_version: String::new(), content_hash: hash::of(b"unsealed") },
            body_kind: if body.is_empty() { BodyKind::None } else { BodyKind::Markdown },
            kind_fields: KindFields::Bundle(BundleFields {
                bundle_name: self.name,
                bundle_version: self.version,
                bundle_hash: self.bundle_hash,
                tree: self.tree,
                touches_roles: self.roles,
                touches_connectors: self.connectors,
                compat_delphi: self.compat_delphi,
                compat_schema: 1,
            }),
            body,
        };
        Item { origin: Origin::Command, record }
    }
}

#[cfg(test)]
#[path = "xenia_bundle_tests.rs"]
mod gate_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RA";

    #[test]
    fn a_publishers_rotated_key_continues_the_old_one_and_nobody_else_does() {
        use delphi_xenia::testkit::TestOrg;
        let mut t = TestOrg::three_admins();
        t.advance(&["alice", "bob"], |t, spec| t.rotate(spec, "dave", "dave_new", false));
        let roster = t.verify().unwrap();
        let (dave, new, erin) = (t.did("dave"), t.did("dave_new"), t.did("erin"));
        // The new key may sign the tree, and so may the old one (a bundle signed before the rotation); it continues the old.
        let (ok, before) = relatives(&roster, &new);
        assert!(ok.contains(&dave) && ok.contains(&new) && !ok.contains(&erin), "{ok:?}");
        assert_eq!(before, vec![dave.clone()]);
        // The old (retired) key continues nobody; its chain still names the new one.
        let (ok, before) = relatives(&roster, &dave);
        assert!(ok.contains(&new) && before.is_empty());
        // A plain member's chain is only itself.
        assert_eq!(relatives(&roster, &erin).0, vec![erin.clone()]);
        // A stranger is in nobody's chain.
        let stranger = delphi_xenia::testkit::key(99).did();
        assert!(!relatives(&roster, &new).0.contains(&stranger));
    }

    #[test]
    fn source_paths_round_trip_and_nothing_else_parses() {
        let p = source_path("acme", "acme-main", ID, 3);
        assert_eq!(parse_source_path(&p), Some(("acme".into(), "acme-main".into(), ID.into(), 3)));
        for bad in [
            "xenia:acme/acme-main/x@3",
            &format!("xenia:acme/acme-main/{ID}@0"),
            &format!("xenia:acme/acme-main/{ID}@03"),
            &format!("xenia:acme/acme-main/{ID}@-1"),
            &format!("xenia:acme/acme-main/{ID}@1000000001"),
            &format!("xenia:acme/../{ID}@3"),
            &format!("xenia:Acme/acme-main/{ID}@3"),
            &format!("xenia:acme/acme-main/{ID}/extra@3"),
            &format!("directory:acme/acme-main/{ID}@3"),
            &format!("xenia:acme/acme-main/{ID}"),
            "",
        ] {
            assert_eq!(parse_source_path(bad), None, "{bad}");
        }
    }
}
