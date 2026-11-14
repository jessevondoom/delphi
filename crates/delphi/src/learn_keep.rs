//! `delphi learn keep <mission>/<id> --as lesson --role <role> | --as rule`: the operator keeps a host note in one step
//! (`specs/108` M2, as amended by A3).
//!
//! A terminal command only, and no daemon method: stdin must be a terminal and the operator types `yes`; there is no
//! `--yes`. What it writes goes through the machinery `delphi customize rule` uses (the live file, the ratified copy and
//! the pin, under the scope's lock, with the hash check), after the same no-relaxation test and the Rule of 25. The note
//! is then closed in its own session's log as `learn.proposal_resolved {resolution: "accepted"}`; that log is opened
//! before anything is written, so while the daemon holds the session the command refuses and writes nothing (as
//! `delphi retro accept` and `delphi learn drop` do).
//!
//! The limit is the charter's for §5.12: a process running as the operator that can drive a terminal can do what the
//! operator can. No agent, host or hook has a path to this code.

use delphi_core::event::EventEnvelope;
use delphi_core::log::replay;
use delphi_engine::customize::{self, Kept, Provenance};
use delphi_engine::learn::{live_notes, LiveNote};
use std::path::Path;

/// The triggers of notes a host staged that can be kept: what a host noted, and what the transcript rules found.
pub const KEEPABLE: [&str; 2] = ["host_note", "host_transcript"];

#[derive(Debug, Clone, PartialEq)]
pub enum As {
    Lesson { role: String },
    Rule,
}

#[derive(Debug, Clone)]
pub struct Args {
    pub reference: String,
    pub kind: As,
    pub scope: String,
    pub text: Option<String>,
}

/// What `keep` will do, worked out and checked before the operator is asked anything.
#[derive(Debug, Clone)]
pub struct Plan {
    pub note: LiveNote,
    pub evidence: String,
    pub source: String,
    pub kind: As,
    pub scope: String,
    pub text: String,
    /// The file it writes, as the operator names it (`local/…` or `operator/…`).
    pub file: String,
    /// The entry as it will be written.
    pub preview: String,
    pub provenance: Provenance,
}

fn usage() -> String {
    "usage: delphi learn keep <mission>/<id> --as lesson --role <role> | --as rule [--scope workspace|operator] [--text \"<edited>\"]".into()
}

/// Read the command line after `keep`.
pub fn parse(args: &[String], reference: Option<&str>) -> Result<Args, String> {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    if args.iter().any(|a| a == "--yes") {
        return Err("`learn keep` has no --yes: you type yes at the terminal, after reading what will be written".into());
    }
    let reference = reference.ok_or_else(usage)?.to_string();
    let scope = flag("--scope").unwrap_or_else(|| "workspace".into());
    if !["workspace", "operator"].contains(&scope.as_str()) {
        return Err("`--scope` is workspace or operator".into());
    }
    let kind = match (flag("--as").as_deref(), flag("--role")) {
        (Some("lesson"), Some(role)) => As::Lesson { role },
        (Some("lesson"), None) => return Err("a lesson is for one role: add --role <role> (see `delphi catalog`)".into()),
        (Some("rule"), None) => As::Rule,
        (Some("rule"), Some(_)) => return Err("a rule is shared by every role; --role goes with --as lesson".into()),
        _ => return Err(usage()),
    };
    Ok(Args { reference, kind, scope, text: flag("--text") })
}

/// The staged `learn.staged` event of a note, from its own mission's log.
fn staged_event<'a>(events: &'a [EventEnvelope], id: &str) -> Option<&'a EventEnvelope> {
    events.iter().find(|e| e.kind == "learn.staged" && e.payload["id"].as_str() == Some(id))
}

/// Find the note, or say why it cannot be kept: unknown, already closed, or not a host note.
fn find_note(ws_dir: &Path, reference: &str) -> Result<(LiveNote, EventEnvelope), String> {
    let (mission, id) = reference.split_once('/').ok_or_else(|| format!("name the note as <mission>/<id> (see `delphi learn list`), not `{}`", reference))?;
    if mission.is_empty() || mission.starts_with('.') || mission.contains('\\') || id.is_empty() || id.contains('/') {
        return Err(format!("no staged note `{}` (see `delphi learn list`)", reference));
    }
    let dir = delphi_core::sessions::dir(ws_dir, mission);
    if !dir.is_dir() {
        return Err(format!("no staged note `{}` (see `delphi learn list`)", reference));
    }
    let events = replay(&dir).map_err(|e| format!("could not read the log of {}: {}", mission, e))?;
    let Some(staged) = staged_event(&events, id).cloned() else { return Err(format!("no staged note `{}` (see `delphi learn list`)", reference)) };
    let Some(note) = live_notes(mission, &events).into_iter().find(|n| n.id == id) else {
        let kept = events.iter().any(|e| e.kind == "learn.proposal_resolved" && e.payload["id"].as_str() == Some(id));
        return Err(format!("`{}` is already {}; nothing was written", reference, if kept { "resolved (kept)" } else { "dropped" }));
    };
    if !KEEPABLE.contains(&note.trigger.as_str()) {
        return Err(format!("`{}` is a `{}` note; `learn keep` keeps a host's notes (`delphi customize rule` writes a rule of your own, `delphi retro` ratifies a proposal)", reference, note.trigger));
    }
    Ok((note, staged))
}

/// Work out what would be written and check it, writing nothing. Every refusal that does not need the operator happens
/// here, before the terminal is asked anything.
pub fn plan(home: &Path, ws_dir: &Path, a: &Args) -> Result<Plan, String> {
    let (note, staged) = find_note(ws_dir, &a.reference)?;
    let (claim, evidence) = delphi_engine::learn::host_claim(&note.summary);
    let text: String = a.text.clone().unwrap_or(claim).split_whitespace().collect::<Vec<_>>().join(" ");
    let source = staged.payload["source"].as_str().unwrap_or("host").to_string();
    let provenance = Provenance { ratified_from: note.reference(), original_tier: note.tier.clone(), tool: source.clone(), date: delphi_core::bundles::today() };
    let (file, preview) = match &a.kind {
        As::Lesson { role } => {
            customize::check_role(home, ws_dir, role)?;
            let lesson = customize::parse_lesson(&text)?;
            let (_, block) = customize::lesson_preview(home, ws_dir, &a.scope, role, &lesson, &provenance)?;
            (customize::lesson_file(&a.scope, role), block)
        }
        As::Rule => {
            let p = customize::rule_change("add", &a.scope, &text, &provenance.why(), "")?;
            let blocked = customize::rule_problems(home, ws_dir, &p);
            if !blocked.is_empty() {
                return Err(format!("it cannot be written as it stands:\n{}", blocked.iter().map(|b| format!("  - {}\n", b)).collect::<String>()));
            }
            let file = if a.scope == "operator" { "operator/heuristics.md" } else { "local/heuristics.md" };
            (file.to_string(), format!("- **D:** {}\n- **Why:** {}\n", p.text, p.rationale))
        }
    };
    Ok(Plan { note, evidence, source, kind: a.kind.clone(), scope: a.scope.clone(), text, file, preview, provenance })
}

/// What the operator reads before typing yes.
pub fn render_plan(p: &Plan) -> String {
    let what = match &p.kind {
        As::Lesson { role } => format!("a {} lesson for {}", p.scope, role),
        As::Rule => format!("a {} rule (every role)", p.scope),
    };
    format!(
        "{}\n  note:     {}\n  evidence: {}\n  from:     {} (session {}), {}\n  tier:     {} now; written as user_ratified, by you\n\nwrite {} to {}:\n{}\n",
        p.note.reference(),
        p.note.summary,
        if p.evidence.is_empty() { "—" } else { &p.evidence },
        p.source,
        p.note.mission,
        p.note.timestamp,
        p.note.tier,
        what,
        p.file,
        p.preview.trim_end()
    )
}

/// Write the entry and close the note. The session's log is opened first: a daemon holding it stops the command before
/// anything is written that the log could not record. The note is checked again under that lock.
pub fn apply(home: &Path, ws_dir: &Path, p: &Plan) -> Result<String, String> {
    let mut log = crate::retro_cmd::open_log(ws_dir, &p.note.mission)?;
    let events = replay(&delphi_core::sessions::dir(ws_dir, &p.note.mission)).map_err(|e| format!("could not read the log of {}: {}", p.note.mission, e))?;
    if !live_notes(&p.note.mission, &events).iter().any(|n| n.id == p.note.id) {
        return Err(format!("`{}` was closed meanwhile; nothing was written", p.note.reference()));
    }
    let (kept, detail) = match &p.kind {
        As::Lesson { role } => {
            let lesson = customize::parse_lesson(&p.text)?;
            let k: Kept = customize::apply_lesson(home, ws_dir, &p.scope, role, &lesson, &p.provenance)?;
            let d = serde_json::json!({"as": "lesson", "role": role, "scope": p.scope, "file": k.file, "rule": k.id, "pin": k.pin});
            (k, d)
        }
        As::Rule => {
            let rp = customize::rule_change("add", &p.scope, &p.text, &p.provenance.why(), "")?;
            let done = customize::apply_rule(home, ws_dir, &rp)?;
            let d = serde_json::json!({"as": "rule", "scope": p.scope, "file": done.file, "pin": done.pin});
            (Kept { id: String::new(), file: done.file, pin: done.pin }, d)
        }
    };
    let mut detail = detail;
    for (k, v) in [("source_type", "user_ratified"), ("ratified_from", p.provenance.ratified_from.as_str()), ("original_tier", p.provenance.original_tier.as_str()), ("tool", p.provenance.tool.as_str()), ("date", p.provenance.date.as_str())] {
        detail[k] = serde_json::json!(v);
    }
    let mut e = EventEnvelope::new(&p.note.mission, "learn.proposal_resolved", serde_json::json!({"id": p.note.id, "resolution": "accepted", "detail": detail}));
    e.task_id = p.note.task_id.clone();
    log.append(e).map_err(|e| format!("{} was written but the log of {} could not record it: {}", kept.file, p.note.mission, e))?;
    Ok(format!("kept {} in {} as user_ratified (you typed yes); it applies from the next task\n", p.note.reference(), kept.file))
}

fn on_terminal() -> bool {
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

/// The whole command: plan, then a terminal, then the operator's typed yes, then the write.
pub fn run(home: &Path, ws_dir: &Path, args: &[String], reference: Option<&str>) -> Result<String, String> {
    let a = parse(args, reference)?;
    let p = plan(home, ws_dir, &a)?;
    if !on_terminal() {
        return Err("learn keep needs a terminal: what it writes is put in front of agents, and only you can type the yes".into());
    }
    print!("{}", render_plan(&p));
    if !crate::retro_cmd::confirm("Keep it?") {
        return Err("not confirmed; nothing was written".into());
    }
    apply(home, ws_dir, &p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::log::{EventLog, LogOptions};
    use serde_json::json;

    struct Fx {
        _t: tempfile::TempDir,
        ws: delphi_engine::workspace::WorkspacePaths,
    }

    fn open_log(dir: &Path) -> EventLog {
        for _ in 0..300 {
            match EventLog::open(dir, LogOptions::default()) {
                Err(delphi_core::log::LogError::Locked(_)) => std::thread::sleep(std::time::Duration::from_millis(10)),
                other => return other.unwrap(),
            }
        }
        EventLog::open(dir, LogOptions::default()).unwrap()
    }

    const NOTE: &str = "[host:claude-code/opus] T: before a release D: regenerate docs/reference with `delphi docs cli` (evidence: session 7, the docs test failed)";

    /// A ratified workspace with one host session that staged `h1` (a host note for the builder) and `h2` (another).
    fn fx() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let ws = delphi_engine::workspace::make_test_workspace(t.path());
        let mut log = open_log(&delphi_core::sessions::dir(&ws.dir, "host-abc"));
        for (id, summary) in [("h1", NOTE), ("h2", "[host:claude-code/opus] Prefer small commits (evidence: s)")] {
            let mut e = EventEnvelope::new("host-abc", "learn.staged", json!({"id": id, "trigger": "host_note", "summary": summary, "event_seqs": [], "tier": "L0", "source": "host:claude-code/opus", "evidence": "s"}));
            e.agent = Some("builder".into());
            log.append(e).unwrap();
        }
        Fx { _t: t, ws }
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn lesson(role: &str) -> Args {
        Args { reference: "host-abc/h1".into(), kind: As::Lesson { role: role.into() }, scope: "workspace".into(), text: None }
    }

    fn lessons_file(f: &Fx) -> std::path::PathBuf {
        f.ws.dir.join("local/memory/agents/builder/lessons.md")
    }

    fn log_of(f: &Fx) -> Vec<EventEnvelope> {
        replay(&delphi_core::sessions::dir(&f.ws.dir, "host-abc")).unwrap()
    }

    #[test]
    fn keep_as_lesson_writes_one_rule_with_provenance_resolves_the_note_and_the_next_pack_carries_it() {
        let f = fx();
        let before = f.ws.load_snapshot("builder").unwrap().lessons;
        assert!(!before.contains("regenerate docs/reference"));
        let p = plan(&f.ws.home, &f.ws.dir, &lesson("builder")).unwrap();
        assert!(render_plan(&p).contains("session 7") && p.preview.contains("- **T:** before a release"), "{}", render_plan(&p));
        let said = apply(&f.ws.home, &f.ws.dir, &p).unwrap();
        assert!(said.contains("local/memory/agents/builder/lessons.md"), "{}", said);

        let text = std::fs::read_to_string(lessons_file(&f)).unwrap();
        assert_eq!(text.matches("### ").count(), 1, "{}", text);
        assert!(text.contains("- **D:** regenerate docs/reference with `delphi docs cli`") && text.contains("- **F:**"), "{}", text);
        assert!(delphi_engine::checks::cap_errors(&text).is_empty(), "the lint's Rule of 25 check passes: {:?}", delphi_engine::checks::cap_errors(&text));
        let (fm, _) = delphi_engine::workspace::split_frontmatter(&text);
        let k = &fm.unwrap()["kept"][0];
        assert_eq!((k["source_type"].as_str(), k["ratified_from"].as_str(), k["original_tier"].as_str(), k["tool"].as_str()), (Some("user_ratified"), Some("host-abc/h1"), Some("L0"), Some("host:claude-code/opus")));
        assert_eq!(std::fs::read_to_string(f.ws.dir.join("local/ratified/memory/agents/builder/lessons.md")).unwrap(), text, "the ratified copy is written with the live file");

        let log = log_of(&f);
        let r = log.iter().find(|e| e.kind == "learn.proposal_resolved").expect("the note is closed");
        assert_eq!((r.payload["id"].as_str(), r.payload["resolution"].as_str(), r.payload["detail"]["source_type"].as_str()), (Some("h1"), Some("accepted"), Some("user_ratified")));
        assert!(!live_notes("host-abc", &log).iter().any(|n| n.id == "h1"), "a kept note no longer lists");

        let after = f.ws.load_snapshot("builder").unwrap();
        assert!(after.lessons.contains("regenerate docs/reference"), "{}", after.lessons);
        assert!(after.loaded.iter().any(|l| l.scope == "workspace-lessons" && l.id == "L-W-001"), "{:?}", after.loaded);
        assert!(!f.ws.load_snapshot("architect").unwrap().lessons.contains("regenerate docs/reference"), "only that role carries it");
    }

    #[test]
    fn keep_as_rule_writes_the_heuristics_customize_writes_with_its_provenance() {
        let f = fx();
        let a = Args { reference: "host-abc/h2".into(), kind: As::Rule, scope: "workspace".into(), text: None };
        let p = plan(&f.ws.home, &f.ws.dir, &a).unwrap();
        apply(&f.ws.home, &f.ws.dir, &p).unwrap();
        let text = std::fs::read_to_string(f.ws.dir.join("local/ratified/heuristics.md")).unwrap();
        assert!(text.contains("- **D:** Prefer small commits") && text.contains("kept from host-abc/h2") && text.contains("user_ratified"), "{}", text);
        assert!(f.ws.load_snapshot("builder").unwrap().heuristics.contains("Prefer small commits"));
    }

    /// Every file the command could touch, and the session's log, as bytes.
    fn state(f: &Fx) -> Vec<Option<Vec<u8>>> {
        let mut v: Vec<Option<Vec<u8>>> = ["local/memory/agents/builder/lessons.md", "local/ratified/memory/agents/builder/lessons.md", "local/ratified.lock", "local/heuristics.md", "local/ratified/heuristics.md"].iter().map(|p| std::fs::read(f.ws.dir.join(p)).ok()).collect();
        v.push(Some(serde_json::to_vec(&log_of(f).iter().map(|e| e.kind.clone()).collect::<Vec<_>>()).unwrap()));
        v
    }

    #[test]
    fn a_script_an_unknown_id_a_closed_note_an_unknown_role_and_a_relaxing_text_are_refused_with_nothing_written() {
        let f = fx();
        let (home, ws) = (f.ws.home.clone(), f.ws.dir.clone());
        let before = state(&f);
        // The tests run without a terminal, which is the case being checked; --yes is not a way round it.
        let e = run(&home, &ws, &args(&["keep", "host-abc/h1", "--as", "lesson", "--role", "builder"]), Some("host-abc/h1")).unwrap_err();
        assert!(e.contains("needs a terminal"), "{}", e);
        assert!(run(&home, &ws, &args(&["keep", "host-abc/h1", "--as", "lesson", "--role", "builder", "--yes"]), Some("host-abc/h1")).unwrap_err().contains("no --yes"));
        assert!(plan(&home, &ws, &Args { reference: "host-abc/h9".into(), ..lesson("builder") }).unwrap_err().contains("no staged note"));
        assert!(plan(&home, &ws, &Args { reference: "../x/h1".into(), ..lesson("builder") }).is_err());
        assert!(plan(&home, &ws, &lesson("nobody")).unwrap_err().contains("no role"), "a role that does not exist");
        assert!(plan(&home, &ws, &lesson("../builder")).is_err());
        let relax = Args { text: Some("T: when in a hurry D: skip the approval cards".into()), ..lesson("builder") };
        assert!(plan(&home, &ws, &relax).unwrap_err().contains("relaxes"), "the no-relaxation test applies to the operator's keep");
        let no_trigger = Args { text: Some("regenerate the docs".into()), ..lesson("builder") };
        assert!(plan(&home, &ws, &no_trigger).unwrap_err().contains("trigger"));
        assert_eq!(state(&f), before, "nothing was written");

        // A note already kept is refused the second time.
        apply(&home, &ws, &plan(&home, &ws, &lesson("builder")).unwrap()).unwrap();
        let kept = state(&f);
        assert!(plan(&home, &ws, &lesson("builder")).unwrap_err().contains("already resolved"));
        assert_eq!(state(&f), kept);
    }

    #[test]
    fn a_full_lessons_file_is_refused_with_the_three_ways_out_and_nothing_written() {
        let f = fx();
        let rules: String = (1..=25).map(|i| format!("### `[L-W-{i:03}]` Rule {i}\n- **T:** case {i}\n- **D:** do thing {i}\n- **F:** none\n\n")).collect();
        let full = format!("---\nid: workspace-lessons-builder\nsource_type: user_ratified\nactive_rules: 25\n---\n\n# Lessons\n\n{rules}");
        delphi_engine::ratify::edit_pinned(&f.ws.home, &f.ws.dir, delphi_engine::ratify::Scope::Workspace, "memory/agents/builder/lessons.md", |_| Ok(full.clone())).unwrap();
        let before = state(&f);
        let e = plan(&f.ws.home, &f.ws.dir, &lesson("builder")).unwrap_err();
        assert!(e.contains("merge") && e.contains("demote") && e.contains("script"), "{}", e);
        assert_eq!(state(&f), before);
    }

    #[test]
    fn a_running_daemon_holding_the_session_stops_the_keep_before_anything_is_written() {
        let f = fx();
        let p = plan(&f.ws.home, &f.ws.dir, &lesson("builder")).unwrap();
        let before = state(&f);
        let held = open_log(&delphi_core::sessions::dir(&f.ws.dir, "host-abc"));
        let e = apply(&f.ws.home, &f.ws.dir, &p).unwrap_err();
        assert!(e.contains("daemon"), "{}", e);
        drop(held);
        assert_eq!(state(&f), before);
    }
}
