//! `delphi validation tier-audit | tier-report | tier rollback | restore | threshold` (`specs/100` §7).
//!
//! The operator's, in a terminal. The caller refuses every one inside an agent seat, so nothing here can be reached by the thing it audits. The
//! logic lives in `delphi_engine::tier_audit`; this is the asking and the printing. The audit is a roll-back trigger, not a promotion gate: it can
//! take a tier away and never grants one.

use delphi_engine::fact_audit::Answer;
use delphi_engine::fact_mode::state_dir;
use delphi_engine::library::Library;
use delphi_engine::tier_audit as ta;
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::Path;

fn now() -> i64 {
    delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(0)
}

/// How many claims are at each audited tier now, by the configured ladder (a latch changes what a claim is shown as, not which claims reached the tier).
fn members(home: &Path) -> BTreeMap<String, usize> {
    let l = Library::in_home(home);
    ["substantiated", "vetted", "batch"].iter().map(|t| (t.to_string(), l.tier_members(t).len())).collect()
}

pub fn report(ws: &Path, home: &Path) -> String {
    ta::report(&state_dir(ws), &members(home), now())
}

/// `delphi validation tier rollback <tier> --reason "<text>"`, `tier restore <tier> --reason "<text>"`, `tier threshold <tier> <percent>`.
pub fn tier(ws: &Path, args: &[String], reason: &str) -> Result<String, String> {
    let state = state_dir(ws);
    let name = args.get(2).map(String::as_str).unwrap_or("");
    match args.get(1).map(String::as_str) {
        Some("rollback") => ta::rollback(&state, name, "operator", reason, now()),
        Some("restore") => ta::restore(&state, name, reason, now()),
        Some("threshold") => {
            let percent = args.get(3).and_then(|p| p.trim_end_matches('%').parse::<f64>().ok()).ok_or("usage: delphi validation tier threshold <tier> <percent>")?;
            ta::set_threshold(&state, name, percent, now())
        }
        _ => Err("usage: delphi validation tier rollback|restore <tier> --reason \"<text>\"  |  tier threshold <tier> <percent>".into()),
    }
}

/// Draw a sample if none is open, then show each claim, take the operator's answer, and only then reveal its sources. `input` is the terminal; a test
/// passes lines. The claim's words are shown first and nothing else about it: not its rung's counts, not its sources.
pub fn audit(ws: &Path, home: &Path, tier: &str, n: usize, input: &mut dyn BufRead, out: &mut dyn Write) -> Result<(), String> {
    let state = state_dir(ws);
    let lib = Library::in_home(home);
    if ta::open_tier_cards(&state).iter().all(|c| c.tier != tier) {
        let candidates = lib.tier_members(tier);
        if candidates.is_empty() {
            let _ = writeln!(out, "{}", if tier == "batch" { "no claim is batch-accepted yet (accept-all is slice L3)" } else { "no claim is at this tier" });
            return Ok(());
        }
        let drawn = ta::draw(&state, tier, &candidates, n, now())?;
        let _ = writeln!(out, "drew {} claim(s) of {} at random, spread over their source classes", drawn.len(), candidates.len());
    }
    let cards: Vec<ta::TierCard> = ta::open_tier_cards(&state).into_iter().filter(|c| c.tier == tier).collect();
    if cards.is_empty() {
        let _ = writeln!(out, "every claim drawn for this tier in this batch has been answered; `delphi validation tier-report` shows where that leaves it");
        return Ok(());
    }
    for c in cards {
        let Some(claim) = lib.claim(&c.card) else {
            let _ = writeln!(out, "{}: the claim is not in the library any more; skipped", c.card);
            continue;
        };
        let _ = writeln!(out, "\nClaim {}:\n  {}\n", c.card, claim["text"].as_str().unwrap_or(""));
        let answer = loop {
            let _ = write!(out, "Is this true, false or unsure? ");
            let _ = out.flush();
            let mut line = String::new();
            if input.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                let _ = writeln!(out, "\n(no more input; the rest stay open)");
                return Ok(());
            }
            if let Some(a) = Answer::parse(&line) {
                break a;
            }
            let _ = writeln!(out, "  true, false or unsure");
        };
        // The answer is on disk before the sources are read.
        let result = ta::answer(&state, &c.card, tier, answer, now())?;
        let _ = writeln!(out, "recorded. Now the sources:");
        for s in lib.claim_sources(&c.card) {
            let _ = writeln!(out, "  - {} ({}; {})", s.0, s.1, s.2);
        }
        let _ = writeln!(out, "  counted origins: {}, trusted sources: {}", claim["support"]["origins"], claim["support"]["trusted_sources"]);
        if let ta::Answered::RolledBack { tier, trigger } = result {
            let _ = writeln!(out, "\nTHE {} TIER IS ROLLED BACK ({trigger}). Its claims read one tier lower until you restore it; nothing was deleted.", tier.to_uppercase());
            return Ok(());
        }
    }
    let _ = writeln!(out, "\nthat was the sample; `delphi validation tier-report` shows what it says");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A library with one claim on the vetted rung: the counts of the top rung, a source with an address, and the validator's dry run recorded.
    fn vetted_claim(d: &Path) -> (std::path::PathBuf, std::path::PathBuf, String) {
        use delphi_engine::library::claims::{AnchorSpec, ClaimOpts};
        use delphi_engine::library::{AddOpts, Budgets};
        let (ws, home) = (d.join("ws"), d.join("home"));
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(&home);
        let page = d.join("page.md");
        std::fs::write(&page, "# Harbour\n\nThe harbour closes at dusk.\n").unwrap();
        let s = l.add_file(&page, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap();
        let a = AnchorSpec { source_id: s.id.clone(), locator: Some("§1".into()), quote: "closes at dusk".into(), ..Default::default() };
        let text = "The harbour closes at dusk.";
        let id = l.add_claim(text, &ClaimOpts::default(), &[a]).unwrap()["id"].as_str().unwrap().to_string();
        let mut c = l.claim(&id).unwrap();
        c["support"]["origins"] = serde_json::json!(5);
        c["support"]["trusted_sources"] = serde_json::json!(2);
        std::fs::write(l.claims_dir().join(format!("{id}.json")), serde_json::to_vec(&c).unwrap()).unwrap();
        l.record_would_ratify(&delphi_engine::fact_store::fact_id(text));
        (ws, home, id)
    }

    #[test]
    fn the_card_shows_the_claim_first_and_the_sources_only_after_the_answer_is_recorded() {
        let d = tempfile::tempdir().unwrap();
        let (ws, home, id) = vetted_claim(d.path());
        let mut out = Vec::new();
        audit(&ws, &home, "vetted", 5, &mut std::io::Cursor::new("maybe\ntrue\n"), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let (claim, asked, recorded, source) = (text.find("The harbour closes at dusk.").unwrap(), text.find("Is this true, false or unsure?").unwrap(), text.find("recorded. Now the sources").unwrap(), text.find("a file you added").unwrap());
        assert!(claim < asked && asked < recorded && recorded < source, "claim, then the question, then the answer is recorded, then the sources:\n{text}");
        assert!(text.contains("true, false or unsure\n"), "an answer that is not one of the three is asked again:\n{text}");
        assert!(!text[..recorded].contains("counted origins") && !text[..recorded].contains("page.md"), "nothing about the claim's support before the answer:\n{text}");
        // the answer is in the journal, and one true answer is a sample of one with no errors
        let st = delphi_engine::tier_audit::stats(&delphi_engine::fact_mode::state_dir(&ws), "vetted");
        assert_eq!((st.sample, st.errors), (1, 0));
        assert!(delphi_engine::tier_audit::latched(&delphi_engine::fact_mode::state_dir(&ws)).is_empty());
        let _ = id;
    }

    #[test]
    fn a_false_answer_on_a_vetted_claim_rolls_the_tier_back_and_says_so_and_the_report_and_needs_show_it() {
        let d = tempfile::tempdir().unwrap();
        let (ws, home, id) = vetted_claim(d.path());
        let before = std::fs::read(Library::in_home(&home).claims_dir().join(format!("{id}.json"))).unwrap();
        let mut out = Vec::new();
        audit(&ws, &home, "vetted", 5, &mut std::io::Cursor::new("false\n"), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("THE VETTED TIER IS ROLLED BACK (hard_failure)") && text.contains("nothing was deleted"), "{text}");
        let state = delphi_engine::fact_mode::state_dir(&ws);
        assert!(delphi_engine::tier_audit::latched(&state).contains("vetted"));
        assert_eq!(std::fs::read(Library::in_home(&home).claims_dir().join(format!("{id}.json"))).unwrap(), before, "the claim on disk is byte for byte what it was");
        let r = report(&ws, &home);
        assert!(r.contains("vetted [ROLLED BACK]") && r.contains("tier.rollback vetted  trigger hard_failure"), "{r}");
        assert!(delphi_engine::tier_audit::needs_line(&state).contains("vetted rolled back"));
        // the claim is still offered to an audit of the tier it reached, until the operator restores it
        assert_eq!(Library::in_home(&home).tier_members("vetted").len(), 1);
    }

    #[test]
    fn an_audit_with_nothing_at_the_tier_says_so() {
        let d = tempfile::tempdir().unwrap();
        let (ws, home) = (d.path().join("ws"), d.path().join("home"));
        std::fs::create_dir_all(&ws).unwrap();
        let mut out = Vec::new();
        audit(&ws, &home, "vetted", 5, &mut std::io::Cursor::new(""), &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("no claim is at this tier"));
        let mut out = Vec::new();
        audit(&ws, &home, "batch", 5, &mut std::io::Cursor::new(""), &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("accept-all is slice L3"));
    }

    #[test]
    fn a_threshold_needs_a_tier_and_a_percentage() {
        let d = tempfile::tempdir().unwrap();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(tier(d.path(), &args(&["tier", "threshold", "vetted"]), "").is_err());
        assert!(tier(d.path(), &args(&["tier", "threshold", "vetted", "banana"]), "").is_err());
        assert!(tier(d.path(), &args(&["tier", "threshold", "corroborated", "5"]), "").is_err());
        assert!(tier(d.path(), &args(&["tier", "threshold", "vetted", "5%"]), "").unwrap().contains("above 5%"));
        assert_eq!(ta::threshold(&state_dir(d.path()), "vetted"), Some(5.0));
    }

    #[test]
    fn a_rollback_and_a_restore_each_need_a_reason() {
        let d = tempfile::tempdir().unwrap();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(tier(d.path(), &args(&["tier", "rollback", "vetted"]), "").is_err());
        assert!(tier(d.path(), &args(&["tier", "rollback", "vetted"]), "it is wrong").is_ok());
        assert!(tier(d.path(), &args(&["tier", "restore", "vetted"]), "x").is_err());
        assert!(tier(d.path(), &args(&["tier", "restore", "vetted"]), "audited again, fine").is_ok());
        assert!(tier(d.path(), &args(&["tier", "bogus"]), "").is_err());
    }
}
