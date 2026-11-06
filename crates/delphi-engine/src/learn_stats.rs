//! Rule and skill statistics for `/retro` (`specs/031` §3.4, slice L4; `specs/004` §5.6).
//!
//! Display and analysis only. Nothing here writes a file, withdraws or rewrites a rule, raises a tier or changes what
//! any prompt loads: the functions take [`TaskRecord`]s (folded from daemon-written events by
//! [`crate::learn::task_records`]; a host-reported payload is never read) and return numbers and text for the operator.
//!
//! Guardrails the Council set, each enforced and tested below:
//! - a score needs [`MIN_TASKS`] finished build tasks carrying the rule, else it reads `not enough data (n=..)`;
//! - a score is always shown with `n` and a 95% Wilson interval, never bare;
//! - the comparison with tasks that did not carry the rule is labelled `co-occurred`, never `caused`;
//! - decay only lowers display priority and has a floor; no rule is ever dropped from the list for looking unused;
//! - a fixed fifth of tasks is a holdout: it is counted apart and never feeds the ordering or the decay;
//! - exposure randomization (off by default, `[learning] exposure_randomization`) never withholds a `security` rule, an
//!   `always` rule, or anything from a holdout task.

use std::collections::{BTreeMap, BTreeSet};

use crate::learn::{Outcome, TaskRecord};

/// Pre-registered minimum: finished build tasks that carried a rule (or skill) before any score is shown for it.
/// Fixed here, not a setting, so it cannot be lowered to make a score appear.
pub const MIN_TASKS: usize = 10;
/// Pre-registered minimum for the tasks that did not carry it, before the comparison is shown.
pub const MIN_COMPARISON: usize = 10;
/// A newly ratified rule is withheld from about half of eligible tasks for this many tasks (`specs/031` §3.4).
pub const EXPOSURE_WINDOW: usize = 20;
/// Tasks in a holdout: one in `HOLDOUT_EVERY`, chosen by a hash of the task id.
pub const HOLDOUT_EVERY: u8 = 5;
/// Display priority halves every this many tasks since the rule was last carried; it never falls below [`DECAY_FLOOR`].
pub const DECAY_HALF_LIFE_TASKS: f64 = 50.0;
pub const DECAY_FLOOR: f64 = 0.25;

fn digest(parts: &[&str]) -> u8 {
    let mut h = blake3::Hasher::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update(&[0]);
    }
    h.finalize().as_bytes()[0]
}

/// True for a task in the holdout. Stable for a task id, so the same tasks are held out on every run.
pub fn is_holdout(task_id: &str) -> bool {
    digest(&["holdout", task_id]) % HOLDOUT_EVERY == 0
}

/// Whether exposure randomization withholds `rule` from `task_id`. `seen` is how many tasks have already carried or been
/// offered the rule since it was ratified. Deterministic (a hash, not a clock or a random draw) so a run can be replayed.
/// `protected` is a `security` or `always` rule. Off means never.
pub fn withhold(enabled: bool, task_id: &str, rule_id: &str, seen: usize, protected: bool) -> bool {
    if !enabled || protected || seen >= EXPOSURE_WINDOW || is_holdout(task_id) {
        return false;
    }
    digest(&["withhold", task_id, rule_id]) % 2 == 0
}

/// `k` of `n` with a 95% Wilson score interval, as fractions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rate {
    pub k: usize,
    pub n: usize,
    pub lo: f64,
    pub hi: f64,
}

impl Rate {
    pub fn new(k: usize, n: usize) -> Rate {
        if n == 0 {
            return Rate { k, n, lo: 0.0, hi: 1.0 };
        }
        let (nf, p, z) = (n as f64, k as f64 / n as f64, 1.96_f64);
        let denom = 1.0 + z * z / nf;
        let centre = (p + z * z / (2.0 * nf)) / denom;
        let half = z * ((p * (1.0 - p) / nf + z * z / (4.0 * nf * nf)).sqrt()) / denom;
        Rate { k, n, lo: (centre - half).max(0.0), hi: (centre + half).min(1.0) }
    }

    /// `11 of 14 (79%, 95% interval 52% to 92%)`.
    pub fn show(&self) -> String {
        let pct = |x: f64| (x * 100.0).round() as i64;
        let p = if self.n == 0 { 0.0 } else { self.k as f64 / self.n as f64 };
        format!("{} of {} ({}%, 95% interval {}% to {}%)", self.k, self.n, pct(p), pct(self.lo), pct(self.hi))
    }
}

/// What a score reads as: a rate with its n and interval, or the count that is too small.
#[derive(Debug, Clone, PartialEq)]
pub enum Score {
    NotEnoughData { n: usize },
    Shown(Rate),
}

impl Score {
    fn of(k: usize, n: usize, min: usize) -> Score {
        if n < min {
            Score::NotEnoughData { n }
        } else {
            Score::Shown(Rate::new(k, n))
        }
    }

    pub fn show(&self) -> String {
        match self {
            Score::NotEnoughData { n } => format!("not enough data (n={}, needs {})", n, MIN_TASKS),
            Score::Shown(r) => r.show(),
        }
    }
}

/// Counts over one set of tasks. Only finished tasks that built (at least one `build.attempt`) count: the first-pass
/// signal means nothing for a task that never builds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub built: usize,
    pub first_pass: usize,
    pub escalated: usize,
}

impl Counts {
    fn add(&mut self, t: &TaskRecord) {
        if t.attempts >= 1 && matches!(t.outcome, Outcome::Done | Outcome::Escalated | Outcome::Failed) {
            self.built += 1;
            if t.first_pass() {
                self.first_pass += 1;
            }
            if t.outcome == Outcome::Escalated {
                self.escalated += 1;
            }
        }
    }
}

/// One rule's (or skill's) numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemStat {
    pub kind: &'static str,
    pub scope: String,
    pub id: String,
    /// Tasks (finished or not, holdout or not) that carried it.
    pub carried: usize,
    /// Tasks that carried it, outside the holdout: what ordering and decay use.
    pub tuning: Counts,
    /// The same, in the holdout. Shown for checking; never used to order or decay.
    pub holdout: Counts,
    /// Tasks outside the holdout that did not carry it, for the `co-occurred` comparison.
    pub without: Counts,
    /// Tasks since a task last carried it (0 = the latest task).
    pub idle_tasks: usize,
}

impl ItemStat {
    pub fn score(&self) -> Score {
        Score::of(self.tuning.first_pass, self.tuning.built, MIN_TASKS)
    }

    pub fn without_score(&self) -> Score {
        Score::of(self.without.first_pass, self.without.built, MIN_COMPARISON)
    }

    /// Display priority in 0.25..=1.0. A rule not carried for a long time sorts lower; it stays listed.
    pub fn priority(&self) -> f64 {
        decay(self.idle_tasks)
    }
}

/// `0.5^(idle / half-life)`, never below [`DECAY_FLOOR`].
pub fn decay(idle_tasks: usize) -> f64 {
    (0.5_f64.powf(idle_tasks as f64 / DECAY_HALF_LIFE_TASKS)).max(DECAY_FLOOR)
}

/// Stats for every rule, lesson and skill any task carried, ordered by display priority then by id. Nothing is dropped:
/// an item with no data still appears, and one that decayed to the floor still appears. Skills come from
/// `snapshot.loaded` `skills` once the daemon logs them; until then the list is rules only.
pub fn item_stats(tasks: &[TaskRecord], skills: &BTreeMap<String, BTreeSet<(String, String)>>) -> Vec<ItemStat> {
    let mut keys: BTreeSet<(&'static str, String, String)> = BTreeSet::new();
    for t in tasks {
        for (s, i) in &t.rules {
            keys.insert(("rule", s.clone(), i.clone()));
        }
        for (s, i) in skills.get(&t.task_id).into_iter().flatten() {
            keys.insert(("skill", s.clone(), i.clone()));
        }
    }
    let carries = |t: &TaskRecord, kind: &str, scope: &str, id: &str| {
        let key = (scope.to_string(), id.to_string());
        if kind == "rule" { t.rules.contains(&key) } else { skills.get(&t.task_id).is_some_and(|s| s.contains(&key)) }
    };
    let mut out: Vec<ItemStat> = keys
        .into_iter()
        .map(|(kind, scope, id)| {
            let mut st = ItemStat { kind, scope: scope.clone(), id: id.clone(), carried: 0, tuning: Counts::default(), holdout: Counts::default(), without: Counts::default(), idle_tasks: tasks.len() };
            for (pos, t) in tasks.iter().enumerate() {
                let held = is_holdout(&t.task_id);
                if carries(t, kind, &scope, &id) {
                    st.carried += 1;
                    st.idle_tasks = tasks.len() - 1 - pos;
                    if held { st.holdout.add(t) } else { st.tuning.add(t) }
                } else if !held {
                    st.without.add(t);
                }
            }
            st
        })
        .collect();
    out.sort_by(|a, b| b.priority().partial_cmp(&a.priority()).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.kind.cmp(b.kind)).then_with(|| a.scope.cmp(&b.scope)).then_with(|| a.id.cmp(&b.id)));
    out
}

/// Skill ids per task from `snapshot.loaded` events (`skills: [{scope, id}]`). Daemon-written only; empty today.
pub fn skills_by_task(events: &[delphi_core::event::EventEnvelope]) -> BTreeMap<String, BTreeSet<(String, String)>> {
    let mut out: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    for e in events.iter().filter(|e| e.kind == "snapshot.loaded") {
        let Some(task) = e.task_id.as_deref() else { continue };
        for r in e.payload["skills"].as_array().into_iter().flatten() {
            if let (Some(s), Some(i)) = (r["scope"].as_str(), r["id"].as_str()) {
                out.entry(task.to_string()).or_default().insert((s.to_string(), i.to_string()));
            }
        }
    }
    out
}

/// The `/retro` stats block. Pure text; says what the numbers are and are not.
pub fn render(stats: &[ItemStat], exposure_randomization: bool) -> String {
    let mut out = String::from("Rule and skill statistics (display only: nothing here changes a rule, a tier or what is loaded; every change is yours to make at /retro)\n");
    out.push_str(&format!("  exposure randomization: {}\n", if exposure_randomization { "on (a ratified rule is withheld from about half of its first 20 tasks; security and always rules, and holdout tasks, never)" } else { "off" }));
    out.push_str(&format!("  scores need {} finished build tasks; one task in {} is a holdout, counted apart and never used to order or decay\n", MIN_TASKS, HOLDOUT_EVERY));
    if stats.is_empty() {
        out.push_str("  no task has carried a rule yet\n");
        return out;
    }
    for s in stats {
        out.push_str(&format!("\n  {} {} ({}): carried by {} task{}, priority {:.2}\n", s.kind, s.id, s.scope, s.carried, if s.carried == 1 { "" } else { "s" }, s.priority()));
        out.push_str(&format!("    first pass with it: {}\n", s.score().show()));
        match (s.score(), s.without_score()) {
            (Score::Shown(_), Score::Shown(_)) => out.push_str(&format!("    first pass without it: {}\n    co-occurred only: other things differ between these tasks; this does not show the {} helps or hurts\n", s.without_score().show(), s.kind)),
            _ => out.push_str(&format!("    comparison with tasks that did not carry it: not enough data (n={}, needs {})\n", s.without.built, MIN_COMPARISON)),
        }
        if s.holdout.built > 0 {
            out.push_str(&format!("    holdout (not used for ordering): {} of {} first pass\n", s.holdout.first_pass, s.holdout.built));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::event::EventEnvelope;
    use serde_json::json;

    fn task(id: &str, rules: &[&str], first_pass: bool) -> TaskRecord {
        TaskRecord { task_id: id.into(), outcome: Outcome::Done, rules: rules.iter().map(|r| ("release".to_string(), r.to_string())).collect(), attempts: if first_pass { 1 } else { 2 }, denied_cards: 0, reverted_decisions: 0, cost_usd: 0.0 }
    }

    fn non_holdout(prefix: &str, n: usize) -> Vec<String> {
        (0..).map(|i| format!("{}{}", prefix, i)).filter(|t| !is_holdout(t)).take(n).collect()
    }

    #[test]
    fn below_the_minimum_no_score_only_the_count() {
        let ids = non_holdout("a", MIN_TASKS - 1);
        let tasks: Vec<_> = ids.iter().map(|i| task(i, &["H-1"], true)).collect();
        let s = item_stats(&tasks, &Default::default());
        assert_eq!(s[0].score(), Score::NotEnoughData { n: MIN_TASKS - 1 });
        assert!(s[0].score().show().contains("not enough data (n=9"));
        assert!(!render(&s, false).contains("95% interval"));
    }

    #[test]
    fn a_score_always_carries_n_and_an_interval() {
        let ids = non_holdout("b", 14);
        let tasks: Vec<_> = ids.iter().enumerate().map(|(i, t)| task(t, &["H-1"], i < 11)).collect();
        let shown = item_stats(&tasks, &Default::default())[0].score().show();
        assert!(shown.starts_with("11 of 14 (79%, 95% interval "), "{}", shown);
        let r = Rate::new(11, 14);
        assert!(r.lo < 0.79 && r.hi > 0.79 && r.lo > 0.4 && r.hi < 0.95);
    }

    #[test]
    fn the_comparison_says_co_occurred_and_never_caused() {
        let with = non_holdout("w", 12);
        let without = non_holdout("x", 12);
        let mut tasks: Vec<_> = with.iter().map(|t| task(t, &["H-1"], true)).collect();
        tasks.extend(without.iter().map(|t| task(t, &[], false)));
        let text = render(&item_stats(&tasks, &Default::default()), false).to_lowercase();
        assert!(text.contains("co-occurred") && text.contains("first pass without it"));
        assert!(!text.contains("caused by") && !text.contains(" causes ") && !text.contains("because of"));
    }

    #[test]
    fn decay_only_lowers_priority_and_nothing_is_dropped() {
        let ids = non_holdout("d", 400);
        let mut tasks: Vec<_> = vec![task(&ids[0], &["H-OLD"], true)];
        tasks.extend(ids[1..].iter().map(|t| task(t, &["H-NEW"], true)));
        let s = item_stats(&tasks, &Default::default());
        assert_eq!(s.len(), 2, "an unused rule stays listed");
        let old = s.iter().find(|x| x.id == "H-OLD").unwrap();
        assert!((old.priority() - DECAY_FLOOR).abs() < 1e-9, "floor, not zero");
        assert_eq!(s[0].id, "H-NEW");
        assert_eq!(decay(0), 1.0);
        assert!(decay(10_000) >= DECAY_FLOOR);
    }

    #[test]
    fn holdout_tasks_never_feed_the_score_or_the_ordering() {
        let held: Vec<String> = (0..).map(|i| format!("h{}", i)).filter(|t| is_holdout(t)).take(30).collect();
        let tasks: Vec<_> = held.iter().map(|t| task(t, &["H-1"], true)).collect();
        let s = &item_stats(&tasks, &Default::default())[0];
        assert_eq!(s.tuning, Counts::default());
        assert_eq!(s.holdout.built, 30);
        assert_eq!(s.score(), Score::NotEnoughData { n: 0 });
        assert!(is_holdout("x") == is_holdout("x"));
    }

    #[test]
    fn exposure_randomization_is_off_by_default_and_spares_what_it_must() {
        let ts: Vec<String> = (0..200).map(|i| format!("t{}", i)).collect();
        assert!(ts.iter().all(|t| !withhold(false, t, "H-1", 0, false)), "off means never");
        assert!(ts.iter().all(|t| !withhold(true, t, "H-1", 0, true)), "protected rules are never withheld");
        assert!(ts.iter().all(|t| !withhold(true, t, "H-1", EXPOSURE_WINDOW, false)), "only the first window");
        assert!(ts.iter().filter(|t| is_holdout(t)).all(|t| !withhold(true, t, "H-1", 0, false)), "holdout always carries it");
        let n = ts.iter().filter(|t| withhold(true, t, "H-1", 0, false)).count();
        assert!(n > 40 && n < 120, "about half of the rest: {}", n);
        assert_eq!(withhold(true, "t5", "H-1", 3, false), withhold(true, "t5", "H-1", 3, false));
    }

    #[test]
    fn only_daemon_event_kinds_are_counted() {
        let mut e = EventEnvelope::new("m", "host.report", json!({"rules": [{"scope": "release", "id": "H-FAKE"}], "status": "done"}));
        e.task_id = Some("t".into());
        let mut loaded = EventEnvelope::new("m", "snapshot.loaded", json!({"rules": [{"scope": "release", "id": "H-1"}]}));
        loaded.task_id = Some("t".into());
        let tasks = crate::learn::task_records(&[e, loaded]);
        let s = item_stats(&tasks, &Default::default());
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].id, "H-1");
    }

    #[test]
    fn skills_get_the_same_stats() {
        let mut e = EventEnvelope::new("m", "snapshot.loaded", json!({"rules": [], "skills": [{"scope": "operator", "id": "deploy"}]}));
        e.task_id = Some("t".into());
        let sk = skills_by_task(&[e.clone()]);
        let tasks = crate::learn::task_records(&[e]);
        let s = item_stats(&tasks, &sk);
        assert_eq!((s[0].kind, s[0].id.as_str()), ("skill", "deploy"));
    }
}
