//! `delphi retro list|show`: the proposals the Orchestrator made with `suggest_rule`, waiting for the operator (`specs/004` §4.2, `specs/031` slice L2).
//!
//! A proposal is a `learn.proposed` event in a mission's log. This slice only reads: nothing here loads a proposal into
//! a prompt, and ratifying one (the apply path and the console deck) is the next slice.

use delphi_core::log::replay;
use delphi_engine::learn::{live_proposals, LiveProposal};
use delphi_core::event::EventEnvelope;
use delphi_core::log::{EventLog, LogError, LogOptions};
use delphi_engine::retro;
use std::path::{Path, PathBuf};

pub struct Options {
    pub home: PathBuf,
    /// `accept` only: ratify at this scope instead of the proposed one (`workspace` or `operator`).
    pub scope: Option<String>,
    pub mission: Option<String>,
    pub json: bool,
}

/// Every unresolved proposal of the chosen missions, newest first.
pub fn collect(ws_dir: &Path, opts: &Options) -> Result<Vec<LiveProposal>, String> {
    let ids = match &opts.mission {
        Some(m) => {
            // A mission id is a directory name; refuse anything that could point elsewhere.
            if m.is_empty() || m.contains(['/', '\\']) || m.starts_with('.') || !ws_dir.join("local/missions").join(m).is_dir() {
                return Err(format!("no mission `{}` in this workspace (see `delphi missions`)", m));
            }
            vec![m.clone()]
        }
        None => crate::missions::list(ws_dir).into_iter().map(|r| r.id).collect(),
    };
    let mut all = Vec::new();
    for id in ids {
        let events = replay(&ws_dir.join("local/missions").join(&id)).map_err(|e| format!("could not read the log of {}: {}", id, e))?;
        all.extend(live_proposals(&id, &events));
    }
    all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.reference().cmp(&a.reference())));
    Ok(all)
}

/// `delphi retro mine`: rank the failure causes in a mission's events and write the report to
/// `<mission>/reports/trace-mining.json`. Reads the log and writes that one file; nothing is applied (`specs/050` ML-5).
pub fn mine(ws_dir: &Path, opts: &Options) -> Result<String, String> {
    let ids = match &opts.mission {
        Some(m) => {
            if m.is_empty() || m.contains(['/', '\\']) || m.starts_with('.') || !ws_dir.join("local/missions").join(m).is_dir() {
                return Err(format!("no mission `{}` in this workspace (see `delphi missions`)", m));
            }
            vec![m.clone()]
        }
        None => crate::missions::list(ws_dir).into_iter().map(|r| r.id).collect(),
    };
    let (mut out, mut reports) = (String::new(), Vec::new());
    for id in &ids {
        let dir = ws_dir.join("local/missions").join(id);
        let events = replay(&dir).map_err(|e| format!("could not read the log of {}: {}", id, e))?;
        let report = delphi_engine::trace_mine::mine(id, &events);
        let body = serde_json::to_string_pretty(&report.to_json()).map_err(|e| e.to_string())? + "\n";
        let path = dir.join("reports/trace-mining.json");
        delphi_core::lock::write_atomic(&path, body.as_bytes()).map_err(|e| format!("could not write {}: {}", path.display(), e))?;
        out.push_str(&report.render());
        out.push_str(&format!("  report: {}\n", path.display()));
        reports.push(report.to_json());
    }
    if opts.json {
        return serde_json::to_string_pretty(&reports).map(|s| s + "\n").map_err(|e| e.to_string());
    }
    if ids.is_empty() {
        return Ok("no missions to mine\n".into());
    }
    out.push_str("Nothing was applied; a cause is a lead for a harness change, proposed and ratified like any other.\n");
    Ok(out)
}

pub fn render_list(items: &[LiveProposal]) -> String {
    if items.is_empty() {
        return "no proposals: the Orchestrator proposes rules after a mission, and they wait here for your review\n".into();
    }
    let width = items.iter().map(|p| p.reference().chars().count()).max().unwrap_or(0);
    let mut out = String::new();
    for p in items {
        out.push_str(&format!("{:<w$}  {:<6}  {:<9}  {}\n", p.reference(), p.op, p.scope, p.text, w = width));
    }
    out.push_str(&format!("{} proposal(s). Nothing is loaded into a prompt until you ratify it; `delphi retro show <ref>` for one.\n", items.len()));
    out
}

pub fn render_one(p: &LiveProposal) -> String {
    let target = if p.target.is_empty() { String::new() } else { format!("  target:    {}\n", p.target) };
    format!(
        "{}\n  op:        {}\n  scope:     {}\n  tier:      {}{}\n  at:        {}\n  task:      {}\n{}\n{}\n\nwhy: {}\n",
        p.reference(),
        p.op,
        p.scope,
        p.tier,
        if p.tier == "L3" { " (composed by the daemon's Orchestrator)" } else { " (from a task that read content that is not validated)" },
        p.timestamp,
        p.task_id.as_deref().unwrap_or("—"),
        target,
        p.text,
        p.rationale
    )
}

/// The rules a ratified `add` would retire (named) or resembles (similar). Shown only; the operator's ratification applies.
pub fn render_candidates(c: &[retro::Candidate]) -> String {
    if c.is_empty() {
        return String::new();
    }
    let lines: String = c.iter().map(|c| format!("  - [{}] {} ({:.0}% of the same words)\n", c.id, if c.named { "named as superseded: ratifying retires it" } else { "similar, not named: stays unless you retire it" }, c.overlap * 100.0)).collect();
    format!("\nsupersession candidates:\n{}", lines)
}

/// The proposal a reference names: `<mission>/<id>`, or a bare id when exactly one mission has it.
pub fn find<'a>(items: &'a [LiveProposal], reference: &str) -> Result<&'a LiveProposal, String> {
    let hits: Vec<&LiveProposal> = items.iter().filter(|p| p.reference() == reference || p.id == reference).collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("no proposal `{}` (see `delphi retro list`)", reference)),
        many => Err(format!("`{}` names {} proposals; give <mission>/<id>: {}", reference, many.len(), many.iter().map(|p| p.reference()).collect::<Vec<_>>().join(", "))),
    }
}

/// The stale list (`specs/050` ML-3): expired notes and rules past their review window. Listed, never changed.
pub fn render_stale(items: &[retro::Stale], json: bool) -> Result<String, String> {
    if json {
        return serde_json::to_string_pretty(&items.iter().map(retro::Stale::to_json).collect::<Vec<_>>()).map(|s| s + "\n").map_err(|e| e.to_string());
    }
    if items.is_empty() {
        return Ok("nothing is stale: no staged note is past its date and no rules file is past its review window\n".into());
    }
    let mut out: String = items.iter().map(|s| format!("{:<6} {}  {}\n", s.kind, s.reference, s.detail)).collect();
    out.push_str(&format!("{} stale entr(ies). Nothing was removed; an expired note is no longer retrieved, and a rules file is reviewed by editing its `last_confirmed`.\n", items.len()));
    Ok(out)
}

pub fn run_in(ws_dir: &Path, verb: &str, reference: Option<&str>, opts: &Options) -> Result<String, String> {
    if verb == "stale" {
        return render_stale(&retro::stale(&opts.home, ws_dir, &delphi_core::bundles::today()), opts.json);
    }
    if verb == "mine" {
        return mine(ws_dir, opts);
    }
    let items = collect(ws_dir, opts)?;
    match verb {
        "list" if opts.json => serde_json::to_string_pretty(&items.iter().map(LiveProposal::to_json).collect::<Vec<_>>()).map(|s| s + "\n").map_err(|e| e.to_string()),
        "list" => Ok(render_list(&items)),
        "show" => {
            let p = find(&items, reference.ok_or("show needs a proposal: delphi retro show <mission>/<id>")?)?;
            if opts.json {
                return serde_json::to_string_pretty(&p.to_json()).map(|s| s + "\n").map_err(|e| e.to_string());
            }
            let mut out = render_one(p);
            out.push_str(&render_candidates(&retro::candidates_for(&opts.home, ws_dir, p)));
            let blocked = retro::check(&opts.home, ws_dir, p);
            if !blocked.is_empty() {
                out.push_str(&format!("\nnot ratifiable as it stands:\n{}", blocked.iter().map(|b| format!("  - {}\n", b)).collect::<String>()));
            }
            Ok(out)
        }
        other => Err(format!("`{}` is not a retro verb (list, show, stale, mine, accept, reject)", other)),
    }
}

/// The mission's log, for writing. A lock held by a live daemon stays held, so the wait is short: it only covers the
/// moment after a writer in this process closed the log, when a forked child may still carry a copy of the descriptor.
pub(crate) fn open_log(ws_dir: &Path, mission: &str) -> Result<EventLog, String> {
    let dir = ws_dir.join("local/missions").join(mission);
    let mut tries = 0;
    loop {
        match EventLog::open(&dir, LogOptions::default()) {
            Ok(log) => return Ok(log),
            Err(LogError::Locked(_)) if tries < 10 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(LogError::Locked(_)) => return Err(format!("the daemon has mission {} open; stop it (`delphi daemon stop`) and run this again", mission)),
            Err(other) => return Err(format!("could not open the log of {}: {}", mission, other)),
        }
    }
}

/// Close a proposal: for `accept`, ratify it into its scope's `heuristics.md` first, then append
/// `learn.proposal_resolved`. The mission log is opened before anything is changed, so a daemon holding the mission
/// stops the command before a rule is written that the log could not record.
pub fn resolve(ws_dir: &Path, p: &LiveProposal, accept: bool, opts: &Options) -> Result<String, String> {
    let mut p = p.clone();
    if let Some(s) = opts.scope.as_deref().filter(|_| accept) {
        if !delphi_engine::learn::PROPOSAL_SCOPES.contains(&s) {
            return Err(format!("`--scope` must be one of {}", delphi_engine::learn::PROPOSAL_SCOPES.join(", ")));
        }
        p.scope = s.to_string();
    }
    let mut log = open_log(ws_dir, &p.mission)?;
    let (resolution, detail) = if accept {
        let done = retro::accept(&opts.home, ws_dir, &p)?;
        ("accepted", serde_json::json!({"scope": p.scope, "file": done.file, "pin": done.pin, "retired": done.retired.iter().map(|(id, text)| serde_json::json!({"id": id, "text": text})).collect::<Vec<_>>()}))
    } else {
        ("rejected", serde_json::json!({}))
    };
    let mut e = EventEnvelope::new(&p.mission, "learn.proposal_resolved", serde_json::json!({"id": p.id, "resolution": resolution, "detail": detail}));
    e.task_id = p.task_id.clone();
    log.append(e).map_err(|e| format!("the change was made but the log of {} could not record it: {}", p.mission, e))?;
    Ok(if accept { format!("ratified {} into the {} rules; it applies from the next task\n", p.reference(), p.scope) } else { format!("rejected {}; nothing was changed\n", p.reference()) })
}

/// Ask on the terminal. Ratifying is the operator's decision, so it needs a person typing: a piped or scripted run, such
/// as an agent's shell, cannot approve its own proposal.
pub(crate) fn confirm(question: &str) -> bool {
    use std::io::{BufRead, Write};
    if !unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 } {
        return false;
    }
    print!("{} [type yes] ", question);
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim() == "yes"
}

pub fn cli(ws_dir: &Path, args: &[String], opts: &Options) -> i32 {
    let positional: Vec<&str> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !["--mission", "--workspace", "--home", "--scope"].contains(&args[*i - 1].as_str()))).map(|(_, a)| a.as_str()).collect();
    let Some(verb) = positional.first() else {
        eprintln!("delphi retro: list | show <mission>/<id> | stale | mine");
        return 2;
    };
    if matches!(*verb, "accept" | "reject") {
        let result = collect(ws_dir, opts).and_then(|items| {
            let p = find(&items, positional.get(1).copied().ok_or(format!("{} needs a proposal: delphi retro {} <mission>/<id>", verb, verb))?)?.clone();
            if !unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 } {
                return Err(format!("{} needs a terminal: it asks you to type yes, and a script cannot approve a rule", verb));
            }
            let mut shown = p.clone();
            if let Some(s) = opts.scope.as_deref().filter(|_| *verb == "accept") {
                shown.scope = s.to_string();
            }
            print!("{}", render_one(&shown));
            if *verb == "accept" {
                print!("{}", render_candidates(&retro::candidates_for(&opts.home, ws_dir, &shown)));
                let blocked = retro::check(&opts.home, ws_dir, &shown);
                if !blocked.is_empty() {
                    return Err(blocked.join("; "));
                }
            }
            if !confirm(&format!("{} {}?", if *verb == "accept" { "Ratify" } else { "Reject" }, shown.reference())) {
                return Err("not confirmed; nothing was changed".into());
            }
            resolve(ws_dir, &p, *verb == "accept", opts)
        });
        return match result {
            Ok(text) => {
                print!("{}", text);
                0
            }
            Err(e) => {
                eprintln!("delphi retro: {}", e);
                1
            }
        };
    }
    match run_in(ws_dir, verb, positional.get(1).copied(), opts) {
        Ok(text) => {
            print!("{}", text);
            0
        }
        Err(e) => {
            eprintln!("delphi retro: {}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::event::EventEnvelope;
    use delphi_core::log::{EventLog, LogOptions};
    use serde_json::json;

    fn workspace_with_proposal() -> tempfile::TempDir {
        let ws = tempfile::tempdir().unwrap();
        let dir = ws.path().join("local/missions/m1");
        std::fs::create_dir_all(&dir).unwrap();
        let mut log = EventLog::open(&dir, LogOptions::default()).unwrap();
        let mut e = EventEnvelope::new("m1", "learn.proposed", json!({"op": "add", "scope": "workspace", "text": "Run the formatter first.", "rationale": "two style failures", "target": "", "tier": "L3"}));
        e.agent = Some("orchestrator".into());
        log.append(e).unwrap();
        ws
    }

    #[test]
    fn mine_writes_one_report_under_the_mission_and_changes_nothing_else() {
        let ws = workspace_with_proposal();
        let dir = ws.path().join("local/missions/m1");
        let mut log = EventLog::open(&dir, LogOptions::default()).unwrap();
        let mut e = EventEnvelope::new("m1", "loop.action", json!({"action": "stop"}));
        e.span_id = Some("sp1".into());
        log.append(e).unwrap();
        drop(log);
        let opts = Options { home: PathBuf::new(), scope: None, mission: Some("m1".into()), json: false };
        let text = run_in(ws.path(), "mine", None, &opts).unwrap();
        assert!(text.contains("loop.hard_stop") && text.contains("Nothing was applied"), "{}", text);
        let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("reports/trace-mining.json")).unwrap()).unwrap();
        assert_eq!((report["causes"][0]["key"].as_str(), report["causes"][0]["events"][0]["seq"].as_u64()), (Some("loop.hard_stop"), Some(2)));
        assert!(run_in(ws.path(), "mine", None, &Options { mission: Some("../x".into()), ..opts }).is_err());
    }

    #[test]
    fn list_and_show_read_a_proposal_and_change_nothing() {
        let ws = workspace_with_proposal();
        let opts = Options { home: PathBuf::new(), scope: None, mission: None, json: false };
        let listed = run_in(ws.path(), "list", None, &opts).unwrap();
        assert!(listed.contains("m1/p1") && listed.contains("Run the formatter first.") && listed.contains("1 proposal(s)"), "{}", listed);
        let shown = run_in(ws.path(), "show", Some("p1"), &opts).unwrap();
        assert!(shown.contains("why: two style failures") && shown.contains("workspace"), "{}", shown);
        let j = run_in(ws.path(), "list", None, &Options { home: PathBuf::new(), scope: None, mission: Some("m1".into()), json: true }).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&j).unwrap()[0]["ref"], "m1/p1");
        assert!(run_in(ws.path(), "show", Some("p9"), &opts).is_err() && run_in(ws.path(), "ratify", None, &opts).is_err());
        assert!(run_in(ws.path(), "list", None, &Options { home: PathBuf::new(), scope: None, mission: Some("../x".into()), json: false }).is_err());
    }

    fn opts_in(home: &Path, scope: Option<&str>) -> Options {
        Options { home: home.to_path_buf(), scope: scope.map(String::from), mission: None, json: false }
    }

    #[test]
    fn accepting_ratifies_the_rule_and_resolves_the_proposal_and_rejecting_changes_no_rule() {
        let ws = workspace_with_proposal();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join("local")).unwrap();
        std::fs::write(ws.path().join("local/ratified.lock"), r#"{"files":{}}"#).unwrap();
        std::fs::create_dir_all(home.path().join("operator")).unwrap();
        std::fs::write(home.path().join("operator/ratified.lock"), r#"{"files":{}}"#).unwrap();
        let opts = opts_in(home.path(), None);
        let p = collect(ws.path(), &opts).unwrap().remove(0);
        let said = resolve(ws.path(), &p, true, &opts).unwrap();
        assert!(said.contains("ratified m1/p1 into the workspace rules"), "{}", said);
        let rules = std::fs::read_to_string(ws.path().join("local/ratified/heuristics.md")).unwrap();
        assert!(rules.contains("### [W-001] Run the formatter first") && rules.contains("**Why:** two style failures"), "{}", rules);
        assert!(collect(ws.path(), &opts).unwrap().is_empty(), "a resolved proposal no longer lists");
        let log = delphi_core::log::replay(&ws.path().join("local/missions/m1")).unwrap();
        let done = log.iter().find(|e| e.kind == "learn.proposal_resolved").unwrap();
        assert_eq!((done.payload["id"].as_str(), done.payload["resolution"].as_str(), done.payload["detail"]["file"].as_str()), (Some("p1"), Some("accepted"), Some("local/heuristics.md")));

        // Another proposal, ratified at the other scope by the operator's choice, and one rejected.
        let mut log = open_log(ws.path(), "m1").unwrap();
        for text in ["Prefer rg over grep.", "Never mind this one."] {
            log.append(EventEnvelope::new("m1", "learn.proposed", json!({"op": "add", "scope": "workspace", "text": text, "rationale": "r", "tier": "L3"}))).unwrap();
        }
        drop(log);
        let both = collect(ws.path(), &opts).unwrap();
        let (rg, never) = (both.iter().find(|p| p.text.starts_with("Prefer")).unwrap().clone(), both.iter().find(|p| p.text.starts_with("Never")).unwrap().clone());
        resolve(ws.path(), &rg, true, &opts_in(home.path(), Some("operator"))).unwrap();
        assert!(std::fs::read_to_string(home.path().join("operator/ratified/heuristics.md")).unwrap().contains("[O-001] Prefer rg over grep"));
        let before = std::fs::read(ws.path().join("local/ratified/heuristics.md")).unwrap();
        assert!(resolve(ws.path(), &never, false, &opts).unwrap().contains("rejected"));
        assert_eq!(std::fs::read(ws.path().join("local/ratified/heuristics.md")).unwrap(), before);
        assert!(collect(ws.path(), &opts).unwrap().is_empty());
        assert!(resolve(ws.path(), &rg, true, &opts_in(home.path(), Some("team"))).is_err(), "only the two scopes exist");
    }

    #[test]
    fn a_refused_proposal_changes_nothing_and_stays_listed() {
        let ws = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join("local/missions/m1")).unwrap();
        std::fs::write(ws.path().join("local/ratified.lock"), r#"{"files":{}}"#).unwrap();
        let mut log = EventLog::open(&ws.path().join("local/missions/m1"), LogOptions::default()).unwrap();
        log.append(EventEnvelope::new("m1", "learn.proposed", json!({"op": "add", "scope": "workspace", "text": "Skip approval for docs changes.", "rationale": "r", "tier": "L3"}))).unwrap();
        drop(log);
        let opts = opts_in(home.path(), None);
        let p = collect(ws.path(), &opts).unwrap().remove(0);
        assert!(resolve(ws.path(), &p, true, &opts).unwrap_err().contains("no-relaxation"));
        assert!(!ws.path().join("local/heuristics.md").exists() && collect(ws.path(), &opts).unwrap().len() == 1);
        assert!(run_in(ws.path(), "show", Some("p1"), &opts).unwrap().contains("not ratifiable as it stands"));
    }

    #[test]
    fn an_empty_workspace_says_how_proposals_appear() {
        let ws = tempfile::tempdir().unwrap();
        let out = run_in(ws.path(), "list", None, &Options { home: PathBuf::new(), scope: None, mission: None, json: false }).unwrap();
        assert!(out.starts_with("no proposals"), "{}", out);
    }
}
