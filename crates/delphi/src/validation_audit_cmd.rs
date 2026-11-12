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

pub fn queue(ws: &Path, home: &Path) -> String {
    let q = fact_audit::queue(&state_dir(ws));
    let mut out = String::new();
    if q.is_empty() {
        out.push_str("no claim is waiting for review\n");
    } else {
        out.extend(q.iter().map(|(f, n)| format!("{f}  referenced {n} time(s)  not validated (budget reached)\n")));
    }
    out.push_str(&lane_listing(ws, home));
    out.push_str(&background_listing(ws, home));
    out
}

/// Whether background validation is on, what it spent today against its cap, how many claims it checked, and when it last ran. Numbers only.
fn background_listing(ws: &Path, home: &Path) -> String {
    let cfg = std::fs::read_to_string(ws.join("config.toml")).unwrap_or_default();
    let s = delphi_engine::fact_background::status_from_files(ws, home, &cfg, now());
    // Silent while it is off and has never run, so an install that did not ask for it sees nothing new.
    if !s.enabled && s.claims_total == 0 {
        return String::new();
    }
    s.lines()
}

/// The library's claims the validator could be offered next, in the fast lane's order (`specs/100` §5). Ids, rungs and counts only, never a
/// claim's text. **Order only**: nothing here changes a claim's status, and the lane's switch is `[research.tiers] fast_lane_enabled` in the
/// workspace's `config.toml`. At most 20 are shown.
fn lane_listing(ws: &Path, home: &Path) -> String {
    let lane_on = std::fs::read_to_string(ws.join("config.toml")).ok().and_then(|t| toml::from_str::<toml::Value>(&t).ok()).map(|v| delphi_engine::boot::settings_from(&v).tiers.fast_lane_on).unwrap_or(true);
    let cands = delphi_engine::library::Library::in_home(home).validation_candidates(lane_on);
    if cands.is_empty() {
        return String::new();
    }
    let mut out = format!("library claims the validator could be offered next ({}):\n", if lane_on { "fast lane: a higher rung first" } else { "fast lane off: by id" });
    for c in cands.iter().take(20) {
        out.push_str(&format!("  {}  {}  {} origin(s), {} trusted source(s)\n", c.id, c.status.as_str(), c.origins, c.trusted_sources));
    }
    if cands.len() > 20 {
        out.push_str(&format!("  ... and {} more\n", cands.len() - 20));
    }
    out.push_str("the order changes nothing about a claim: its status is the same with the lane on or off\n");
    out
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
    fn the_queue_lists_the_librarys_claims_in_lane_order_with_ids_and_counts_and_never_their_text() {
        use delphi_engine::library::claims::{AnchorSpec, ClaimOpts};
        use delphi_engine::library::{AddOpts, Budgets, Library};
        let d = tempfile::tempdir().unwrap();
        let (ws, home) = (d.path().join("ws"), d.path().join("home"));
        std::fs::create_dir_all(&ws).unwrap();
        // with no library at all the queue is just the budget queue
        assert_eq!(queue(&ws, &home), "no claim is waiting for review\n");
        let l = Library::in_home(&home);
        let page = d.path().join("page.md");
        std::fs::write(&page, "# Page\n\nThe harbour closes at dusk. Boats leave at dawn.\n").unwrap();
        let s = l.add_file(&page, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap();
        let mut ids = Vec::new();
        for (text, quote, origins, trusted) in [("The harbour closes at dusk.", "closes at dusk", 1, 0), ("Boats leave at dawn.", "leave at dawn", 3, 1)] {
            let a = AnchorSpec { source_id: s.id.clone(), locator: Some("§1".into()), quote: quote.into(), ..Default::default() };
            let id = l.add_claim(text, &ClaimOpts::default(), &[a]).unwrap()["id"].as_str().unwrap().to_string();
            let mut c = l.claim(&id).unwrap();
            c["support"]["origins"] = serde_json::json!(origins);
            c["support"]["trusted_sources"] = serde_json::json!(trusted);
            std::fs::write(l.claims_dir().join(format!("{id}.json")), serde_json::to_vec(&c).unwrap()).unwrap();
            ids.push(id);
        }
        let out = queue(&ws, &home);
        assert!(out.contains("fast lane: a higher rung first"), "{out}");
        let (first, second) = (out.find(&ids[1]).unwrap(), out.find(&ids[0]).unwrap());
        assert!(first < second, "the substantiated claim comes before the noted one:\n{out}");
        assert!(out.contains("substantiated  3 origin(s), 1 trusted source(s)") && out.contains("its status is the same with the lane on or off"), "{out}");
        assert!(!out.contains("harbour") && !out.contains("dawn"), "ids, rungs and counts only, never the claim:\n{out}");
        // the lane's switch is the workspace's config
        std::fs::write(ws.join("config.toml"), "[research.tiers]\nfast_lane_enabled = false\n").unwrap();
        let off = queue(&ws, &home);
        assert!(off.contains("fast lane off: by id"), "{off}");
    }

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
