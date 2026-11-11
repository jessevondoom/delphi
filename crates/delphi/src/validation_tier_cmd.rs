//! `delphi validation tier-audit | tier-report | tier rollback | restore | threshold` (`specs/100` §7).
//!
//! The operator's, in a terminal. The caller refuses every one inside an agent seat, so nothing here can be reached by the thing it audits. The
//! logic lives in `delphi_engine::tier_audit`; this is the asking and the printing. The audit is a roll-back trigger, not a promotion gate: it can
//! take a tier away and never grants one.

use delphi_engine::accept_all as ac;
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

/// After a roll-back: flag the batch-accepted claims accepted since the last good audit `re-audit due` (`specs/100` §7.3). Idempotent.
pub fn flag_reaudit(ws: &Path, home: &Path) -> usize {
    ac::flag_reaudit(&Library::in_home(home), &state_dir(ws), now())
}

/// `delphi validation accept-all --list`: what an accept-all would take now and what it leaves out, with nothing written. Reading only; it needs no
/// terminal, because it asks nothing and accepts nothing.
pub fn accept_all_list(ws: &Path, home: &Path, out: &mut dyn Write) -> Result<(), String> {
    let (state, lib) = (state_dir(ws), Library::in_home(home));
    show_plan(&ac::plan(&lib, &state, false), &state, out);
    Ok(())
}

fn show_plan(p: &ac::Plan, state: &Path, out: &mut dyn Write) {
    if let Some(why) = &p.disabled {
        let _ = writeln!(out, "accept-all is off: {why}");
        return;
    }
    if p.rows.is_empty() {
        let _ = writeln!(out, "no claim reads vetted and is free to accept; nothing waits");
    } else {
        let _ = writeln!(out, "{} vetted claim(s) in {} group(s). They passed the validator's dry run and are NOT validated until you accept. Claims carry no topic, so they are grouped by project, else by scope.\n", p.rows.len(), ac::grouped(&p.rows).len());
        for (group, rows) in ac::grouped(&p.rows) {
            let _ = writeln!(out, "{group} ({})", rows.len());
            for r in rows {
                let _ = writeln!(out, "  {}  {}", r.id, r.text);
                let _ = writeln!(out, "      {} origins ({}), {} trusted sources, checked {}", r.origins, r.domains.join(", "), r.trusted_sources, if r.checked_at.is_empty() { "at an unrecorded time" } else { r.checked_at.as_str() });
            }
        }
    }
    if !p.skipped.is_empty() {
        let _ = writeln!(out, "\nleft out of any batch ({}):", p.skipped.len());
        for s in &p.skipped {
            let _ = writeln!(out, "  {}  {}\n      {}", s.id, s.text, s.why);
        }
    }
    if !p.rows.is_empty() {
        let _ = writeln!(out, "\nSources are re-checked when you accept; a claim whose sources changed since the validator checked it is left out then.");
        let _ = writeln!(out, "The audit batch is {}.", delphi_engine::fact_audit::current_batch(state));
    }
}

/// `delphi validation accept-all [--only id,id]`: show the list, the three-claim glance and what is left out, then ask. Nothing is accepted unless the
/// operator types the phrase; anything else leaves every claim as it was. `input` is the terminal; a test passes lines. The caller needs a real
/// terminal and refuses inside an agent seat, so an agent cannot answer for the operator.
pub fn accept_all(ws: &Path, home: &Path, only: Option<Vec<String>>, input: &mut dyn BufRead, out: &mut dyn Write) -> Result<(), String> {
    let (state, lib) = (state_dir(ws), Library::in_home(home));
    let p = ac::plan(&lib, &state, false);
    show_plan(&p, &state, out);
    if p.disabled.is_some() || p.rows.is_empty() {
        return Ok(());
    }
    let batch = delphi_engine::fact_audit::current_batch(&state);
    let _ = writeln!(out, "\nA glance at three, with their quotes:");
    for r in ac::sample(&p.rows, &batch) {
        let _ = writeln!(out, "  {}\n    \"{}\"", r.text, r.quote);
    }
    let _ = writeln!(out, "\n{}\n", ac::GLANCE_NOTE);
    let (phrase, count) = match &only {
        Some(ids) => ("accept some", ids.len()),
        None => ("accept all", p.rows.len()),
    };
    let _ = write!(out, "Type `{phrase}` to accept {count} claim(s) as validated by you; anything else leaves them as they are: ");
    let _ = out.flush();
    let mut line = String::new();
    if input.read_line(&mut line).map_err(|e| e.to_string())? == 0 || line.trim() != phrase {
        let _ = writeln!(out, "\nnothing was accepted");
        return Ok(());
    }
    let o = ac::accept(&lib, &state, only.as_deref(), now())?;
    let _ = writeln!(out, "\naccepted {} claim(s) as validated by you, each marked batch-accepted in audit batch {}.", o.accepted.len(), o.audit_batch);
    for (id, why) in &o.refused {
        let _ = writeln!(out, "  not accepted: {id}: {why}");
    }
    if !o.skipped.is_empty() {
        let _ = writeln!(out, "  left out: {} (see above)", o.skipped.len());
    }
    let _ = writeln!(out, "Audit them: `delphi validation tier-audit --tier batch`. The audit is the bound; a failed one rolls the vetted tier back and flags these `re-audit due`.");
    Ok(())
}

/// Draw a sample if none is open, then show each claim, take the operator's answer, and only then reveal its sources. `input` is the terminal; a test
/// passes lines. The claim's words are shown first and nothing else about it: not its rung's counts, not its sources.
pub fn audit(ws: &Path, home: &Path, tier: &str, n: usize, input: &mut dyn BufRead, out: &mut dyn Write) -> Result<(), String> {
    let state = state_dir(ws);
    let lib = Library::in_home(home);
    if ta::open_tier_cards(&state).iter().all(|c| c.tier != tier) {
        let candidates = lib.tier_members(tier);
        if candidates.is_empty() {
            let _ = writeln!(out, "{}", if tier == "batch" { "no claim is batch-accepted yet (`delphi validation accept-all` marks them)" } else { "no claim is at this tier" });
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
        let was_true = answer == Answer::True;
        let result = ta::answer(&state, &c.card, tier, answer, now())?;
        if tier == "batch" && was_true {
            lib.clear_reaudit_due(&c.card);
        }
        let _ = writeln!(out, "recorded. Now the sources:");
        for s in lib.claim_sources(&c.card) {
            let _ = writeln!(out, "  - {} ({}; {})", s.0, s.1, s.2);
        }
        let _ = writeln!(out, "  counted origins: {}, trusted sources: {}", claim["support"]["origins"], claim["support"]["trusted_sources"]);
        if let ta::Answered::RolledBack { tier, trigger } = result {
            let _ = writeln!(out, "\nTHE {} TIER IS ROLLED BACK ({trigger}). Its claims read one tier lower until you restore it; nothing was deleted.", tier.to_uppercase());
            let due = ac::flag_reaudit(&lib, &state, now());
            if due > 0 {
                let _ = writeln!(out, "{due} batch-accepted claim(s) accepted since the last good audit are flagged re-audit due; they stay validated, and `delphi validation tier-audit --tier batch` samples them. Accept-all is off until you restore the tier.");
            }
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

    /// A claim on the vetted rung the real way: five trusted sources as the fetcher leaves them, the claim anchored in all five, support computed by the
    /// library, the validator's dry run recorded. (`vetted_claim` above writes the counts by hand; accept-all rechecks them, so it needs the real thing.)
    fn real_vetted(d: &Path, name: &str, text: &str) -> (std::path::PathBuf, std::path::PathBuf, String) {
        use delphi_engine::library::claims::{AnchorSpec, ClaimOpts};
        use delphi_engine::library::{AddOpts, Budgets};
        let (ws, home) = (d.join("ws"), d.join("home"));
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(&home);
        let quote = text.trim_end_matches('.').to_string();
        let specs: Vec<AnchorSpec> = ["harbours", "fishing", "navigation", "weather", "lighthouses"]
            .iter()
            .enumerate()
            .map(|(i, topic)| {
                let host = format!("{name}-{i}.example");
                let page = d.join(format!("{host}.md"));
                std::fs::write(&page, format!("# {topic}\n\nA separate write-up about {topic} for {name}, with its own wording and its own examples. Here {text} {topic} is all this page covers, in {host}.\n")).unwrap();
                let s = l.add_file(&page, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap();
                let mut m = l.read_meta(&s.id).unwrap();
                m["canonical_url"] = serde_json::json!(format!("https://{host}/page"));
                m["origin"] = serde_json::json!(host);
                m["content_hash"] = serde_json::json!(format!("sha256:{host}"));
                m["source_class"] = serde_json::json!("reputable_secondary");
                m["source_unfamiliar"] = serde_json::json!(false);
                std::fs::write(l.source_dir(&s.id).join("meta.json"), serde_json::to_vec(&m).unwrap()).unwrap();
                AnchorSpec { source_id: s.id, locator: Some("§1".into()), quote: quote.clone(), ..Default::default() }
            })
            .collect();
        let id = l.add_claim(text, &ClaimOpts::default(), &specs).unwrap()["id"].as_str().unwrap().to_string();
        l.refresh_support(&id).unwrap();
        assert_eq!(l.record_would_ratify(&delphi_engine::fact_store::fact_id(text)), vec![id.clone()]);
        (ws, home, id)
    }

    #[test]
    fn the_list_prints_the_groups_and_what_is_left_out_and_writes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let (ws, home, id) = real_vetted(d.path(), "harbour", "The harbour closes at dusk.");
        let before = std::fs::read(Library::in_home(&home).claims_dir().join(format!("{id}.json"))).unwrap();
        let mut out = Vec::new();
        accept_all_list(&ws, &home, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("1 vetted claim(s) in 1 group(s)") && text.contains(&id) && text.contains("5 origins") && text.contains("5 trusted sources") && text.contains("NOT validated until you accept"), "{text}");
        assert_eq!(std::fs::read(Library::in_home(&home).claims_dir().join(format!("{id}.json"))).unwrap(), before, "listing writes nothing");
        assert!(delphi_engine::fact_audit::Journal::new(&state_dir(&ws)).all().iter().all(|e| e["kind"] != "accept_all"));
    }

    #[test]
    fn nothing_is_accepted_unless_the_operator_types_the_phrase_and_the_glance_says_it_bounds_nothing() {
        let d = tempfile::tempdir().unwrap();
        let (ws, home, id) = real_vetted(d.path(), "harbour", "The harbour closes at dusk.");
        for typed in ["yes\n", "", "accept\n", "ACCEPT ALL please\n"] {
            let mut out = Vec::new();
            accept_all(&ws, &home, None, &mut std::io::Cursor::new(typed), &mut out).unwrap();
            let text = String::from_utf8(out).unwrap();
            assert!(text.contains("A glance at three") && text.contains("bounds nothing") && text.contains("nothing was accepted"), "{typed:?}: {text}");
            assert!(Library::in_home(&home).claim(&id).unwrap()["validation"].is_null(), "{typed:?}: not accepted");
        }
        let mut out = Vec::new();
        accept_all(&ws, &home, None, &mut std::io::Cursor::new("accept all\n"), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("accepted 1 claim(s) as validated by you") && text.contains("batch-accepted") && text.contains("tier-audit --tier batch"), "{text}");
        let c = Library::in_home(&home).claim(&id).unwrap();
        assert_eq!((c["validation"]["by"].as_str(), c["validation"]["batch"]["mark"].as_str()), (Some("operator"), Some("batch-accepted")));
        assert_eq!(Library::in_home(&home).tier_members("batch").len(), 1);
    }

    #[test]
    fn accept_all_says_it_is_off_while_the_tier_is_rolled_back_and_accepts_nothing_even_if_the_phrase_is_typed() {
        let d = tempfile::tempdir().unwrap();
        let (ws, home, id) = real_vetted(d.path(), "harbour", "The harbour closes at dusk.");
        ta::rollback(&state_dir(&ws), "vetted", "operator", "an audit found a false claim", 1).unwrap();
        let mut out = Vec::new();
        accept_all(&ws, &home, None, &mut std::io::Cursor::new("accept all\n"), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("accept-all is off") && text.contains("rolled back") && !text.contains("accepted 1"), "{text}");
        assert!(Library::in_home(&home).claim(&id).unwrap()["validation"].is_null());
    }

    #[test]
    fn a_false_batch_audit_rolls_vetted_back_flags_the_batch_re_audit_due_and_a_true_answer_clears_one() {
        let d = tempfile::tempdir().unwrap();
        let (ws, home, a) = real_vetted(d.path(), "harbour", "The harbour closes at dusk.");
        let (_, _, b) = real_vetted(d.path(), "lamp", "The lamp is lit by hand.");
        accept_all(&ws, &home, None, &mut std::io::Cursor::new("accept all\n"), &mut Vec::new()).unwrap();
        let lib = Library::in_home(&home);
        assert_eq!(lib.tier_members("batch").len(), 2);
        // the first card is called false: a hard failure of a batch-accepted claim
        let mut out = Vec::new();
        audit(&ws, &home, "batch", 5, &mut std::io::Cursor::new("false\n"), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("THE VETTED TIER IS ROLLED BACK (hard_failure)") && text.contains("2 batch-accepted claim(s)") && text.contains("re-audit due"), "{text}");
        assert!(delphi_engine::tier_audit::latched(&state_dir(&ws)).contains("vetted"));
        for id in [&a, &b] {
            let c = lib.claim(id).unwrap();
            assert_eq!(c["validation"]["batch"]["reaudit_due"], true, "{id}");
            assert_eq!(c["validation"]["by"], "operator", "still validated: the operator accepted it");
        }
        assert_eq!(ac::reaudit_due_count(&lib), 2);
        assert!(delphi_engine::accept_all::needs_line(&lib, &state_dir(&ws)).contains("Re-audit due: 2"));
        // the other drawn claim is audited and called true: its flag clears, the first one's stays
        let mut out = Vec::new();
        audit(&ws, &home, "batch", 5, &mut std::io::Cursor::new("true\n"), &mut out).unwrap();
        assert_eq!(ac::reaudit_due_count(&lib), 1, "one audited true, one still due: {}", String::from_utf8_lossy(&out));
        // and accept-all is off now
        let mut out = Vec::new();
        accept_all(&ws, &home, None, &mut std::io::Cursor::new("accept all\n"), &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("accept-all is off"));
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
        assert!(String::from_utf8(out).unwrap().contains("accept-all` marks them"));
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
