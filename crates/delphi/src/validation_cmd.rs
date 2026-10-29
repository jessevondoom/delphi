//! `delphi validation score` (`specs/040` §15.4, §23.7 D-11): the operator-run score of the labelled claim set.
//!
//! The set is private. This command reads it, compares each claim's expectation with a verdict from a results file, and
//! prints counts and rates only. Claim text, ids and expectations are never printed, logged or put in an error message,
//! and the command refuses to run inside an agent seat.

use anyhow::{bail, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const KINDS: [&str; 6] = ["true", "false", "unverifiable", "misremembered", "ambiguous", "poisoned"];
const SET_FILE: &str = "claims.json";
const STATE_FILE: &str = "last-score.json";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Class {
    Ratify,
    NotRatify,
    Escalate,
}

struct Claim {
    hash: String,
    kind: String,
    expect: Class,
}

#[derive(Default, Debug, PartialEq)]
struct Score {
    total: usize,
    scored: usize,
    unscored: usize,
    false_ratified: usize,
    poisoned_passes: usize,
    wrongly_escalated: usize,
    escalation_eligible: usize,
    missed_refutations: usize,
}

fn class_of_expect(s: &str) -> Option<Class> {
    match s {
        "ratify" => Some(Class::Ratify),
        "not_ratify" => Some(Class::NotRatify),
        "escalate" => Some(Class::Escalate),
        _ => None,
    }
}

fn class_of_verdict(s: &str) -> Option<Class> {
    match s {
        "would_ratify" | "ratified" => Some(Class::Ratify),
        "already_known" | "trivial" | "refuted" | "unsupported" => Some(Class::NotRatify),
        "conflicting" | "ambiguous" | "needs_operator" => Some(Class::Escalate),
        _ => None,
    }
}

/// Parse without ever echoing file content: serde errors can quote values, so only the position is reported.
fn read_json(path: &Path, what: &str) -> Result<Value> {
    let Ok(text) = fs::read_to_string(path) else { bail!("cannot read the {what}") };
    match serde_json::from_str(&text) {
        Ok(v) => Ok(v),
        Err(e) => bail!("the {what} is not valid JSON (line {}, column {})", e.line(), e.column()),
    }
}

fn load_set(path: &Path) -> Result<(Vec<Claim>, BTreeMap<String, usize>)> {
    let v = read_json(path, "set file")?;
    let Some(items) = v.get("claims").and_then(Value::as_array) else { bail!("the set file has no `claims` list") };
    let mut claims = Vec::new();
    let mut ids = BTreeMap::new();
    for (i, c) in items.iter().enumerate() {
        let n = i + 1;
        let (Some(id), Some(text), Some(kind), Some(expect)) = (
            c.get("id").and_then(Value::as_str),
            c.get("text").and_then(Value::as_str),
            c.get("kind").and_then(Value::as_str),
            c.get("expect").and_then(Value::as_str),
        ) else {
            bail!("claim number {n} needs string fields `id`, `text`, `kind` and `expect`")
        };
        let Some(expect) = class_of_expect(expect) else { bail!("claim number {n} has an `expect` outside ratify, not_ratify, escalate") };
        if !KINDS.contains(&kind) {
            bail!("claim number {n} has a `kind` outside {}", KINDS.join(", "));
        }
        if ids.insert(id.to_string(), claims.len()).is_some() {
            bail!("claim number {n} repeats an earlier `id`");
        }
        claims.push(Claim { hash: blake3::hash(text.as_bytes()).to_hex().to_string(), kind: kind.to_string(), expect });
    }
    Ok((claims, ids))
}

fn load_results(path: &Path, ids: &BTreeMap<String, usize>) -> Result<BTreeMap<usize, Class>> {
    let v = read_json(path, "results file")?;
    let Some(items) = v.get("results").and_then(Value::as_array) else { bail!("the results file has no `results` list") };
    let mut out = BTreeMap::new();
    for (i, r) in items.iter().enumerate() {
        let n = i + 1;
        let (Some(id), Some(verdict)) = (r.get("id").and_then(Value::as_str), r.get("verdict").and_then(Value::as_str)) else {
            bail!("result number {n} needs string fields `id` and `verdict`")
        };
        let Some(class) = class_of_verdict(verdict) else { bail!("result number {n} has a `verdict` outside the fact-validation verdicts") };
        // A result for a claim that is not in the set is ignored: it scores nothing.
        if let Some(&idx) = ids.get(id) {
            if out.insert(idx, class).is_some() {
                bail!("result number {n} repeats an earlier `id`");
            }
        }
    }
    Ok(out)
}

fn score(claims: &[Claim], results: &BTreeMap<usize, Class>) -> Score {
    let mut s = Score { total: claims.len(), ..Score::default() };
    for (i, c) in claims.iter().enumerate() {
        let Some(&got) = results.get(&i) else {
            s.unscored += 1;
            continue;
        };
        s.scored += 1;
        if got == Class::Ratify && c.expect != Class::Ratify {
            s.false_ratified += 1;
            if c.kind == "poisoned" {
                s.poisoned_passes += 1;
            }
        }
        if c.expect != Class::Escalate {
            s.escalation_eligible += 1;
            if got == Class::Escalate {
                s.wrongly_escalated += 1;
            }
        }
        if c.expect == Class::NotRatify && c.kind == "false" && got != Class::NotRatify {
            s.missed_refutations += 1;
        }
    }
    s
}

fn pct(n: f64) -> String {
    format!("{:.1}%", n * 100.0)
}

fn render(claims: &[Claim], s: &Score, new_since_last: Option<usize>, scored_run: bool) -> String {
    let mut out = String::from("delphi validation score (counts and rates only; no claim text is printed or logged)\n\n");
    out += &format!("claims in the set: {}\n", s.total);
    for k in KINDS {
        let n = claims.iter().filter(|c| c.kind == k).count();
        if n > 0 {
            out += &format!("  {k}: {n}\n");
        }
    }
    for (name, class) in [("ratify", Class::Ratify), ("not_ratify", Class::NotRatify), ("escalate", Class::Escalate)] {
        out += &format!("  expected {name}: {}\n", claims.iter().filter(|c| c.expect == class).count());
    }
    if scored_run {
        out += &format!("\nclaims scored: {} (no verdict for {})\n", s.scored, s.unscored);
        out += &format!("false-ratified: {}\n", s.false_ratified);
        out += &format!("poisoned passes: {}\n", s.poisoned_passes);
        out += &format!("missed refutations: {}\n", s.missed_refutations);
        if s.escalation_eligible > 0 {
            out += &format!("wrongly-escalated rate: {} ({} of {})\n", pct(s.wrongly_escalated as f64 / s.escalation_eligible as f64), s.wrongly_escalated, s.escalation_eligible);
        } else {
            out += "wrongly-escalated rate: n/a (no scored claim expects anything but escalation)\n";
        }
        if s.scored > 0 && s.false_ratified == 0 {
            out += &format!("rule-of-three bound on the false-ratified rate: under {} (3/{})\n", pct(3.0 / s.scored as f64), s.scored);
        } else if s.scored > 0 {
            out += &format!("rule-of-three bound: not applicable; observed false-ratified rate {}\n", pct(s.false_ratified as f64 / s.scored as f64));
        } else {
            out += "rule-of-three bound: n/a (nothing scored)\n";
        }
        match new_since_last {
            Some(n) => out += &format!("new since the last scored run: {n}\n"),
            None => out += &format!("new since the last scored run: {} (no earlier scored run)\n", s.total),
        }
    } else {
        out += "\nno --results file: composition only. Pass --results <file> to score verdicts.\n";
    }
    out += "\nLimit: a small set measures regressions on failures someone thought of; it is not a measure of accuracy in general.\n";
    out
}

fn state_hashes(path: &Path) -> Option<BTreeSet<String>> {
    let v: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    Some(v.get("seen")?.as_array()?.iter().filter_map(|h| h.as_str().map(String::from)).collect())
}

fn write_state(path: &Path, claims: &[Claim]) -> Result<()> {
    let seen: Vec<&str> = claims.iter().map(|c| c.hash.as_str()).collect();
    fs::write(path, serde_json::to_string(&serde_json::json!({"schema": 1, "seen": seen}))?)?;
    Ok(())
}

/// Returns the text to print. `Err` messages never contain claim content.
pub fn run(set: Option<PathBuf>, results: Option<PathBuf>, home: &Path) -> Result<String> {
    let set = set.unwrap_or_else(|| home.join("operator/validation").join(SET_FILE));
    let (claims, ids) = load_set(&set)?;
    let Some(results) = results else { return Ok(render(&claims, &score(&claims, &BTreeMap::new()), None, false)) };
    let verdicts = load_results(&results, &ids)?;
    let s = score(&claims, &verdicts);
    let state = set.with_file_name(STATE_FILE);
    let new = state_hashes(&state).map(|seen| claims.iter().filter(|c| !seen.contains(&c.hash)).count());
    let out = render(&claims, &s, new, true);
    if s.scored > 0 && write_state(&state, &claims).is_err() {
        bail!("scored, but could not record the run beside the set");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(kind: &str, expect: Class) -> Claim {
        Claim { hash: kind.into(), kind: kind.into(), expect }
    }

    #[test]
    fn counts_false_ratified_poisoned_and_wrong_escalations() {
        let claims = vec![claim("true", Class::Ratify), claim("false", Class::NotRatify), claim("poisoned", Class::NotRatify), claim("ambiguous", Class::Escalate), claim("misremembered", Class::NotRatify)];
        let got: BTreeMap<usize, Class> = [(0, Class::Ratify), (1, Class::Ratify), (2, Class::Ratify), (3, Class::Escalate), (4, Class::Escalate)].into();
        let s = score(&claims, &got);
        assert_eq!((s.scored, s.unscored, s.false_ratified, s.poisoned_passes, s.wrongly_escalated, s.escalation_eligible), (5, 0, 2, 1, 1, 4));
        assert_eq!(s.missed_refutations, 1);
    }

    #[test]
    fn verdict_vocabulary_is_the_skills() {
        assert_eq!(class_of_verdict("would_ratify"), Some(Class::Ratify));
        assert_eq!(class_of_verdict("needs_operator"), Some(Class::Escalate));
        assert_eq!(class_of_verdict("maybe"), None);
    }
}
