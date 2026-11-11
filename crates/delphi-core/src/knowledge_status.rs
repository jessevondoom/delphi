//! Knowledge status (`specs/052` §3.1, KS-1; `specs/040` §18.2, §23.1): how far a stored claim can be relied on.
//!
//! One table, derived by the daemon from the tier the index holds, the zone the file lives in and its lifecycle
//! state, never from anything a file says about its own validity. Display only: nothing here changes a tier, a gate or
//! a store. Nothing in the tree yet records a validator verdict, a queued check or a contradiction, so `Checking`,
//! `Contested` and `Disproved` exist in the table but are not yet derived; they arrive with the slices that write
//! those records.

use crate::provenance::Tier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Noted,
    /// Three or more independent origins state it (`specs/098` §7.6): a count made by code, between `Noted` and `Validated`. It is
    /// weaker than `Validated`, it is not a taint level, and nothing reads it as a reason to trust, promote or permit anything.
    Corroborated,
    /// Three or more independent origins, at least one of them a trusted source (`specs/100` §3.2). A count made by code; weaker than
    /// `Validated`; not a taint level.
    Substantiated,
    /// Five or more independent origins, at least two trusted sources, **and** the validator has recorded a `would_ratify` for the claim
    /// (`specs/100` §4). Still `not validated` underneath: it waits for the operator, and nothing is written to the validated facts.
    Vetted,
    Checking,
    Validated,
    Contested,
    Disproved,
    Retired,
}

/// How many independent origins make a claim `corroborated` (`specs/098` §7.6). The default of `TierParams`.
pub const CORROBORATED_MIN_ORIGINS: usize = 3;

/// The numbers and switches of the ladder (`specs/100` §3.2; `[research.tiers]`). The four numbers are **defaults the operator calibrates**, a
/// first guess and not a pre-registered threshold. A tier switched off is skipped: a claim reads the highest tier that is on and whose
/// condition holds, which is how a roll-back falls back one tier (`specs/100` §7.3, slice L6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierParams {
    pub corroborated_origins: usize,
    pub substantiated_trusted: usize,
    pub vetted_origins: usize,
    pub vetted_trusted: usize,
    pub corroborated_on: bool,
    pub substantiated_on: bool,
    pub vetted_on: bool,
    /// The fast lane (`specs/100` §5): a claim of a higher rung is offered to the validator before one of a lower rung. **Queue order only**:
    /// it reads the tier, never writes one, and the same claim has the same status with the lane on or off.
    pub fast_lane_on: bool,
}

impl Default for TierParams {
    fn default() -> TierParams {
        TierParams { corroborated_origins: CORROBORATED_MIN_ORIGINS, substantiated_trusted: 1, vetted_origins: 5, vetted_trusted: 2, corroborated_on: true, substantiated_on: true, vetted_on: true, fast_lane_on: true }
    }
}

impl TierParams {
    /// The same numbers kept in order: a higher tier never asks for fewer origins or trusted sources than the tier below it, so `vetted`
    /// cannot be reached by a claim that is not also `substantiated` and `corroborated`.
    pub fn ordered(mut self) -> TierParams {
        self.vetted_origins = self.vetted_origins.max(self.corroborated_origins);
        self.vetted_trusted = self.vetted_trusted.max(self.substantiated_trusted);
        self
    }
}

static TIERS: std::sync::RwLock<Option<TierParams>> = std::sync::RwLock::new(None);

/// The ladder as the daemon was started with it. Set once at start from `[research.tiers]` (`set_tier_params`); until then, and in a process
/// that never reads the config, the defaults of `TierParams`.
pub fn tier_params() -> TierParams {
    TIERS.read().ok().and_then(|g| g.clone()).unwrap_or_default()
}

/// Install the operator's ladder. Called by the daemon at start and nowhere else; nothing a model, a tool argument or a file says reaches it.
pub fn set_tier_params(p: TierParams) {
    if let Ok(mut g) = TIERS.write() {
        *g = Some(p.ordered());
    }
}

/// What the tier of a claim is derived from: tags the daemon wrote, never a field a caller can set (`specs/100` §2 item 2, §10 item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tags {
    /// Distinct origin keys of daemon-refetched, quote-verified citations (`specs/098` §7.6).
    pub origins: usize,
    /// Of those origins, how many are trusted sources (the operator's list within its topic scope, or a Primary or Reputable class).
    pub trusted_sources: usize,
    /// The validator has recorded `would_ratify` for the claim (`specs/100` §4): the advisory-mode dry run passed.
    pub would_ratify: bool,
    /// The claim is in a protected class (`specs/040` §9.2). Such a claim never rises.
    pub protected: bool,
}

/// The tier a claim's tags give, ignoring every other status. The highest **enabled** tier whose condition holds; the conditions nest
/// (`vetted` needs the `substantiated` and `corroborated` conditions too, because `TierParams::ordered` keeps the numbers in order).
pub fn tier_for(tags: &Tags, p: &TierParams) -> Status {
    if tags.protected {
        return Status::Noted;
    }
    let corroborated = tags.origins >= p.corroborated_origins;
    let substantiated = corroborated && tags.trusted_sources >= p.substantiated_trusted;
    let vetted = substantiated && tags.origins >= p.vetted_origins && tags.trusted_sources >= p.vetted_trusted && tags.would_ratify;
    if vetted && p.vetted_on {
        Status::Vetted
    } else if substantiated && p.substantiated_on {
        Status::Substantiated
    } else if corroborated && p.corroborated_on {
        Status::Corroborated
    } else {
        Status::Noted
    }
}

/// A claim that would read `noted` or `checking` reads the tier its tags give. Every other status wins over a count: `validated`,
/// `contested`, `disproved`, `retired` and the rest are never changed by one, so a contradiction or a lost source always lowers a claim
/// (`specs/100` §3.2 gates; `specs/098` §7.6 "Disagreement wins"). `checking` is an activity (a claim queued with the validator), not a rung.
pub fn raise_for_tags(status: Status, tags: &Tags, p: &TierParams) -> Status {
    if !matches!(status, Status::Noted | Status::Checking) {
        return status;
    }
    match tier_for(tags, p) {
        Status::Noted => status,
        tier => tier,
    }
}

/// What to print beside a tier: the counts and the words that keep it from reading as proof (`specs/100` §1 table). `None` for a status
/// that is not a rung of the ladder.
pub fn tier_detail(status: Status, origins: usize, trusted_sources: usize) -> Option<String> {
    let trusted = |n: usize| if n == 1 { "1 trusted source".to_string() } else { format!("{} trusted sources", n) };
    match status {
        Status::Corroborated => Some(format!("not validated · corroborated ({} origins) · weaker than validated", origins)),
        Status::Substantiated => Some(format!("not validated · substantiated ({} origins, {}) · weaker than validated", origins, trusted(trusted_sources))),
        Status::Vetted => Some(format!("vetted ({} origins, {}; awaiting your accept) · weaker than validated", origins, trusted(trusted_sources))),
        _ => None,
    }
}

/// A claim that would read `noted` or `checking` reads `corroborated` when `origins` independent origins support it. `checking` is an
/// activity (a claim queued with the validator), not a rung, and a claim waiting for a check can still have three origins; the
/// spec makes a corroborated claim a candidate for validation (`specs/098` §7.6 "A path up"). It raises nothing else: `validated`,
/// `contested`, `disproved` and `retired` all win over a count, so a contradiction or a lost source always lowers a corroborated
/// claim (`specs/098` §7.6 "Disagreement wins").
pub fn raise_for_support(status: Status, origins: usize) -> Status {
    if matches!(status, Status::Noted | Status::Checking) && origins >= CORROBORATED_MIN_ORIGINS {
        Status::Corroborated
    } else {
        status
    }
}

/// What to print beside a corroborated claim: the count and the words that keep it from reading as proof. `trusted_sources` is the
/// flag of `specs/098` §7.6: shown, never a condition of the status.
pub fn support_detail(origins: usize, trusted_sources: usize) -> String {
    format!("corroborated · {} origins · trusted_sources: {} · weaker than validated", origins, trusted_sources)
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Noted => "noted",
            Status::Corroborated => "corroborated",
            Status::Substantiated => "substantiated",
            Status::Vetted => "vetted",
            Status::Checking => "checking",
            Status::Validated => "validated",
            Status::Contested => "contested",
            Status::Disproved => "disproved",
            Status::Retired => "retired",
        }
    }

    /// The operator label (`specs/040` §18.2, §23.1): `validated`, `not validated`, `disproved`, and, since `specs/100`, `vetted`, the one new
    /// top-level word. `corroborated` and `substantiated` stay `not validated`; their detail (`tier_detail`) says which rung and that it is weaker.
    pub fn label(&self) -> &'static str {
        match self {
            Status::Validated => "validated",
            Status::Disproved => "disproved",
            Status::Vetted => "vetted",
            _ => "not validated",
        }
    }
}

/// Zones whose files an operator or an agent writes at run time. Their frontmatter is a claim, not a record, so
/// however high it says its tier is they stay `noted` (`specs/052` §5: "status is derived").
const SELF_DECLARED_ZONES: [&str; 5] = ["missions", "memory", "staged", "knowledge", "xenia"];

/// The status of a stored document from its zone, the tier name the index holds (`source_type`) and its lifecycle
/// `status` field. Only the tier a release or the operator gave a file in a trusted zone (L4, L5) reads as validated;
/// L3 needs a validator record (`web_corroborated`), which nothing writes yet, so it is `noted`.
///
/// `vouched_by` is the bundle input (`specs/052` §3.1, `specs/024` §3.7): the org of the **lock entry** of an added
/// bundle, supplied by the daemon from `local/bundles.lock` and from nothing a file says. A bundle's document with an
/// entry is `validated`; without one it stays `noted`, whatever its own frontmatter claims. It is a label for
/// reliability and citability only: the stored tier stays L2 and nothing is raised.
pub fn derive(zone: &str, tier_name: &str, doc_status: &str, vouched_by: Option<&str>) -> Status {
    if matches!(doc_status, "superseded" | "retired" | "expired") {
        return Status::Retired;
    }
    if vouched_by.is_some() && matches!(zone, "bundles" | "skills" | "library") {
        return Status::Validated;
    }
    if SELF_DECLARED_ZONES.contains(&zone) || zone.starts_with("bundle") {
        return Status::Noted;
    }
    let tier = [Tier::L0, Tier::L1, Tier::L2, Tier::L3, Tier::L4, Tier::L5].into_iter().find(|t| t.name() == tier_name);
    match tier {
        Some(t) if t >= Tier::L4 => Status::Validated,
        _ => Status::Noted,
    }
}

/// The label to print: the operator's three words, with `· vouched by <org>` for bundle content that a lock entry
/// vouches for. The six-tier lattice is never shown (`specs/052` §3.1).
pub fn shown_label(status: Status, vouched_by: Option<&str>) -> String {
    match (status, vouched_by) {
        (Status::Validated, Some(org)) => format!("validated · vouched by {}", org),
        _ => status.label().to_string(),
    }
}

/// The label of imported bundle content whose vouching has ended (`specs/024` §4.1a).
pub const NO_LONGER_VOUCHED: &str = "not validated · no longer vouched";

/// The status of a record under `context/facts/` (`specs/040` §7.3, §23.1, §24.4). `web_corroborated` is the marker the
/// daemon's fact store writes into a ratified fact (nothing else may write that directory); an `internal_generated`
/// (L3) record with it and an active status reads as validated, and L3 without it stays `noted`. A `contested` or
/// `disproved` status is the daemon lowering a record, and lowering always wins. An attribution is never validated.
pub fn derive_fact(tier_name: &str, doc_status: &str, web_corroborated: bool) -> Status {
    match doc_status {
        "superseded" | "retired" | "expired" => Status::Retired,
        "contested" => Status::Contested,
        "disproved" => Status::Disproved,
        "active" if web_corroborated && tier_name == Tier::L3.name() => Status::Validated,
        _ => Status::Noted,
    }
}

/// The operator label for a context slice read from a handoff pointer, from the tier the daemon stamped on it
/// (`L0`..`L5`; anything else, such as `unavailable`, is not validated). The tier comes from the provenance index,
/// never from the file's own frontmatter, so a file cannot label itself. Only slices read from a pointer carry a
/// label; text the daemon composes (`@runtime/...`) does not.
pub fn slice_label(slice_label: &str, tier: &str) -> Option<&'static str> {
    if slice_label.starts_with("@runtime/") {
        return None;
    }
    let status = match Tier::parse(tier) {
        Some(t) if t >= Tier::L4 => Status::Validated,
        _ => Status::Noted,
    };
    Some(status.label())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ratified_fact_is_validated_only_with_the_marker_and_lowering_wins() {
        assert_eq!(derive_fact("internal_generated", "active", true), Status::Validated);
        assert_eq!(derive_fact("internal_generated", "active", false), Status::Noted);
        assert_eq!(derive_fact("external_untrusted", "active", true), Status::Noted);
        assert_eq!(derive_fact("internal_generated", "attributed", false).label(), "not validated");
        assert_eq!(derive_fact("internal_generated", "contested", true), Status::Contested);
        assert_eq!(derive_fact("internal_generated", "disproved", true).label(), "disproved");
        assert_eq!(derive_fact("internal_generated", "superseded", true), Status::Retired);
    }

    #[test]
    fn a_slice_read_from_a_pointer_is_labelled_by_its_stamped_tier() {
        assert_eq!(["L0", "L1", "L2", "L3", "unavailable", ""].map(|t| slice_label("specs/a.md", t)), [Some("not validated"); 6]);
        assert_eq!(["L4", "L5"].map(|t| slice_label("specs/a.md", t)), [Some("validated"); 2]);
        assert_eq!(slice_label("@runtime/change-log", "L5"), None, "daemon-composed text carries no label");
    }

    #[test]
    fn three_independent_origins_raise_a_noted_or_queued_claim_and_anything_else_wins_over_a_count() {
        assert_eq!(raise_for_support(Status::Noted, 2), Status::Noted);
        assert_eq!(raise_for_support(Status::Noted, 3), Status::Corroborated);
        assert_eq!(raise_for_support(Status::Noted, 40), Status::Corroborated);
        assert_eq!(raise_for_support(Status::Checking, 3), Status::Corroborated, "a queued check is an activity, not a rung");
        assert_eq!(raise_for_support(Status::Checking, 2), Status::Checking);
        for lowered in [Status::Contested, Status::Disproved, Status::Retired, Status::Validated] {
            assert_eq!(raise_for_support(lowered, 9), lowered, "{lowered:?}: a count never overrides another status");
        }
        assert_eq!(Status::Corroborated.as_str(), "corroborated");
        assert_eq!(Status::Corroborated.label(), "not validated", "the operator still reads one of three words");
        let d = support_detail(3, 1);
        assert!(d.contains("3 origins") && d.contains("trusted_sources: 1") && d.contains("weaker than validated"), "{d}");
        assert!(!d.contains("verified"), "a count is not verification: {d}");
    }

    #[test]
    fn labels_are_the_operator_words_and_never_untrusted() {
        for s in [Status::Noted, Status::Corroborated, Status::Substantiated, Status::Vetted, Status::Checking, Status::Validated, Status::Contested, Status::Disproved, Status::Retired] {
            assert!(["validated", "not validated", "disproved", "vetted"].contains(&s.label()));
            assert!(!s.label().contains("untrusted") && !s.as_str().contains("untrusted"));
        }
        assert_eq!(Status::Contested.label(), "not validated");
        // `vetted` is the only new top-level word; the two rungs below it stay `not validated`
        assert_eq!((Status::Corroborated.label(), Status::Substantiated.label(), Status::Vetted.label()), ("not validated", "not validated", "vetted"));
    }

    fn tags(origins: usize, trusted_sources: usize, would_ratify: bool) -> Tags {
        Tags { origins, trusted_sources, would_ratify, protected: false }
    }

    #[test]
    fn the_tier_table_gives_each_rung_at_its_gate_and_not_before() {
        let p = TierParams::default();
        let t = |o, k, w| tier_for(&tags(o, k, w), &p);
        // (origins, trusted, would_ratify) -> tier
        let table = [
            (0, 0, false, Status::Noted), (1, 0, false, Status::Noted), (2, 5, true, Status::Noted),
            (3, 0, false, Status::Corroborated), (4, 0, true, Status::Corroborated),
            (3, 1, false, Status::Substantiated), (4, 1, true, Status::Substantiated),
            (5, 1, true, Status::Substantiated), (5, 2, false, Status::Substantiated),
            (4, 2, true, Status::Substantiated), (5, 2, true, Status::Vetted), (9, 4, true, Status::Vetted),
        ];
        for (o, k, w, want) in table {
            assert_eq!(t(o, k, w), want, "origins {o}, trusted {k}, would_ratify {w}");
        }
    }

    #[test]
    fn trusted_sources_cannot_exceed_what_the_origins_allow_to_count_alone() {
        // two origins never rise however many trusted sources the tags say; the gate is origins first
        assert_eq!(tier_for(&tags(2, 9, true), &TierParams::default()), Status::Noted);
    }

    #[test]
    fn a_protected_claim_never_rises() {
        let t = Tags { origins: 9, trusted_sources: 9, would_ratify: true, protected: true };
        assert_eq!(tier_for(&t, &TierParams::default()), Status::Noted);
    }

    #[test]
    fn contested_disproved_retired_and_validated_never_rise_and_checking_and_noted_do() {
        let p = TierParams::default();
        let top = tags(9, 4, true);
        for fixed in [Status::Contested, Status::Disproved, Status::Retired, Status::Validated] {
            assert_eq!(raise_for_tags(fixed, &top, &p), fixed, "{fixed:?}");
        }
        for open in [Status::Noted, Status::Checking] {
            assert_eq!(raise_for_tags(open, &top, &p), Status::Vetted);
            assert_eq!(raise_for_tags(open, &tags(1, 0, false), &p), open);
        }
    }

    #[test]
    fn a_retraction_lowers_the_tier_because_the_tier_is_derived_from_the_current_tags() {
        let p = TierParams::default();
        let before = raise_for_tags(Status::Noted, &tags(5, 2, true), &p);
        let after = raise_for_tags(Status::Noted, &tags(4, 1, true), &p);
        let gone = raise_for_tags(Status::Noted, &tags(2, 1, true), &p);
        assert_eq!((before, after, gone), (Status::Vetted, Status::Substantiated, Status::Noted));
    }

    #[test]
    fn the_numbers_are_the_operators_and_a_switched_off_tier_falls_back_one_rung() {
        let top = tags(6, 3, true);
        let strict = TierParams { corroborated_origins: 4, substantiated_trusted: 2, vetted_origins: 6, vetted_trusted: 3, ..TierParams::default() };
        assert_eq!(tier_for(&top, &strict), Status::Vetted);
        assert_eq!(tier_for(&tags(5, 3, true), &strict), Status::Substantiated);
        assert_eq!(tier_for(&tags(3, 3, true), &strict), Status::Noted);
        let no_vetted = TierParams { vetted_on: false, ..TierParams::default() };
        assert_eq!(tier_for(&tags(9, 4, true), &no_vetted), Status::Substantiated);
        let neither = TierParams { vetted_on: false, substantiated_on: false, ..TierParams::default() };
        assert_eq!(tier_for(&tags(9, 4, true), &neither), Status::Corroborated);
        let none = TierParams { vetted_on: false, substantiated_on: false, corroborated_on: false, ..TierParams::default() };
        assert_eq!(tier_for(&tags(9, 4, true), &none), Status::Noted);
    }

    #[test]
    fn an_out_of_order_setting_is_put_in_order_so_vetted_never_needs_less_than_the_rung_below() {
        let odd = TierParams { corroborated_origins: 6, vetted_origins: 2, substantiated_trusted: 3, vetted_trusted: 1, ..TierParams::default() }.ordered();
        assert_eq!((odd.vetted_origins, odd.vetted_trusted), (6, 3));
    }

    #[test]
    fn the_detail_says_which_rung_and_that_it_is_weaker_than_validated() {
        let s = tier_detail(Status::Substantiated, 3, 1).unwrap();
        assert!(s.starts_with("not validated · substantiated (3 origins, 1 trusted source)") && s.contains("weaker than validated"), "{s}");
        let v = tier_detail(Status::Vetted, 5, 2).unwrap();
        assert!(v.starts_with("vetted (5 origins, 2 trusted sources") && v.contains("weaker than validated"), "{v}");
        assert!(tier_detail(Status::Corroborated, 3, 0).unwrap().contains("weaker than validated"));
        assert_eq!(tier_detail(Status::Noted, 1, 0), None);
        assert_eq!(tier_detail(Status::Validated, 9, 9), None);
    }

    #[test]
    fn only_a_lock_entry_vouches_for_a_bundle_document() {
        // a document whose own frontmatter says it is validated and ratified is still noted without an entry
        assert_eq!(derive("bundles", "user_ratified", "active", None), Status::Noted);
        assert_eq!(derive("bundles", "federated_verified", "active", None), Status::Noted);
        assert_eq!(derive("bundles", "federated_verified", "active", Some("acme")), Status::Validated);
        assert_eq!(shown_label(Status::Validated, Some("acme")), "validated · vouched by acme");
        assert_eq!(shown_label(Status::Noted, Some("acme")), "not validated");
        assert_eq!(derive("bundles", "federated_verified", "superseded", Some("acme")), Status::Retired);
        assert!(!shown_label(Status::Validated, Some("acme")).contains("federated"));
    }

    #[test]
    fn a_file_cannot_declare_itself_validated() {
        assert_eq!(derive("memory", "user_ratified", "active", None), Status::Noted);
        assert_eq!(derive("staged", "internal_empirical", "active", None), Status::Noted);
        assert_eq!(derive("knowledge", "federated_verified", "active", None), Status::Noted, "research output at L2");
        assert_eq!(derive("agents", "user_ratified", "active", None), Status::Validated, "shipped with a release");
        assert_eq!(derive("specs", "internal_generated", "active", None), Status::Noted);
        assert_eq!(derive("context", "external_untrusted", "active", None), Status::Noted);
        assert_eq!(derive("agents", "user_ratified", "superseded", None), Status::Retired);
    }
}
