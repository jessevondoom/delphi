//! The tier audit and the roll-back (`specs/100` §7): a way back for a tier that proves unreliable.
//!
//! The audit is a **roll-back trigger, not a promotion gate** (the operator, 2026-11-10). It draws a random sample, stratified by source class,
//! of the claims that reached a tier, shows the operator each claim first, records their own `true`, `false` or `unsure`, and only then reveals
//! the sources. What is new is what the result does: a tier whose sample shows a hard failure, or whose error rate's upper bound is above the
//! threshold the operator set, is **latched**. The latch is a daemon-state file beside the validator's own (`<workspace>/local/validation/`, which no
//! agent reads or writes); the labels fall back one tier, nothing is deleted, and only the operator raises it again. It uses the audit journal and
//! the answer type of `specs/040` §15.3 rather than a second mechanism.
//!
//! * The report counts only `true` and `false` answers and reads only events this module and the journal wrote. A small sample bounds nothing,
//!   so below the floor the report prints the numbers and **no verdict** (`specs/100` §7.4).
//! * Nothing here reads a tier as a criterion for anything but "which claims to sample": the tier is the claim's status, derived from its tags.

use crate::fact_audit::{current_batch, start_new_batch, Answer, Journal};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The sample floor per tier (`specs/100` §7.4, OD-2): 30 audited claims bound a clean result to about 10%, 60 to about 5%. A default for the
/// operator to change, not a pre-registered number.
pub const SAMPLE_FLOOR: usize = 30;
/// How many claims one `tier-audit` run draws when the operator does not say.
pub const DEFAULT_SAMPLE: usize = 10;
/// The latch file's name in the validator's state directory.
pub const LATCH_FILE: &str = "tier_latch.json";

/// The tiers an audit can sample: `batch` is the claims an accept-all marked `batch-accepted` (`delphi validation accept-all`).
pub const AUDITABLE: [&str; 3] = ["substantiated", "vetted", "batch"];
/// The tiers a roll-back can latch. `corroborated` is spec 098's count and has no audit of its own.
pub const ROLLABLE: [&str; 2] = ["substantiated", "vetted"];

pub fn latch_file(state: &Path) -> PathBuf {
    state.join(LATCH_FILE)
}

/// The tiers that are latched now. No file means none; a file that cannot be read means **both** (fail safe, as the validator's latch does).
pub fn latched(state: &Path) -> BTreeSet<String> {
    match std::fs::read(latch_file(state)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
        Err(_) => ROLLABLE.iter().map(|s| s.to_string()).collect(),
        Ok(b) => match serde_json::from_slice::<Value>(&b) {
            Ok(v) => v["latched"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default(),
            Err(_) => ROLLABLE.iter().map(|s| s.to_string()).collect(),
        },
    }
}

fn write_latch(state: &Path, map: &serde_json::Map<String, Value>) -> Result<(), String> {
    std::fs::create_dir_all(state).map_err(|e| e.to_string())?;
    let body = serde_json::to_vec_pretty(&json!({"latched": map})).map_err(|e| e.to_string())?;
    std::fs::write(latch_file(state), body).map_err(|e| e.to_string())
}

fn read_latch_map(state: &Path) -> serde_json::Map<String, Value> {
    std::fs::read(latch_file(state)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v["latched"].as_object().cloned()).unwrap_or_default()
}

/// What an audit of a tier has found so far, in the current batch: `true` and `false` answers only; `unsure` counts for nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub sample: usize,
    pub errors: usize,
    pub unsure: usize,
    /// The observed error rate, 0 when there is no sample.
    pub rate: f64,
    /// The rule-of-three bound (3 divided by the sample) when there were no errors; otherwise the upper end of the 95% Wilson interval.
    pub bound: f64,
}

/// The upper bound on an error rate: `3/n` for no errors (the rule of three), else the upper end of the Wilson 95% interval. 1.0 for no sample.
pub fn upper_bound(sample: usize, errors: usize) -> f64 {
    if sample == 0 {
        return 1.0;
    }
    let n = sample as f64;
    if errors == 0 {
        return (3.0 / n).min(1.0);
    }
    let (p, z) = (errors as f64 / n, 1.96_f64);
    ((p + z * z / (2.0 * n) + z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt()) / (1.0 + z * z / n)).min(1.0)
}

/// The audit's answers for a tier in the current batch. A restore starts a new batch, so a tier that was latched is judged on fresh answers.
pub fn stats(state: &Path, tier: &str) -> Stats {
    let batch = current_batch(state);
    let (mut sample, mut errors, mut unsure) = (0, 0, 0);
    for e in Journal::new(state).all().iter().filter(|e| e["kind"] == "tier_audit" && e["tier"] == tier && e["batch"] == batch.as_str()) {
        match e["answer"].as_str() {
            Some("true") => sample += 1,
            Some("false") => {
                sample += 1;
                errors += 1;
            }
            Some("unsure") => unsure += 1,
            _ => {}
        }
    }
    Stats { sample, errors, unsure, rate: if sample == 0 { 0.0 } else { errors as f64 / sample as f64 }, bound: upper_bound(sample, errors) }
}

/// The error-rate threshold the operator set for a tier (percent), if any. It has **no default** (`specs/100` §7.2 item 1): the operator sets it
/// when first running the audit, with 040 §15.4's own figures as reference. The latest setting wins.
pub fn threshold(state: &Path, tier: &str) -> Option<f64> {
    Journal::new(state).all().iter().rev().find(|e| e["kind"] == "tier_threshold" && e["tier"] == tier).and_then(|e| e["percent"].as_f64())
}

/// Operator act: set a tier's threshold. Journaled as the operator's.
pub fn set_threshold(state: &Path, tier: &str, percent: f64, now: i64) -> Result<String, String> {
    if !AUDITABLE.contains(&tier) {
        return Err(format!("`{tier}` is not a tier that is audited (substantiated, vetted or batch)"));
    }
    if !(0.0..=100.0).contains(&percent) || percent.is_nan() {
        return Err("the threshold is a percentage between 0 and 100".into());
    }
    Journal::new(state).append("tier_threshold", json!({"tier": tier, "percent": percent, "by": "operator"}), now);
    Ok(format!("the {tier} tier rolls back when the upper bound of its error rate, after at least {SAMPLE_FLOOR} audited claims, is above {percent}%"))
}

/// Latch a tier. The tier's label and fast lane switch off (the labels fall back one tier), **nothing is deleted or re-labelled in storage**, and a
/// `tier.rollback` event (`tier_rollback` in the journal) records the trigger, the sample, the errors, the bound and the audit batch. Idempotent.
///
/// `trigger` is `operator`, `hard_failure` or `error_rate`. An operator roll-back needs a reason of at least 8 characters. Items batch-accepted since
/// the last good audit are flagged `re-audit due` by `accept_all::flag_reaudit`, which the caller runs after a roll-back (this module has no library); the count in this event is 0 for that reason.
pub fn rollback(state: &Path, tier: &str, trigger: &str, reason: &str, now: i64) -> Result<String, String> {
    if !ROLLABLE.contains(&tier) {
        return Err(format!("`{tier}` cannot be rolled back; the tiers are substantiated and vetted"));
    }
    if !matches!(trigger, "operator" | "hard_failure" | "error_rate") {
        return Err("the trigger is operator, hard_failure or error_rate".into());
    }
    if trigger == "operator" && reason.trim().chars().count() < 8 {
        return Err("a roll-back needs a reason of at least 8 characters".into());
    }
    let mut map = read_latch_map(state);
    if map.contains_key(tier) {
        return Ok(format!("the {tier} tier is already rolled back"));
    }
    let st = stats(state, tier);
    let batch = current_batch(state);
    map.insert(tier.to_string(), json!({"trigger": trigger, "reason": reason.chars().take(200).collect::<String>(), "at": now, "batch": batch}));
    write_latch(state, &map)?;
    Journal::new(state).append(
        "tier_rollback",
        json!({"tier": tier, "trigger": trigger, "reason": reason.chars().take(200).collect::<String>(), "sample": st.sample, "errors": st.errors, "bound": st.bound, "audit_batch": batch, "reaudit_due": 0}),
        now,
    );
    let falls_to = if tier == "vetted" { "substantiated" } else { "corroborated" };
    Ok(format!("the {tier} tier is rolled back ({trigger}): its claims read {falls_to} until you restore it; nothing was deleted and every claim keeps its tags"))
}

/// Operator act: raise a latched tier again. Needs a reason; starts a new audit batch, so the tier is judged on fresh answers. An agent cannot
/// run it: the caller (`delphi validation tier restore`) refuses inside an agent seat.
pub fn restore(state: &Path, tier: &str, reason: &str, now: i64) -> Result<String, String> {
    if !ROLLABLE.contains(&tier) {
        return Err(format!("`{tier}` is not a tier that is rolled back"));
    }
    if reason.trim().chars().count() < 8 {
        return Err("a restore needs a reason of at least 8 characters".into());
    }
    let mut map = read_latch_map(state);
    if map.remove(tier).is_none() {
        return Err(format!("the {tier} tier is not rolled back"));
    }
    write_latch(state, &map)?;
    let batch = start_new_batch(state);
    Journal::new(state).append("tier_restore", json!({"tier": tier, "reason": reason.chars().take(200).collect::<String>(), "by": "operator", "audit_batch": batch}), now);
    Ok(format!("the {tier} tier is restored; a new audit batch ({batch}) starts, so it is judged on fresh answers"))
}

/// The audit's third trigger (`specs/100` §7.2 item 2): a hard failure the library recorded on a claim (a protected-class claim that reached vetted, a
/// poisoned page that passed) latches the vetted tier at once, as an audited false claim does. The same roll-back and event as every other trigger,
/// so it only lowers a label and deletes nothing; `accept_all::latch_on_hard_failures` is the one caller.
pub fn trip_on_hard_failure(state: &Path, reason: &str, now: i64) -> Result<String, String> {
    rollback(state, "vetted", "hard_failure", reason, now)
}

/// When the operator last restored this tier, if ever. A hard failure recorded before it has been dealt with by that restore and does not latch the
/// tier again; one recorded after it does.
pub fn last_restore(state: &Path, tier: &str) -> Option<i64> {
    Journal::new(state).all().iter().filter(|e| e["kind"] == "tier_restore" && e["tier"] == tier).filter_map(|e| e["at"].as_i64()).max()
}

/// A claim the operator is asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierCard {
    pub card: String,
    pub tier: String,
    pub class: String,
    pub at: i64,
}

/// The cards drawn in this batch that nobody has answered.
pub fn open_tier_cards(state: &Path) -> Vec<TierCard> {
    let batch = current_batch(state);
    let events = Journal::new(state).all();
    let answered: BTreeSet<(String, String)> =
        events.iter().filter(|e| e["kind"] == "tier_audit" && e["batch"] == batch.as_str()).filter_map(|e| Some((e["card"].as_str()?.to_string(), e["tier"].as_str()?.to_string()))).collect();
    events
        .iter()
        .filter(|e| e["kind"] == "tier_card" && e["batch"] == batch.as_str())
        .filter_map(|c| Some(TierCard { card: c["card"].as_str()?.into(), tier: c["tier"].as_str()?.into(), class: c["class"].as_str().unwrap_or("unknown").into(), at: c["at"].as_i64()? }))
        .filter(|c| !answered.contains(&(c.card.clone(), c.tier.clone())))
        .collect()
}

/// Draw up to `n` claims of a tier, **random and stratified by source class**: the candidates (`id`, `class`) are grouped by class, ordered inside a
/// class by a hash of the batch and the id (a fixed shuffle, so a test can predict it and a re-run in the same batch draws the same ones), and taken
/// round-robin across the classes. A claim already carded in this batch for this tier is not drawn twice. Each is written as a `tier_card`. Returns the ids.
pub fn draw(state: &Path, tier: &str, candidates: &[(String, String)], n: usize, now: i64) -> Result<Vec<String>, String> {
    if !AUDITABLE.contains(&tier) {
        return Err(format!("`{tier}` is not a tier that is audited (substantiated, vetted or batch)"));
    }
    let batch = current_batch(state);
    let events = Journal::new(state).all();
    let carded: BTreeSet<&str> = events.iter().filter(|e| e["kind"] == "tier_card" && e["tier"] == tier && e["batch"] == batch.as_str()).filter_map(|e| e["card"].as_str()).collect();
    let mut strata: BTreeMap<&str, Vec<(String, &str)>> = BTreeMap::new();
    for (id, class) in candidates.iter().filter(|(id, _)| !carded.contains(id.as_str())) {
        let key = blake3::hash(format!("{batch}\u{0}{id}").as_bytes()).to_hex().to_string();
        strata.entry(class.as_str()).or_default().push((key, id.as_str()));
    }
    for v in strata.values_mut() {
        v.sort();
    }
    let mut drawn: Vec<(String, String)> = Vec::new();
    let mut round = 0;
    while drawn.len() < n && strata.values().any(|v| round < v.len()) {
        for (class, v) in &strata {
            if drawn.len() < n && round < v.len() {
                drawn.push((v[round].1.to_string(), class.to_string()));
            }
        }
        round += 1;
    }
    let j = Journal::new(state);
    for (id, class) in &drawn {
        j.append("tier_card", json!({"card": id, "tier": tier, "class": class, "batch": batch}), now);
    }
    Ok(drawn.into_iter().map(|(id, _)| id).collect())
}

/// What an answer did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answered {
    /// Recorded; nothing tripped.
    Recorded,
    /// A hard failure or the error-rate bound latched this tier now.
    RolledBack { tier: String, trigger: String },
}

/// Record the operator's answer for a card. The answer is on disk before anything else is read. Then the two triggers are checked:
///
/// * **a hard failure** (`specs/100` §7.2 item 2): the claim was called `false` and it was **vetted** or batch-accepted: the vetted tier is latched at once;
/// * **the error rate**: after the sample floor, if the operator set a threshold and the upper bound of the tier's error rate is above it, the tier is latched.
///
/// A `false` answer on a **substantiated** claim is an error in the sample, not a hard failure: it counts toward the bound. (A protected-class claim that
/// reached vetted, and a poisoned page that passed, are also hard failures in the spec. The library records each on the claim and
/// [`trip_on_hard_failure`] latches the tier for it, from the validator's loop and the operator's accept; neither needs an audit answer.)
pub fn answer(state: &Path, card: &str, tier: &str, a: Answer, now: i64) -> Result<Answered, String> {
    if !open_tier_cards(state).iter().any(|c| c.card == card && c.tier == tier) {
        return Err("no such card, or it was already answered".into());
    }
    let word = match a {
        Answer::True => "true",
        Answer::False => "false",
        Answer::Unsure => "unsure",
    };
    Journal::new(state).append("tier_audit", json!({"card": card, "tier": tier, "answer": word, "batch": current_batch(state)}), now);
    if a == Answer::False && matches!(tier, "vetted" | "batch") {
        rollback(state, "vetted", "hard_failure", "an audited vetted or batch-accepted claim was called false", now)?;
        return Ok(Answered::RolledBack { tier: "vetted".into(), trigger: "hard_failure".into() });
    }
    let target = if tier == "batch" { "vetted" } else { tier };
    if let Some(t) = evaluate(state, target, now)? {
        return Ok(Answered::RolledBack { tier: target.into(), trigger: t });
    }
    Ok(Answered::Recorded)
}

/// The error-rate trigger: latch the tier when, after the sample floor, the upper bound of its error rate is above the operator's threshold. Returns
/// the trigger name when it fired. A tier already latched, or one with no threshold set, is left alone.
pub fn evaluate(state: &Path, tier: &str, now: i64) -> Result<Option<String>, String> {
    if !ROLLABLE.contains(&tier) || latched(state).contains(tier) {
        return Ok(None);
    }
    let Some(limit) = threshold(state, tier) else { return Ok(None) };
    let st = stats(state, tier);
    if st.sample >= SAMPLE_FLOOR && st.bound * 100.0 > limit {
        rollback(state, tier, "error_rate", &format!("the upper bound of the error rate, {:.1}%, is above your {limit}%", st.bound * 100.0), now)?;
        return Ok(Some("error_rate".into()));
    }
    Ok(None)
}

/// The first half of what a card shows after the answer: nothing here is read before the answer is recorded (the caller shows the claim first, calls
/// [`answer`], and only then asks the library for the sources).
fn verdict_line(st: &Stats, limit: Option<f64>) -> String {
    if st.sample < SAMPLE_FLOOR {
        return format!("no verdict: {} audited claim(s) are below the floor of {SAMPLE_FLOOR}, and a small sample bounds nothing (zero errors in 10 is compatible with an error rate of about 26%)", st.sample);
    }
    match limit {
        None => "the sample meets the floor; you have not set a threshold, so nothing can trigger a roll-back (`delphi validation tier threshold <tier> <percent>`; zero errors in 60 bounds the rate to about 5%)".to_string(),
        Some(l) if st.bound * 100.0 > l => format!("ABOVE your {l}% threshold: the upper bound is {:.1}%", st.bound * 100.0),
        Some(l) => format!("within your {l}% threshold: the upper bound is {:.1}% (a bound on this sample, not a proof)", st.bound * 100.0),
    }
}

/// `delphi validation tier-report`: per tier, the sample, the errors, the bound, how many claims are not yet audited, whether the floor is met, the age
/// of the last audit and the roll-backs. `members` is how many claims are at each tier now (the library's count). It reads only the journal and the
/// latch file, never text a span could influence, and prints no claim.
pub fn report(state: &Path, members: &BTreeMap<String, usize>, outcomes: &BTreeMap<String, (usize, usize, usize)>, now: i64) -> String {
    let events = Journal::new(state).all();
    let batch = current_batch(state);
    let held = latched(state);
    let mut out = format!("Tier audit, batch {batch}. The audit is a roll-back trigger, not a promotion gate: it can take a tier away and never grants one.\n");
    for tier in AUDITABLE {
        let st = stats(state, tier);
        let audited: BTreeSet<&str> = events.iter().filter(|e| e["kind"] == "tier_audit" && e["tier"] == tier && e["batch"] == batch.as_str()).filter_map(|e| e["card"].as_str()).collect();
        let total = members.get(tier).copied().unwrap_or(0);
        let last = events.iter().filter(|e| e["kind"] == "tier_audit" && e["tier"] == tier).filter_map(|e| e["at"].as_i64()).max();
        let age = last.map(|t| format!("{} day(s) ago", (now - t).max(0) / 86_400)).unwrap_or_else(|| "never".into());
        let state_word = if held.contains(tier) { " [ROLLED BACK]" } else { "" };
        out.push_str(&format!("\n{tier}{state_word}\n"));
        if tier == "batch" && total == 0 && st.sample == 0 {
            out.push_str("  no claim is batch-accepted yet (`delphi validation accept-all` marks them)\n");
            continue;
        }
        let rate = if st.sample == 0 { "-".to_string() } else { format!("{:.1}%", st.rate * 100.0) };
        out.push_str(&format!("  audited: {} (true or false), errors: {}, unsure: {} (counts for nothing), observed rate: {rate}\n", st.sample, st.errors, st.unsure));
        out.push_str(&format!("  upper bound on the error rate: {:.1}% ({})\n", st.bound * 100.0, if st.errors == 0 { "rule of three: 3 / sample" } else { "95% interval" }));
        out.push_str(&format!("  claims at this tier now: {total}; not yet audited: {}; last audit: {age}\n", total.saturating_sub(audited.len())));
        out.push_str(&format!("  floor {SAMPLE_FLOOR}: {}\n", if st.sample >= SAMPLE_FLOOR { "met" } else { "NOT met" }));
        out.push_str(&format!("  {}\n", verdict_line(&st, threshold(state, tier))));
    }
    let rolls: Vec<&Value> = events.iter().filter(|e| e["kind"] == "tier_rollback" || e["kind"] == "tier_restore").collect();
    if !rolls.is_empty() {
        out.push_str("\nroll-backs and restores (newest last):\n");
        for e in rolls {
            if e["kind"] == "tier_rollback" {
                out.push_str(&format!("  tier.rollback {}  trigger {}  sample {}  errors {}  bound {:.1}%  batch {}\n", e["tier"].as_str().unwrap_or("?"), e["trigger"].as_str().unwrap_or("?"), e["sample"], e["errors"], e["bound"].as_f64().unwrap_or(1.0) * 100.0, e["audit_batch"].as_str().unwrap_or("?")));
            } else {
                out.push_str(&format!("  restored {}  new batch {}\n", e["tier"].as_str().unwrap_or("?"), e["audit_batch"].as_str().unwrap_or("?")));
            }
        }
    }
    out.push_str(&calibration(outcomes));
    out
}

/// The calibration section (`specs/100` §8): per rung, how many claims have read it and how many of those were later contested or disproved
/// (`specs/055` §3.8's counts, joined by the rung a claim read, not the one it holds now). **No verdict below the floor**: under
/// [`SAMPLE_FLOOR`] claims the numbers print and a line says they mean little; at or above it the observed share and its upper bound print.
/// It is information for changing the defaults of §3.2 on evidence. It takes nothing away and grants nothing, and it reads no claim text.
pub fn calibration(outcomes: &BTreeMap<String, (usize, usize, usize)>) -> String {
    let mut out = String::from("\nCalibration: how often the claims that read a rung were later contested or disproved (the library's counts, specs/055 section 3.8). Counts start when the daemon began recording which rung a claim read; a claim already at a rung is counted from its next refresh, so early numbers are a lower bound. This is information for changing the defaults on evidence; it is not a roll-back trigger and not your audit.\n");
    for rung in ["corroborated", "substantiated", "vetted"] {
        let (reached, contested, disproved) = outcomes.get(rung).copied().unwrap_or((0, 0, 0));
        out.push_str(&format!("\n{rung}\n  claims that read this rung: {reached}; later contested: {contested}; later disproved: {disproved}\n"));
        if reached == 0 {
            out.push_str("  no verdict: no claim has read this rung yet\n");
            continue;
        }
        let bad = contested + disproved;
        let share = bad as f64 / reached as f64;
        if reached < SAMPLE_FLOOR {
            out.push_str(&format!("  no verdict: {reached} claim(s) are below the floor of {SAMPLE_FLOOR} (observed share {:.1}%, which bounds nothing at this size)\n", share * 100.0));
        } else {
            out.push_str(&format!("  observed share later contested or disproved: {:.1}%; upper bound {:.1}% ({}); a bound on this sample, not a proof\n", share * 100.0, upper_bound(reached, bad) * 100.0, if bad == 0 { "rule of three: 3 / sample" } else { "95% interval" }));
        }
    }
    out
}

/// One line for `delphi needs`: which tiers are rolled back, so a roll-back is not something found only by running the report. Empty when none.
pub fn needs_line(state: &Path) -> String {
    let held = latched(state);
    if held.is_empty() {
        return String::new();
    }
    format!("Tier roll-back: {} rolled back; `delphi validation tier-report` shows why, `delphi validation tier restore <tier> --reason \"<text>\"` is yours alone\n", held.into_iter().collect::<Vec<_>>().join(" and "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("local/validation");
        (d, s)
    }

    /// Draw `n` cards for a tier from `ids` (one class) and answer them all with `word`.
    fn answered(s: &Path, tier: &str, ids: &[&str], word: Answer, now: i64) {
        let c: Vec<(String, String)> = ids.iter().map(|i| (i.to_string(), "primary".to_string())).collect();
        draw(s, tier, &c, ids.len(), now).unwrap();
        for i in ids {
            answer(s, i, tier, word, now).unwrap();
        }
    }

    #[test]
    fn the_bound_is_the_rule_of_three_with_no_errors_and_an_interval_with_some() {
        assert_eq!(upper_bound(0, 0), 1.0, "no sample bounds nothing");
        assert!((upper_bound(10, 0) - 0.3).abs() < 1e-9 && (upper_bound(30, 0) - 0.1).abs() < 1e-9 && (upper_bound(60, 0) - 0.05).abs() < 1e-9);
        let b = upper_bound(30, 3);
        assert!(b > 0.1 && b < 0.3, "three errors in thirty: {b}");
        assert!(upper_bound(30, 3) > upper_bound(30, 1), "more errors, a higher bound");
        assert!(upper_bound(100, 3) < upper_bound(30, 3), "a bigger sample, a tighter bound");
        assert!(upper_bound(2, 2) <= 1.0 && upper_bound(2, 2) > 0.5, "a bound is a probability");
    }

    #[test]
    fn only_true_and_false_answers_count_and_unsure_counts_for_nothing() {
        let (_d, s) = state();
        answered(&s, "substantiated", &["c1", "c2"], Answer::True, 10);
        answered(&s, "substantiated", &["c3"], Answer::False, 11);
        answered(&s, "substantiated", &["c4", "c5"], Answer::Unsure, 12);
        let st = stats(&s, "substantiated");
        assert_eq!((st.sample, st.errors, st.unsure), (3, 1, 2));
        assert!((st.rate - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(stats(&s, "vetted").sample, 0, "another tier is its own sample");
    }

    #[test]
    fn the_report_refuses_a_verdict_below_the_floor_and_says_what_a_small_sample_cannot_show() {
        let (_d, s) = state();
        answered(&s, "vetted", &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"], Answer::True, 100);
        let members = BTreeMap::from([("vetted".to_string(), 40usize)]);
        let out = report(&s, &members, &BTreeMap::new(), 100 + 3 * 86_400);
        assert!(out.contains("audited: 10") && out.contains("errors: 0") && out.contains("upper bound on the error rate: 30.0%") && out.contains("rule of three"), "{out}");
        assert!(out.contains("floor 30: NOT met") && out.contains("no verdict") && out.contains("about 26%"), "{out}");
        assert!(out.contains("claims at this tier now: 40; not yet audited: 30") && out.contains("last audit: 3 day(s) ago"), "{out}");
        assert!(!out.to_lowercase().contains("clean"), "never called clean below the floor: {out}");
        assert!(out.contains("no claim is batch-accepted yet"), "{out}");
    }

    #[test]
    fn at_the_floor_with_no_threshold_nothing_triggers_and_with_one_the_bound_decides() {
        let (_d, s) = state();
        let ids: Vec<String> = (0..30).map(|i| format!("c{i:02}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        answered(&s, "substantiated", &refs, Answer::True, 100);
        let members = BTreeMap::from([("substantiated".to_string(), 90usize)]);
        let out = report(&s, &members, &BTreeMap::new(), 200);
        assert!(out.contains("floor 30: met") && out.contains("you have not set a threshold"), "{out}");
        assert!(latched(&s).is_empty(), "no threshold, no roll-back");
        // zero errors in 30 bounds the rate to 10%: within a 15% limit, above a 5% one
        set_threshold(&s, "substantiated", 15.0, 201).unwrap();
        assert_eq!(evaluate(&s, "substantiated", 202).unwrap(), None);
        assert!(report(&s, &members, &BTreeMap::new(), 203).contains("within your 15% threshold"));
        set_threshold(&s, "substantiated", 5.0, 204).unwrap();
        assert!(report(&s, &members, &BTreeMap::new(), 205).contains("ABOVE your 5% threshold"));
        assert_eq!(evaluate(&s, "substantiated", 206).unwrap(), Some("error_rate".into()));
        assert!(latched(&s).contains("substantiated"));
        let ev = Journal::new(&s).all();
        let roll = ev.iter().find(|e| e["kind"] == "tier_rollback").expect("a tier.rollback event");
        assert_eq!((roll["tier"].as_str(), roll["trigger"].as_str(), roll["sample"].as_u64(), roll["errors"].as_u64()), (Some("substantiated"), Some("error_rate"), Some(30), Some(0)));
        assert!(roll["bound"].as_f64().unwrap() > 0.09 && roll["audit_batch"].is_string() && roll["reaudit_due"] == 0, "{roll}");
    }

    fn outcomes(rows: &[(&str, usize, usize, usize)]) -> BTreeMap<String, (usize, usize, usize)> {
        rows.iter().map(|(r, a, b, c)| (r.to_string(), (*a, *b, *c))).collect()
    }

    #[test]
    fn the_calibration_section_prints_every_rung_and_gives_no_verdict_below_the_floor() {
        // synthetic outcomes: corroborated has volume, substantiated is below the floor, vetted is empty
        let out = calibration(&outcomes(&[("corroborated", 40, 3, 1), ("substantiated", 12, 2, 0)]));
        assert!(out.contains("\ncorroborated\n  claims that read this rung: 40; later contested: 3; later disproved: 1\n"), "{out}");
        // at the floor and above: the observed share and the bound, and never a word that calls it proven
        assert!(out.contains("observed share later contested or disproved: 10.0%; upper bound") && out.contains("95% interval") && out.contains("not a proof"), "{out}");
        // below the floor: the numbers print, the verdict does not
        assert!(out.contains("\nsubstantiated\n  claims that read this rung: 12; later contested: 2; later disproved: 0\n"), "{out}");
        assert!(out.contains("no verdict: 12 claim(s) are below the floor of 30 (observed share 16.7%, which bounds nothing at this size)"), "{out}");
        // a rung nobody has read says so instead of printing a rate
        assert!(out.contains("\nvetted\n  claims that read this rung: 0; later contested: 0; later disproved: 0\n  no verdict: no claim has read this rung yet\n"), "{out}");
        // information only: it says it is not a roll-back trigger, and it never offers a verdict word
        assert!(out.contains("not a roll-back trigger") && !out.to_lowercase().contains("clean") && !out.contains("ABOVE"), "{out}");
    }

    #[test]
    fn a_clean_rung_with_volume_uses_the_rule_of_three_not_a_zero() {
        let out = calibration(&outcomes(&[("vetted", 60, 0, 0)]));
        assert!(out.contains("observed share later contested or disproved: 0.0%; upper bound 5.0% (rule of three: 3 / sample)"), "{out}");
    }

    #[test]
    fn the_report_carries_the_calibration_section_after_the_audit_numbers() {
        let (_d, s) = state();
        let out = report(&s, &BTreeMap::new(), &outcomes(&[("corroborated", 5, 1, 0)]), 100);
        let (audit, cal) = (out.find("Tier audit, batch").expect("audit header"), out.find("Calibration: how often").expect("calibration header"));
        assert!(audit < cal && out.contains("claims that read this rung: 5; later contested: 1"), "{out}");
        assert!(!out.contains("is the next slice"), "the placeholder is gone: {out}");
    }

    #[test]
    fn the_error_rate_does_not_trigger_below_the_floor_however_bad_the_sample_looks() {
        let (_d, s) = state();
        set_threshold(&s, "substantiated", 1.0, 1).unwrap();
        answered(&s, "substantiated", &["a", "b", "c"], Answer::False, 2);
        assert!(latched(&s).is_empty(), "three answers bound nothing; the floor is {SAMPLE_FLOOR}");
    }

    #[test]
    fn one_false_vetted_or_batch_accepted_claim_latches_vetted_at_once_and_a_false_substantiated_one_does_not() {
        let (_d, s) = state();
        answered(&s, "substantiated", &["s1"], Answer::False, 1);
        assert!(latched(&s).is_empty(), "an error in the sample, counted toward the bound, not a hard failure");
        let c = vec![("v1".to_string(), "primary".to_string()), ("v2".to_string(), "primary".to_string())];
        draw(&s, "vetted", &c, 2, 2).unwrap();
        assert_eq!(answer(&s, "v1", "vetted", Answer::True, 3).unwrap(), Answered::Recorded);
        assert_eq!(answer(&s, "v2", "vetted", Answer::False, 4).unwrap(), Answered::RolledBack { tier: "vetted".into(), trigger: "hard_failure".into() });
        assert_eq!(latched(&s), BTreeSet::from(["vetted".to_string()]));
        // a batch-accepted claim called false latches vetted too
        let (_d2, s2) = state();
        draw(&s2, "batch", &[("b1".to_string(), "primary".to_string())], 1, 1).unwrap();
        assert_eq!(answer(&s2, "b1", "batch", Answer::False, 2).unwrap(), Answered::RolledBack { tier: "vetted".into(), trigger: "hard_failure".into() });
    }

    #[test]
    fn a_rollback_is_idempotent_deletes_nothing_and_is_an_event_in_the_journal() {
        let (_d, s) = state();
        answered(&s, "vetted", &["a"], Answer::True, 1);
        let before = Journal::new(&s).all().len();
        let msg = rollback(&s, "vetted", "operator", "I do not trust it any more", 5).unwrap();
        assert!(msg.contains("rolled back") && msg.contains("substantiated") && msg.contains("nothing was deleted"), "{msg}");
        assert!(rollback(&s, "vetted", "operator", "again, for good measure", 6).unwrap().contains("already"));
        let ev = Journal::new(&s).all();
        assert_eq!(ev.len(), before + 1, "one event, and every earlier record is still there");
        assert_eq!(ev.iter().filter(|e| e["kind"] == "tier_rollback").count(), 1);
        assert!(rollback(&s, "corroborated", "operator", "not a tier with a roll-back", 7).is_err());
        assert!(rollback(&s, "substantiated", "operator", "short", 8).is_err(), "a reason of at least 8 characters");
        assert!(rollback(&s, "substantiated", "operator", "", 9).is_err(), "an operator roll-back needs a reason");
        assert!(!latched(&s).contains("substantiated"), "a refused roll-back latches nothing");
    }

    #[test]
    fn a_restore_needs_a_reason_and_a_latched_tier_and_starts_a_new_batch_so_fresh_answers_decide() {
        let (_d, s) = state();
        assert!(restore(&s, "vetted", "nothing to restore here", 1).is_err(), "it is not rolled back");
        answered(&s, "vetted", &["a", "b"], Answer::True, 2);
        rollback(&s, "vetted", "operator", "audit it again first", 3).unwrap();
        assert!(restore(&s, "vetted", "no", 4).is_err(), "a reason of at least 8 characters");
        assert!(latched(&s).contains("vetted"));
        let before = current_batch(&s);
        let msg = restore(&s, "vetted", "audited again, the sample was fine", 5).unwrap();
        assert!(latched(&s).is_empty() && msg.contains("new audit batch"), "{msg}");
        assert_ne!(current_batch(&s), before);
        assert_eq!(stats(&s, "vetted").sample, 0, "the old answers belong to the old batch");
        let ev = Journal::new(&s).all();
        assert!(ev.iter().any(|e| e["kind"] == "tier_restore" && e["by"] == "operator"));
    }

    #[test]
    fn the_latch_is_a_file_that_outlives_the_process_and_an_unreadable_one_fails_safe() {
        let (_d, s) = state();
        rollback(&s, "substantiated", "operator", "a reason that is long enough", 1).unwrap();
        // a "restart" is a new read of the file
        assert_eq!(latched(&s), BTreeSet::from(["substantiated".to_string()]));
        std::fs::write(latch_file(&s), b"{ not json").unwrap();
        assert_eq!(latched(&s), BTreeSet::from(["substantiated".to_string(), "vetted".to_string()]), "unreadable: both are held");
        std::fs::remove_file(latch_file(&s)).unwrap();
        assert!(latched(&s).is_empty(), "no file, no latch");
    }

    #[test]
    fn the_draw_is_random_stratified_by_class_stable_and_never_repeats_a_card() {
        let (_d, s) = state();
        let mut c: Vec<(String, String)> = (0..6).map(|i| (format!("p{i}"), "primary".into())).collect();
        c.extend((0..3).map(|i| (format!("r{i}"), "reputable_secondary".into())));
        c.push(("u0".into(), "unknown".into()));
        let first = draw(&s, "substantiated", &c, 4, 1).unwrap();
        assert_eq!(first.len(), 4);
        let classes: BTreeSet<&str> = first.iter().map(|id| c.iter().find(|x| &x.0 == id).unwrap().1.as_str()).collect();
        assert_eq!(classes.len(), 3, "one from each class before a second from any: {first:?}");
        // the same batch draws the rest, never the same card
        let second = draw(&s, "substantiated", &c, 10, 2).unwrap();
        assert_eq!(second.len(), 6);
        assert!(first.iter().all(|f| !second.contains(f)));
        assert_eq!(open_tier_cards(&s).len(), 10);
        // the order is a fixed shuffle: a fresh state in the same batch draws the same first four
        let (_d2, s2) = state();
        assert_eq!(draw(&s2, "substantiated", &c, 4, 1).unwrap(), first);
        assert!(draw(&s, "corroborated", &c, 1, 3).is_err(), "only substantiated, vetted and batch are audited");
    }

    #[test]
    fn an_answer_needs_an_open_card_and_each_card_is_answered_once() {
        let (_d, s) = state();
        assert!(answer(&s, "ghost", "vetted", Answer::True, 1).is_err());
        draw(&s, "vetted", &[("a".to_string(), "primary".to_string())], 1, 1).unwrap();
        answer(&s, "a", "vetted", Answer::True, 2).unwrap();
        assert!(answer(&s, "a", "vetted", Answer::False, 3).is_err(), "answered already; the first answer stands");
        assert_eq!(stats(&s, "vetted").errors, 0);
    }

    #[test]
    fn the_needs_line_names_the_rolled_back_tiers_and_is_empty_otherwise() {
        let (_d, s) = state();
        assert_eq!(needs_line(&s), "");
        rollback(&s, "vetted", "operator", "a reason that is long enough", 1).unwrap();
        let l = needs_line(&s);
        assert!(l.contains("vetted rolled back") && l.contains("tier-report") && l.contains("yours alone"), "{l}");
    }
}
