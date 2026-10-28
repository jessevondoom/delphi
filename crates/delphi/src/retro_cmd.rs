//! `delphi retro list|show`: the proposals the Orchestrator made with `suggest_rule`, waiting for the operator (`specs/004` §4.2, `specs/031` slice L2).
//!
//! A proposal is a `learn.proposed` event in a mission's log. This slice only reads: nothing here loads a proposal into
//! a prompt, and ratifying one (the apply path and the console deck) is the next slice.

use delphi_core::log::replay;
use delphi_engine::learn::{live_proposals, LiveProposal};
use std::path::Path;

pub struct Options {
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
        if p.tier == "L3" { " (composed by the daemon's Orchestrator)" } else { " (from a task that read untrusted content)" },
        p.timestamp,
        p.task_id.as_deref().unwrap_or("—"),
        target,
        p.text,
        p.rationale
    )
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

pub fn run_in(ws_dir: &Path, verb: &str, reference: Option<&str>, opts: &Options) -> Result<String, String> {
    let items = collect(ws_dir, opts)?;
    match verb {
        "list" if opts.json => serde_json::to_string_pretty(&items.iter().map(LiveProposal::to_json).collect::<Vec<_>>()).map(|s| s + "\n").map_err(|e| e.to_string()),
        "list" => Ok(render_list(&items)),
        "show" => {
            let p = find(&items, reference.ok_or("show needs a proposal: delphi retro show <mission>/<id>")?)?;
            if opts.json {
                return serde_json::to_string_pretty(&p.to_json()).map(|s| s + "\n").map_err(|e| e.to_string());
            }
            Ok(render_one(p))
        }
        other => Err(format!("`{}` is not a retro verb (list, show)", other)),
    }
}

pub fn cli(ws_dir: &Path, args: &[String], opts: &Options) -> i32 {
    let positional: Vec<&str> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !["--mission", "--workspace", "--home"].contains(&args[*i - 1].as_str()))).map(|(_, a)| a.as_str()).collect();
    let Some(verb) = positional.first() else {
        eprintln!("delphi retro: list | show <mission>/<id>");
        return 2;
    };
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
    fn list_and_show_read_a_proposal_and_change_nothing() {
        let ws = workspace_with_proposal();
        let opts = Options { mission: None, json: false };
        let listed = run_in(ws.path(), "list", None, &opts).unwrap();
        assert!(listed.contains("m1/p1") && listed.contains("Run the formatter first.") && listed.contains("1 proposal(s)"), "{}", listed);
        let shown = run_in(ws.path(), "show", Some("p1"), &opts).unwrap();
        assert!(shown.contains("why: two style failures") && shown.contains("workspace"), "{}", shown);
        let j = run_in(ws.path(), "list", None, &Options { mission: Some("m1".into()), json: true }).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&j).unwrap()[0]["ref"], "m1/p1");
        assert!(run_in(ws.path(), "show", Some("p9"), &opts).is_err() && run_in(ws.path(), "ratify", None, &opts).is_err());
        assert!(run_in(ws.path(), "list", None, &Options { mission: Some("../x".into()), json: false }).is_err());
    }

    #[test]
    fn an_empty_workspace_says_how_proposals_appear() {
        let ws = tempfile::tempdir().unwrap();
        let out = run_in(ws.path(), "list", None, &Options { mission: None, json: false }).unwrap();
        assert!(out.starts_with("no proposals"), "{}", out);
    }
}
