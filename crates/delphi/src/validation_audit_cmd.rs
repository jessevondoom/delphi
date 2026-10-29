//! `delphi validation audit | report | latch raise | retract --all` (`specs/040` §15, §24).
//!
//! The operator's, in a terminal. Every one is refused inside an agent seat by the caller, so nothing here can be
//! reached by the thing it audits. The logic lives in `delphi_engine::fact_audit`; this is the reading and the printing.

use delphi_adapter::config::{Config, ValidatorMode};
use delphi_core::ledger::{GroupBy, Ledger};
use delphi_engine::fact_audit::{self, Answer, Inputs, Labelled, Outcome};
use delphi_engine::fact_mode::{effective, read_latch, state_dir};
use delphi_engine::fact_store::FactStore;
use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::Path;

fn now() -> i64 {
    delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(0)
}

fn config_mode(ws: &Path) -> ValidatorMode {
    std::fs::read_to_string(ws.join("config.toml")).ok().and_then(|t| Config::parse(&t).ok()).map(|c| c.validator.mode).unwrap_or(ValidatorMode::Advisory)
}

/// The labelled-set counts that `validation score` left beside the set, if it has run.
fn labelled(home: &Path) -> Option<Labelled> {
    let t = std::fs::read_to_string(home.join("operator/validation/last-score.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&t).ok()?;
    Some(Labelled { claims: v.get("claims")?.as_u64()? as usize, false_ratified: v.get("false_ratified")?.as_u64()? as usize, poisoned_passes: v.get("poisoned_passes")?.as_u64()? as usize })
}

pub fn report(ws: &Path, home: &Path) -> Result<String, String> {
    let state = state_dir(ws);
    let eff = effective(config_mode(ws), &state);
    let other_tokens: BTreeMap<String, u64> = Ledger::open(&ws.join("local/state.db"))
        .ok()
        .and_then(|l| l.totals(GroupBy::Mission, None).ok())
        .map(|t| t.into_iter().map(|x| (x.key, x.input_tokens + x.output_tokens + x.thinking_tokens)).collect())
        .unwrap_or_default();
    let t = now();
    let out = fact_audit::report(&state, &eff, &read_latch(&state), &Inputs { labelled: labelled(home), other_tokens, now: t });
    fact_audit::write_promote(&state, &out, t).map_err(|e| format!("could not write promote.json: {e}"))?;
    Ok(out.text)
}

pub fn latch_raise(ws: &Path, to: &str, reason: &str, force: bool) -> Result<String, String> {
    let to = match ValidatorMode::parse(to) {
        Some(m @ (ValidatorMode::Advisory | ValidatorMode::Ratify)) => m,
        _ => return Err("--to is advisory or ratify".into()),
    };
    let batch = fact_audit::raise_latch(&state_dir(ws), to, reason, force, now())?;
    Ok(format!("latch raised to {}; a new audit batch ({batch}) starts, so older facts stay with the old one\n", to.as_str()))
}

pub fn unquarantine(ws: &Path, fact: &str, reason: &str) -> Result<String, String> {
    fact_audit::unquarantine(&state_dir(ws), &FactStore::new(ws), fact, reason, now())?;
    Ok(format!("{fact} is out of quarantine; the reason is journaled as yours\n"))
}

pub fn more(ws: &Path, mission: &str, reason: &str) -> Result<String, String> {
    Ok(format!("{}; the raise is journaled as yours\n", fact_audit::raise_share(&state_dir(ws), mission, reason, now())?))
}

pub fn queue(ws: &Path) -> String {
    let q = fact_audit::queue(&state_dir(ws));
    if q.is_empty() {
        return "no claim is waiting for review\n".into();
    }
    q.iter().map(|(f, n)| format!("{f}  referenced {n} time(s)  not validated (budget reached)\n")).collect()
}

pub fn retract(ws: &Path) -> String {
    let r = fact_audit::retract_all(ws, &state_dir(ws), &FactStore::new(ws), now());
    let mut out = format!("retracted {} fact(s); none was deleted, each is marked superseded by its retraction\n", r.retracted.len());
    if r.readers.is_empty() {
        out.push_str("no mission is recorded as having read one of them\n");
    } else {
        out.push_str("missions that read a retracted fact (nothing was marked stale; this is your list):\n");
        for x in &r.readers {
            out.push_str(&format!("  {}  read {} by {}\n", x.mission, x.fact, x.span));
        }
    }
    out
}

/// Show the claim, take the answer, only then reveal. `input` is the terminal; a test passes lines.
pub fn audit(ws: &Path, card: Option<&str>, input: &mut dyn BufRead, out: &mut dyn std::io::Write) -> Result<(), String> {
    let state = state_dir(ws);
    let open = fact_audit::open_cards(&state);
    let (n, oldest) = fact_audit::backlog(&state, now());
    let Some(card) = card.map(String::from).or_else(|| open.first().map(|c| c.card.clone())) else {
        let _ = writeln!(out, "no cards are waiting");
        return Ok(());
    };
    let claim = fact_audit::show_claim(ws, &state, &card)?;
    let _ = writeln!(out, "{n} card(s) waiting, the oldest {} day(s)\n\nclaim: {claim}\n", oldest / 86_400);
    let _ = write!(out, "Is it true? [true/false/unsure] ");
    let _ = out.flush();
    let mut line = String::new();
    input.read_line(&mut line).map_err(|e| e.to_string())?;
    let a = Answer::parse(&line).ok_or("answer true, false or unsure; nothing was recorded")?;
    let r = fact_audit::answer(ws, &state, &FactStore::new(ws), &card, a, now())?;
    let _ = writeln!(out, "\nthe validator said: {}", r.verdict);
    for s in &r.sources {
        let _ = writeln!(out, "  source: {s}");
    }
    for why in &r.reasons {
        let _ = writeln!(out, "  - {why}");
    }
    let _ = writeln!(
        out,
        "{}",
        match r.outcome {
            Outcome::Confirmed => "confirmed".to_string(),
            Outcome::Rejected { quarantined } => format!("REJECTED: the latch is now advisory; promote.json says no; {} fact(s) of the batch are quarantined", quarantined.len()),
            Outcome::Unsure => "recorded as unsure; it is not a confirmed audit".into(),
            Outcome::Catch => "a confirmed catch".into(),
            Outcome::FalseRefutation => "the validator refuted a true claim; recorded".into(),
            Outcome::Noted => "noted".into(),
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_without_any_audit_says_no_and_writes_the_record() {
        let d = tempfile::tempdir().unwrap();
        let (ws, home) = (d.path().join("ws"), d.path().join("home"));
        std::fs::create_dir_all(&ws).unwrap();
        let text = report(&ws, &home).unwrap();
        assert!(text.contains("PROMOTE: no"), "{text}");
        assert!(std::fs::read_to_string(state_dir(&ws).join("promote.json")).unwrap().contains("\"promote\":false"));
    }

    #[test]
    fn a_latch_raise_needs_a_reason_and_a_real_raise() {
        let d = tempfile::tempdir().unwrap();
        assert!(latch_raise(d.path(), "ratify", "x", false).is_err());
        assert!(latch_raise(d.path(), "off", "because I said so, at length", false).is_err());
        assert!(latch_raise(d.path(), "ratify", "reviewed the batch at length", false).is_err(), "ratify needs PROMOTE or --force");
    }

    #[test]
    fn audit_with_no_cards_says_so_and_a_bad_answer_records_nothing() {
        let d = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        audit(d.path(), None, &mut std::io::Cursor::new(""), &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("no cards"));
        assert!(audit(d.path(), Some("fact-nope"), &mut std::io::Cursor::new("true\n"), &mut Vec::new()).is_err());
    }
}
