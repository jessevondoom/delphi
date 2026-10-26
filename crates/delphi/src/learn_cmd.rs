//! `delphi learn list|show|drop`: the notes the daemon staged from what happened (`specs/031` §3.1-§3.2).
//!
//! A staged note is a `learn.staged` event in a mission's log. Nothing here loads a note into a prompt, and `drop`
//! appends `learn.dropped`: no event is ever removed. Listing and showing read files only; `drop` needs the mission's
//! writer lock, so it refuses while a daemon has the mission open.

use delphi_core::event::EventEnvelope;
use delphi_core::log::{replay, EventLog, LogError, LogOptions};
use delphi_engine::learn::{live_notes, LiveNote};
use std::path::Path;

pub struct Options {
    pub mission: Option<String>,
    pub json: bool,
}

fn mission_ids(ws_dir: &Path, only: &Option<String>) -> Result<Vec<String>, String> {
    match only {
        Some(m) => {
            // A mission id is a directory name; refuse anything that could point elsewhere.
            if m.is_empty() || m.contains(['/', '\\']) || m.starts_with('.') || !ws_dir.join("local/missions").join(m).is_dir() {
                return Err(format!("no mission `{}` in this workspace (see `delphi missions`)", m));
            }
            Ok(vec![m.clone()])
        }
        None => Ok(crate::missions::list(ws_dir).into_iter().map(|r| r.id).collect()),
    }
}

/// Every live note of the chosen missions, newest first.
pub fn collect(ws_dir: &Path, opts: &Options) -> Result<Vec<LiveNote>, String> {
    let mut all = Vec::new();
    for id in mission_ids(ws_dir, &opts.mission)? {
        let events = replay(&ws_dir.join("local/missions").join(&id)).map_err(|e| format!("could not read the log of {}: {}", id, e))?;
        all.extend(live_notes(&id, &events));
    }
    all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.reference().cmp(&a.reference())));
    Ok(all)
}

pub fn render_list(notes: &[LiveNote]) -> String {
    if notes.is_empty() {
        return "no staged notes: they appear when a card is denied with a reason, you correct the team, a task recovers on a retry or is escalated, or a provisional decision is rejected\n".into();
    }
    let mut out = String::new();
    let width = notes.iter().map(|n| n.reference().chars().count()).max().unwrap_or(0);
    for n in notes {
        out.push_str(&format!("{:<w$}  {:<17}  {}  {}\n", n.reference(), n.trigger, n.tier, n.summary, w = width));
    }
    out.push_str(&format!("{} note(s). Staged notes are never loaded into a prompt; `delphi learn show <ref>` for one, `delphi learn drop <ref>` to discard it.\n", notes.len()));
    out
}

pub fn render_one(n: &LiveNote) -> String {
    let source = if n.tier == "L5" { "typed by the operator" } else if n.tier == "L3" { "composed by the daemon from the log" } else { "from a task that read untrusted content" };
    format!(
        "{}\n  trigger: {}\n  at:      {}\n  task:    {}\n  role:    {}\n  tier:    {} ({}); promotion needs L4 or above\n  events:  {}\n\n{}\n",
        n.reference(),
        n.trigger,
        n.timestamp,
        n.task_id.as_deref().unwrap_or("—"),
        n.role.as_deref().unwrap_or("—"),
        n.tier,
        source,
        n.event_seqs.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", "),
        n.summary
    )
}

/// The note a reference names: `<mission>/<id>`, or a bare id when exactly one mission has it.
pub fn find<'a>(notes: &'a [LiveNote], reference: &str) -> Result<&'a LiveNote, String> {
    let hits: Vec<&LiveNote> = notes.iter().filter(|n| n.reference() == reference || n.id == reference).collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("no staged note `{}` (see `delphi learn list`)", reference)),
        many => Err(format!("`{}` names {} notes; give <mission>/<id>: {}", reference, many.len(), many.iter().map(|n| n.reference()).collect::<Vec<_>>().join(", "))),
    }
}

/// Append `learn.dropped` for a note. The log is not rewritten and the note stays in it.
pub fn drop_note(ws_dir: &Path, note: &LiveNote) -> Result<(), String> {
    let dir = ws_dir.join("local/missions").join(&note.mission);
    let mut log = EventLog::open(&dir, LogOptions::default()).map_err(|e| match e {
        LogError::Locked(_) => format!("the daemon has mission {} open; stop it (`delphi daemon stop`) and run this again", note.mission),
        other => format!("could not open the log of {}: {}", note.mission, other),
    })?;
    let mut e = EventEnvelope::new(&note.mission, "learn.dropped", serde_json::json!({"id": note.id, "reason": "operator"}));
    e.task_id = note.task_id.clone();
    log.append(e).map_err(|e| format!("could not write to the log of {}: {}", note.mission, e))?;
    Ok(())
}

pub fn run_in(ws_dir: &Path, verb: &str, reference: Option<&str>, opts: &Options) -> Result<String, String> {
    let notes = collect(ws_dir, opts)?;
    match verb {
        "list" => {
            if opts.json {
                return serde_json::to_string_pretty(&notes.iter().map(LiveNote::to_json).collect::<Vec<_>>()).map(|s| s + "\n").map_err(|e| e.to_string());
            }
            Ok(render_list(&notes))
        }
        "show" => {
            let n = find(&notes, reference.ok_or("show needs a note: delphi learn show <mission>/<id>")?)?;
            if opts.json {
                return serde_json::to_string_pretty(&n.to_json()).map(|s| s + "\n").map_err(|e| e.to_string());
            }
            Ok(render_one(n))
        }
        "drop" => {
            let n = find(&notes, reference.ok_or("drop needs a note: delphi learn drop <mission>/<id>")?)?;
            drop_note(ws_dir, n)?;
            Ok(format!("dropped {} (the event stays in the log; the note no longer lists)\n", n.reference()))
        }
        other => Err(format!("`{}` is not a learn verb (list, show, drop)", other)),
    }
}

pub fn cli(ws_dir: &Path, args: &[String], opts: &Options) -> i32 {
    let positional: Vec<&str> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !["--mission", "--workspace", "--home"].contains(&args[*i - 1].as_str()))).map(|(_, a)| a.as_str()).collect();
    let Some(verb) = positional.first() else {
        eprintln!("delphi learn: list | show <mission>/<id> | drop <mission>/<id>");
        return 2;
    };
    match run_in(ws_dir, verb, positional.get(1).copied(), opts) {
        Ok(text) => {
            print!("{}", text);
            0
        }
        Err(e) => {
            eprintln!("delphi learn: {}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mission(ws: &Path, id: &str, events: Vec<EventEnvelope>) {
        let mut log = EventLog::open(&ws.join("local/missions").join(id), LogOptions::default()).unwrap();
        for e in events {
            log.append(e).unwrap();
        }
    }

    fn staged(mission: &str, id: &str, summary: &str, tier: &str) -> EventEnvelope {
        let mut e = EventEnvelope::new(mission, "learn.staged", json!({"id": id, "trigger": "denied_card", "summary": summary, "event_seqs": [3], "tier": tier}));
        e.task_id = Some("t_0001".into());
        e
    }

    fn opts() -> Options {
        Options { mission: None, json: false }
    }

    #[test]
    fn list_show_and_drop_work_over_the_log_and_leave_the_event_in_it() {
        let t = tempfile::tempdir().unwrap();
        mission(t.path(), "m_a", vec![staged("m_a", "n3", "A card was denied: use the helper", "L5"), staged("m_a", "n9", "second", "L3")]);
        mission(t.path(), "m_b", vec![staged("m_b", "n3", "other mission", "L0")]);
        let listed = run_in(t.path(), "list", None, &opts()).unwrap();
        assert!(listed.contains("m_a/n3") && listed.contains("m_b/n3") && listed.contains("3 note(s)"), "{}", listed);
        let shown = run_in(t.path(), "show", Some("m_a/n3"), &opts()).unwrap();
        assert!(shown.contains("typed by the operator") && shown.contains("use the helper"), "{}", shown);
        assert!(run_in(t.path(), "show", Some("m_b/n3"), &opts()).unwrap().contains("untrusted"));
        assert!(run_in(t.path(), "show", Some("n3"), &opts()).unwrap_err().contains("names 2 notes"), "a bare id that two missions share is ambiguous");

        run_in(t.path(), "drop", Some("m_a/n3"), &opts()).unwrap();
        let after = run_in(t.path(), "list", None, &opts()).unwrap();
        assert!(!after.contains("m_a/n3") && after.contains("m_a/n9") && after.contains("2 note(s)"), "{}", after);
        let log = replay(&t.path().join("local/missions/m_a")).unwrap();
        assert_eq!(log.iter().filter(|e| e.kind == "learn.staged").count(), 2, "dropping removes nothing from the log");
        assert_eq!(log.iter().filter(|e| e.kind == "learn.dropped").count(), 1);
    }

    #[test]
    fn drop_refuses_while_a_daemon_holds_the_mission() {
        let t = tempfile::tempdir().unwrap();
        mission(t.path(), "m_a", vec![staged("m_a", "n3", "x", "L5")]);
        let _held = EventLog::open(&t.path().join("local/missions/m_a"), LogOptions::default()).unwrap();
        let err = run_in(t.path(), "drop", Some("m_a/n3"), &opts()).unwrap_err();
        assert!(err.contains("daemon") && err.contains("m_a"), "{}", err);
    }

    #[test]
    fn json_output_and_bad_requests() {
        let t = tempfile::tempdir().unwrap();
        mission(t.path(), "m_a", vec![staged("m_a", "n3", "x", "L5")]);
        let j = run_in(t.path(), "list", None, &Options { mission: Some("m_a".into()), json: true }).unwrap();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!((v[0]["ref"].as_str(), v[0]["tier"].as_str()), (Some("m_a/n3"), Some("L5")));
        assert!(run_in(t.path(), "list", None, &Options { mission: Some("../x".into()), json: false }).is_err());
        assert!(run_in(t.path(), "frob", None, &opts()).is_err());
        assert!(run_in(t.path(), "show", None, &opts()).is_err());
        assert!(render_list(&[]).contains("no staged notes"));
    }
}
