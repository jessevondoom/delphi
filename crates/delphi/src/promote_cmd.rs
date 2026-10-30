//! `delphi retro promote|promotions|withdraw`, and `show|accept|reject` for a promotion (`specs/058` §7, slice PS5).
//!
//! A promotion is a proposal kind of `/retro`: project records summarized for home memory. The checks and the
//! compare-and-swap live in `delphi_engine::promote`; this file asks the operator and appends the outcome to the mission's
//! log. Accepting and withdrawing need a terminal and a typed yes, so a script cannot approve a promotion.

use crate::retro_cmd::{confirm, open_log, Options};
use delphi_engine::promote::{self, Promotion};
use delphi_engine::scope::ProjectId;
use std::path::Path;

fn value_of(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn tty() -> bool {
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

pub fn render_one(p: &Promotion) -> String {
    let mut out = format!("{}  (promotion from project {} to {})\n  version:   {}\n  at:        {}\n  records:\n", p.reference(), p.project.as_str(), p.destination, &p.version()[..12], p.timestamp);
    for r in &p.records {
        out.push_str(&format!("    - {} ({})\n", r.rel, r.source_type));
    }
    out.push_str(&format!("\n{}\n", p.summary));
    if !p.uncertainty.is_empty() {
        out.push_str(&format!("\nuncertainty: {}\n", p.uncertainty));
    }
    if p.matches.is_empty() {
        out.push_str("\nno duplicate or conflict found in home memory\n");
    } else {
        out.push_str("\nin home memory (shown only: nothing is merged, overwritten or superseded):\n");
        for m in &p.matches {
            out.push_str(&format!("  - {} {} ({:.0}% of the same words)\n", m.class, m.path, m.overlap * 100.0));
        }
    }
    out.push_str("Accepting ratifies this summary as home memory at its records' source tier; it validates no fact.\n");
    out
}

fn find(ws_dir: &Path, reference: &str) -> Option<Promotion> {
    promote::collect(ws_dir).into_iter().find(|p| p.reference() == reference)
}

fn list(ws_dir: &Path, json: bool) -> Result<String, String> {
    let items = promote::collect(ws_dir);
    if json {
        return serde_json::to_string_pretty(&items.iter().map(Promotion::to_json).collect::<Vec<_>>()).map(|s| s + "\n").map_err(|e| e.to_string());
    }
    if items.is_empty() {
        return Ok("no promotions: `delphi retro promote <project> <record>... --summary <text> --mission <id>` proposes one\n".into());
    }
    let mut out: String = items.iter().map(|p| format!("{}  project {}  {} record(s)  {}\n", p.reference(), p.project.as_str(), p.records.len(), p.summary)).collect();
    out.push_str(&format!("{} promotion(s). Nothing reaches home memory until you accept it; `delphi retro show <ref>` for one.\n", items.len()));
    Ok(out)
}

fn propose(ws_dir: &Path, positional: &[&str], args: &[String], opts: &Options) -> Result<String, String> {
    let (Some(project), records) = (positional.get(1), positional.get(2..).unwrap_or(&[])) else { return Err("promote needs a project and records: delphi retro promote <project-id> <record>... --summary <text> --mission <id>".into()) };
    let project = ProjectId::parse(project)?;
    let mission = opts.mission.clone().ok_or("promote needs --mission <id>: the mission whose log records the proposal")?;
    let summary = value_of(args, "--summary").ok_or("promote needs --summary <text>")?;
    let rels: Vec<String> = records.iter().map(|s| s.to_string()).collect();
    let (event, matches) = promote::propose(ws_dir, &mission, &project, &rels, &summary, &value_of(args, "--uncertainty").unwrap_or_default()).map_err(|r| r.to_string())?;
    let mut log = open_log(ws_dir, &mission)?;
    let stored = log.append(event).map_err(|e| format!("could not record the proposal in {}: {}", mission, e))?;
    let mut out = format!("proposed {}/p{}; nothing was written to home memory\n", mission, stored);
    for m in matches {
        out.push_str(&format!("  in home memory: {} {} ({:.0}%)\n", m.class, m.path, m.overlap * 100.0));
    }
    Ok(out)
}

fn resolve(ws_dir: &Path, p: &Promotion, accept: bool, args: &[String]) -> Result<String, String> {
    if !tty() {
        return Err(format!("{} needs a terminal: it asks you to type yes, and a script cannot approve a promotion", if accept { "accept" } else { "reject" }));
    }
    print!("{}", render_one(p));
    if !confirm(&format!("{} {}?", if accept { "Accept" } else { "Reject" }, p.reference())) {
        return Err("not confirmed; nothing was changed".into());
    }
    let mut log = open_log(ws_dir, &p.mission)?;
    let by = "cli";
    if !accept {
        log.append(promote::rejection_event(ws_dir, p, by, &value_of(args, "--reason").unwrap_or_default())).map_err(|e| e.to_string())?;
        return Ok(format!("rejected {}; nothing was changed\n", p.reference()));
    }
    let done = promote::accept(ws_dir, p, &p.version(), by, &delphi_core::time::now_rfc3339()).map_err(|r| r.to_string())?;
    log.append(promote::acceptance_event(p, &done, by)).map_err(|e| format!("the entry was written but the log of {} could not record it: {}", p.mission, e))?;
    Ok(format!("accepted {} as home memory entry {} ({}); withdraw it with `delphi retro withdraw {}`\n", p.reference(), done.entry, done.source_type, done.entry))
}

fn withdraw(ws_dir: &Path, entry: Option<&&str>, args: &[String]) -> Result<String, String> {
    let entry = entry.ok_or("withdraw needs an entry: delphi retro withdraw <entry> [--reason <text>]")?;
    if !tty() {
        return Err("withdraw needs a terminal: it asks you to type yes".into());
    }
    if !confirm(&format!("Retire promoted entry {}? (nothing is deleted)", entry)) {
        return Err("not confirmed; nothing was changed".into());
    }
    let w = promote::withdraw(ws_dir, entry, "cli", &value_of(args, "--reason").unwrap_or_default(), &delphi_core::time::now_rfc3339()).map_err(|r| r.to_string())?;
    let mut log = open_log(ws_dir, &w.mission)?;
    log.append(promote::withdrawal_event(&w, "cli")).map_err(|e| format!("the entry was retired but the log of {} could not record it: {}", w.mission, e))?;
    Ok(format!("retired {} (audit {}); the entry and its sources are kept\n", w.entry, w.audit))
}

/// Run a promotion verb; `None` when `verb` is not one (a rule proposal's `show`, `accept` and `reject` fall through).
pub fn cli(ws_dir: &Path, positional: &[&str], args: &[String], opts: &Options) -> Option<Result<String, String>> {
    let verb = *positional.first()?;
    let promotion = || positional.get(1).and_then(|r| find(ws_dir, r));
    match verb {
        "promote" => Some(propose(ws_dir, positional, args, opts)),
        "promotions" => Some(list(ws_dir, opts.json)),
        "withdraw" => Some(withdraw(ws_dir, positional.get(1), args)),
        "show" => promotion().map(|p| if opts.json { serde_json::to_string_pretty(&p.to_json()).map(|s| s + "\n").map_err(|e| e.to_string()) } else { Ok(render_one(&p)) }),
        "accept" | "reject" => promotion().map(|p| resolve(ws_dir, &p, verb == "accept", args)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_cannot_accept_reject_or_withdraw_a_promotion() {
        let t = tempfile::tempdir().unwrap();
        let p = Promotion {
            mission: "m1".into(),
            id: "p1".into(),
            project: ProjectId::default_project(),
            records: vec![],
            summary: "s".into(),
            uncertainty: String::new(),
            destination: promote::DESTINATION.into(),
            home_version: String::new(),
            matches: vec![],
            timestamp: "t".into(),
        };
        for accept in [true, false] {
            assert!(resolve(t.path(), &p, accept, &[]).unwrap_err().contains("needs a terminal"));
        }
        assert!(withdraw(t.path(), Some(&"m1-p1"), &[]).unwrap_err().contains("needs a terminal"));
        assert!(render_one(&p).contains("validates no fact"));
    }
}
