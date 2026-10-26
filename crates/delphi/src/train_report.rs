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
        })
    }).collect()
}

#[derive(Debug, Clone, PartialEq)]
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
        RunRow { case: case.into(), arm: arm.into(), seed, passed, cost_usd: cost, tokens_in: tin, tokens_out: tout, attempts: 1, promotions: 0 }
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
        let mk = |arm: &str, passed: usize, graded: usize, cost: f64| ArmStats { arm: arm.into(), graded, ungraded: 0, passed, seeds: 3, cost_usd: cost * graded as f64, tokens_in: 0, tokens_out: 0, attempts: 1.0, promotions: 0 };
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

    #[test]
    fn the_command_needs_a_run_directory() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(cli(&["report".into(), "--work-dir".into(), t.path().display().to_string()]), 2);
    }
}
