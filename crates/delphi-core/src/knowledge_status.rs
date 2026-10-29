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
    Checking,
    Validated,
    Contested,
    Disproved,
    Retired,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Noted => "noted",
            Status::Checking => "checking",
            Status::Validated => "validated",
            Status::Contested => "contested",
            Status::Disproved => "disproved",
            Status::Retired => "retired",
        }
    }

    /// The operator label (`specs/040` §18.2, §23.1): `validated`, `not validated` or `disproved`.
    pub fn label(&self) -> &'static str {
        match self {
            Status::Validated => "validated",
            Status::Disproved => "disproved",
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
    fn labels_are_the_three_operator_words_and_never_untrusted() {
        for s in [Status::Noted, Status::Checking, Status::Validated, Status::Contested, Status::Disproved, Status::Retired] {
            assert!(["validated", "not validated", "disproved"].contains(&s.label()));
            assert!(!s.label().contains("untrusted") && !s.as_str().contains("untrusted"));
        }
        assert_eq!(Status::Contested.label(), "not validated");
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
