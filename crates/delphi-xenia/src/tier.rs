//! Standing, endorsements, tombstones and tiers (spec 011 §2.2-§2.3, §3.4.3, §3.4.5-6, §5.1, §7.3-§7.4; spec 012
//! §5.2-§5.3; milestone X5).
//!
//! **A pure function.** [`compute`] takes the records that already passed signature and structure
//! ([`VerifiedRecord`], built only from a [`Validated`]), the verified roster, the freshness state and the tag
//! history the caller keeps, and returns, for every record, an [`Outcome`], and for every `memory` its
//! visibility and tier. No I/O, no clock (freshness was judged with `now` by `freshness::assess`), no
//! network, no dependence on the order of the input or on git commit order: everything is sorted and every tie
//! has a stated rule that never reads a ULID, a timestamp or a `signed_at` for effect.
//!
//! What each stage does, in order. A record dropped at a stage never reaches the next, so a hostile file cannot
//! use a rule meant for good files:
//!
//! 1. **Standing** (spec 011 §3.4.3): the signer is in the head roster and `active`. One fail-hidden exception: a
//!    record that can only hide or demote (a `withdraw` tombstone, an endorsement `withdraw` or `dispute`)
//!    from a retired or removed DID still counts, unless that entry sets `voids_tombstones`.
//! 2. **Id uniqueness across kinds** (spec 012 test 5): an id that appears under more than one kind is refused in
//!    every kind. No kind wins, because any winner rule would let a member displace someone else's record by
//!    copying its id.
//! 3. **Duplicates** (spec 011 §3.4.5): one endorsement per (signer, target `id@rev`) and one tombstone per
//!    (signer, target); the smallest ULID survives. A tie-break only.
//! 4. **Tombstones** (spec 012 §5.3): who may sign (the author's person, or a steward whose areas cover the
//!    **union** of the target's tags over all revisions), then the effective status: the author decides, except
//!    that a steward's `sensitive` withdraw is reversed only by that steward; otherwise stewards must agree,
//!    else `contested` (hidden).
//! 5. **Endorsements** (spec 011 §7.3): steward, areas cover the exact revision's tags, scope inside areas and
//!    tags, `target_hash` equal to the revision's bytes, not the author's person, counted once per person, and
//!    only while the freshness statement is current. A `withdraw` or `dispute` from the same person cancels
//!    that person's endorsement; a `dispute` from anyone else flags the record and changes no tier.
//! 6. **Tier**: `L2` for an endorsed visible `memory`, else `L1`, clamped to the author's `source_ceiling`;
//!    a hidden record is `L0`.
//!
//! What this module cannot see, because it only sees the tip: who created an `id` first and whether a `rev`
//! went backwards. Those are history rules and live in `walk` and `sticky`; this module adds the rule they
//! share ([`author_transition_allowed`]) and the cross-record checks that need the whole tip (target resolution,
//! uniqueness across kinds). The tags of earlier revisions are an input ([`Inputs::prior_tags`]) for the same
//! reason.

use crate::freshness::FreshnessState;
use crate::layout::Kind;
use crate::record::{AuthorStatus, KindFields, Record, SourceCeiling, TombstoneAction, TombstoneReason, Validated, Verdict};
use crate::roster::{Areas, VerifiedRoster};
use std::collections::{BTreeMap, BTreeSet};

/// Spec 012 §2.1: the author's own state machine. A new `rev` may move `active -> superseded | withdrawn` or stay
/// `active`; `withdrawn` is terminal.
///
/// **Decision (spec gap, recorded in the plan):** 012 test 7 forbids only `withdrawn -> active`, and X4b left
/// `superseded -> active` allowed. The rule text lists no move out of `superseded` at all, and an author-hidden
/// record that could come back unreviewed is the resurfacing the rotation rules go out of their way to prevent,
/// so it is refused here too: nothing hidden by its author returns to `active`. Staying `superseded` and
/// `superseded -> withdrawn` (both stay hidden) are allowed; `withdrawn` only stays `withdrawn`.
pub fn author_transition_allowed(old: AuthorStatus, new: AuthorStatus) -> bool {
    use AuthorStatus::{Active, Superseded, Withdrawn};
    matches!((old, new), (Active, _) | (Superseded, Superseded | Withdrawn) | (Withdrawn, Withdrawn))
}

/// A record that passed signature, structure and cross-checks, and whose bytes hash to `hash`. Standing is not
/// part of it: [`compute`] decides that against the roster. Obtainable only from a [`Validated`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRecord {
    record: Record,
    signer: String,
    hash: String,
}

impl VerifiedRecord {
    pub fn from_validated(v: &Validated) -> VerifiedRecord {
        VerifiedRecord { record: v.record().clone(), signer: v.sidecar().kid.clone(), hash: v.sidecar().hash.clone() }
    }

    pub fn record(&self) -> &Record {
        &self.record
    }

    /// The sidecar `kid`: who signed.
    pub fn signer(&self) -> &str {
        &self.signer
    }

    /// `blake3:` of the record file's raw bytes (verified against the sidecar before this type existed).
    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn path(&self) -> String {
        format!("records/{}/{}.md", self.record.kind().as_str(), self.record.id)
    }

    /// Can only hide or demote (spec 011 §3.4.3): the fail-hidden class.
    pub fn hide_only(&self) -> bool {
        match &self.record.kind_fields {
            KindFields::Tombstone(t) => t.action == TombstoneAction::Withdraw,
            KindFields::Endorsement(e) => matches!(e.verdict, Verdict::Withdraw | Verdict::Dispute),
            KindFields::Memory(_) => false,
        }
    }
}

/// Trust tiers a Xenia record can have (spec 011 §5.1). Never above `L2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    L0,
    L1,
    L2,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::L0 => "L0",
            Tier::L1 => "L1",
            Tier::L2 => "L2",
        }
    }

    pub fn source_type(self) -> &'static str {
        match self {
            Tier::L0 => "external_untrusted",
            Tier::L1 => "federated_peer",
            Tier::L2 => "federated_verified",
        }
    }
}

/// The anti-laundering clamp (spec 011 §5.1): the author's `source_ceiling` on the shared lattice, and the
/// receiver's own xenia ceiling of `L2` regardless.
fn ceiling_cap(c: SourceCeiling) -> Tier {
    match c {
        SourceCeiling::ExternalUntrusted => Tier::L0,
        SourceCeiling::FederatedPeer => Tier::L1,
        _ => Tier::L2,
    }
}

/// What became of one record. The reasons are fixed slugs, never store text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It does what it says (a `memory` that is in force; an endorsement that counts; a tombstone that decides).
    Effective,
    /// Valid, but it changes nothing now (target missing or of another kind, bound to another revision, the
    /// author's own endorsement, freshness stale, ...). It may start to count when its target arrives.
    NoEffect(&'static str),
    /// Not valid at this install (no standing, a role it may not use, outside its areas, a duplicate, a
    /// collision). Listed by `status`; never loaded.
    Quarantined(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HiddenBy {
    AuthorSuperseded,
    AuthorWithdrawn,
    /// The author's own `withdraw` tombstone.
    AuthorTombstone,
    /// Covering stewards withdrew it and none disagrees.
    StewardTombstone,
    /// A steward's `sensitive` withdraw that only that steward can reverse.
    SensitiveHold,
    /// Stewards disagree: hidden and flagged to humans (spec 012 §5.3). Carries the stewards' persons.
    Contested(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    Visible,
    Hidden(HiddenBy),
}

impl Visibility {
    pub fn is_hidden(&self) -> bool {
        matches!(self, Visibility::Hidden(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endorser {
    pub person: String,
    /// The DID that signed (the lowest DID of the person when several signed).
    pub did: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispute {
    pub person: String,
    pub did: String,
    /// False when it names an earlier revision or other bytes: kept as history, not as a warning on this one.
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryState {
    pub id: String,
    pub rev: u64,
    pub hash: String,
    pub signer: String,
    pub author_person: String,
    pub visibility: Visibility,
    /// The tier after the endorsement rules, before the ceiling clamp.
    pub class_tier: Tier,
    /// The tier consumers use: `class_tier` clamped to `source_ceiling`; `L0` while hidden.
    pub tier: Tier,
    pub clamped_by_ceiling: bool,
    /// People whose endorsement counts (sorted).
    pub endorsed_by: Vec<Endorser>,
    /// People who would count if the freshness statement were current (non-empty only while it is not).
    pub withheld_by_staleness: Vec<Endorser>,
    pub disputes: Vec<Dispute>,
}

impl MemoryState {
    /// Warning flag: someone disputed exactly this revision.
    pub fn disputed(&self) -> bool {
        self.disputes.iter().any(|d| d.current)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Computed {
    /// By `id`.
    pub memories: BTreeMap<String, MemoryState>,
    /// By record path (`records/<kind>/<id>.md`), for every input record.
    pub outcomes: BTreeMap<String, Outcome>,
}

impl Computed {
    pub fn tier_of(&self, id: &str) -> Option<Tier> {
        self.memories.get(id).map(|m| m.tier)
    }

    pub fn outcome(&self, kind: Kind, id: &str) -> Option<&Outcome> {
        self.outcomes.get(&format!("records/{}/{id}.md", kind.as_str()))
    }
}

/// Everything [`compute`] reads.
pub struct Inputs<'a> {
    pub roster: &'a VerifiedRoster,
    pub records: &'a [VerifiedRecord],
    pub freshness: &'a FreshnessState,
    /// For each `memory` id, the union of tags over every revision this install has verified before the one at
    /// the tip (spec 011 §2.3: a tombstone's area check uses all revisions so an author cannot retag to escape a
    /// steward). Empty is safe but weaker: only the tip's tags are then used. The caller keeps it from the
    /// history walk; the tip alone cannot know it.
    pub prior_tags: &'a BTreeMap<String, BTreeSet<String>>,
}

type Dids = BTreeSet<String>;

struct Ctx<'a> {
    roster: &'a VerifiedRoster,
}

impl Ctx<'_> {
    /// Same human (spec 011 §2.1): equal `person`, or one DID in the other's `successor` chain. The chain
    /// check is belt and braces: a roster successor always inherits the person, but this decision should not
    /// rest on that alone.
    fn same_person(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        match (self.roster.person_of(a), self.roster.person_of(b)) {
            (Some(x), Some(y)) if x == y => true,
            _ => self.roster.successor_chain(a).iter().any(|d| d == b) || self.roster.successor_chain(b).iter().any(|d| d == a),
        }
    }

    fn person(&self, did: &str) -> String {
        self.roster.person_of(did).unwrap_or("").to_string()
    }

    /// Is `did`'s tombstone/endorsement dominated: a later DID in its own successor chain has one too? Then the
    /// successor's word replaces the predecessor's (the same person changed their mind after rotating).
    fn dominated(&self, did: &str, others: &Dids) -> bool {
        self.roster.successor_chain(did).iter().skip(1).any(|d| others.contains(d))
    }
}

/// The whole computation. See the module docs for the stages.
pub fn compute(inp: &Inputs) -> Computed {
    let cx = Ctx { roster: inp.roster };
    let mut out = Computed::default();

    // Stage 0: one file per path. The same path twice with different bytes cannot come from a git tree, but the
    // function does not rely on its caller for that.
    let mut by_path: BTreeMap<String, Vec<&VerifiedRecord>> = BTreeMap::new();
    for r in inp.records {
        by_path.entry(r.path()).or_default().push(r);
    }
    let mut live: Vec<&VerifiedRecord> = Vec::new();
    for (path, group) in &by_path {
        let first = group[0];
        if group.iter().all(|g| g.hash == first.hash && g.signer == first.signer) {
            live.push(first);
        } else {
            out.outcomes.insert(path.clone(), Outcome::Quarantined("duplicate-path"));
        }
    }

    // Stage 1: standing.
    let mut standing: Vec<&VerifiedRecord> = Vec::new();
    for r in live {
        let verdict = match inp.roster.member(&r.signer) {
            _ if r.record.author != r.signer => Err("author-mismatch"),
            None => Err("not-a-member"),
            Some(m) if m.is_active() => Ok(()),
            Some(m) if r.hide_only() && !m.voids_tombstones => Ok(()),
            Some(_) if r.hide_only() => Err("signer-voided"),
            Some(_) => Err("not-active"),
        };
        match verdict {
            Ok(()) => standing.push(r),
            Err(why) => {
                out.outcomes.insert(r.path(), Outcome::Quarantined(why));
            }
        }
    }

    // Stage 2: an id lives under one kind only.
    let mut kinds_of: BTreeMap<&str, BTreeSet<&'static str>> = BTreeMap::new();
    for r in &standing {
        kinds_of.entry(r.record.id.as_str()).or_default().insert(r.record.kind().as_str());
    }
    let mut unique: Vec<&VerifiedRecord> = Vec::new();
    for r in standing {
        if kinds_of[r.record.id.as_str()].len() > 1 {
            out.outcomes.insert(r.path(), Outcome::Quarantined("id-collision"));
        } else {
            unique.push(r);
        }
    }

    // Stage 3: duplicates. Smallest ULID survives.
    let mut groups: BTreeMap<(u8, String, String, u64), Vec<&VerifiedRecord>> = BTreeMap::new();
    let mut singles: Vec<&VerifiedRecord> = Vec::new();
    for r in unique {
        match &r.record.kind_fields {
            KindFields::Endorsement(e) => groups.entry((1, r.signer.clone(), e.target_id.clone(), e.target_rev)).or_default().push(r),
            KindFields::Tombstone(t) => groups.entry((2, r.signer.clone(), t.target.clone(), 0)).or_default().push(r),
            KindFields::Memory(_) => singles.push(r),
        }
    }
    for (_, mut g) in groups {
        g.sort_by(|a, b| a.record.id.cmp(&b.record.id));
        for (i, r) in g.into_iter().enumerate() {
            if i == 0 {
                singles.push(r);
            } else {
                out.outcomes.insert(r.path(), Outcome::Quarantined("duplicate"));
            }
        }
    }
    singles.sort_by_key(|r| r.path());

    let memories: BTreeMap<&str, &VerifiedRecord> = singles.iter().filter(|r| r.record.kind() == Kind::Memory).map(|r| (r.record.id.as_str(), *r)).collect();
    let kind_of: BTreeMap<&str, Kind> = singles.iter().map(|r| (r.record.id.as_str(), r.record.kind())).collect();

    // Stage 4: tombstones.
    let mut tomb_by_target: BTreeMap<&str, Vec<TombstoneVote>> = BTreeMap::new();
    for r in singles.iter().filter(|r| r.record.kind() == Kind::Tombstone) {
        let KindFields::Tombstone(t) = &r.record.kind_fields else { continue };
        let Some(mem) = memories.get(t.target.as_str()) else {
            let why = if kind_of.contains_key(t.target.as_str()) { "target-wrong-kind" } else { "target-missing" };
            out.outcomes.insert(r.path(), Outcome::NoEffect(why));
            continue;
        };
        let author = cx.same_person(&r.signer, &mem.signer);
        if !author {
            let member = inp.roster.member(&r.signer).expect("standing passed");
            let mut tags: Vec<String> = mem.record.tags.clone();
            if let Some(prior) = inp.prior_tags.get(&t.target) {
                tags.extend(prior.iter().cloned());
            }
            if !member.steward_areas.is_steward() || !member.steward_areas.covers(&tags) {
                out.outcomes.insert(r.path(), Outcome::Quarantined("tombstone-not-author-or-covering-steward"));
                continue;
            }
        }
        tomb_by_target.entry(t.target.as_str()).or_default().push(TombstoneVote {
            path: r.path(),
            did: r.signer.clone(),
            person: cx.person(&r.signer),
            author,
            action: t.action,
            sensitive: t.reason == TombstoneReason::Sensitive,
        });
    }
    let mut visibility: BTreeMap<&str, Visibility> = BTreeMap::new();
    for (id, mem) in &memories {
        let votes = tomb_by_target.remove(id).unwrap_or_default();
        let tombstone = resolve_tombstones(&cx, &votes, &mut out.outcomes);
        let vis = match mem.record.status {
            AuthorStatus::Withdrawn => Visibility::Hidden(HiddenBy::AuthorWithdrawn),
            AuthorStatus::Superseded => Visibility::Hidden(HiddenBy::AuthorSuperseded),
            AuthorStatus::Active => tombstone,
        };
        visibility.insert(id, vis);
    }

    // Stage 5 and 6: endorsements, then tiers.
    let fresh = inp.freshness.endorsements_count();
    let mut per_memory: BTreeMap<&str, Votes> = BTreeMap::new();
    for r in singles.iter().filter(|r| r.record.kind() == Kind::Endorsement) {
        let KindFields::Endorsement(e) = &r.record.kind_fields else { continue };
        let path = r.path();
        let member = inp.roster.member(&r.signer).expect("standing passed");
        let needs_steward = e.verdict != Verdict::Dispute;
        if needs_steward && !member.steward_areas.is_steward() {
            out.outcomes.insert(path, Outcome::Quarantined("endorsement-not-steward"));
            continue;
        }
        let Some(mem) = memories.get(e.target_id.as_str()) else {
            let why = if kind_of.contains_key(e.target_id.as_str()) { "target-wrong-kind" } else { "target-missing" };
            out.outcomes.insert(path, Outcome::NoEffect(why));
            continue;
        };
        if needs_steward {
            if !member.steward_areas.covers(&mem.record.tags) {
                out.outcomes.insert(path, Outcome::Quarantined("outside-areas"));
                continue;
            }
            if e.verdict == Verdict::Endorse && !scope_ok(&member.steward_areas, &e.scope, &mem.record.tags) {
                out.outcomes.insert(path, Outcome::Quarantined("scope-outside-areas"));
                continue;
            }
        }
        let rev_ok = e.target_rev == mem.record.rev;
        let bound = rev_ok && e.target_hash == mem.hash;
        let person = cx.person(&r.signer);
        let votes = per_memory.entry(mem.record.id.as_str()).or_default();
        match e.verdict {
            Verdict::Endorse => {
                if !rev_ok {
                    out.outcomes.insert(path, Outcome::NoEffect("target-rev-mismatch"));
                } else if !bound {
                    out.outcomes.insert(path, Outcome::NoEffect("hash-mismatch"));
                } else if cx.same_person(&r.signer, &mem.signer) {
                    out.outcomes.insert(path, Outcome::NoEffect("self-endorsement"));
                } else if visibility[mem.record.id.as_str()].is_hidden() {
                    out.outcomes.insert(path, Outcome::NoEffect("target-hidden"));
                } else {
                    votes.endorse.push((person, r.signer.clone(), path));
                }
            }
            Verdict::Withdraw => {
                out.outcomes.insert(path, Outcome::Effective);
                if rev_ok {
                    votes.cancel.insert(person);
                }
            }
            Verdict::Dispute => {
                if rev_ok {
                    votes.cancel.insert(person.clone());
                }
                votes.disputes.push(Dispute { person, did: r.signer.clone(), current: bound });
                out.outcomes.insert(path, if bound { Outcome::Effective } else { Outcome::NoEffect("dispute-of-another-revision") });
            }
        }
    }

    for (id, mem) in &memories {
        let mut votes = per_memory.remove(id).unwrap_or_default();
        let author_person = cx.person(&mem.signer);
        // Per person: one endorser; a withdraw or dispute from any of the person's DIDs cancels it.
        let mut counted: BTreeMap<String, Endorser> = BTreeMap::new();
        votes.endorse.sort();
        for (person, did, path) in votes.endorse {
            if votes.cancel.contains(&person) {
                out.outcomes.insert(path, Outcome::NoEffect("cancelled-by-same-person"));
                continue;
            }
            if !fresh {
                out.outcomes.insert(path.clone(), Outcome::NoEffect("freshness-stale"));
            } else {
                out.outcomes.insert(path.clone(), Outcome::Effective);
            }
            counted.entry(person.clone()).or_insert(Endorser { person, did });
        }
        let counted: Vec<Endorser> = counted.into_values().collect();
        let hidden = visibility[id].is_hidden();
        let (endorsed_by, withheld) = if fresh { (counted, Vec::new()) } else { (Vec::new(), counted) };
        let class_tier = if !endorsed_by.is_empty() && !hidden { Tier::L2 } else { Tier::L1 };
        let cap = ceiling_cap(mem.record.source_ceiling);
        let tier = if hidden { Tier::L0 } else { class_tier.min(cap) };
        votes.disputes.sort_by(|a, b| (&a.person, &a.did, a.current).cmp(&(&b.person, &b.did, b.current)));
        out.outcomes.insert(mem.path(), Outcome::Effective);
        out.memories.insert(
            (*id).to_string(),
            MemoryState {
                id: (*id).to_string(),
                rev: mem.record.rev,
                hash: mem.hash.clone(),
                signer: mem.signer.clone(),
                author_person,
                visibility: visibility[id].clone(),
                class_tier,
                tier,
                clamped_by_ceiling: !hidden && tier < class_tier,
                endorsed_by,
                withheld_by_staleness: withheld,
                disputes: votes.disputes,
            },
        );
    }
    out
}

#[derive(Default)]
struct Votes {
    /// `(person, did, path)` of endorsements that would count.
    endorse: Vec<(String, String, String)>,
    /// People with a `withdraw` or `dispute` of this revision.
    cancel: BTreeSet<String>,
    disputes: Vec<Dispute>,
}

struct TombstoneVote {
    path: String,
    did: String,
    person: String,
    author: bool,
    action: TombstoneAction,
    sensitive: bool,
}

/// Endorsement scope (spec 012 §5.2): inside the signer's areas intersected with the revision's tags. A `*`
/// steward's areas are everything, so the bound is the tags; and on a record with no tags the bound is empty
/// and unusable, so there a `*` steward's scope is taken as written.
///
/// Spec gap: 011 §2.3 says an untagged record "can be endorsed only by a `*` steward" while 012 §5.2 requires a
/// non-empty scope inside `areas ∩ tags`, which is empty. Chosen: the only way both can hold.
fn scope_ok(areas: &Areas, scope: &[String], tags: &[String]) -> bool {
    match areas {
        Areas::All => tags.is_empty() || scope.iter().all(|s| tags.contains(s)),
        Areas::Tags(a) => scope.iter().all(|s| a.contains(s) && tags.contains(s)),
    }
}

/// One person's stance on one target after successors replace predecessors. `withdraw` wins a conflict between
/// two DIDs of the same person that are not in a chain (fail hidden: commit order is not authenticated).
struct Stance {
    action: TombstoneAction,
    sensitive_withdraw: bool,
}

fn person_stance(cx: &Ctx, votes: &[&TombstoneVote], outcomes: &mut BTreeMap<String, Outcome>) -> Stance {
    let dids: Dids = votes.iter().map(|v| v.did.clone()).collect();
    let mut live = Vec::new();
    for v in votes {
        if cx.dominated(&v.did, &dids) {
            outcomes.insert(v.path.clone(), Outcome::NoEffect("replaced-by-successor"));
        } else {
            outcomes.insert(v.path.clone(), Outcome::Effective);
            live.push(*v);
        }
    }
    let sensitive_withdraw = live.iter().any(|v| v.action == TombstoneAction::Withdraw && v.sensitive);
    let action = if live.iter().any(|v| v.action == TombstoneAction::Withdraw) { TombstoneAction::Withdraw } else { TombstoneAction::Restore };
    Stance { action, sensitive_withdraw }
}

fn resolve_tombstones(cx: &Ctx, votes: &[TombstoneVote], outcomes: &mut BTreeMap<String, Outcome>) -> Visibility {
    let author: Vec<&TombstoneVote> = votes.iter().filter(|v| v.author).collect();
    let mut stewards: BTreeMap<&str, Vec<&TombstoneVote>> = BTreeMap::new();
    for v in votes.iter().filter(|v| !v.author) {
        stewards.entry(v.person.as_str()).or_default().push(v);
    }
    let author_action = (!author.is_empty()).then(|| person_stance(cx, &author, outcomes).action);
    let stances: Vec<(&str, Stance)> = stewards.iter().map(|(p, vs)| (*p, person_stance(cx, vs, outcomes))).collect();
    let hold = stances.iter().any(|(_, s)| s.sensitive_withdraw);
    match author_action {
        Some(TombstoneAction::Withdraw) => Visibility::Hidden(HiddenBy::AuthorTombstone),
        Some(TombstoneAction::Restore) if hold => Visibility::Hidden(HiddenBy::SensitiveHold),
        Some(TombstoneAction::Restore) => Visibility::Visible,
        None if stances.is_empty() => Visibility::Visible,
        None => {
            let withdraws = stances.iter().filter(|(_, s)| s.action == TombstoneAction::Withdraw).count();
            if withdraws == stances.len() {
                Visibility::Hidden(if hold { HiddenBy::SensitiveHold } else { HiddenBy::StewardTombstone })
            } else if withdraws == 0 {
                Visibility::Visible
            } else {
                Visibility::Hidden(HiddenBy::Contested(stances.iter().map(|(p, _)| (*p).to_string()).collect()))
            }
        }
    }
}

#[cfg(test)]
#[path = "tier_tests.rs"]
mod tests;
