//! The operator-facing validation labels (`specs/040` §18.2): `validated`, `not validated`, `disproved`.
//!
//! Display only. The six-tier lattice stays internal; a label is derived from the tier the daemon holds for
//! content, never from what a file says about itself, and a label never changes what a span may do.

pub const VALIDATED: &str = "validated";
pub const NOT_VALIDATED: &str = "not validated";
pub const DISPROVED: &str = "disproved";

/// The label for content at a provenance tier (`L0`..`L5`). Operator-ratified (L5) and empirically checked (L4)
/// content is validated; everything else, including anything that cannot be placed, is not validated. `disproved`
/// comes from a recorded refutation, not from a tier, so it is never returned here.
pub fn label_for_tier(tier: &str) -> &'static str {
    match tier {
        "L4" | "L5" => VALIDATED,
        _ => NOT_VALIDATED,
    }
}

/// Is this a slice read from a pointer (a file, spec or knowledge-base span), as opposed to text the daemon
/// composes itself (`@runtime/...`)? Only the former carry a label.
pub fn labelled_slice(label: &str) -> bool {
    !label.starts_with("@runtime/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_checked_or_ratified_tiers_are_validated() {
        assert_eq!(["L0", "L1", "L2", "L3"].map(label_for_tier), [NOT_VALIDATED; 4]);
        assert_eq!(["L4", "L5"].map(label_for_tier), [VALIDATED; 2]);
        assert_eq!((label_for_tier("unavailable"), label_for_tier("")), (NOT_VALIDATED, NOT_VALIDATED));
    }
}
