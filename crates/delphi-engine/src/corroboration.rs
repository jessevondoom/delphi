//! Corroboration (`specs/098` §7.6, slice S1b): how many independent origins state a claim, and how many of them are trusted sources.
//!
//! The count is made by code over citations the daemon itself captured and in which it found the quote; a host's say-so never enters
//! it. Origins are clustered as `specs/040` §6.2 does for a fact check: the registrable domain, then near-duplicate text, then the
//! citation graph. Three or more clusters make a claim `corroborated` (`delphi_core::knowledge_status`). That is a weaker statement than
//! `validated` and it is a *display*: nothing here, and nothing that reads this, grants trust, promotion, a tool or an approval
//! (`specs/098` §12 item 17; `crates/delphi/tests/corroboration_grants_nothing.rs` pins it).
//!
//! `trusted_sources` is a second number beside it: how many of the counted origins the operator's trusted-source list (inside the
//! topic scope the operator set) or a Primary/Reputable class vouches for. It changes no status, taint, tier or queue order.

use crate::fact_check::{jaccard, shingles, NEAR_DUPLICATE};
use crate::fact_sources::Class;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// The numbers of `specs/098` §18. Defaults are proposals the slices calibrate; `[research]` overrides them.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// A registrable domain younger than this counts zero **when its age is known**. Nothing supplies an age yet, so today this never
    /// fires; an unknown age counts.
    pub young_domain_days: u32,
    /// Citations first seen within this many hours of each other count once, when their first-seen times are known.
    pub burst_hours: u32,
    /// Shingle similarity above which two pages are one origin. `specs/040` §6.2 merges at [`NEAR_DUPLICATE`], so the effective
    /// threshold is the lower of the two: a corroborated claim is never counted more leniently than the validator counts independence.
    pub similar_above: f64,
    /// Whether a domain no list names (the classifier calls it `Weak` by default) counts. On by default: otherwise no ordinary web page
    /// could corroborate anything. A domain a list or rule calls weak, and every excluded one, counts zero either way.
    pub unfamiliar_counts: bool,
}

impl Default for Params {
    fn default() -> Params {
        Params { young_domain_days: 180, burst_hours: 24, similar_above: 0.8, unfamiliar_counts: true }
    }
}

/// One citation of a claim: a page the daemon captured, with what the count needs to know about it.
#[derive(Debug, Clone)]
pub struct Cite {
    pub source_id: String,
    pub url: String,
    /// The registrable domain, derived by the fetcher (`fact_fetch::registrable_domain`). Empty means unknown: the source stands alone.
    pub origin: String,
    pub class: Class,
    /// `Weak` only because no list names the domain (`SourceList::unfamiliar`).
    pub unfamiliar: bool,
    /// The captured text, for the near-duplicate and citation-graph tests. May be empty (a pointer-only source).
    pub text: String,
    /// When the page first appeared, as unix seconds, if the capture recorded it. The capture time is not that, so it is `None` unless
    /// the page said.
    pub first_seen: Option<i64>,
    pub domain_age_days: Option<u32>,
    pub registrant: Option<String>,
    /// On the operator's trusted-source list when the source was captured, inside the topic scope of the entry, and listed before the
    /// claim was captured.
    pub listed_in_scope: bool,
    /// The source is built from claims (a generated page), so counting it would let a claim support itself (`specs/098` §7.6 cycle check).
    pub derived: bool,
}

/// What the count found. `zeroed` names each citation that did not count and why, so a reader can see it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Support {
    pub origins: usize,
    pub trusted_sources: usize,
    /// Citations that were counted before clustering.
    pub counted: usize,
    pub zeroed: Vec<(String, &'static str)>,
}

impl Support {
    pub fn to_json(&self) -> Value {
        json!({"origins": self.origins, "trusted_sources": self.trusted_sources, "counted": self.counted,
               "zeroed": self.zeroed.iter().map(|(s, r)| json!({"source": s, "reason": r})).collect::<Vec<_>>()})
    }
}

/// Why a citation counts zero, if it does. The fixed words are shown to the operator.
pub fn zero_reason(c: &Cite, p: &Params) -> Option<&'static str> {
    if c.derived {
        return Some("derived");
    }
    match c.class {
        Class::Excluded => return Some("excluded"),
        Class::Weak if !c.unfamiliar => return Some("weak"),
        Class::Weak if !p.unfamiliar_counts => return Some("unfamiliar"),
        _ => {}
    }
    if c.domain_age_days.is_some_and(|d| d < p.young_domain_days) {
        return Some("young_domain");
    }
    None
}

fn find(parent: &mut [usize], x: usize) -> usize {
    if parent[x] != x {
        let r = find(parent, parent[x]);
        parent[x] = r;
    }
    parent[x]
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    parent[ra] = rb;
}

/// Count the independent origins among `cites` and how many of them are trusted sources.
pub fn support(cites: &[Cite], p: &Params) -> Support {
    let mut out = Support::default();
    let mut live: Vec<&Cite> = Vec::new();
    for c in cites {
        match zero_reason(c, p) {
            Some(why) => out.zeroed.push((c.source_id.clone(), why)),
            None => live.push(c),
        }
    }
    out.counted = live.len();
    let n = live.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let threshold = p.similar_above.min(NEAR_DUPLICATE);
    let sh: Vec<_> = live.iter().map(|c| shingles(&c.text)).collect();
    let lower: Vec<String> = live.iter().map(|c| c.text.to_lowercase()).collect();
    for i in 0..n {
        for j in (i + 1)..n {
            let (a, b) = (live[i], live[j]);
            let same_origin = !a.origin.is_empty() && a.origin == b.origin;
            let same_registrant = a.registrant.is_some() && a.registrant == b.registrant;
            let copy = !a.text.is_empty() && !b.text.is_empty() && jaccard(&sh[i], &sh[j]) >= threshold;
            // A page that names the other's domain or address cites it (`specs/040` §6.2).
            let cites = |x: usize, y: usize| {
                let (o, u) = (live[y].origin.to_lowercase(), live[y].url.to_lowercase());
                (!o.is_empty() && lower[x].contains(&o)) || (!u.is_empty() && lower[x].contains(&u))
            };
            if same_origin || same_registrant || copy || cites(i, j) || cites(j, i) {
                union(&mut parent, i, j);
            }
        }
    }
    // Citations first seen within `burst_hours` of each other count once, when their first-seen times are known.
    let mut timed: Vec<(i64, usize)> = live.iter().enumerate().filter_map(|(i, c)| c.first_seen.map(|t| (t, i))).collect();
    timed.sort();
    let window = i64::from(p.burst_hours) * 3600;
    for w in timed.windows(2) {
        if w[1].0 - w[0].0 <= window {
            union(&mut parent, w[0].1, w[1].1);
        }
    }
    let mut clusters: BTreeMap<usize, bool> = BTreeMap::new();
    for (i, cite) in live.iter().enumerate() {
        let root = find(&mut parent, i);
        let trusted = cite.listed_in_scope || matches!(cite.class, Class::Primary | Class::ReputableSecondary);
        *clusters.entry(root).or_insert(false) |= trusted;
    }
    out.origins = clusters.len();
    out.trusted_sources = clusters.values().filter(|t| **t).count();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cite(id: &str, origin: &str, text: &str) -> Cite {
        Cite { source_id: id.into(), url: format!("https://{origin}/{id}"), origin: origin.into(), class: Class::ReputableSecondary, unfamiliar: false, text: text.into(), first_seen: None, domain_age_days: None, registrant: None, listed_in_scope: false, derived: false }
    }
    fn unfamiliar(id: &str, origin: &str, text: &str) -> Cite {
        Cite { class: Class::Weak, unfamiliar: true, ..cite(id, origin, text) }
    }
    const A: &str = "the harbour bridge opened in march 1932 after eight years of construction by a large workforce";
    const B: &str = "council records show the tunnel under the river was dug between 1971 and 1975 by two crews";
    const C: &str = "an engineering survey found the lighthouse was rebuilt in stone after the great storm of 1859";
    const D: &str = "the museum catalogue lists the painting as acquired from a private collector in the year 1964";

    #[test]
    fn three_distinct_origins_with_different_text_are_three() {
        let s = support(&[cite("1", "a.example", A), cite("2", "b.example", B), cite("3", "c.example", C)], &Params::default());
        assert_eq!((s.origins, s.counted), (3, 3));
        assert!(s.zeroed.is_empty());
    }

    #[test]
    fn three_citations_from_one_origin_count_once() {
        let s = support(&[cite("1", "a.example", A), cite("2", "a.example", B), cite("3", "a.example", C)], &Params::default());
        assert_eq!(s.origins, 1);
    }

    #[test]
    fn a_syndicated_copy_and_a_page_that_cites_another_share_an_origin() {
        let copy = support(&[cite("1", "a.example", A), cite("2", "b.example", A), cite("3", "c.example", C)], &Params::default());
        assert_eq!(copy.origins, 2, "the same text under two domains is one origin");
        let cited = format!("{} as reported by a.example earlier", B);
        let g = support(&[cite("1", "a.example", A), cite("2", "b.example", &cited), cite("3", "c.example", C)], &Params::default());
        assert_eq!(g.origins, 2, "a page that names the other's domain is not independent of it");
    }

    #[test]
    fn the_effective_similarity_threshold_is_never_looser_than_the_validators() {
        // 0.5 is 040's merge point; a configured 0.9 must not make near-duplicates look independent.
        let near = "the harbour bridge opened in march 1932 after eight years of construction by a large workforce and a famous engineer";
        let loose = Params { similar_above: 0.9, ..Params::default() };
        let strict = Params { similar_above: 0.2, ..Params::default() };
        let pair = [cite("1", "a.example", A), cite("2", "b.example", near)];
        assert_eq!(support(&pair, &loose).origins, 1, "near-duplicate text merges at the validator's threshold, not the looser one");
        assert_eq!(support(&pair, &strict).origins, 1);
        let unrelated = [cite("1", "a.example", A), cite("2", "b.example", B)];
        assert_eq!(support(&unrelated, &strict).origins, 2);
    }

    #[test]
    fn explicitly_weak_and_excluded_origins_count_zero_and_unfamiliar_ones_count_by_default() {
        let weak = Cite { class: Class::Weak, unfamiliar: false, ..cite("w", "reddit.example", A) };
        let excl = Cite { class: Class::Excluded, ..cite("x", "bit.example", B) };
        let s = support(&[weak, excl, unfamiliar("u", "blog.example", C), cite("r", "r.example", D)], &Params::default());
        assert_eq!(s.origins, 2, "the unfamiliar blog and the reputable page count");
        assert_eq!(s.zeroed, vec![("w".to_string(), "weak"), ("x".to_string(), "excluded")]);
        let off = Params { unfamiliar_counts: false, ..Params::default() };
        let s = support(&[unfamiliar("u", "blog.example", C), cite("r", "r.example", D)], &off);
        assert_eq!((s.origins, s.zeroed.clone()), (1, vec![("u".to_string(), "unfamiliar")]));
    }

    #[test]
    fn a_young_domain_counts_zero_only_when_its_age_is_known() {
        let young = Cite { domain_age_days: Some(10), ..cite("y", "new.example", A) };
        let unknown = cite("u", "who.example", B);
        let old = Cite { domain_age_days: Some(4000), ..cite("o", "old.example", C) };
        let s = support(&[young, unknown, old], &Params::default());
        assert_eq!(s.origins, 2);
        assert_eq!(s.zeroed, vec![("y".to_string(), "young_domain")]);
    }

    #[test]
    fn one_registrant_and_a_burst_of_known_first_seen_times_each_count_once() {
        let r1 = Cite { registrant: Some("acme".into()), ..cite("1", "a.example", A) };
        let r2 = Cite { registrant: Some("acme".into()), ..cite("2", "b.example", B) };
        assert_eq!(support(&[r1, r2, cite("3", "c.example", C)], &Params::default()).origins, 2);
        let t = |id: &str, o: &str, text: &str, at: i64| Cite { first_seen: Some(at), ..cite(id, o, text) };
        let burst = support(&[t("1", "a.example", A, 1_000_000), t("2", "b.example", B, 1_000_000 + 3600), t("3", "c.example", C, 1_000_000 + 7200)], &Params::default());
        assert_eq!(burst.origins, 1, "three pages first seen within a day are one burst");
        let spread = support(&[t("1", "a.example", A, 0), t("2", "b.example", B, 10 * 86_400), t("3", "c.example", C, 20 * 86_400)], &Params::default());
        assert_eq!(spread.origins, 3);
        let unknown = support(&[cite("1", "a.example", A), cite("2", "b.example", B), cite("3", "c.example", C)], &Params::default());
        assert_eq!(unknown.origins, 3, "no first-seen time, no burst rule");
    }

    #[test]
    fn a_derived_source_adds_no_origin_to_the_claim_it_was_built_from() {
        let page = Cite { derived: true, ..cite("p", "pages.example", D) };
        let s = support(&[cite("1", "a.example", A), cite("2", "b.example", B), page], &Params::default());
        assert_eq!(s.origins, 2);
        assert_eq!(s.zeroed, vec![("p".to_string(), "derived")]);
    }

    #[test]
    fn trusted_sources_are_listed_in_scope_or_primary_or_reputable_and_never_more_than_the_origins() {
        let listed = Cite { listed_in_scope: true, ..unfamiliar("l", "listed.example", A) };
        let primary = Cite { class: Class::Primary, ..cite("p", "p.example", B) };
        let plain = unfamiliar("u", "blog.example", C);
        let s = support(&[listed, primary, plain], &Params::default());
        assert_eq!((s.origins, s.trusted_sources), (3, 2));
        // A weak-by-list host on the trusted list is vetoed: it counts zero, so it cannot be a trusted source either.
        let weak_listed = Cite { class: Class::Weak, unfamiliar: false, listed_in_scope: true, ..cite("w", "w.example", A) };
        let s = support(&[weak_listed, unfamiliar("u", "blog.example", C)], &Params::default());
        assert_eq!((s.origins, s.trusted_sources), (1, 0));
        // Two cites in one cluster are one trusted source.
        let a = Cite { listed_in_scope: true, ..cite("1", "same.example", A) };
        let b = cite("2", "same.example", B);
        assert_eq!(support(&[a, b], &Params::default()).trusted_sources, 1);
    }

    #[test]
    fn the_count_is_deterministic_and_reports_json() {
        let cites = [cite("1", "a.example", A), cite("2", "b.example", B)];
        assert_eq!(support(&cites, &Params::default()), support(&cites, &Params::default()));
        let j = support(&cites, &Params::default()).to_json();
        assert_eq!((j["origins"].as_u64(), j["trusted_sources"].as_u64()), (Some(2), Some(2)));
    }
}
