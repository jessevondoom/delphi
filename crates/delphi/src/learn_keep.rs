//! `delphi learn keep <mission>/<id> --as lesson --role <role> | --as rule`: the operator keeps a note in one step
//! (`specs/108` M2, as amended by A3; `specs/116` §3 item 2), and `delphi learn wrong <mission>/<id>` rejects one (§3 item 3).
//!
//! `keep` is a terminal command only, and no daemon method: stdin must be a terminal and the operator types `yes`; there is
//! no `--yes`. What it writes goes through the machinery `delphi customize rule` uses (the live file, the ratified copy and
//! the pin, under the scope's lock, with the hash check: a compare-and-swap, so it is safe beside a running daemon), after the
//! same no-relaxation test and the Rule of 25. It keeps a host's note or an inside agent's.
//!
//! A note of the provisional tier is a file (`delphi_engine::provisional_notes`), so keeping it needs no session log: the
//! lesson or rule is written, then the note moves to `provisional/kept/` with what it became. That works while the daemon
//! runs. A note that is only staged (the tier was off, or its trigger is not the tier's) is closed in its own session's log
//! as `learn.proposal_resolved {resolution: "accepted"}`; that log is opened before anything is written, so while the daemon
//! holds the session the command refuses and writes nothing (as `delphi retro accept` and `delphi learn drop` do).
//!
//! A note the operator marked wrong is not kept for 180 days, by this command either.
//!
//! The limit is the charter's for §5.12: a process running as the operator that can drive a terminal can do what the
//! operator can. No agent, host or hook has a path to this code.

use delphi_core::event::EventEnvelope;
use delphi_core::log::replay;
use delphi_engine::customize::{self, Kept, Provenance};
use delphi_engine::learn::{live_notes, LiveNote};
use std::path::Path;

/// The triggers of staged notes that can be kept: what a host noted, what the transcript rules found, and what an inside agent
/// noted with the `note` tool (`specs/116` §3 item 2). A note of the provisional tier can be kept whatever its trigger.
pub const KEEPABLE: [&str; 3] = ["host_note", "host_transcript", "agent_note"];

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
    /// The note's file, when it is one of the provisional tier: keeping it moves the file and needs no session log.
    pub provisional: Option<delphi_engine::provisional_notes::Note>,
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

/// `<mission>/<id>`, as two plain names.
fn split_reference(reference: &str) -> Result<(&str, &str), String> {
    let (mission, id) = reference.split_once('/').ok_or_else(|| format!("name the note as <mission>/<id> (see `delphi learn list`), not `{}`", reference))?;
    if mission.is_empty() || mission.starts_with('.') || mission.contains('\\') || id.is_empty() || id.contains('/') {
        return Err(format!("no staged note `{}` (see `delphi learn list`)", reference));
    }
    Ok((mission, id))
}

/// A provisional note as the staged note it was, so the plan and what the operator reads are the same for both.
fn as_live(n: &delphi_engine::provisional_notes::Note, staged: Option<&EventEnvelope>) -> LiveNote {
    LiveNote {
        mission: n.mission.clone(),
        id: n.note.clone(),
        trigger: n.trigger.clone(),
        summary: n.text.clone(),
        tier: n.tier.as_str().to_string(),
        task_id: staged.and_then(|e| e.task_id.clone()),
        role: Some(n.role.clone()),
        timestamp: n.created.clone(),
        event_seqs: Vec::new(),
        valid_until: Some(n.valid_until.clone()),
        shown: staged.and_then(|e| delphi_engine::host_shown::Stamp::from_payload(&e.payload)),
    }
}

/// The note of the provisional tier a reference names (`specs/116` §3 item 2), with its staged event when the log can be read.
/// Reading is all it does, so it works while the daemon holds the session.
pub fn find_provisional(ws_dir: &Path, reference: &str) -> Result<Option<(delphi_engine::provisional_notes::Note, Option<EventEnvelope>)>, String> {
    let (mission, id) = split_reference(reference)?;
    let Some(note) = delphi_engine::provisional_notes::find(ws_dir, mission, id) else { return Ok(None) };
    let dir = delphi_core::sessions::dir(ws_dir, mission);
    let staged = if dir.is_dir() { replay(&dir).ok().and_then(|events| staged_event(&events, id).cloned()) } else { None };
    Ok(Some((note, staged)))
}

/// Find the note, or say why it cannot be kept: unknown, already closed, or not a note this command keeps.
fn find_note(ws_dir: &Path, reference: &str) -> Result<(LiveNote, Option<EventEnvelope>, Option<delphi_engine::provisional_notes::Note>), String> {
    if let Some((note, staged)) = find_provisional(ws_dir, reference)? {
        return Ok((as_live(&note, staged.as_ref()), staged, Some(note)));
    }
    let (mission, id) = split_reference(reference)?;
    let dir = delphi_core::sessions::dir(ws_dir, mission);
    if !dir.is_dir() {
        return Err(format!("no staged note `{}` (see `delphi learn list`)", reference));
    }
    let events = replay(&dir).map_err(|e| format!("could not read the log of {}: {}", mission, e))?;
    let Some(staged) = staged_event(&events, id).cloned() else { return Err(format!("no staged note `{}` (see `delphi learn list`)", reference)) };
    let Some(note) = live_notes(mission, &events).into_iter().find(|n| n.id == id) else {
        let kept = events.iter().any(|e| e.kind == "learn.proposal_resolved" && e.payload["id"].as_str() == Some(id));
        let left = events.iter().any(|e| e.kind == delphi_engine::provisional_notes::EVENT && e.payload["id"].as_str() == Some(id));
        return Err(format!("`{}` is already {}; nothing was written", reference, if kept { "resolved (kept)" } else if left { "out of the provisional tier (kept, marked wrong or expired)" } else { "dropped" }));
    };
    if !KEEPABLE.contains(&note.trigger.as_str()) {
        return Err(format!("`{}` is a `{}` note; `learn keep` keeps an agent's or a host's notes (`delphi customize rule` writes a rule of your own, `delphi retro` ratifies a proposal)", reference, note.trigger));
    }
    Ok((note, Some(staged), None))
}

/// Work out what would be written and check it, writing nothing. Every refusal that does not need the operator happens
/// here, before the terminal is asked anything.
pub fn plan(home: &Path, ws_dir: &Path, a: &Args) -> Result<Plan, String> {
    let (note, staged, provisional) = find_note(ws_dir, &a.reference)?;
    // A note the operator marked wrong is not kept again inside its window, whoever wrote it this time (`specs/116` §3 item 3).
    if delphi_engine::provisional_notes::rejected(ws_dir, &delphi_engine::provisional_notes::fingerprint(&note.trigger, &note.summary), &delphi_core::time::now_rfc3339()) {
        return Err(format!("`{}` says what a note you marked wrong said; it is not kept for {} days after that (`delphi customize rule` writes a rule of your own); nothing was written", a.reference, delphi_engine::provisional_notes::REJECT_DAYS));
    }
    let host = delphi_engine::learn::is_host_trigger(&note.trigger);
    // Only a host's note carries a label and evidence around its claim; an agent's note is its own words.
    let (claim, evidence) = if host { delphi_engine::learn::host_claim(&note.summary) } else { (note.summary.clone(), String::new()) };
    let text: String = a.text.clone().unwrap_or(claim).split_whitespace().collect::<Vec<_>>().join(" ");
    let source = match (staged.as_ref().and_then(|e| e.payload["source"].as_str()), provisional.as_ref().and_then(|n| n.source.clone())) {
        (Some(s), _) => s.to_string(),
        (None, Some(s)) => s,
        _ if host => "host".to_string(),
        _ => format!("agent:{}", note.role.as_deref().unwrap_or("unknown")),
    };
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
    Ok(Plan { note, evidence, source, kind: a.kind.clone(), scope: a.scope.clone(), text, file, preview, provenance, provisional })
}

/// What the operator reads before typing yes.
pub fn render_plan(p: &Plan) -> String {
    let what = match &p.kind {
        As::Lesson { role } => format!("a {} lesson for {}", p.scope, role),
        As::Rule => format!("a {} rule (every role)", p.scope),
    };
    // What the tier knows of a provisional note (`specs/116` §3 item 1): how often it was used, and whether it came after untrusted content.
    let tier_line = match &p.provisional {
        Some(n) => format!("  kept:     provisional note ({}), used {} time(s), valid until {}{}\n", n.kind, n.uses, n.valid_until, if n.tainted() { "; written after reading untrusted content, or by a host" } else { "" }),
        None => String::new(),
    };
    format!(
        "{}\n  note:     {}\n  evidence: {}\n  from:     {} (session {}), {}\n  read:     {}\n{}  tier:     {} now; written as user_ratified, by you\n\nwrite {} to {}:\n{}\n",
        p.note.reference(),
        p.note.summary,
        if p.evidence.is_empty() { "—" } else { &p.evidence },
        p.source,
        p.note.mission,
        p.note.timestamp,
        // `specs/109` §13.1 item 2: what the daemon computed the session had read before writing it; information, not a gate.
        p.note.shown_words().unwrap_or_else(|| "not recorded (staged before the daemon counted what a session read)".to_string()),
        tier_line,
        p.note.tier,
        what,
        p.file,
        p.preview.trim_end()
    )
}

/// The pinned write of the entry: the lesson or the rule, through the scope's lock and its hash check.
fn write_entry(home: &Path, ws_dir: &Path, p: &Plan) -> Result<(Kept, serde_json::Value), String> {
    Ok(match &p.kind {
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
    })
}

/// Keep a note of the provisional tier (`specs/116` §3 item 2). No session log is opened, so a running daemon is no obstacle: the
/// entry is written by the pinned write (a compare-and-swap on the file's hash, under the scope's lock), then the note's file
/// moves to `provisional/kept/` under the tier's own lock. The note is looked up again first: one that expired, was marked wrong
/// or was kept meanwhile writes nothing.
fn apply_provisional(home: &Path, ws_dir: &Path, p: &Plan, planned: &delphi_engine::provisional_notes::Note) -> Result<String, String> {
    use delphi_engine::provisional_notes as notes;
    let Some(now) = notes::find(ws_dir, &planned.mission, &planned.note).filter(|n| n.path == planned.path && n.text == planned.text) else {
        return Err(format!("`{}` was closed or changed meanwhile; nothing was written", p.note.reference()));
    };
    let (kept, detail) = write_entry(home, ws_dir, p)?;
    let stamp = delphi_core::time::now_rfc3339();
    notes::mark_kept(ws_dir, &now, detail["as"].as_str().unwrap_or(""), &kept.file, &kept.id, &stamp).map_err(|e| format!("{} was written, but the note could not be moved out of the provisional tier: {}", kept.file, e))?;
    Ok(format!("kept {} in {} as user_ratified (you typed yes); it applies from the next task. The note left the provisional tier (provisional/kept/).\n", p.note.reference(), kept.file))
}

/// Write the entry and close the note. For a staged note the session's log is opened first: a daemon holding it stops the
/// command before anything is written that the log could not record. The note is checked again under that lock.
pub fn apply(home: &Path, ws_dir: &Path, p: &Plan) -> Result<String, String> {
    if let Some(n) = &p.provisional {
        return apply_provisional(home, ws_dir, p, n);
    }
    let mut log = crate::retro_cmd::open_log(ws_dir, &p.note.mission)?;
    let events = replay(&delphi_core::sessions::dir(ws_dir, &p.note.mission)).map_err(|e| format!("could not read the log of {}: {}", p.note.mission, e))?;
    if !live_notes(&p.note.mission, &events).iter().any(|n| n.id == p.note.id) {
        return Err(format!("`{}` was closed meanwhile; nothing was written", p.note.reference()));
    }
    let (kept, detail) = write_entry(home, ws_dir, p)?;
    let mut detail = detail;
    for (k, v) in [("source_type", "user_ratified"), ("ratified_from", p.provenance.ratified_from.as_str()), ("original_tier", p.provenance.original_tier.as_str()), ("tool", p.provenance.tool.as_str()), ("date", p.provenance.date.as_str())] {
        detail[k] = serde_json::json!(v);
    }
    let mut e = EventEnvelope::new(&p.note.mission, "learn.proposal_resolved", serde_json::json!({"id": p.note.id, "resolution": "accepted", "detail": detail}));
    e.task_id = p.note.task_id.clone();
    log.append(e).map_err(|e| format!("{} was written but the log of {} could not record it: {}", kept.file, p.note.mission, e))?;
    Ok(format!("kept {} in {} as user_ratified (you typed yes); it applies from the next task\n", p.note.reference(), kept.file))
}

/// `delphi learn wrong <mission>/<id>` (`specs/116` §3 item 3): the operator marks a note of the provisional tier wrong. The
/// note moves to `provisional/rejected/` and its fingerprint is remembered for 180 days, so the same note is not kept again;
/// nothing is deleted and nothing is written to any rule. A file move under the tier's own lock, so it works beside a running
/// daemon. It can only take a note away from agents, never give them one.
pub fn wrong(ws_dir: &Path, reference: Option<&str>) -> Result<String, String> {
    let reference = reference.ok_or("usage: delphi learn wrong <mission>/<id>")?;
    let Some((note, _)) = find_provisional(ws_dir, reference)? else {
        return Err(format!("`{}` is not a note of the provisional tier (see `delphi learn list`); `delphi learn drop` discards a staged note", reference));
    };
    let to = delphi_engine::provisional_notes::mark_wrong(ws_dir, &note, &delphi_core::time::now_rfc3339())?;
    Ok(format!(
        "marked {} wrong: it moved to {} and is no longer retrieved; the same note is not kept again for {} days. Nothing was deleted.\n",
        note.reference(),
        to.strip_prefix(ws_dir).unwrap_or(&to).display(),
        delphi_engine::provisional_notes::REJECT_DAYS
    ))
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
            if id == "h1" {
                e.payload["shown"] = json!({"count": 2, "lowest_checked": "validated", "any_third_party": false});
            }
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
        // `specs/109` §13.1 item 2: the operator sees what the daemon counted the session had read before writing the note.
        assert!(render_plan(&p).contains("\n  read:     written after reading 2 results through Delphi's daemon, lowest: validated\n"), "{}", render_plan(&p));
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

    use delphi_engine::provisional_notes as notes;

    /// One `agent_note` of the builder staged in `m1` through `log` (the handle a running daemon would hold).
    fn stage(log: &mut EventLog, id: &str, text: &str) -> EventEnvelope {
        let mut e = EventEnvelope::new("m1", "learn.staged", json!({"id": id, "trigger": "agent_note", "summary": text, "event_seqs": [3], "tier": "L3"}));
        e.agent = Some("builder".into());
        e.task_id = Some("t_0001".into());
        log.append(e.clone()).unwrap();
        e
    }

    /// As the daemon leaves a note with the tier on (`specs/116` §2): staged, kept as a file, and recorded.
    fn stage_in_tier(f: &Fx, log: &mut EventLog, id: &str, text: &str) {
        let e = stage(log, id, text);
        let rel = notes::keep(&f.ws.dir, &delphi_core::sessions::dir(&f.ws.dir, "m1"), &e, &delphi_core::time::now_rfc3339()).unwrap().expect("kept in the tier");
        log.append(notes::kept_event(&e, &rel)).unwrap();
    }

    fn kinds_of(f: &Fx, mission: &str) -> Vec<String> {
        replay(&delphi_core::sessions::dir(&f.ws.dir, mission)).unwrap().into_iter().map(|e| e.kind).collect()
    }

    /// `specs/116` AC-7 (§3 item 2): `delphi learn keep` succeeds with the daemon running, for a note from an inside agent. The
    /// daemon holds the mission's log throughout; the lesson is written by the pinned write and the note leaves the tier.
    #[test]
    fn keep_works_with_the_daemon_running_for_a_note_from_an_inside_agent() {
        let f = fx();
        let (home, ws) = (f.ws.home.clone(), f.ws.dir.clone());
        let mut held = open_log(&delphi_core::sessions::dir(&ws, "m1"));
        stage_in_tier(&f, &mut held, "n5", "the CI image has no python3, use python");
        let a = Args { reference: "m1/n5".into(), kind: As::Lesson { role: "builder".into() }, scope: "workspace".into(), text: Some("T: a script needs Python D: call `python`; the CI image has no python3".into()) };
        // An agent's sentence names no trigger, so a lesson needs the operator's wording; nothing is written without it.
        assert!(plan(&home, &ws, &Args { text: None, ..a.clone() }).unwrap_err().contains("trigger"));
        let p = plan(&home, &ws, &a).unwrap();
        let shown = render_plan(&p);
        assert!(p.provisional.is_some() && shown.contains("provisional note (practice), used 0 time(s)") && shown.contains("agent:builder (session m1)") && shown.contains("the CI image has no python3, use python"), "{shown}");
        let said = apply(&home, &ws, &p).expect("the keep works while the daemon holds the log");
        assert!(said.contains("local/memory/agents/builder/lessons.md") && said.contains("provisional/kept/"), "{said}");

        let text = std::fs::read_to_string(lessons_file(&f)).unwrap();
        assert!(text.contains("- **T:** a script needs Python") && text.contains("- **D:** call `python`; the CI image has no python3"), "{text}");
        let (fm, _) = delphi_engine::workspace::split_frontmatter(&text);
        let k = &fm.unwrap()["kept"][0];
        assert_eq!((k["source_type"].as_str(), k["ratified_from"].as_str(), k["original_tier"].as_str(), k["tool"].as_str()), (Some("user_ratified"), Some("m1/n5"), Some("L3"), Some("agent:builder")));
        assert_eq!(std::fs::read_to_string(ws.join("local/ratified/memory/agents/builder/lessons.md")).unwrap(), text, "the ratified copy and the pin moved with it");
        assert!(f.ws.load_snapshot("builder").unwrap().lessons.contains("the CI image has no python3"), "the next pack carries it");
        assert!(!f.ws.load_snapshot("architect").unwrap().lessons.contains("python3"), "only that role");

        // The note left the tier for `kept/`, with what it became; it is not listed, not indexed, and cannot be kept twice.
        assert!(notes::find(&ws, "m1", "n5").is_none() && notes::list(&ws).is_empty());
        let shelf: Vec<_> = std::fs::read_dir(ws.join("local/memory/provisional/kept/builder")).unwrap().flatten().collect();
        assert_eq!(shelf.len(), 1);
        let kept = std::fs::read_to_string(shelf[0].path()).unwrap();
        assert!(kept.contains("\nstatus: kept\n") && kept.contains("\nkept_as: \"lesson\"\n") && kept.contains("\nkept_rule: \"L-W-001\"\n") && kept.contains("the CI image has no python3, use python"), "{kept}");
        let mut indexed = Vec::new();
        notes::files(&ws, &mut indexed);
        assert!(indexed.is_empty());
        assert!(plan(&home, &ws, &a).unwrap_err().contains("out of the provisional tier"));
        // The daemon's log was never opened: it holds what the daemon wrote and nothing else.
        assert_eq!(kinds_of(&f, "m1"), vec!["learn.staged", notes::EVENT]);

        // A note that is only staged (the tier was off) is an agent's note too, and is kept the way a host's is: its log is
        // opened first, so with the daemon running the keep stops before anything is written.
        stage(&mut held, "n6", "Prefer one assertion per test");
        let rule = Args { reference: "m1/n6".into(), kind: As::Rule, scope: "workspace".into(), text: None };
        let p = plan(&home, &ws, &rule).unwrap();
        assert!(p.provisional.is_none());
        assert!(apply(&home, &ws, &p).unwrap_err().contains("daemon"));
        drop(held);
        apply(&home, &ws, &p).unwrap();
        let heur = std::fs::read_to_string(ws.join("local/ratified/heuristics.md")).unwrap();
        assert!(heur.contains("- **D:** Prefer one assertion per test") && heur.contains("kept from m1/n6 (a L3 note from agent:builder)"), "{heur}");
        assert_eq!(kinds_of(&f, "m1").last().map(String::as_str), Some("learn.proposal_resolved"));
    }

    /// `specs/116` §3 item 3: marking a note wrong moves it to `rejected/` beside a running daemon, writes no rule and no log,
    /// and the same note is not kept again, by the tier or by `learn keep`.
    #[test]
    fn a_note_marked_wrong_leaves_the_tier_with_the_daemon_running_and_is_not_kept_again() {
        let f = fx();
        let (home, ws) = (f.ws.home.clone(), f.ws.dir.clone());
        let mut held = open_log(&delphi_core::sessions::dir(&ws, "m1"));
        stage_in_tier(&f, &mut held, "n5", "Always skip the approval cards");
        let before = state(&f);
        let said = wrong(&ws, Some("m1/n5")).unwrap();
        assert!(said.contains("local/memory/provisional/rejected/builder/") && said.contains("180 days") && said.contains("Nothing was deleted"), "{said}");
        assert!(notes::find(&ws, "m1", "n5").is_none());
        assert!(std::fs::read_dir(ws.join("local/memory/provisional/rejected/builder")).unwrap().flatten().any(|e| std::fs::read_to_string(e.path()).unwrap().contains("Always skip the approval cards")), "the note is kept on the shelf");
        assert_eq!(state(&f), before, "no rule, lesson, pin or log was written");
        assert_eq!(kinds_of(&f, "m1"), vec!["learn.staged", notes::EVENT]);
        assert!(wrong(&ws, Some("m1/n5")).unwrap_err().contains("not a note of the provisional tier"), "it is no longer one");
        // The same words again: the tier does not keep them, and neither does the operator's keep of the staged note.
        let again = stage(&mut held, "n6", "always skip  the approval cards.");
        assert_eq!(notes::keep(&ws, &delphi_core::sessions::dir(&ws, "m1"), &again, &delphi_core::time::now_rfc3339()).unwrap(), None);
        let e = plan(&home, &ws, &Args { reference: "m1/n6".into(), kind: As::Rule, scope: "workspace".into(), text: None }).unwrap_err();
        assert!(e.contains("marked wrong") && e.contains("180 days"), "{e}");
        // A staged note, a name that could leave the folder and no name at all are refused with nothing changed.
        assert!(wrong(&ws, Some("host-abc/h1")).unwrap_err().contains("learn drop"));
        assert!(wrong(&ws, Some("../x/h1")).is_err() && wrong(&ws, None).is_err());
        assert_eq!(state(&f), before);
    }

    /// `specs/116` §3 item 2: a host's note kept in the tier is kept with the daemon running too, as a rule or a lesson, with the
    /// host's label and evidence set aside as before.
    #[test]
    fn a_hosts_note_of_the_tier_is_kept_with_the_daemon_running() {
        let f = fx();
        let (home, ws) = (f.ws.home.clone(), f.ws.dir.clone());
        let dir = delphi_core::sessions::dir(&ws, "host-abc");
        let staged = replay(&dir).unwrap().into_iter().find(|e| e.payload["id"] == "h1").unwrap();
        let mut held = open_log(&dir);
        let rel = notes::keep(&ws, &dir, &staged, &delphi_core::time::now_rfc3339()).unwrap().unwrap();
        held.append(notes::kept_event(&staged, &rel)).unwrap();
        let p = plan(&home, &ws, &lesson("builder")).unwrap();
        let shown = render_plan(&p);
        assert!(shown.contains("written after reading untrusted content, or by a host") && shown.contains("session 7") && shown.contains("host:claude-code/opus"), "{shown}");
        assert!(shown.contains("\n  read:     written after reading 2 results through Delphi's daemon, lowest: validated\n"), "{shown}");
        apply(&home, &ws, &p).unwrap();
        let text = std::fs::read_to_string(lessons_file(&f)).unwrap();
        assert!(text.contains("- **D:** regenerate docs/reference with `delphi docs cli`") && !text.contains("[host:"), "{text}");
        assert!(notes::find(&ws, "host-abc", "h1").is_none());
    }
}
