//! The operator's blind review and the disagreement ledger (`specs/037` §6.2, §6.3).
//!
//! `delphi train review` shows the operator pairs of bundles by blind id, in a random-looking order, and records a preference
//! and a one-line reason. It never shows the key from blind id to arm. About one pair in ten is shown again, flipped, so the
//! report can say how consistent the operator is: the operator is not assumed to be correct.
//!
//! A **disagreement** is the operator and the jury's consensus choosing opposite bundles, or the operator calling a tie where
//! the jury saw a gap of two or more points. It is not averaged: it goes on the ledger as `contested`, with both sides'
//! reasons, until the operator and the Council's reasons have been discussed and the cause is classified. The ledger is
//! append-only; the latest entry per item is its state.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::train_judge::{read_bundle, read_judgements, read_key, reference_for, resolve, JudgeRow, KeyRow, Resolved};

/// Why a disagreement happened (`specs/037` §6.3).
pub const CAUSES: [&str; 5] = ["rubric_gap", "judge_bias", "operator_taste", "operator_missed", "ambiguity"];

pub fn pair_id(a: &str, b: &str) -> String {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    format!("{}|{}", lo, hi)
}

fn coin(pair: &str, tag: &str) -> u8 {
    blake3::hash(format!("{}|{}", tag, pair).as_bytes()).as_bytes()[0]
}

/// Every pair of bundles of the same case and seed made by different arms, in a stable order.
pub fn pairs(key: &[KeyRow]) -> Vec<(KeyRow, KeyRow)> {
    let mut groups: BTreeMap<(String, u32), Vec<&KeyRow>> = BTreeMap::new();
    for k in key {
        groups.entry((k.case.clone(), k.seed)).or_default().push(k);
    }
    let mut out = Vec::new();
    for members in groups.values() {
        for (i, a) in members.iter().enumerate() {
            for b in members.iter().skip(i + 1).filter(|b| b.arm != a.arm) {
                out.push(((*a).clone(), (*b).clone()));
            }
        }
    }
    out
}

// ── the operator's answers ──────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct OpRow {
    pub pair: String,
    /// The blind id shown first.
    pub first: String,
    /// The preferred blind id, or `None` for a tie.
    pub winner: Option<String>,
    pub reason: String,
    pub repeat: bool,
}

impl OpRow {
    fn to_json(&self) -> Value {
        json!({"pair": self.pair, "first": self.first, "winner": self.winner, "reason": self.reason, "repeat": self.repeat})
    }
}

pub fn read_operator(work: &Path) -> Vec<OpRow> {
    fs::read_to_string(work.join("operator.jsonl")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter_map(|v| {
        Some(OpRow { pair: v["pair"].as_str()?.into(), first: v["first"].as_str()?.into(), winner: v["winner"].as_str().map(String::from), reason: v["reason"].as_str().unwrap_or("").into(), repeat: v["repeat"].as_bool().unwrap_or(false) })
    }).collect()
}

fn append(path: &Path, v: &Value) -> Result<(), String> {
    let mut line = v.to_string();
    line.push('\n');
    fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut f| f.write_all(line.as_bytes())).map_err(|e| e.to_string())
}

/// The pairs still to show, in order: those with no answer, then the repeats (a deterministic tenth of the answered
/// ones, each once).
pub fn queue(key: &[KeyRow], answered: &[OpRow], case: Option<&str>) -> Vec<(KeyRow, KeyRow, bool)> {
    let mut fresh = Vec::new();
    let mut again = Vec::new();
    for (a, b) in pairs(key).into_iter().filter(|(a, _)| case.is_none_or(|c| a.case == c || a.case.ends_with(c))) {
        let id = pair_id(&a.blind, &b.blind);
        let mine: Vec<&OpRow> = answered.iter().filter(|r| r.pair == id).collect();
        match mine.len() {
            0 => fresh.push((a, b, false)),
            1 if coin(&id, "repeat") % 10 == 0 => again.push((a, b, true)),
            _ => {}
        }
    }
    fresh.extend(again);
    fresh
}

/// Which bundle is shown first. A repeat shows the other one first.
fn shown_first(a: &str, b: &str, repeat: bool, answered: &[OpRow]) -> String {
    let id = pair_id(a, b);
    let original = answered.iter().find(|r| r.pair == id).map(|r| r.first.clone());
    match (repeat, original) {
        (true, Some(f)) => (if f == a { b } else { a }).to_string(),
        _ => (if coin(&id, "order") % 2 == 0 { a } else { b }).to_string(),
    }
}

/// How often the operator's repeat answers agree with their first ones: `(agreed, repeated)`.
pub fn consistency(rows: &[OpRow]) -> (usize, usize) {
    let mut by: BTreeMap<&str, Vec<&OpRow>> = BTreeMap::new();
    for r in rows {
        by.entry(r.pair.as_str()).or_default().push(r);
    }
    let both: Vec<(&OpRow, &OpRow)> = by.values().filter_map(|v| Some((*v.iter().find(|r| !r.repeat)?, *v.iter().find(|r| r.repeat)?))).collect();
    (both.iter().filter(|(a, b)| a.winner == b.winner).count(), both.len())
}

/// The interactive loop. Reads one answer per pair from `input`: `1` (the first shown is better), `2`, `t` (tie), `s`
/// (skip for now) or `q` (stop), then a one-line reason. Returns how many answers were recorded.
pub fn review_loop(work: &Path, cases: &[&crate::train::Case], case: Option<&str>, limit: usize, input: &mut dyn BufRead, out: &mut dyn Write) -> Result<usize, String> {
    let key = read_key(work);
    let mut answered = read_operator(work);
    let todo = queue(&key, &answered, case);
    let mut done = 0;
    writeln!(out, "{} pair(s) to review; each is shown by blind id only. [1] first is better, [2] second is better, [t] tie, [s] skip, [q] quit.", todo.len()).ok();
    for (a, b, repeat) in todo.into_iter().take(limit) {
        let id = pair_id(&a.blind, &b.blind);
        let first = shown_first(&a.blind, &b.blind, repeat, &answered);
        let second = if first == a.blind { b.blind.clone() } else { a.blind.clone() };
        let (bf, bs) = (read_bundle(work, &first)?, read_bundle(work, &second)?);
        let case_obj = cases.iter().find(|c| format!("{}/{}", c.suite, c.id) == a.case);
        writeln!(out, "\n──── {} (seed {}){} ────\nTASK: {}\n", a.case, a.seed, if repeat { " [shown again]" } else { "" }, bf.task.trim()).ok();
        if let Some(c) = case_obj {
            writeln!(out, "REFERENCE:\n{}\n", reference_for(c).trim()).ok();
        }
        writeln!(out, "=== FIRST ===\n{}\n\n=== SECOND ===\n{}\n", bf.text.trim(), bs.text.trim()).ok();
        write!(out, "better? [1/2/t/s/q] ").ok();
        out.flush().ok();
        let mut line = String::new();
        if input.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let winner = match line.trim().to_lowercase().as_str() {
            "1" => Some(Some(first.clone())),
            "2" => Some(Some(second.clone())),
            "t" => Some(None),
            "s" => continue,
            "q" => break,
            _ => {
                writeln!(out, "(not understood; skipped)").ok();
                continue;
            }
        };
        write!(out, "why, in a line? ").ok();
        out.flush().ok();
        let mut reason = String::new();
        input.read_line(&mut reason).map_err(|e| e.to_string())?;
        let row = OpRow { pair: id, first, winner: winner.unwrap_or(None), reason: reason.trim().chars().take(500).collect(), repeat };
        append(&work.join("operator.jsonl"), &row.to_json())?;
        answered.push(row);
        done += 1;
    }
    Ok(done)
}

// ── disagreements and the ledger ────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Dispute {
    pub id: String,
    pub pair: String,
    pub operator: Option<String>,
    pub operator_reason: String,
    /// The jury's consensus: the winning blind id, or `None` for no decisive consensus.
    pub jury: Option<String>,
    pub jury_gap: f64,
    pub jury_reasons: Vec<String>,
}

/// The jury's consensus on a pair: the bundle most judges chose after both orders, and the mean score gap of the rows.
pub fn jury_view(rows: &[JudgeRow], a: &str, b: &str) -> (Option<String>, f64, Vec<String>) {
    let mine: Vec<&JudgeRow> = rows.iter().filter(|r| (r.x == a && r.y == b) || (r.x == b && r.y == a)).collect();
    let mut judges: Vec<&str> = mine.iter().map(|r| r.judge.as_str()).collect();
    judges.sort();
    judges.dedup();
    let mut wins: BTreeMap<String, usize> = BTreeMap::new();
    for j in judges {
        let theirs: Vec<&JudgeRow> = mine.iter().filter(|r| r.judge == j).copied().collect();
        if let Resolved::Winner(w) = resolve(&theirs, a, b) {
            *wins.entry(w).or_default() += 1;
        }
    }
    let top = wins.iter().max_by_key(|(_, n)| **n).map(|(w, n)| (w.clone(), *n));
    let consensus = top.filter(|(w, n)| wins.iter().all(|(o, m)| o == w || m < n)).map(|(w, _)| w);
    let gaps: Vec<f64> = mine.iter().filter_map(|r| r.verdict.as_ref().and_then(|v| Some((f64::from(v.score_x?) - f64::from(v.score_y?)).abs()))).collect();
    let gap = if gaps.is_empty() { 0.0 } else { gaps.iter().sum::<f64>() / gaps.len() as f64 };
    (consensus, gap, mine.iter().filter_map(|r| r.verdict.as_ref().map(|v| format!("{}: {}", r.judge, v.reason))).collect())
}

/// Pairs on which the operator's first answer and the jury disagree. Repeat answers do not create disputes.
pub fn disputes(work: &Path) -> Vec<Dispute> {
    let (ops, rows) = (read_operator(work), read_judgements(work));
    let mut out = Vec::new();
    for o in ops.iter().filter(|o| !o.repeat) {
        let Some((a, b)) = o.pair.split_once('|') else { continue };
        let (jury, gap, reasons) = jury_view(&rows, a, b);
        let contested = match (&o.winner, &jury) {
            (Some(w), Some(j)) => w != j,
            (None, Some(_)) => gap >= 2.0,
            (Some(_), None) => false,
            (None, None) => false,
        };
        if contested {
            out.push(Dispute { id: format!("d{}", &blake3::hash(o.pair.as_bytes()).to_hex()[..8]), pair: o.pair.clone(), operator: o.winner.clone(), operator_reason: o.reason.clone(), jury, jury_gap: gap, jury_reasons: reasons });
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct LedgerEntry {
    pub id: String,
    pub pair: String,
    /// `contested` or `resolved`.
    pub status: String,
    pub cause: Option<String>,
    pub note: String,
}

/// The ledger as it stands: the last entry per item.
pub fn ledger(work: &Path) -> Vec<LedgerEntry> {
    let mut last: BTreeMap<String, LedgerEntry> = BTreeMap::new();
    for v in fs::read_to_string(work.join("ledger.jsonl")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        if let (Some(id), Some(pair), Some(status)) = (v["id"].as_str(), v["pair"].as_str(), v["status"].as_str()) {
            last.insert(id.into(), LedgerEntry { id: id.into(), pair: pair.into(), status: status.into(), cause: v["cause"].as_str().map(String::from), note: v["note"].as_str().unwrap_or("").into() });
        }
    }
    last.into_values().collect()
}

/// Put every new disagreement on the ledger as `contested`. Returns the ones added.
pub fn sync_ledger(work: &Path) -> Result<Vec<Dispute>, String> {
    let known: Vec<String> = ledger(work).into_iter().map(|e| e.id).collect();
    let new: Vec<Dispute> = disputes(work).into_iter().filter(|d| !known.contains(&d.id)).collect();
    for d in &new {
        append(&work.join("ledger.jsonl"), &json!({"id": d.id, "pair": d.pair, "status": "contested", "cause": null, "note": ""}))?;
    }
    Ok(new)
}

/// Classify and close a disagreement after it has been discussed. The cause must be one of [`CAUSES`] and the note says what
/// was concluded (a rubric edit, a calibration label, the operator's change of mind).
pub fn resolve_dispute(work: &Path, id: &str, cause: &str, note: &str) -> Result<(), String> {
    if !CAUSES.contains(&cause) {
        return Err(format!("`{}` is not a cause ({})", cause, CAUSES.join(", ")));
    }
    if note.trim().is_empty() {
        return Err("say what was concluded in --note".into());
    }
    let entry = ledger(work).into_iter().find(|e| e.id == id).ok_or_else(|| format!("no disagreement `{}` on the ledger (run `delphi train disputes` first)", id))?;
    append(&work.join("ledger.jsonl"), &json!({"id": id, "pair": entry.pair, "status": "resolved", "cause": cause, "note": note.trim()}))
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn work_dir(args: &[String], sub: &str) -> Result<PathBuf, i32> {
    let w = PathBuf::from(flag(args, "--work-dir").unwrap_or_default());
    if !w.join(crate::train::MARKER).is_file() || !w.join("bundles/key.jsonl").is_file() {
        eprintln!("delphi train {}: --work-dir must be a `delphi train run` work directory with bundles", sub);
        return Err(2);
    }
    Ok(w)
}

/// `delphi train review`, `disputes` and `resolve`.
pub fn cli(sub: &str, args: &[String], cases: &[&crate::train::Case]) -> i32 {
    let work = match work_dir(args, sub) {
        Ok(w) => w,
        Err(c) => return c,
    };
    match sub {
        "review" => {
            let limit = flag(args, "--limit").and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
            let stdin = std::io::stdin();
            match review_loop(&work, cases, flag(args, "--case").as_deref(), limit, &mut stdin.lock(), &mut std::io::stdout()) {
                Ok(n) => {
                    let (agreed, total) = consistency(&read_operator(&work));
                    println!("\n{} answer(s) recorded{}", n, if total > 0 { format!("; your repeat answers agreed with your first {} of {}", agreed, total) } else { String::new() });
                    0
                }
                Err(e) => {
                    eprintln!("delphi train review: {}", e);
                    1
                }
            }
        }
        "disputes" => {
            let new = match sync_ledger(&work) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("delphi train disputes: {}", e);
                    return 1;
                }
            };
            let all = disputes(&work);
            let state = ledger(&work);
            let key = read_key(&work);
            let unblind = args.iter().any(|a| a == "--unblind");
            let name = |blind: &str| if unblind { key.iter().find(|k| k.blind == blind).map_or(blind.to_string(), |k| format!("{} ({})", k.blind, k.arm)) } else { blind.to_string() };
            println!("{} disagreement(s) ({} new)", all.len(), new.len());
            for d in &all {
                let st = state.iter().find(|e| e.id == d.id);
                println!("\n{}  {}  {}", d.id, d.pair, st.map_or("contested".into(), |e| match &e.cause { Some(c) => format!("resolved: {}", c), None => "contested".into() }));
                println!("  you:  {}  \"{}\"", d.operator.as_deref().map_or("tie".to_string(), name), d.operator_reason);
                println!("  jury: {}  (mean score gap {:.1})", d.jury.as_deref().map_or("no consensus".to_string(), name), d.jury_gap);
                for r in &d.jury_reasons {
                    println!("    {}", r);
                }
            }
            println!("\nClassify each one after discussing it: delphi train resolve --work-dir <dir> --id <id> --cause <{}> --note \"...\"", CAUSES.join("|"));
            0
        }
        "resolve" => match (flag(args, "--id"), flag(args, "--cause"), flag(args, "--note")) {
            (Some(id), Some(cause), Some(note)) => match resolve_dispute(&work, &id, &cause, &note) {
                Ok(()) => {
                    println!("{} resolved: {}", id, cause);
                    0
                }
                Err(e) => {
                    eprintln!("delphi train resolve: {}", e);
                    1
                }
            },
            _ => {
                eprintln!("delphi train resolve --work-dir <dir> --id <id> --cause <{}> --note \"what was concluded\"", CAUSES.join("|"));
                2
            }
        },
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::train_judge::{file_bundle, Draft, Filing, Side, Verdict};

    fn key(blind: &str, arm: &str, seed: u32) -> KeyRow {
        KeyRow { blind: blind.into(), case: "r/q1".into(), arm: arm.into(), seed, models: vec![], passed: Some(true) }
    }

    /// A work dir with three bundles of one case, from three arms, plus an unrelated one of a second seed.
    fn work(root: &Path) -> (PathBuf, Vec<String>) {
        let w = root.join("work");
        crate::train::prepare_work_home(&w).unwrap();
        let mut ids = Vec::new();
        for (arm, text) in [("a1", "answer one"), ("a2", "answer two"), ("a3", "answer three")] {
            ids.push(file_bundle(&w, &Draft { kind: "research".into(), task: "Q?".into(), deliverable: text.into() }, &Filing { arm, case: "r/q1", seed: 1, models: &[], passed: Some(true), failed: &[] }).unwrap());
        }
        (w, ids)
    }

    fn input(s: &str) -> std::io::Cursor<Vec<u8>> {
        std::io::Cursor::new(s.as_bytes().to_vec())
    }

    #[test]
    fn the_queue_shows_every_pair_once_by_blind_id_and_never_the_key() {
        let t = tempfile::tempdir().unwrap();
        let (w, ids) = work(t.path());
        let mut out = Vec::new();
        let n = review_loop(&w, &[], None, 99, &mut input("1\nfirst reason\n2\nsecond reason\nt\ntie reason\n"), &mut out).unwrap();
        assert_eq!(n, 3, "three arms make three pairs");
        let shown = String::from_utf8(out).unwrap();
        assert!(shown.contains("=== FIRST ===") && shown.contains("answer one"), "{}", shown);
        for hidden in ["a1", "a2", "a3", "arm"] {
            assert!(!shown.contains(hidden), "the key leaked: {}", hidden);
        }
        let ops = read_operator(&w);
        assert_eq!(ops.len(), 3);
        assert!(ops.iter().all(|o| ids.iter().any(|i| o.pair.contains(i.as_str()))));
        assert_eq!(ops.iter().filter(|o| o.winner.is_none()).count(), 1, "the tie");
        // Nothing left to review but the repeats.
        let q = queue(&read_key(&w), &ops, None);
        assert!(q.iter().all(|(_, _, repeat)| *repeat));
    }

    #[test]
    fn skipping_quitting_and_unknown_answers_record_nothing_and_a_limit_stops_the_loop() {
        let t = tempfile::tempdir().unwrap();
        let (w, _) = work(t.path());
        let mut out = Vec::new();
        assert_eq!(review_loop(&w, &[], None, 99, &mut input("s\nx\nq\n"), &mut out).unwrap(), 0);
        assert!(read_operator(&w).is_empty());
        assert_eq!(review_loop(&w, &[], None, 1, &mut input("1\nwhy\n1\nmore\n"), &mut Vec::new()).unwrap(), 1, "--limit 1");
    }

    #[test]
    fn about_a_tenth_of_answered_pairs_come_back_flipped_and_the_operators_consistency_is_measured() {
        // Enough pairs that some land on the repeat tenth.
        let keys: Vec<KeyRow> = (0..60).map(|i| key(&format!("b{:010x}", i), &format!("arm{}", i), 1)).collect();
        let all = pairs(&keys);
        let answered: Vec<OpRow> = all.iter().map(|(a, b)| OpRow { pair: pair_id(&a.blind, &b.blind), first: a.blind.clone(), winner: Some(a.blind.clone()), reason: String::new(), repeat: false }).collect();
        let again = queue(&keys, &answered, None);
        assert!(!again.is_empty() && again.len() < all.len() / 4 && again.iter().all(|(_, _, r)| *r), "{} of {}", again.len(), all.len());
        let (a, b, _) = &again[0];
        let id = pair_id(&a.blind, &b.blind);
        assert_ne!(shown_first(&a.blind, &b.blind, true, &answered), answered.iter().find(|r| r.pair == id).unwrap().first, "a repeat flips the order");
        // Consistency: one repeat agrees, one does not.
        let mut rows = vec![
            OpRow { pair: "p1".into(), first: "x".into(), winner: Some("x".into()), reason: String::new(), repeat: false },
            OpRow { pair: "p1".into(), first: "y".into(), winner: Some("x".into()), reason: String::new(), repeat: true },
            OpRow { pair: "p2".into(), first: "x".into(), winner: Some("x".into()), reason: String::new(), repeat: false },
            OpRow { pair: "p2".into(), first: "y".into(), winner: Some("y".into()), reason: String::new(), repeat: true },
            OpRow { pair: "p3".into(), first: "x".into(), winner: None, reason: String::new(), repeat: false },
        ];
        assert_eq!(consistency(&rows), (1, 2));
        rows.clear();
        assert_eq!(consistency(&rows), (0, 0));
    }

    fn jrow(judge: &str, x: &str, y: &str, w: Side, sx: u8, sy: u8) -> JudgeRow {
        JudgeRow { case: "r/q1".into(), seed: 1, judge: judge.into(), x: x.into(), y: y.into(), verdict: Some(Verdict { winner: w, score_x: Some(sx), score_y: Some(sy), reason: format!("{} liked it", judge) }), cost_usd: 0.0, error: None }
    }

    fn write_jury(w: &Path, rows: &[JudgeRow]) {
        for r in rows {
            append(&w.join("judgements.jsonl"), &r.to_json()).unwrap();
        }
    }

    #[test]
    fn a_disagreement_is_the_operator_against_the_juries_consensus_or_a_tie_over_a_wide_gap() {
        let t = tempfile::tempdir().unwrap();
        let (w, ids) = work(t.path());
        let (a, b, c) = (ids[0].clone(), ids[1].clone(), ids[2].clone());
        // The jury (two judges, both orders) picks `a` over `b`, and `a` over `c` by a wide margin.
        let mut rows = Vec::new();
        for j in ["j1", "j2"] {
            rows.push(jrow(j, &a, &b, Side::X, 4, 3));
            rows.push(jrow(j, &b, &a, Side::Y, 3, 4));
            rows.push(jrow(j, &a, &c, Side::X, 5, 1));
            rows.push(jrow(j, &c, &a, Side::Y, 1, 5));
        }
        write_jury(&w, &rows);
        let op = |pair: String, winner: Option<&str>| append(&w.join("operator.jsonl"), &json!({"pair": pair, "first": a, "winner": winner, "reason": "operator reason", "repeat": false})).unwrap();
        op(pair_id(&a, &b), Some(&b)); // opposite of the jury: contested
        op(pair_id(&a, &c), None); // a tie against a gap of 4: contested
        op(pair_id(&b, &c), Some(&b)); // the jury has no view: not contested
        let d = disputes(&w);
        assert_eq!(d.len(), 2, "{:?}", d);
        assert!(d.iter().any(|x| x.pair == pair_id(&a, &b) && x.jury.as_deref() == Some(a.as_str()) && x.operator.as_deref() == Some(b.as_str())));
        assert!(d.iter().any(|x| x.pair == pair_id(&a, &c) && x.operator.is_none() && x.jury_gap >= 2.0));
        assert!(d[0].jury_reasons.iter().any(|r| r.contains("liked it")), "the council's reasons sit beside the operator's");
        // A repeat answer (even one that agrees with the jury, or one that disagrees) never adds or removes a dispute.
        append(&w.join("operator.jsonl"), &json!({"pair": pair_id(&a, &c), "first": c, "winner": a, "reason": "again", "repeat": true})).unwrap();
        assert_eq!(disputes(&w).len(), 2);
    }

    #[test]
    fn a_split_jury_or_inconsistent_judges_give_no_consensus() {
        let (a, b) = ("ba", "bb");
        let split = vec![jrow("j1", a, b, Side::X, 4, 2), jrow("j1", b, a, Side::Y, 2, 4), jrow("j2", a, b, Side::Y, 2, 4), jrow("j2", b, a, Side::X, 4, 2)];
        assert_eq!(jury_view(&split, a, b).0, None, "one judge each way");
        let follower = vec![jrow("j1", a, b, Side::X, 4, 2), jrow("j1", b, a, Side::X, 4, 2)];
        assert_eq!(jury_view(&follower, a, b).0, None, "a position-follower is no vote");
        let three = vec![jrow("j1", a, b, Side::X, 4, 2), jrow("j1", b, a, Side::Y, 2, 4), jrow("j2", a, b, Side::X, 4, 2), jrow("j2", b, a, Side::Y, 2, 4), jrow("j3", a, b, Side::Y, 2, 4), jrow("j3", b, a, Side::X, 4, 2)];
        assert_eq!(jury_view(&three, a, b).0.as_deref(), Some(a), "two of three");
    }

    #[test]
    fn the_ledger_keeps_contested_items_open_until_a_cause_and_a_conclusion_are_recorded() {
        let t = tempfile::tempdir().unwrap();
        let (w, ids) = work(t.path());
        write_jury(&w, &[jrow("j1", &ids[0], &ids[1], Side::X, 4, 2), jrow("j1", &ids[1], &ids[0], Side::Y, 2, 4)]);
        append(&w.join("operator.jsonl"), &json!({"pair": pair_id(&ids[0], &ids[1]), "first": ids[0], "winner": ids[1], "reason": "B is shorter", "repeat": false})).unwrap();
        let new = sync_ledger(&w).unwrap();
        assert_eq!(new.len(), 1);
        assert!(sync_ledger(&w).unwrap().is_empty(), "synced once");
        let id = new[0].id.clone();
        assert_eq!(ledger(&w)[0].status, "contested");
        assert!(resolve_dispute(&w, &id, "because", "x").unwrap_err().contains("not a cause"));
        assert!(resolve_dispute(&w, &id, "operator_taste", "  ").is_err(), "a conclusion is required");
        assert!(resolve_dispute(&w, "dnope", "rubric_gap", "x").is_err());
        resolve_dispute(&w, &id, "rubric_gap", "the rubric did not say length should not matter; added").unwrap();
        let e = &ledger(&w)[0];
        assert_eq!((e.status.as_str(), e.cause.as_deref()), ("resolved", Some("rubric_gap")));
        assert!(e.note.contains("rubric did not say"));
        assert_eq!(fs::read_to_string(w.join("ledger.jsonl")).unwrap().lines().count(), 2, "append-only: the contested entry stays in the file");
    }

    #[test]
    fn the_commands_refuse_a_directory_that_is_not_a_run() {
        let t = tempfile::tempdir().unwrap();
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cli("review", &a(&["review", "--work-dir", t.path().to_str().unwrap()]), &[]), 2);
        let (w, _) = work(t.path());
        assert_eq!(cli("resolve", &a(&["resolve", "--work-dir", w.to_str().unwrap()]), &[]), 2, "needs --id --cause --note");
        assert_eq!(cli("disputes", &a(&["disputes", "--work-dir", w.to_str().unwrap()]), &[]), 0);
    }
}
