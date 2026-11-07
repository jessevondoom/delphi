//! `delphi train report`: what a training experiment found (`specs/037` §5.3, §6).
//!
//! Numbers only; no model output is printed. It joins the run's rows (`rows.jsonl`) with the key from blind ids to arms, the
//! jury's judgements, the operator's answers and the disagreement ledger. It reports pass rate, tokens and cost per passing
//! solution by arm, which arms are on the quality-against-cost frontier, how each judge's wins break down by arm (and whether a
//! judge favors arms of its own family), how the operator and the jury agree, and what is still contested. It says plainly when
//! there are too few seeds to rank anything.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::train_judge::{read_judgements, read_key, resolve, JudgeRow, KeyRow, Resolved};
use crate::train_run::SeatNote;
use crate::train_review::{consistency, disputes, ledger, pairs, read_operator, OpRow};

/// Seeds per arm needed before the report ranks anything.
pub const MIN_SEEDS: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    pub case: String,
    pub arm: String,
    pub seed: u32,
    pub passed: Option<bool>,
    pub cost_usd: f64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub attempts: u32,
    pub promotions: u32,
    /// The plain arm this one varies (`specs/048` BM-1), and the model ids the run used.
    pub base: Option<String>,
    pub models: Vec<String>,
    /// Effort (`specs/048` BM-2); zero in a row written before these were kept.
    pub turns: u32,
    pub tool_calls: u32,
    pub tool_errors: u32,
    pub nudges: u32,
    pub hard_stops: u32,
    pub escalations: u32,
    pub cache_read: u64,
    pub cache_input: u64,
    /// The seats of the run, from its `seat.receipt` events (`specs/091` SR-6); empty in a row written before they were kept.
    pub seats: Vec<SeatNote>,
    /// A seeded-defect review run's figures (`train_defects`); a diagnostic, never a score.
    pub review: Option<crate::train_defects::Score>,
}

pub fn read_rows(work: &Path) -> Vec<RunRow> {
    fs::read_to_string(work.join("rows.jsonl")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter_map(|v| {
        Some(RunRow {
            case: v["case"].as_str()?.into(),
            arm: v["arm"].as_str()?.into(),
            seed: v["seed"].as_u64()? as u32,
            passed: v["passed"].as_bool(),
            cost_usd: v["cost_usd"].as_f64().unwrap_or(0.0),
            tokens_in: v["tokens_in"].as_u64().unwrap_or(0),
            tokens_out: v["tokens_out"].as_u64().unwrap_or(0),
            attempts: v["attempts"].as_u64().unwrap_or(0) as u32,
            promotions: v["promotions"].as_u64().unwrap_or(0) as u32,
            base: v["base"].as_str().map(String::from),
            models: v["models"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default(),
            turns: v["turns"].as_u64().unwrap_or(0) as u32,
            tool_calls: v["tool_calls"].as_u64().unwrap_or(0) as u32,
            tool_errors: v["by_tool"].as_object().map(|m| m.values().map(|t| t["errors"].as_u64().unwrap_or(0) as u32).sum()).unwrap_or(0),
            nudges: v["nudges"].as_u64().unwrap_or(0) as u32,
            hard_stops: v["hard_stops"].as_u64().unwrap_or(0) as u32,
            escalations: v["escalations"].as_u64().unwrap_or(0) as u32,
            cache_read: v["cache_read_tokens"].as_u64().unwrap_or(0),
            cache_input: v["cache_input_tokens"].as_u64().unwrap_or(0),
            seats: v["seats"].as_array().map(|a| a.iter().filter_map(SeatNote::from_json).collect()).unwrap_or_default(),
            review: crate::train_defects::Score::from_note(&v["review"]),
        })
    }).collect()
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ArmStats {
    pub arm: String,
    pub graded: usize,
    pub ungraded: usize,
    pub passed: usize,
    pub seeds: usize,
    pub cost_usd: f64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub attempts: f64,
    pub promotions: u32,
    /// The plain arm this one varies, and the models its runs used (`specs/048` BM-1).
    pub base: Option<String>,
    pub models: BTreeSet<String>,
    /// Sums over the graded runs (`specs/048` BM-2). `has_effort` is false for rows written before these were kept.
    pub has_effort: bool,
    pub turns: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub nudges: u64,
    pub hard_stops: u64,
    pub escalations: u64,
    pub cache_read: u64,
    pub cache_input: u64,
}

impl ArmStats {
    pub fn pass_rate(&self) -> Option<f64> {
        (self.graded > 0).then(|| self.passed as f64 / self.graded as f64)
    }

    /// Dollars per graded run, the x of the cost frontier.
    pub fn cost_per_run(&self) -> Option<f64> {
        (self.graded > 0).then(|| self.cost_usd / self.graded as f64)
    }

    /// Total spend (graded runs) over passing runs: what a passing solution cost, failures included. `None` with no pass.
    pub fn cost_per_pass(&self) -> Option<f64> {
        (self.passed > 0).then(|| self.cost_usd / self.passed as f64)
    }

    pub fn tokens_per_pass(&self) -> Option<f64> {
        (self.passed > 0).then(|| (self.tokens_in + self.tokens_out) as f64 / self.passed as f64)
    }

    fn tokens_per_run(&self) -> Option<f64> {
        (self.graded > 0).then(|| (self.tokens_in + self.tokens_out) as f64 / self.graded as f64)
    }

    pub fn turns_per_pass(&self) -> Option<f64> {
        (self.passed > 0 && self.has_effort).then(|| self.turns as f64 / self.passed as f64)
    }

    pub fn calls_per_pass(&self) -> Option<f64> {
        (self.passed > 0 && self.has_effort).then(|| self.tool_calls as f64 / self.passed as f64)
    }

    /// Cache reads over the input side of the calls after each span's first, 0 to 1.
    pub fn cache_hit_rate(&self) -> Option<f64> {
        (self.cache_input > 0).then(|| self.cache_read as f64 / self.cache_input as f64)
    }
}

pub fn arm_stats(rows: &[RunRow]) -> Vec<ArmStats> {
    let mut by: BTreeMap<&str, Vec<&RunRow>> = BTreeMap::new();
    for r in rows {
        by.entry(r.arm.as_str()).or_default().push(r);
    }
    by.into_iter().map(|(arm, rs)| {
        let graded: Vec<&&RunRow> = rs.iter().filter(|r| r.passed.is_some()).collect();
        ArmStats {
            arm: arm.to_string(),
            graded: graded.len(),
            ungraded: rs.len() - graded.len(),
            passed: graded.iter().filter(|r| r.passed == Some(true)).count(),
            seeds: graded.iter().map(|r| r.seed).collect::<BTreeSet<_>>().len(),
            cost_usd: graded.iter().map(|r| r.cost_usd).sum(),
            tokens_in: graded.iter().map(|r| r.tokens_in).sum(),
            tokens_out: graded.iter().map(|r| r.tokens_out).sum(),
            attempts: if graded.is_empty() { 0.0 } else { graded.iter().map(|r| f64::from(r.attempts)).sum::<f64>() / graded.len() as f64 },
            promotions: graded.iter().map(|r| r.promotions).sum(),
            base: rs.iter().find_map(|r| r.base.clone()),
            models: graded.iter().flat_map(|r| r.models.iter().cloned()).collect(),
            has_effort: graded.iter().any(|r| r.turns > 0),
            turns: graded.iter().map(|r| u64::from(r.turns)).sum(),
            tool_calls: graded.iter().map(|r| u64::from(r.tool_calls)).sum(),
            tool_errors: graded.iter().map(|r| u64::from(r.tool_errors)).sum(),
            nudges: graded.iter().map(|r| u64::from(r.nudges)).sum(),
            hard_stops: graded.iter().map(|r| u64::from(r.hard_stops)).sum(),
            escalations: graded.iter().map(|r| u64::from(r.escalations)).sum(),
            cache_read: graded.iter().map(|r| r.cache_read).sum(),
            cache_input: graded.iter().map(|r| r.cache_input).sum(),
        }
    }).collect()
}

/// The arms no other arm beats on both axes: higher pass rate, and lower `cost` (cost per run, or tokens per run).
pub fn frontier(stats: &[ArmStats], cost: impl Fn(&ArmStats) -> Option<f64>) -> Vec<String> {
    let pts: Vec<(&ArmStats, f64, f64)> = stats.iter().filter_map(|s| Some((s, cost(s)?, s.pass_rate()?))).collect();
    pts.iter()
        .filter(|(_, c, q)| !pts.iter().any(|(_, c2, q2)| c2 <= c && q2 >= q && (c2 < c || q2 > q)))
        .map(|(s, _, _)| s.arm.clone())
        .collect()
}

/// A model id's family: its first word (`claude-sonnet-5` is `claude`).
pub fn family(model: &str) -> String {
    model.split(['-', '/', ':', '.']).next().unwrap_or(model).to_lowercase()
}

/// For each judge and arm: `(decisive pairs the arm won, decisive pairs it was in)`, counting only pairs the judge resolved
/// consistently (both orders picked the same bundle) and not ties.
pub fn judge_table(rows: &[JudgeRow], key: &[KeyRow]) -> BTreeMap<String, BTreeMap<String, (usize, usize)>> {
    let arm_of = |blind: &str| key.iter().find(|k| k.blind == blind).map(|k| k.arm.clone());
    let mut out: BTreeMap<String, BTreeMap<String, (usize, usize)>> = BTreeMap::new();
    for (a, b) in pairs(key) {
        for judge in rows.iter().map(|r| r.judge.as_str()).collect::<BTreeSet<_>>() {
            let theirs: Vec<&JudgeRow> = rows.iter().filter(|r| r.judge == judge).collect();
            if let Resolved::Winner(w) = resolve(&theirs, &a.blind, &b.blind) {
                let (winner_arm, loser_arm) = if w == a.blind { (arm_of(&a.blind), arm_of(&b.blind)) } else { (arm_of(&b.blind), arm_of(&a.blind)) };
                let t = out.entry(judge.to_string()).or_default();
                if let (Some(wa), Some(la)) = (winner_arm, loser_arm) {
                    t.entry(wa.clone()).or_default().0 += 1;
                    t.entry(wa).or_default().1 += 1;
                    t.entry(la).or_default().1 += 1;
                }
            }
        }
    }
    out
}

/// Of the pairs a judge resolved, the share where both orders agreed (the rest followed the position or failed).
pub fn judge_consistency(rows: &[JudgeRow], key: &[KeyRow]) -> BTreeMap<String, (usize, usize)> {
    let mut out: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (a, b) in pairs(key) {
        for judge in rows.iter().map(|r| r.judge.as_str()).collect::<BTreeSet<_>>() {
            let theirs: Vec<&JudgeRow> = rows.iter().filter(|r| r.judge == judge).collect();
            let e = out.entry(judge.to_string()).or_default();
            match resolve(&theirs, &a.blind, &b.blind) {
                Resolved::Missing => {}
                Resolved::Inconsistent => e.1 += 1,
                _ => {
                    e.0 += 1;
                    e.1 += 1;
                }
            }
        }
    }
    out
}

/// `specs/048` OD-2 (provisional): tokens per passing case up by more than this share is a cost to flag.
pub const COST_FLAG: f64 = 0.25;

/// What a harness arm did against its base, on the same model, cases and seeds (`specs/048` BM-1, BM-2, AC-2).
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Either side has fewer than [`MIN_SEEDS`] graded seeds: nothing is concluded (`specs/031` section 4.5).
    NoEvidence(usize),
    /// One side never passed, so tokens per pass cannot be compared.
    NoPasses,
    /// More passes, beyond the noise, for tokens per pass up by more than [`COST_FLAG`]: a trade, not a win.
    BetterAtMoreCost,
    Better,
    /// Fewer passes, beyond the noise.
    Worse,
    /// Tokens per pass up by more than [`COST_FLAG`] and the pass rate no better than the noise: the flag of OD-2.
    CostlierNotBetter,
    CheaperSameQuality,
    NoDifference,
}

impl Effect {
    pub fn text(&self) -> String {
        match self {
            Effect::NoEvidence(n) => format!("no evidence (fewest graded seeds {}, below {})", n, MIN_SEEDS),
            Effect::NoPasses => "no comparison (an arm has no passing run)".into(),
            Effect::BetterAtMoreCost => format!("a trade: more passes for tokens per pass up by over {:.0}% <- flagged", COST_FLAG * 100.0),
            Effect::Better => "better, at no large extra cost".into(),
            Effect::Worse => "worse: fewer passes, beyond the noise".into(),
            Effect::CostlierNotBetter => format!("costlier, not better: tokens per pass up by over {:.0}% with the pass rate inside the noise <- flagged", COST_FLAG * 100.0),
            Effect::CheaperSameQuality => format!("cheaper, same quality: tokens per pass down by over {:.0}% inside the noise", COST_FLAG * 100.0),
            Effect::NoDifference => "no difference beyond the noise".into(),
        }
    }
}

/// The standard error of the difference between two pass rates: the noise a gain must beat to count.
pub fn noise(base: &ArmStats, arm: &ArmStats) -> f64 {
    let term = |s: &ArmStats| s.pass_rate().map_or(0.0, |p| p * (1.0 - p) / s.graded as f64);
    (term(base) + term(arm)).sqrt()
}

pub fn effect(base: &ArmStats, arm: &ArmStats) -> Effect {
    let fewest = base.seeds.min(arm.seeds);
    if fewest < MIN_SEEDS {
        return Effect::NoEvidence(fewest);
    }
    let (Some(bt), Some(at), Some(br), Some(ar)) = (base.tokens_per_pass(), arm.tokens_per_pass(), base.pass_rate(), arm.pass_rate()) else { return Effect::NoPasses };
    let (gain, se, rel) = (ar - br, noise(base, arm), at / bt - 1.0);
    if gain > se {
        if rel > COST_FLAG { Effect::BetterAtMoreCost } else { Effect::Better }
    } else if gain < -se {
        Effect::Worse
    } else if rel > COST_FLAG {
        Effect::CostlierNotBetter
    } else if rel < -COST_FLAG {
        Effect::CheaperSameQuality
    } else {
        Effect::NoDifference
    }
}

fn delta(arm: Option<f64>, base: Option<f64>) -> String {
    match (arm, base) {
        (Some(a), Some(b)) if b > 0.0 => format!("{:+.0}%", (a / b - 1.0) * 100.0),
        _ => "—".into(),
    }
}

fn per(sum: u64, n: usize) -> String {
    if n == 0 { "—".into() } else { format!("{:.1}", sum as f64 / n as f64) }
}

fn ratio(v: Option<f64>) -> String {
    v.map_or("—".into(), |v| format!("{:.0}%", v * 100.0))
}

/// What one arm's seats were, pooled over its runs (`specs/091` SR-6).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArmSeats {
    pub carriers: BTreeSet<String>,
    pub served: BTreeSet<String>,
    /// Served families that are known, and whether any seat's was `unknown`.
    pub families: BTreeSet<String>,
    pub unknown_family: bool,
    pub tools: BTreeSet<String>,
    /// `(carrier, dispatch allowed)`: the carrier's policy on what the seat could do.
    pub policy: BTreeSet<(String, bool)>,
    pub prompts: BTreeSet<String>,
    pub mismatches: BTreeSet<String>,
    pub seats: usize,
}

pub fn arm_seats(rows: &[RunRow]) -> BTreeMap<String, ArmSeats> {
    let mut out: BTreeMap<String, ArmSeats> = BTreeMap::new();
    for r in rows.iter().filter(|r| !r.seats.is_empty()) {
        let a = out.entry(r.arm.clone()).or_default();
        for s in &r.seats {
            a.seats += 1;
            a.carriers.insert(s.carrier.clone());
            a.served.insert(s.served_model.clone().unwrap_or_else(|| "unknown".into()));
            if s.family == delphi_engine::seat::UNKNOWN {
                a.unknown_family = true;
            } else {
                a.families.insert(s.family.clone());
            }
            a.tools.extend(s.tools.iter().cloned());
            a.policy.insert((s.carrier.clone(), s.dispatch_allowed));
            a.prompts.insert(s.prompt_hash.clone());
            if s.mismatch() {
                a.mismatches.insert(format!("asked {}, served {}", s.requested, s.served_model.as_deref().unwrap_or("unknown")));
            }
        }
    }
    out
}

/// Why two arms whose served families differ cannot be compared as families: the prompt, the tools offered or the carrier's policy
/// differ too. Empty when the comparison is clean.
pub fn confounds(a: &ArmSeats, b: &ArmSeats) -> Vec<&'static str> {
    let mut why = Vec::new();
    if a.prompts != b.prompts {
        why.push("prompt differs");
    }
    if a.tools != b.tools {
        why.push("tools offered differ");
    }
    if a.policy != b.policy {
        why.push("carrier policy differs");
    }
    why
}

/// The served model and family of each arm, the findings among them, and the family comparisons (`specs/091` SR-6). Nothing when no
/// row kept receipts. A family is the carrier's claim mapped by the family table; `unknown` is shown as unknown.
fn seat_sections(rows: &[RunRow]) -> String {
    let arms = arm_seats(rows);
    if arms.is_empty() {
        return String::new();
    }
    let fam_label = |a: &ArmSeats| if a.unknown_family { if a.families.is_empty() { "unknown".to_string() } else { format!("{},unknown", a.families.iter().cloned().collect::<Vec<_>>().join(",")) } } else { a.families.iter().cloned().collect::<Vec<_>>().join(",") };
    let join = |s: &BTreeSet<String>| if s.is_empty() { "none".to_string() } else { s.iter().cloned().collect::<Vec<_>>().join(",") };
    let mut out = String::from("\nSEATS SERVED (from seat receipts: what the carrier reported, not what was asked)\n");
    out.push_str(&format!("{:<22}{:<12}{:<30}{:<18}{}\n", "arm", "carrier", "served model", "family", "tools offered"));
    for (arm, a) in &arms {
        let fam = fam_label(a);
        out.push_str(&format!("{:<22}{:<12}{:<30}{:<18}{}\n", arm, join(&a.carriers), join(&a.served), fam, a.tools.len()));
    }
    for (arm, a) in &arms {
        for m in &a.mismatches {
            out.push_str(&format!("  FINDING: {arm}: the served model differs from the requested one ({m})\n"));
        }
    }
    let names: Vec<&String> = arms.keys().collect();
    let mut lines = Vec::new();
    for (i, x) in names.iter().enumerate() {
        for y in &names[i + 1..] {
            let (a, b) = (&arms[*x], &arms[*y]);
            if a.families == b.families && !(a.unknown_family || b.unknown_family) {
                continue;
            }
            let unknown: Vec<&str> = [(x.as_str(), a), (y.as_str(), b)].iter().filter(|(_, s)| s.unknown_family).map(|(n, _)| *n).collect();
            let why = confounds(a, b);
            let verdict = if !unknown.is_empty() {
                format!("served family unknown for {}: the comparison cannot be called clean{}", unknown.join(", "), if why.is_empty() { String::new() } else { format!(" ({})", why.join("; ")) })
            } else if why.is_empty() {
                "clean (same prompt, tools offered and carrier policy)".to_string()
            } else {
                format!("CONFOUNDED ({})", why.join("; "))
            };
            lines.push(format!("  {} vs {}: {} vs {}: {}\n", x, y, fam_label(a), fam_label(b), verdict));
        }
    }
    if !lines.is_empty() {
        out.push_str("FAMILY COMPARISONS (a comparison across families is clean only when prompt, tools offered and carrier policy are the same)\n");
        lines.iter().for_each(|l| out.push_str(l));
    }
    out
}

/// The efficiency table and, when any arm varies another, the fixed-model comparison (`specs/048` BM-1, BM-2).
fn effort_sections(stats: &[ArmStats]) -> String {
    let mut out = String::new();
    if stats.iter().any(|s| s.has_effort) {
        out.push_str("\nEFFORT (per graded run; per-pass columns divide by passing runs)\n");
        out.push_str(&format!("{:<22}{:>10}{:>10}{:>9}{:>11}{:>11}{:>10}{:>8}{:>7}{:>7}\n", "arm", "turns/run", "calls/run", "errors", "turns/pass", "calls/pass", "cache hit", "nudges", "stops", "escal."));
        for s in stats.iter().filter(|s| s.has_effort) {
            let n = |v: Option<f64>| v.map_or("—".into(), |v| format!("{:.1}", v));
            out.push_str(&format!("{:<22}{:>10}{:>10}{:>9}{:>11}{:>11}{:>10}{:>8}{:>7}{:>7}\n", s.arm, per(s.turns, s.graded), per(s.tool_calls, s.graded), s.tool_errors, n(s.turns_per_pass()), n(s.calls_per_pass()), ratio(s.cache_hit_rate()), s.nudges, s.hard_stops, s.escalations));
        }
    }
    let variants: Vec<&ArmStats> = stats.iter().filter(|s| s.base.is_some()).collect();
    if !variants.is_empty() {
        out.push_str("\nHARNESS EFFECT (fixed model: each harness arm against its base, same cases and seeds)\n");
        let mut groups: BTreeMap<String, Vec<&ArmStats>> = BTreeMap::new();
        for v in variants {
            groups.entry(v.models.iter().cloned().collect::<Vec<_>>().join(", ")).or_default().push(v);
        }
        for (models, arms) in groups {
            out.push_str(&format!("  model: {}\n", if models.is_empty() { "(not recorded)".into() } else { models }));
            for a in arms {
                let base_name = a.base.as_deref().unwrap_or("");
                let Some(b) = stats.iter().find(|s| s.arm == base_name) else {
                    out.push_str(&format!("    {} vs {}: the base has no runs in this study\n", a.arm, base_name));
                    continue;
                };
                let (ar, br) = (a.pass_rate().unwrap_or(0.0), b.pass_rate().unwrap_or(0.0));
                out.push_str(&format!("    {} vs {}: pass rate {} vs {} ({:+.0} points, noise {:.0}); tokens/pass {} vs {} ({}); turns/pass {}; calls/pass {}\n      -> {}\n",
                    a.arm, b.arm, pct(a.passed, a.graded), pct(b.passed, b.graded), (ar - br) * 100.0, noise(b, a) * 100.0,
                    toks(a.tokens_per_pass()), toks(b.tokens_per_pass()), delta(a.tokens_per_pass(), b.tokens_per_pass()),
                    delta(a.turns_per_pass(), b.turns_per_pass()), delta(a.calls_per_pass(), b.calls_per_pass()), effect(b, a).text()));
            }
        }
    }
    out
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 { "—".into() } else { format!("{:.0}%", 100.0 * n as f64 / d as f64) }
}

/// Dollars, or a dash for no dollars at all: a subscription seat reports tokens, not a price, and `$0.000` would read as free.
fn money(v: Option<f64>) -> String {
    v.filter(|v| *v > 0.0).map_or("—".into(), |v| format!("${:.3}", v))
}

fn toks(v: Option<f64>) -> String {
    v.map_or("—".into(), |v| if v >= 1000.0 { format!("{:.1}k", v / 1000.0) } else { format!("{:.0}", v) })
}

/// The whole report as text. `judge_models` maps a judge name to the model ids it declared, for the same-family check.
pub fn render(work: &Path, judge_models: &BTreeMap<String, Vec<String>>) -> String {
    let (rows, key, judged, ops) = (read_rows(work), read_key(work), read_judgements(work), read_operator(work));
    let stats = arm_stats(&rows);
    let mut out = String::new();
    out.push_str(&format!("{} run(s) over {} arm(s)\n\n", rows.len(), stats.len()));
    out.push_str("ARMS (graded runs; a run whose staffing or checks could not be read is not graded)\n");
    out.push_str(&format!("{:<22}{:>7}{:>8}{:>8}{:>12}{:>12}{:>12}{:>13}{:>9}\n", "arm", "graded", "passed", "rate", "tokens/run", "$/run", "$/pass", "tokens/pass", "att/run"));
    for s in &stats {
        out.push_str(&format!("{:<22}{:>7}{:>8}{:>8}{:>12}{:>12}{:>12}{:>13}{:>9.1}{}\n", s.arm, s.graded, s.passed, pct(s.passed, s.graded), toks(s.tokens_per_run()), money(s.cost_per_run()), money(s.cost_per_pass()), toks(s.tokens_per_pass()), s.attempts,
            if s.ungraded > 0 { format!("   ({} ungraded)", s.ungraded) } else { String::new() }));
    }
    let by_cost = frontier(&stats, |s| s.cost_per_run());
    let by_tokens = frontier(&stats, |s| s.tokens_per_run());
    out.push_str(&format!("\nFRONTIER (no other arm passes at least as often for no more): by cost: {}; by tokens: {}\n", if by_cost.is_empty() { "—".into() } else { by_cost.join(", ") }, if by_tokens.is_empty() { "—".into() } else { by_tokens.join(", ") }));
    let min_seeds = stats.iter().map(|s| s.seeds).min().unwrap_or(0);
    if min_seeds < MIN_SEEDS {
        out.push_str(&format!("NO EVIDENCE FOR A RANKING: the arm with the fewest graded seeds has {} (the report ranks nothing below {}). These numbers size the study; they do not choose between arms.\n", min_seeds, MIN_SEEDS));
    }

    out.push_str(&effort_sections(&stats));
    out.push_str(&seat_sections(&rows));
    out.push_str(&review_section(&rows));

    // Case by arm.
    let cases: BTreeSet<&str> = rows.iter().map(|r| r.case.as_str()).collect();
    if !cases.is_empty() {
        out.push_str("\nPASSES BY CASE (passed/graded)\n");
        for c in &cases {
            let cells: Vec<String> = stats.iter().map(|s| {
                let rs: Vec<&RunRow> = rows.iter().filter(|r| r.case == *c && r.arm == s.arm && r.passed.is_some()).collect();
                format!("{} {}/{}", s.arm, rs.iter().filter(|r| r.passed == Some(true)).count(), rs.len())
            }).collect();
            out.push_str(&format!("  {:<34} {}\n", c, cells.join("  ")));
        }
    }

    // The jury.
    if !judged.is_empty() {
        let table = judge_table(&judged, &key);
        let cons = judge_consistency(&judged, &key);
        out.push_str("\nJURY (a pair counts for a judge only if both orders picked the same bundle)\n");
        for (judge, arms) in &table {
            let (ok, total) = cons.get(judge).copied().unwrap_or((0, 0));
            out.push_str(&format!("  {} (consistent on {} of {} pairs): {}\n", judge, ok, total, arms.iter().map(|(a, (w, n))| format!("{} {}/{}", a, w, n)).collect::<Vec<_>>().join("  ")));
            // Self-preference: does this judge win its own family's arms more often than the others?
            if let Some(models) = judge_models.get(judge) {
                let fams: BTreeSet<String> = models.iter().map(|m| family(m)).collect();
                let arm_family = |arm: &str| -> BTreeSet<String> { key.iter().filter(|k| k.arm == arm).flat_map(|k| k.models.iter().map(|m| family(m))).collect() };
                let (mut own, mut other) = ((0usize, 0usize), (0usize, 0usize));
                for (arm, (w, n)) in arms {
                    let bucket = if arm_family(arm).iter().any(|f| fams.contains(f)) { &mut own } else { &mut other };
                    bucket.0 += w;
                    bucket.1 += n;
                }
                if own.1 > 0 && other.1 > 0 {
                    let (o, x) = (own.0 as f64 / own.1 as f64, other.0 as f64 / other.1 as f64);
                    out.push_str(&format!("    same-family arms win {} of their pairs, other arms {}{}\n", pct(own.0, own.1), pct(other.0, other.1), if o - x >= 0.2 { "  <- possible self-preference" } else { "" }));
                }
            }
        }
    }

    // The operator and the ledger.
    if !ops.is_empty() {
        let first: Vec<&OpRow> = ops.iter().filter(|o| !o.repeat).collect();
        let (agreed, total) = consistency(&ops);
        let disp = disputes(work);
        let state = ledger(work);
        let open = state.iter().filter(|e| e.status == "contested").count();
        out.push_str(&format!("\nOPERATOR: {} pair(s) answered; own consistency {} of {} repeats\n", first.len(), agreed, total));
        out.push_str(&format!("DISAGREEMENTS with the jury: {} ({} still contested)\n", disp.len(), open));
        let mut causes: BTreeMap<String, usize> = BTreeMap::new();
        for e in state.iter().filter(|e| e.status == "resolved") {
            *causes.entry(e.cause.clone().unwrap_or_default()).or_default() += 1;
        }
        if !causes.is_empty() {
            out.push_str(&format!("  resolved by cause: {}\n", causes.iter().map(|(c, n)| format!("{} {}", c, n)).collect::<Vec<_>>().join(", ")));
        }
        if open > 0 {
            out.push_str("  Contested items are unresolved: a conclusion that changes a default (a routing table, a tier) waits for the discussion and the operator's go/no-go.\n");
        }
    }
    out.push_str("\nLimits: deterministic checks are a floor; the jury is blind to names but not to style; one repository and a handful of topics. See specs/037 §11.\n");
    out
}

/// Seeded-defect review figures by arm, from graded runs that carry them (`train_defects`).
fn review_section(rows: &[RunRow]) -> String {
    let runs: Vec<crate::train_defects::RunReview> = rows.iter().filter(|r| r.passed.is_some()).filter_map(|r| r.review.as_ref().map(|s| crate::train_defects::RunReview { case: &r.case, arm: &r.arm, score: s })).collect();
    crate::train_defects::report_section(&runs)
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// `delphi train report --work-dir <dir> [--judges-file <judges.toml>]`.
pub fn cli(args: &[String]) -> i32 {
    let work = std::path::PathBuf::from(flag(args, "--work-dir").unwrap_or_default());
    if !work.join(crate::train::MARKER).is_file() || !work.join("rows.jsonl").is_file() {
        eprintln!("delphi train report: --work-dir must be a `delphi train run` work directory (it needs rows.jsonl)");
        return 2;
    }
    let judge_models: BTreeMap<String, Vec<String>> = flag(args, "--judges-file").and_then(|f| crate::train_run::parse_arms(Path::new(&f)).ok()).map(|a| a.into_iter().map(|j| (j.name, j.expect_models)).collect()).unwrap_or_default();
    print!("{}", render(&work, &judge_models));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::train_judge::{Side, Verdict};
    use serde_json::json;

    fn run(case: &str, arm: &str, seed: u32, passed: Option<bool>, cost: f64, tin: u64, tout: u64) -> RunRow {
        RunRow { case: case.into(), arm: arm.into(), seed, passed, cost_usd: cost, tokens_in: tin, tokens_out: tout, attempts: 1, promotions: 0, base: None, models: vec![], turns: 0, tool_calls: 0, tool_errors: 0, nudges: 0, hard_stops: 0, escalations: 0, cache_read: 0, cache_input: 0, seats: vec![], review: None }
    }

    #[test]
    fn arms_are_summarized_by_pass_rate_cost_per_pass_and_what_could_not_be_graded() {
        let rows = vec![
            run("c", "cheap", 1, Some(true), 0.10, 1000, 100),
            run("c", "cheap", 2, Some(false), 0.10, 1000, 100),
            run("c", "cheap", 3, None, 5.0, 0, 0),
            run("c", "big", 1, Some(true), 1.00, 9000, 900),
            run("c", "big", 2, Some(true), 1.00, 9000, 900),
        ];
        let s = arm_stats(&rows);
        let cheap = s.iter().find(|a| a.arm == "cheap").unwrap();
        assert_eq!((cheap.graded, cheap.ungraded, cheap.passed, cheap.seeds), (2, 1, 1, 2));
        assert!((cheap.cost_per_pass().unwrap() - 0.2).abs() < 1e-9, "failures are paid for: 0.20 spent per pass");
        assert_eq!(cheap.pass_rate(), Some(0.5));
        let big = s.iter().find(|a| a.arm == "big").unwrap();
        assert_eq!((big.pass_rate(), big.cost_per_run()), (Some(1.0), Some(1.0)));
        assert!((big.tokens_per_pass().unwrap() - 9900.0).abs() < 1e-9);
        // An arm with no pass has no cost per pass.
        let none = arm_stats(&[run("c", "x", 1, Some(false), 1.0, 1, 1)]);
        assert_eq!((none[0].cost_per_pass(), none[0].pass_rate()), (None, Some(0.0)));
    }

    #[test]
    fn the_frontier_keeps_the_arms_nothing_beats_on_both_axes() {
        let mk = |arm: &str, passed: usize, graded: usize, cost: f64| ArmStats { arm: arm.into(), graded, ungraded: 0, passed, seeds: 3, cost_usd: cost * graded as f64, tokens_in: 0, tokens_out: 0, attempts: 1.0, promotions: 0, ..Default::default() };
        let stats = vec![mk("cheap-weak", 2, 10, 0.1), mk("mid", 7, 10, 0.5), mk("dominated", 6, 10, 0.9), mk("big", 9, 10, 2.0), mk("big-twin", 9, 10, 2.5)];
        let f = frontier(&stats, |s| s.cost_per_run());
        assert_eq!(f, vec!["cheap-weak", "mid", "big"], "dominated (worse than mid for more) and big-twin (same as big for more) drop out");
        assert!(frontier(&[], |s| s.cost_per_run()).is_empty());
    }

    #[test]
    fn family_is_the_first_word_of_a_model_id() {
        assert_eq!((family("claude-sonnet-5-5").as_str(), family("gemini-2.5-flash").as_str(), family("qwen3:8b").as_str(), family("local/llama").as_str()), ("claude", "gemini", "qwen3", "local"));
    }

    fn key(blind: &str, arm: &str, models: &[&str]) -> KeyRow {
        KeyRow { blind: blind.into(), case: "r/q".into(), arm: arm.into(), seed: 1, models: models.iter().map(|m| m.to_string()).collect(), passed: Some(true) }
    }

    fn jr(judge: &str, x: &str, y: &str, w: Side) -> JudgeRow {
        JudgeRow { case: "r/q".into(), seed: 1, judge: judge.into(), x: x.into(), y: y.into(), verdict: Some(Verdict { winner: w, score_x: Some(4), score_y: Some(2), reason: "r".into() }), cost_usd: 0.0, error: None }
    }

    #[test]
    fn the_judge_table_counts_only_consistent_decisive_pairs_by_arm() {
        let keys = [key("ba", "claude-arm", &["claude-haiku"]), key("bb", "gemini-arm", &["gemini-flash"])];
        // j1 picks the claude arm in both orders; j2 follows the position.
        let rows = vec![jr("j1", "ba", "bb", Side::X), jr("j1", "bb", "ba", Side::Y), jr("j2", "ba", "bb", Side::X), jr("j2", "bb", "ba", Side::X)];
        let t = judge_table(&rows, &keys);
        assert_eq!(t["j1"]["claude-arm"], (1, 1));
        assert_eq!(t["j1"]["gemini-arm"], (0, 1));
        assert!(!t.contains_key("j2"), "a position-follower contributes no wins");
        let c = judge_consistency(&rows, &keys);
        assert_eq!((c["j1"], c["j2"]), ((1, 1), (0, 1)));
    }

    #[test]
    fn the_report_ranks_nothing_on_few_seeds_flags_self_preference_and_names_open_disagreements() {
        let t = tempfile::tempdir().unwrap();
        let w = t.path().join("w");
        crate::train::prepare_work_home(&w).unwrap();
        let lines: Vec<String> = [("claude-arm", true, 0.1), ("gemini-arm", false, 0.2)].iter().map(|(a, p, c)| json!({"case": "r/q", "arm": a, "seed": 1, "passed": p, "cost_usd": c, "tokens_in": 100, "tokens_out": 50, "attempts": 1, "promotions": 0}).to_string()).collect();
        fs::write(w.join("rows.jsonl"), lines.join("\n") + "\n").unwrap();
        fs::create_dir_all(w.join("bundles")).unwrap();
        let keys = [key("ba", "claude-arm", &["claude-haiku"]), key("bb", "gemini-arm", &["gemini-flash"])];
        let kl: Vec<String> = keys.iter().map(|k| json!({"blind": k.blind, "case": k.case, "arm": k.arm, "seed": 1, "models": k.models, "passed": true}).to_string()).collect();
        fs::write(w.join("bundles/key.jsonl"), kl.join("\n") + "\n").unwrap();
        let rows = [jr("j-claude", "ba", "bb", Side::X), jr("j-claude", "bb", "ba", Side::Y)];
        fs::write(w.join("judgements.jsonl"), rows.iter().map(|r| r.to_json().to_string()).collect::<Vec<_>>().join("\n") + "\n").unwrap();
        fs::write(w.join("operator.jsonl"), json!({"pair": "ba|bb", "first": "ba", "winner": "bb", "reason": "B", "repeat": false}).to_string() + "\n").unwrap();
        crate::train_review::sync_ledger(&w).unwrap();
        let judge_models: BTreeMap<String, Vec<String>> = [("j-claude".to_string(), vec!["claude-sonnet".to_string()])].into();
        let r = render(&w, &judge_models);
        for want in ["2 run(s) over 2 arm(s)", "claude-arm", "NO EVIDENCE FOR A RANKING", "fewest graded seeds has 1", "possible self-preference", "j-claude (consistent on 1 of 1 pairs)", "DISAGREEMENTS with the jury: 1 (1 still contested)", "waits for the discussion and the operator's go/no-go"] {
            assert!(r.contains(want), "{} in\n{}", want, r);
        }
        assert!(r.contains("FRONTIER") && r.contains("by cost: claude-arm"), "{}", r);
    }

    /// Twelve graded runs (4 cases x 3 seeds) of one arm: `passed` of them pass, each using `tokens` tokens and `turns` turns.
    fn twelve(arm: &str, base: Option<&str>, passed: usize, tokens: u64, turns: u32, seeds: u32) -> Vec<RunRow> {
        (0..12u32).map(|i| {
            let mut r = run(&format!("c{}", i % 4), arm, 1 + i % seeds, Some((i as usize) < passed), 0.0, tokens, 0);
            (r.base, r.models, r.turns, r.tool_calls) = (base.map(String::from), vec!["haiku".into()], turns, turns * 2);
            r
        }).collect()
    }

    #[test]
    fn a_harness_arm_is_judged_against_its_base_by_passes_beyond_the_noise_and_tokens_per_pass() {
        let base = |passed, tokens, seeds| arm_stats(&twelve("base", None, passed, tokens, 10, seeds)).remove(0);
        let arm = |passed, tokens, seeds| arm_stats(&twelve("var", Some("base"), passed, tokens, 10, seeds)).remove(0);
        // Base: 6 of 12 for 1000 tokens a run, so 2000 tokens per pass.
        let b = base(6, 1000, 3);
        assert!((b.tokens_per_pass().unwrap() - 2000.0).abs() < 1e-9);
        // AC-2: more passes (10 of 12) but 2500 tokens a run, so 3000 per pass, up 50 %: flagged as a trade, not a win.
        let e = effect(&b, &arm(10, 2500, 3));
        assert_eq!(e, Effect::BetterAtMoreCost);
        assert!(e.text().contains("<- flagged") && e.text().contains("25%"), "{}", e.text());
        assert_eq!(effect(&b, &arm(10, 1100, 3)), Effect::Better, "10 of 12 for 1320 per pass is a gain at no large cost");
        // The OD-2 flag: no more passes, 3000 tokens per pass.
        let e = effect(&b, &arm(6, 1500, 3));
        assert_eq!(e, Effect::CostlierNotBetter);
        assert!(e.text().starts_with("costlier, not better") && e.text().contains("<- flagged"));
        assert_eq!(effect(&b, &arm(6, 500, 3)), Effect::CheaperSameQuality);
        assert_eq!(effect(&b, &arm(6, 1100, 3)), Effect::NoDifference, "10 % more tokens is inside the share");
        assert_eq!(effect(&b, &arm(7, 1000, 3)), Effect::NoDifference, "one more pass in twelve is inside the noise");
        assert_eq!(effect(&b, &arm(2, 1000, 3)), Effect::Worse);
        // Too few seeds on either side concludes nothing, however large the gap.
        assert_eq!(effect(&b, &arm(12, 100, 2)), Effect::NoEvidence(2));
        assert_eq!(effect(&base(6, 1000, 1), &arm(12, 100, 3)), Effect::NoEvidence(1));
        // An arm that never passes has no tokens per pass to compare.
        assert_eq!(effect(&b, &arm(0, 1000, 3)), Effect::NoPasses);
    }

    #[test]
    fn the_report_shows_effort_and_sets_each_harness_arm_against_its_base_and_leaves_old_rows_alone() {
        let t = tempfile::tempdir().unwrap();
        let w = t.path().join("w");
        crate::train::prepare_work_home(&w).unwrap();
        let to_line = |r: &RunRow| json!({"case": r.case, "arm": r.arm, "seed": r.seed, "passed": r.passed, "cost_usd": 0.0, "tokens_in": r.tokens_in, "tokens_out": 0, "attempts": 1, "promotions": 0,
            "base": r.base, "models": {"haiku": {"calls": 1}}, "turns": r.turns, "tool_calls": r.tool_calls, "nudges": 2, "hard_stops": 1, "escalations": 0, "cache_read_tokens": 60, "cache_input_tokens": 100,
            "by_tool": {"read_file": {"calls": 5, "errors": 3}}}).to_string();
        let rows: Vec<RunRow> = [twelve("solo-haiku", None, 6, 1000, 10, 3), twelve("nudge-off", Some("solo-haiku"), 6, 1500, 14, 3), twelve("orphan", Some("missing"), 6, 1000, 10, 3)].concat();
        fs::write(w.join("rows.jsonl"), rows.iter().map(to_line).collect::<Vec<_>>().join("\n") + "\n").unwrap();
        let r = render(&w, &BTreeMap::new());
        for want in ["EFFORT", "turns/pass", "nudge-off", "60%", "model: haiku", "nudge-off vs solo-haiku: pass rate 50% vs 50% (+0 points", "tokens/pass 3.0k vs 2.0k (+50%)", "turns/pass +40%", "-> costlier, not better", "orphan vs missing: the base has no runs in this study"] {
            assert!(r.contains(want), "{} in\n{}", want, r);
        }
        assert!(!r.contains("solo-haiku vs"), "a plain arm is not compared with anything");
        // Rows written before effort was kept: no effort table, and no harness section without a variant.
        let old = json!({"case": "c0", "arm": "solo-haiku", "seed": 1, "passed": true, "cost_usd": 0.1, "tokens_in": 10, "tokens_out": 5, "attempts": 1, "promotions": 0}).to_string();
        fs::write(w.join("rows.jsonl"), old + "\n").unwrap();
        let r = render(&w, &BTreeMap::new());
        assert!(!r.contains("EFFORT") && !r.contains("HARNESS EFFECT"), "{}", r);
    }

    #[test]
    fn the_report_carries_a_review_section_only_for_graded_runs_that_have_figures() {
        let t = tempfile::tempdir().unwrap();
        let w = t.path().join("w");
        crate::train::prepare_work_home(&w).unwrap();
        let line = |arm: &str, passed: Option<bool>, caught: &[&str], fp: u64| json!({"case": "review/rev-1-5", "arm": arm, "seed": 1, "passed": passed, "cost_usd": 0.0, "tokens_in": 1, "tokens_out": 1, "attempts": 1, "promotions": 0,
            "review": {"defects": 4, "caught": caught, "false_positives": fp, "unmatched": 0, "findings": caught.len() as u64 + fp}}).to_string();
        let rows = [line("tools", Some(true), &["10:x", "20:y", "30:z"], 0), line("toolless", Some(false), &["10:x"], 2), line("ungraded", None, &["99:q"], 0)];
        fs::write(w.join("rows.jsonl"), rows.join("\n") + "\n").unwrap();
        let r = render(&w, &BTreeMap::new());
        for want in ["SEEDED-DEFECT REVIEW (planted defects found, by arm; diagnostics only)", "tools", "75%", "3/4", "toolless", "25%", "1/4", "unique", "not a seat score", "specs/078"] {
            assert!(r.contains(want), "{} in\n{}", want, r);
        }
        assert!(!r.lines().any(|l| l.starts_with("ungraded") && l.contains("%")), "an ungraded run is not in the section:\n{}", r);
        let unique = |arm: &str| r.lines().filter(|l| l.starts_with(arm) && l.contains("/4")).map(|l| l.split_whitespace().last().unwrap().to_string()).next();
        assert_eq!((unique("tools").as_deref(), unique("toolless").as_deref()), (Some("2"), Some("0")), "{}", r);
        // No review figures, no section.
        fs::write(w.join("rows.jsonl"), json!({"case": "c0", "arm": "a", "seed": 1, "passed": true, "cost_usd": 0.1, "tokens_in": 10, "tokens_out": 5, "attempts": 1, "promotions": 0}).to_string() + "\n").unwrap();
        assert!(!render(&w, &BTreeMap::new()).contains("SEEDED-DEFECT"));
    }

    fn seat(requested: &str, served: Option<&str>, family: &str, carrier: &str, tools: &[&str], dispatch: bool, hash: &str) -> SeatNote {
        SeatNote { requested: requested.into(), served_model: served.map(String::from), family: family.into(), carrier: carrier.into(), tools: tools.iter().map(|t| t.to_string()).collect(), dispatch_allowed: dispatch, prompt_hash: hash.into(), outcome: "ok".into() }
    }

    fn with_seats(arm: &str, seats: Vec<SeatNote>) -> RunRow {
        let mut r = run("c", arm, 1, Some(true), 0.0, 10, 5);
        r.seats = seats;
        r
    }

    #[test]
    fn the_report_shows_what_was_served_flags_a_mismatch_and_labels_a_confounded_family_comparison() {
        let t = tempfile::tempdir().unwrap();
        let w = t.path().join("w");
        crate::train::prepare_work_home(&w).unwrap();
        let rows = [
            with_seats("on-claude", vec![seat("claude-haiku-4.5", Some("claude-haiku-4-5-20251001"), "claude", "claude", &["read_file", "dispatch_agent"], true, "blake3:a")]),
            // Another family on another carrier that refused the dispatch tool: carrier policy and tools differ.
            with_seats("on-agy", vec![seat("gemini-4-argon", Some("gemini-4-argon"), "gemini", "agy", &["read_file"], false, "blake3:a")]),
            // Same prompt, tools and policy as on-claude, a different family: clean.
            with_seats("on-api", vec![seat("gpt-6-luna", Some("gpt-6-luna"), "openai", "claude", &["read_file", "dispatch_agent"], true, "blake3:a")]),
            // A carrier that reports nothing: unknown stays unknown. Its request was for a different model than any served.
            with_seats("on-copilot", vec![seat("gpt-6.1-terra", None, "unknown", "copilot", &[], false, "blake3:a")]),
            with_seats("swapped", vec![seat("claude-sonnet-5.5", Some("gemini-4-argon"), "gemini", "claude", &["read_file", "dispatch_agent"], true, "blake3:a")]),
        ];
        let to_line = |r: &RunRow| json!({"case": r.case, "arm": r.arm, "seed": 1, "passed": true, "cost_usd": 0.0, "tokens_in": 10, "tokens_out": 5, "attempts": 1, "promotions": 0,
            "seats": r.seats.iter().map(SeatNote::to_json).collect::<Vec<_>>()}).to_string();
        fs::write(w.join("rows.jsonl"), rows.iter().map(to_line).collect::<Vec<_>>().join("\n") + "\n").unwrap();
        let r = render(&w, &BTreeMap::new());
        for want in [
            "SEATS SERVED",
            "FINDING: swapped: the served model differs from the requested one (asked claude-sonnet-5.5, served gemini-4-argon)",
            "on-agy vs on-claude: gemini vs claude: CONFOUNDED (tools offered differ; carrier policy differs)",
            "on-api vs on-claude: openai vs claude: clean (same prompt, tools offered and carrier policy)",
            "on-claude vs on-copilot: claude vs unknown: served family unknown for on-copilot: the comparison cannot be called clean",
        ] {
            assert!(r.contains(want), "{} in\n{}", want, r);
        }
        // A dated suffix is the same model, not a finding.
        assert!(!r.contains("asked claude-haiku-4.5"), "{}", r);
        // Two arms in one family are not a family comparison.
        let same = arm_seats(&[with_seats("a", vec![seat("m", Some("m"), "claude", "claude", &[], false, "x")]), with_seats("b", vec![seat("m", Some("m"), "claude", "agy", &[], false, "y")])]);
        assert_eq!(same["a"].families, same["b"].families);
        // A different prompt alone confounds.
        let (a, b) = (&arm_seats(&[with_seats("a", vec![seat("m", Some("m"), "claude", "claude", &[], false, "x")])])["a"].clone(), &arm_seats(&[with_seats("b", vec![seat("n", Some("n"), "gemini", "claude", &[], false, "y")])])["b"].clone());
        assert_eq!(confounds(a, b), vec!["prompt differs"]);
        // Old rows without receipts add no section.
        let old = json!({"case": "c0", "arm": "x", "seed": 1, "passed": true, "cost_usd": 0.1, "tokens_in": 10, "tokens_out": 5, "attempts": 1, "promotions": 0}).to_string();
        fs::write(w.join("rows.jsonl"), old + "\n").unwrap();
        assert!(!render(&w, &BTreeMap::new()).contains("SEATS SERVED"));
    }

    #[test]
    fn the_command_needs_a_run_directory() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(cli(&["report".into(), "--work-dir".into(), t.path().display().to_string()]), 2);
    }
}
