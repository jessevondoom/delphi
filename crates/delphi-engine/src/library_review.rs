//! Contested pairs and review-due claims (`specs/052` §3.4, slice KS4), read from the library's claims.
//!
//! Both are derived on every read and write nothing: a pair is two live claims of one kind that say nearly the same
//! thing in different words (no model is asked, so the match is a word overlap and may be a false alarm); a claim is
//! due for review when it has not been confirmed for 90 days or its `valid_until` is near. The daemon never picks a
//! winner and never retires, edits or promotes anything here: the operator reads both sides, and a claim that
//! `valid_until` has passed is already retired by `claims::status`. `confirm_claim` is the one write, the operator's:
//! it moves `last_confirmed` and nothing else, so it cannot raise a status.

use super::claims::{status, words};
use super::*;
use delphi_core::knowledge_status::Status;

/// A claim not confirmed for this many days is due for review (`specs/050` ML-3, OD-3).
pub const REVIEW_DAYS: i64 = 90;
/// A `valid_until` within this many days is due too.
pub const EXPIRY_WARN_DAYS: i64 = 14;
/// Word overlap (Jaccard, words of four letters or more) from which two different claims are a possible pair.
const SIMILAR: f64 = 0.6;
const MIN_WORDS: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub a: String,
    pub b: String,
}

impl Pair {
    pub fn id(&self) -> String {
        format!("pair:{}:{}", self.a, self.b)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Due {
    pub id: String,
    pub reason: String,
    pub age_days: i64,
}

fn day_number(rfc: &str) -> Option<i64> {
    delphi_core::time::parse_rfc3339(rfc).map(|t| t.div_euclid(86_400))
}

fn live(c: &Value) -> bool {
    !matches!(status(c), Status::Disproved | Status::Retired) && c["kind"] != "attributed"
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Whether the two texts say nearly the same thing but not the same words.
fn possible_conflict(a: &str, b: &str) -> bool {
    let (wa, wb) = (words(a), words(b));
    if wa.len() < MIN_WORDS || wb.len() < MIN_WORDS || norm(a) == norm(b) {
        return false;
    }
    let both = wa.intersection(&wb).count() as f64;
    let any = wa.union(&wb).count() as f64;
    both / any >= SIMILAR
}

/// Ages and reasons for a claim record; `None` when it is not due.
pub fn due_reason(c: &Value, today: i64) -> Option<Due> {
    if !live(c) {
        return None;
    }
    let id = c["id"].as_str()?.to_string();
    let age = c["last_confirmed"].as_str().and_then(day_number).map_or(0, |d| (today - d).max(0));
    if let Some(left) = c["valid_until"].as_str().and_then(|v| day_number(&format!("{}T00:00:00Z", v))).map(|d| d - today) {
        if left <= EXPIRY_WARN_DAYS {
            return Some(Due { id, reason: format!("valid until {}: {} day(s) left", c["valid_until"].as_str().unwrap_or(""), left.max(0)), age_days: age });
        }
    }
    (age >= REVIEW_DAYS).then(|| Due { id, reason: format!("not confirmed for {} days (the window is {})", age, REVIEW_DAYS), age_days: age })
}

impl Library {
    /// Pairs of live claims, same kind and scope, that may disagree. Superseded claims are retired and so never pair; a
    /// claim in no pair is not mentioned. Sorted, each pair once.
    pub fn contested_pairs(&self) -> Vec<Pair> {
        let live: Vec<Value> = self.claims().into_iter().filter(live).collect();
        let mut out = Vec::new();
        for (i, a) in live.iter().enumerate() {
            for b in &live[i + 1..] {
                if a["kind"] == b["kind"] && a["scope"] == b["scope"] && possible_conflict(a["text"].as_str().unwrap_or(""), b["text"].as_str().unwrap_or("")) {
                    out.push(Pair { a: a["id"].as_str().unwrap_or("").into(), b: b["id"].as_str().unwrap_or("").into() });
                }
            }
        }
        out
    }

    /// Claims due for review at `now` (an RFC 3339 timestamp), oldest first.
    pub fn review_due(&self, now: &str) -> Vec<Due> {
        let Some(today) = day_number(now) else { return Vec::new() };
        let mut out: Vec<Due> = self.claims().iter().filter_map(|c| due_reason(c, today)).collect();
        out.sort_by(|a, b| b.age_days.cmp(&a.age_days).then(a.id.cmp(&b.id)));
        out
    }

    /// The operator says the claim still holds: `last_confirmed` moves to now. Nothing else changes, least of all the
    /// status, which no confirmation can raise.
    pub fn confirm_claim(&self, id: &str) -> Result<Value, String> {
        let mut c = self.claim(id).ok_or_else(|| format!("no claim `{}`", id))?;
        if !live(&c) {
            return Err("the claim is retired, disproved or an opinion; there is nothing to confirm".into());
        }
        c["last_confirmed"] = json!(delphi_core::time::now_rfc3339());
        self.save_claim(&c)?;
        self.log("claim_confirmed", json!({"id": id}));
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::claims::ClaimOpts;

    fn lib() -> (tempfile::TempDir, Library) {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        (d, l)
    }

    fn put(l: &Library, text: &str, opts: ClaimOpts) -> String {
        l.add_claim(text, &opts, &[]).unwrap()["id"].as_str().unwrap().to_string()
    }

    fn set(l: &Library, id: &str, k: &str, v: &str) {
        let mut c = l.claim(id).unwrap();
        c[k] = json!(v);
        l.save_claim(&c).unwrap();
    }

    #[test]
    fn two_claims_that_say_nearly_the_same_in_different_words_are_a_pair_and_nothing_is_changed() {
        let (_d, l) = lib();
        let a = put(&l, "the retry limit for the billing service is three attempts", ClaimOpts::default());
        let b = put(&l, "the retry limit for the billing service is five attempts", ClaimOpts::default());
        put(&l, "release notes are written on fridays by the docs team", ClaimOpts::default());
        let before: Vec<Value> = l.claims();
        let pairs = l.contested_pairs();
        assert_eq!(pairs.len(), 1);
        let ids = [&pairs[0].a, &pairs[0].b];
        assert!(ids.contains(&&a) && ids.contains(&&b));
        assert_eq!(l.claims(), before, "reading pairs writes nothing; no winner is picked");
        assert_eq!(put(&l, "the retry limit for the billing service is three attempts", ClaimOpts::default()), a, "the same words are the same claim, not a pair");
        assert_eq!(l.contested_pairs().len(), 1);
    }

    #[test]
    fn a_superseded_claim_and_an_opinion_are_not_paired() {
        let (_d, l) = lib();
        let old = put(&l, "the retry limit for the billing service is three attempts", ClaimOpts::default());
        put(&l, "the retry limit for the billing service is five attempts", ClaimOpts { supersedes: Some(old), ..Default::default() });
        assert!(l.contested_pairs().is_empty());
        let k = |kind: &str| ClaimOpts { kind: Some(kind.into()), ..Default::default() };
        put(&l, "the billing service retry limit is clearly three attempts", k("attributed"));
        put(&l, "the billing service retry limit is clearly four attempts", k("attributed"));
        assert!(l.contested_pairs().is_empty());
    }

    #[test]
    fn a_claim_is_due_after_ninety_days_or_when_valid_until_is_near_and_confirming_changes_only_the_date() {
        let (_d, l) = lib();
        let old = put(&l, "the staging cluster runs on three nodes", ClaimOpts::default());
        let fresh = put(&l, "the production cluster runs on nine nodes", ClaimOpts::default());
        let soon = put(&l, "the sandbox profile allows loopback only", ClaimOpts { valid_until: Some("2026-11-05".into()), ..Default::default() });
        set(&l, &old, "last_confirmed", "2026-07-01T00:00:00.000Z");
        set(&l, &fresh, "last_confirmed", "2026-10-20T00:00:00.000Z");
        set(&l, &soon, "last_confirmed", "2026-10-20T00:00:00.000Z");
        let due = l.review_due("2026-10-29T09:00:00.000Z");
        assert_eq!(due.iter().map(|d| d.id.clone()).collect::<Vec<_>>(), vec![old.clone(), soon.clone()]);
        assert!(due[0].reason.contains("not confirmed for 120 days") && due[1].reason.contains("7 day(s) left"));
        let before = status(&l.claim(&old).unwrap());
        let c = l.confirm_claim(&old).unwrap();
        assert_eq!(status(&c), before, "a confirmation cannot raise a status");
        assert!(c["validation"].is_null());
        assert!(!l.review_due(&delphi_core::time::now_rfc3339()).iter().any(|d| d.id == old));
        let expired = put(&l, "the legacy endpoint is available", ClaimOpts { valid_until: Some("2020-01-01".into()), ..Default::default() });
        assert!(l.review_due("2026-10-29T09:00:00.000Z").iter().all(|d| d.id != expired), "an expired claim is already retired, not due");
        assert!(l.confirm_claim(&expired).is_err());
    }
}
