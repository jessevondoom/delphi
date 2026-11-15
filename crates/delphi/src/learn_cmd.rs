//! `delphi learn list|show|drop|wrong`: the notes the daemon staged from what happened (`specs/031` §3.1-§3.2), and the notes
//! of the provisional tier (`specs/116` §3).
//!
//! A staged note is a `learn.staged` event in a mission's log. Nothing here loads a note into a prompt, and `drop`
//! appends `learn.dropped`: no event is ever removed. Listing and showing read files only; `drop` needs the mission's
//! writer lock, so it refuses while a daemon has the mission open.
//!
//! A provisional note is a file the daemon wrote (`delphi_engine::provisional_notes`). `list` shows them after the staged
//! notes, the most used first, then the oldest; `wrong` (see `learn_keep.rs`) moves one to `provisional/rejected/`.

use delphi_core::event::EventEnvelope;
use delphi_core::log::{replay, EventLog, LogError, LogOptions};
use delphi_engine::learn::{count_label, live_notes, order_by_recurrence, scan, LiveNote, Recurrence};
use std::path::Path;

pub struct Options {
    pub mission: Option<String>,
    pub json: bool,
}

fn mission_ids(ws_dir: &Path, only: &Option<String>) -> Result<Vec<String>, String> {
    match only {
        Some(m) => {
            // A mission id is a directory name; refuse anything that could point elsewhere.
            if m.is_empty() || m.contains(['/', '\\']) || m.starts_with('.') || !delphi_core::sessions::dir(ws_dir, m).is_dir() {
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
        let events = replay(&delphi_core::sessions::dir(ws_dir, &id)).map_err(|e| format!("could not read the log of {}: {}", id, e))?;
        all.extend(live_notes(&id, &events));
    }
    all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.reference().cmp(&a.reference())));
    Ok(all)
}

/// The list with how often each claim recurred (`specs/083` §5): shown from two missions up, host counts as a report.
pub fn render_list_with(notes: &[LiveNote], r: &Recurrence) -> String {
    if notes.is_empty() {
        return "no staged notes: they appear when a card is denied with a reason, you correct the team, a task recovers on a retry or is escalated, or a provisional decision is rejected\n".into();
    }
    let mut out = String::new();
    let width = notes.iter().map(|n| n.reference().chars().count()).max().unwrap_or(0);
    for n in notes {
        let count = r.count(&n.trigger, &n.summary);
        let seen = if count >= 2 { format!("  [{}]", count_label(&n.trigger, count)) } else { String::new() };
        out.push_str(&format!("{:<w$}  {:<17}  {}  {}{}\n", n.reference(), n.trigger, n.tier, n.summary, seen, w = width));
        // What the daemon computed the session had read before writing it (`specs/109` §13.1 item 2): information, never a gate.
        if let Some(words) = n.shown_words() {
            out.push_str(&format!("{:<w$}  ({})\n", "", words, w = width));
        }
        // A host note that recurred in two or more sessions is marked, with the one command that keeps it (`specs/108` M2).
        if count >= 2 && crate::learn_keep::KEEPABLE.contains(&n.trigger.as_str()) {
            out.push_str(&format!("{:<w$}  * recurring: keep it with `{}`\n", "", keep_command(n), w = width));
        }
    }
    out.push_str(&format!("{} note(s). Staged notes are never loaded into a prompt; `delphi learn show <ref>` for one, `delphi learn drop <ref>` to discard it.\n", notes.len()));
    out
}

/// The notes of the provisional tier, in the order of review (`specs/116` §3 item 1): the most used first, then the oldest.
pub fn provisional(ws_dir: &Path, opts: &Options) -> Vec<delphi_engine::provisional_notes::Note> {
    let mut notes = delphi_engine::provisional_notes::list(ws_dir);
    if let Some(m) = &opts.mission {
        notes.retain(|n| &n.mission == m);
    }
    delphi_engine::provisional_notes::order_for_review(&mut notes);
    notes
}

/// The provisional notes as `list` prints them, after the staged ones; nothing when there are none.
pub fn render_provisional(notes: &[delphi_engine::provisional_notes::Note]) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let mut out = String::from("\nprovisional notes (unreviewed; a later run of the same role may be given one as data, never as an instruction), the most used first:\n");
    let width = notes.iter().map(|n| n.reference().chars().count()).max().unwrap_or(0);
    for n in notes {
        let from = if n.tainted() { "  [from untrusted content: never given automatically]" } else { "" };
        out.push_str(&format!("{:<w$}  {:<12}  {:<10}  uses {:<3}  until {}  {}{}\n", n.reference(), n.role, n.kind, n.uses, n.valid_until, n.text, from, w = width));
    }
    out.push_str(&format!(
        "{} provisional note(s). `delphi learn keep <ref> --as lesson --role <role> | --as rule` keeps one (it works while the daemon runs); `delphi learn wrong <ref>` marks one wrong; one left alone expires.\n",
        notes.len()
    ));
    out
}

/// One provisional note, as `show` prints it.
pub fn render_provisional_one(n: &delphi_engine::provisional_notes::Note) -> String {
    format!(
        "{}\n  provisional note: {}\n  role:    {}\n  kind:    {}\n  at:      {}\n  until:   {}\n  uses:    {}\n  tier:    {} ({})\n  trigger: {}\n  file:    {}\n\n{}\n",
        n.reference(),
        n.label(),
        n.role,
        n.kind,
        n.created,
        n.valid_until,
        n.uses,
        n.tier.as_str(),
        if n.tainted() { "written after reading untrusted content, or by a host: never given to an agent automatically" } else { "its role may be given it as data" },
        n.trigger,
        n.rel_path(),
        n.text
    )
}

fn provisional_named(ws_dir: &Path, reference: &str) -> Option<delphi_engine::provisional_notes::Note> {
    let (mission, id) = reference.split_once('/')?;
    delphi_engine::provisional_notes::find(ws_dir, mission, id)
}

/// The `learn keep` command for a host note: a lesson for the role the host named, or a rule when it named none.
pub fn keep_command(n: &LiveNote) -> String {
    match n.role.as_deref().filter(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')) {
        // A note already worded as `T: … D: …` is kept as it stands; any other needs its trigger said.
        Some(role) if delphi_engine::customize::parse_lesson(&delphi_engine::learn::host_claim(&n.summary).0).is_ok() => format!("delphi learn keep {} --as lesson --role {}", n.reference(), role),
        Some(role) => format!("delphi learn keep {} --as lesson --role {} --text \"T: <when it applies> D: <what to do>\"", n.reference(), role),
        None => format!("delphi learn keep {} --as rule", n.reference()),
    }
}

pub fn render_one(n: &LiveNote) -> String {
    let source = if n.tier == "L5" { "typed by the operator" } else if n.tier == "L3" { "composed by the daemon from the log" } else { "from a task that read content that is not validated" };
    let shown = n.shown_words().map(|w| format!("  read:    {w}\n")).unwrap_or_default();
    format!(
        "{}\n  trigger: {}\n  at:      {}\n  task:    {}\n  role:    {}\n  tier:    {} ({}); promotion needs L4 or above\n  events:  {}\n{}\n{}\n",
        n.reference(),
        n.trigger,
        n.timestamp,
        n.task_id.as_deref().unwrap_or("—"),
        n.role.as_deref().unwrap_or("—"),
        n.tier,
        source,
        n.event_seqs.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", "),
        shown,
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
    let dir = delphi_core::sessions::dir(ws_dir, &note.mission);
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
    let mut notes = collect(ws_dir, opts)?;
    match verb {
        "list" => {
            // Counts come from a read-only scan of every mission's log; nothing is stored (`specs/083` §5).
            let r = scan(ws_dir).1;
            order_by_recurrence(&mut notes, &r);
            // The provisional tier (`specs/116` §3 item 1): after the staged notes, the most used first, then the oldest.
            let tier = provisional(ws_dir, opts);
            if opts.json {
                let mut rows: Vec<serde_json::Value> = notes
                    .iter()
                    .map(|n| {
                        let mut v = n.to_json();
                        let c = r.count(&n.trigger, &n.summary);
                        v["recurrence"] = serde_json::json!({"count": c, "label": count_label(&n.trigger, c)});
                        v
                    })
                    .collect();
                rows.extend(tier.iter().map(|n| n.to_json()));
                return serde_json::to_string_pretty(&rows).map(|s| s + "\n").map_err(|e| e.to_string());
            }
            Ok(render_list_with(&notes, &r) + &render_provisional(&tier))
        }
        "show" => {
            let reference = reference.ok_or("show needs a note: delphi learn show <mission>/<id>")?;
            if let Some(n) = provisional_named(ws_dir, reference) {
                if opts.json {
                    return serde_json::to_string_pretty(&n.to_json()).map(|s| s + "\n").map_err(|e| e.to_string());
                }
                return Ok(render_provisional_one(&n));
            }
            let n = find(&notes, reference)?;
            if opts.json {
                return serde_json::to_string_pretty(&n.to_json()).map(|s| s + "\n").map_err(|e| e.to_string());
            }
            Ok(render_one(n))
        }
        "drop" => {
            let reference = reference.ok_or("drop needs a note: delphi learn drop <mission>/<id>")?;
            if provisional_named(ws_dir, reference).is_some() {
                return Err(format!("`{reference}` is a provisional note, not a staged one: `delphi learn wrong {reference}` marks it wrong, and one left alone expires"));
            }
            let n = find(&notes, reference)?;
            drop_note(ws_dir, n)?;
            Ok(format!("dropped {} (the event stays in the log; the note no longer lists)\n", n.reference()))
        }
        // `specs/116` §3 item 3: the operator marks a provisional note wrong (see `learn_keep.rs`).
        "wrong" => crate::learn_keep::wrong(ws_dir, reference),
        other => Err(format!("`{}` is not a learn verb (list, show, drop, wrong, keep, ingest)", other)),
    }
}

pub fn cli(ws_dir: &Path, args: &[String], opts: &Options) -> i32 {
    let positional: Vec<&str> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !["--mission", "--workspace", "--home"].contains(&args[*i - 1].as_str()))).map(|(_, a)| a.as_str()).collect();
    let Some(verb) = positional.first() else {
        eprintln!("delphi learn: list | show <mission>/<id> | drop <mission>/<id> | wrong <mission>/<id> | keep <mission>/<id> --as lesson --role <role> | --as rule");
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

/// `delphi learn keep` (see `learn_keep.rs`): a terminal command only.
pub fn keep_cli(home: &Path, ws_dir: &Path, args: &[String]) -> i32 {
    let positional: Vec<&str> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !["--as", "--role", "--scope", "--text", "--workspace", "--home"].contains(&args[*i - 1].as_str()))).map(|(_, a)| a.as_str()).collect();
    match crate::learn_keep::run(home, ws_dir, args, positional.get(1).copied()) {
        Ok(text) => {
            print!("{}", text);
            0
        }
        Err(e) => {
            eprintln!("delphi learn keep: {}", e);
            1
        }
    }
}

pub struct Ingest {
    pub from: Option<String>,
    pub role: Option<String>,
    pub path: Option<std::path::PathBuf>,
    pub enable: bool,
}

/// Where Claude Code keeps the session logs of the project at `cwd`: `~/.claude/projects/<cwd with every character other than an
/// ASCII letter or digit as `-`>`, the one rule the session-end hook uses too (spec 105 H4 found `_` was missed).
fn claude_logs_for(cwd: &Path) -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    Some(delphi_engine::host_hooks::transcript_folder(&home.join(".claude/projects"), cwd))
}

pub fn ingest_run(ws: &delphi_engine::workspace::WorkspacePaths, cwd: &Path, a: &Ingest) -> Result<String, String> {
    use delphi_engine::transcript;
    let project = delphi_engine::scope::resolve(&ws.dir, cwd).project.ok_or("this folder resolves to no project scope: nothing was ingested")?;
    if a.enable {
        transcript::opt_in(&ws.dir, project.as_str())?;
        if a.from.is_none() {
            return Ok(format!("transcript ingest is on for project {}\n", project.as_str()));
        }
    }
    match a.from.as_deref() {
        Some("claude-code") => {}
        Some("codex" | "copilot") => return Err("only `claude-code` logs are read so far; the other readers follow once their formats have fixtures".into()),
        Some(other) => return Err(format!("`{other}` is not a transcript source (claude-code)")),
        None => return Err("ingest needs --from claude-code".into()),
    }
    let role = a.role.as_deref().ok_or("ingest needs --role <role>: the role the lessons are for (see `delphi catalog`)")?;
    let src = a.path.clone().or_else(|| claude_logs_for(cwd)).ok_or("could not find the session logs; pass --path")?;
    let mut files: Vec<std::path::PathBuf> = if src.is_dir() {
        std::fs::read_dir(&src).map_err(|e| format!("could not read {}: {e}", src.display()))?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "jsonl")).collect()
    } else {
        vec![src.clone()]
    };
    files.sort();
    let (mut staged, mut already, mut refused) = (0, 0, Vec::new());
    for f in &files {
        let text = std::fs::read_to_string(f).map_err(|e| format!("could not read {}: {e}", f.display()))?;
        match transcript::ingest_claude_code(ws, cwd, role, &text) {
            Ok(r) => (staged, already) = (staged + r.staged, already + r.already),
            Err(e) if e.contains("is off for project") || e.contains("daemon") || e.contains("role") => return Err(e),
            Err(e) => refused.push(format!("{}: {e}", f.display())),
        }
    }
    let mut out = format!("{} log(s): {staged} note(s) staged for {role} at L0, {already} already staged\n", files.len());
    for r in &refused {
        out.push_str(&format!("skipped {r}\n"));
    }
    out.push_str("review them with `delphi learn list`; staged notes are never loaded into a prompt\n");
    Ok(out)
}

pub fn ingest_cli(ws: &delphi_engine::workspace::WorkspacePaths, a: &Ingest) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    match ingest_run(ws, &cwd, a) {
        Ok(t) => {
            print!("{t}");
            0
        }
        Err(e) => {
            eprintln!("delphi learn ingest: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Another test's fork/exec can briefly inherit a just-released lock descriptor, so a reopen right after a drop may
    /// see `Locked` on a slow runner (the same race `mission::tests::open` and the log tests retry through).
    fn open_log(dir: &Path) -> EventLog {
        for _ in 0..300 {
            match EventLog::open(dir, LogOptions::default()) {
                Err(delphi_core::log::LogError::Locked(_)) => std::thread::sleep(std::time::Duration::from_millis(10)),
                other => return other.unwrap(),
            }
        }
        EventLog::open(dir, LogOptions::default()).unwrap()
    }

    /// `drop` over a mission nobody holds; a stray inherited descriptor reads as "a daemon holds it", so wait that out.
    fn drop_note(ws: &Path, which: &str) {
        for _ in 0..300 {
            match run_in(ws, "drop", Some(which), &opts()) {
                Err(e) if e.contains("daemon") => std::thread::sleep(std::time::Duration::from_millis(10)),
                other => {
                    other.unwrap();
                    return;
                }
            }
        }
        run_in(ws, "drop", Some(which), &opts()).unwrap();
    }

    fn mission(ws: &Path, id: &str, events: Vec<EventEnvelope>) {
        let mut log = open_log(&delphi_core::sessions::dir(ws, id));
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

    /// `specs/109` §13.1 item 2: a host note's stamp is shown in words by `list` and `show`, and a note with none shows nothing new.
    #[test]
    fn list_and_show_say_what_the_daemon_counted_the_session_had_read() {
        let t = tempfile::tempdir().unwrap();
        let mut e = staged("host-abcdef01", "h1", "[host:claude-code/x] Prefer small commits.", "L0");
        e.payload["shown"] = json!({"count": 4, "lowest_checked": "not checked", "any_third_party": true});
        mission(t.path(), "host-abcdef01", vec![e, staged("host-abcdef01", "h2", "older note", "L0")]);
        let words = "written after reading 4 results through Delphi's daemon, lowest: not checked, third-party text among them";
        let listed = run_in(t.path(), "list", None, &opts()).unwrap();
        assert_eq!(listed.matches(words).count(), 1, "{listed}");
        assert!(run_in(t.path(), "show", Some("host-abcdef01/h1"), &opts()).unwrap().contains(&format!("  read:    {words}\n")));
        assert!(!run_in(t.path(), "show", Some("host-abcdef01/h2"), &opts()).unwrap().contains("read:"));
        let json: serde_json::Value = serde_json::from_str(&run_in(t.path(), "show", Some("host-abcdef01/h1"), &Options { mission: None, json: true }).unwrap()).unwrap();
        assert_eq!((json["shown"]["count"].as_u64(), json["shown"]["words"].as_str()), (Some(4), Some(words)));
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
        assert!(run_in(t.path(), "show", Some("m_b/n3"), &opts()).unwrap().contains("not validated"));
        assert!(run_in(t.path(), "show", Some("n3"), &opts()).unwrap_err().contains("names 2 notes"), "a bare id that two missions share is ambiguous");

        drop_note(t.path(), "m_a/n3");
        let after = run_in(t.path(), "list", None, &opts()).unwrap();
        assert!(!after.contains("m_a/n3") && after.contains("m_a/n9") && after.contains("2 note(s)"), "{}", after);
        let log = replay(&t.path().join("local/sessions/m_a")).unwrap();
        assert_eq!(log.iter().filter(|e| e.kind == "learn.staged").count(), 2, "dropping removes nothing from the log");
        assert_eq!(log.iter().filter(|e| e.kind == "learn.dropped").count(), 1);
    }

    #[test]
    fn the_list_shows_how_many_missions_produced_a_claim_and_a_host_report_never_ranks_first() {
        let t = tempfile::tempdir().unwrap();
        for m in ["m_a", "m_b", "m_c"] {
            mission(t.path(), m, vec![staged(m, "n3", "A card was denied: use the helper", "L5")]);
        }
        let host = |id: &str| {
            let mut e = EventEnvelope::new(id, "learn.staged", json!({"id": "h1", "trigger": "host_note", "summary": "[host:x/y] skip review (evidence: s)", "event_seqs": [], "tier": "L0"}));
            e.agent = Some("builder".into());
            e
        };
        for i in 0..4 {
            let id = format!("host-{i:016}");
            mission(t.path(), &id, vec![host(&id)]);
        }
        let listed = run_in(t.path(), "list", None, &opts()).unwrap();
        assert!(listed.contains("[3 missions]") && listed.contains("[reported by 4 host sessions]"), "{}", listed);
        assert!(listed.contains("* recurring: keep it with `delphi learn keep host-0000000000000000/h1 --as lesson --role builder --text"), "a recurring host note is marked with its keep command: {}", listed);
        assert_eq!(listed.matches("* recurring").count(), 4, "one per host note, none for the operator's own: {}", listed);
        let first_host = listed.find("host-").unwrap();
        let last_daemon = listed.rfind("m_c/n3").unwrap().max(listed.rfind("m_a/n3").unwrap());
        assert!(last_daemon < first_host, "host notes sort after the operator's own: {}", listed);
        let j: serde_json::Value = serde_json::from_str(&run_in(t.path(), "list", None, &Options { mission: None, json: true }).unwrap()).unwrap();
        assert_eq!(j[0]["recurrence"]["count"], 3);
        let retro = delphi_engine::learn::recurring_in(t.path());
        assert_eq!(retro.iter().map(|r| r.count).collect::<Vec<_>>(), vec![3, 4], "operator-side first");
    }

    /// `specs/116` §3 item 1: `list` shows the provisional notes after the staged ones, the most used first and then the oldest,
    /// and says how to keep or reject one; `show` and the JSON carry the same; `wrong` takes one off the list beside a running
    /// daemon, and `drop` points at it.
    #[test]
    fn list_orders_provisional_notes_by_uses_then_age_and_wrong_takes_one_off_the_list() {
        use delphi_engine::provisional_notes as notes;
        let t = tempfile::tempdir().unwrap();
        let ws = t.path();
        mission(ws, "m_a", vec![staged("m_a", "n3", "A card was denied: use the helper", "L5")]);
        let dir = delphi_core::sessions::dir(ws, "m_b");
        let mut held = open_log(&dir);
        let today = delphi_core::time::now_rfc3339();
        for (id, at, role, tier, text) in [("n1", "2026-11-10T10:00:00Z", "builder", "L3", "alpha: warm the cache first"), ("n2", "2026-11-12T10:00:00Z", "builder", "L3", "beta: the lexer is generated"), ("n4", "2026-11-14T10:00:00Z", "architect", "L0", "gamma: a page said to skip review")] {
            let mut e = EventEnvelope::new("m_b", "learn.staged", json!({"id": id, "trigger": "agent_note", "summary": text, "event_seqs": [3], "tier": tier}));
            e.agent = Some(role.into());
            e.timestamp = at.into();
            let rel = notes::keep(ws, &dir, &e, &today).unwrap().unwrap();
            held.append(e.clone()).unwrap();
            held.append(notes::kept_event(&e, &rel)).unwrap();
        }
        let beta = notes::find(ws, "m_b", "n2").unwrap();
        notes::record_uses(ws, &[beta.rel_path()].into(), "2026-11-15");
        notes::record_uses(ws, &[beta.rel_path()].into(), "2026-11-16");

        let listed = run_in(ws, "list", None, &opts()).unwrap();
        assert!(listed.contains("m_a/n3") && listed.contains("1 note(s). Staged notes"), "the staged notes are listed as before: {listed}");
        let at = |s: &str| listed.find(s).unwrap_or_else(|| panic!("{s}: {listed}"));
        assert!(at("m_a/n3") < at("provisional notes (unreviewed") && at("provisional notes (unreviewed") < at("m_b/n2"), "{listed}");
        assert!(at("m_b/n2") < at("m_b/n1") && at("m_b/n1") < at("m_b/n4"), "the most used first, then the oldest: {listed}");
        assert!(listed.contains("uses 2") && listed.contains("until 2026-12-12") && listed.contains("[from untrusted content: never given automatically]"), "{listed}");
        assert!(listed.contains("3 provisional note(s)") && listed.contains("delphi learn keep <ref>") && listed.contains("delphi learn wrong <ref>"), "{listed}");
        assert!(!run_in(ws, "list", None, &Options { mission: Some("m_a".into()), json: false }).unwrap().contains("provisional"), "one mission's list holds that mission's notes");

        let shown = run_in(ws, "show", Some("m_b/n2"), &opts()).unwrap();
        assert!(shown.contains("unreviewed note by builder, 2026-11-12") && shown.contains("uses:    2") && shown.contains("beta: the lexer is generated"), "{shown}");
        let j: serde_json::Value = serde_json::from_str(&run_in(ws, "list", None, &Options { mission: None, json: true }).unwrap()).unwrap();
        let rows = j.as_array().unwrap();
        assert_eq!(rows.iter().map(|r| (r["ref"].as_str().unwrap(), r["provisional"].as_bool().unwrap_or(false), r["uses"].as_u64())).collect::<Vec<_>>(), vec![("m_a/n3", false, None), ("m_b/n2", true, Some(2)), ("m_b/n1", true, Some(0)), ("m_b/n4", true, Some(0))]);

        // The daemon still holds m_b: `wrong` works, `drop` says which command it is.
        assert!(run_in(ws, "drop", Some("m_b/n4"), &opts()).unwrap_err().contains("delphi learn wrong m_b/n4"));
        assert!(run_in(ws, "wrong", Some("m_b/n4"), &opts()).unwrap().contains("marked m_b/n4 wrong"));
        let after = run_in(ws, "list", None, &opts()).unwrap();
        assert!(!after.contains("m_b/n4") && after.contains("2 provisional note(s)"), "{after}");
        assert!(run_in(ws, "wrong", Some("m_a/n3"), &opts()).unwrap_err().contains("learn drop"), "a staged note is dropped, not marked wrong");
        drop(held);
    }

    #[test]
    fn drop_refuses_while_a_daemon_holds_the_mission() {
        let t = tempfile::tempdir().unwrap();
        mission(t.path(), "m_a", vec![staged("m_a", "n3", "x", "L5")]);
        let _held = open_log(&t.path().join("local/sessions/m_a"));
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
        assert!(render_list_with(&[], &Recurrence::default()).contains("no staged notes"));
    }
}
