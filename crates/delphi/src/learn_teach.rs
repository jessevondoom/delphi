//! `delphi teach <role> "<text>" [--goal "<why>"] [--scope operator]`, `delphi learn lessons [<role>]` and
//! `delphi learn unteach <role> <lesson id>` (`specs/129` S3, §5.1 item 2, §7, §8).
//!
//! `teach` and `unteach` are terminal commands only, like `learn keep`: stdin and stdout must be a terminal, the operator reads
//! what will change and types `yes`, and there is no `--yes`. The operator's typing at a terminal is the card and the accept in one
//! act, so a taught lesson's origin is `operator-terminal`. Both write with the pinned write `learn keep` uses (live file, ratified
//! copy and pin together, under the scope's lock, a compare-and-swap on the file's hash), so they work while the daemon runs, and
//! every check runs again at the write: the redactor of `specs/102` §3.1, the per-lesson cap, the no-relaxation test, the operator
//! scope's privacy rules, the near-duplicate test and the Rule of 25.
//!
//! **The lessons log.** What was taught and what was unteached is recorded in a log of its own, one per scope beside the files it
//! describes: `<workspace>/local/lessons-log/` for workspace lessons and `<home>/operator/lessons-log/` for operator lessons. Only
//! these commands write it; the daemon never opens it, so a running daemon that holds a mission's log is no obstacle (a mission
//! log is held by the daemon while it works in it, and a fresh workspace has none to borrow). The log is opened, and its writer lock
//! held, before the lesson file is touched: if it cannot be opened, nothing is written. `learn.unlearned` keeps the removed lesson's
//! words, so teaching them again restores it exactly (`specs/129` OD-4).
//!
//! No agent, host, hook or daemon method reaches this code (`specs/129` §9 item 1). The limit is the charter's for §5.12: a process
//! running as the operator that can drive a terminal can do what the operator can.

use delphi_core::event::EventEnvelope;
use delphi_core::log::{replay, EventLog, LogError, LogOptions};
use delphi_engine::customize::{self, Lesson, LessonEntry, Provenance};
use std::path::{Path, PathBuf};

/// The event a `delphi teach` writes to the lessons log.
pub const TAUGHT: &str = "learn.taught";
/// The event a `delphi learn unteach` writes to the lessons log (`specs/129` §8 item 3).
pub const UNLEARNED: &str = "learn.unlearned";

/// Where the lessons log of `scope` lives: beside the lessons it describes.
pub fn ledger_dir(home: &Path, ws_dir: &Path, scope: &str) -> PathBuf {
    if scope == "operator" {
        home.join("operator/lessons-log")
    } else {
        ws_dir.join("local/lessons-log")
    }
}

/// Open the lessons log for writing. Only these commands write it, so a lock held means another of them is running: wait a moment.
fn open_ledger(dir: &Path) -> Result<EventLog, String> {
    for _ in 0..100 {
        match EventLog::open(dir, LogOptions::default()) {
            Err(LogError::Locked(_)) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Ok(log) => return Ok(log),
            Err(e) => return Err(format!("could not open the lessons log ({}): {}; nothing was written", dir.display(), e)),
        }
    }
    Err(format!("the lessons log ({}) is held by another teach or unteach; try again; nothing was written", dir.display()))
}

/// Every event of both lessons logs, oldest first within each.
pub fn ledger_events(home: &Path, ws_dir: &Path) -> Vec<EventEnvelope> {
    ["operator", "workspace"].iter().flat_map(|s| replay(&ledger_dir(home, ws_dir, s)).unwrap_or_default()).collect()
}

fn on_terminal() -> bool {
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

const VALUE_FLAGS: [&str; 4] = ["--goal", "--scope", "--workspace", "--home"];

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn positionals(args: &[String]) -> Vec<&str> {
    args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !VALUE_FLAGS.contains(&args[*i - 1].as_str()))).map(|(_, a)| a.as_str()).collect()
}

/// A word for a shell, single-quoted when it needs to be.
pub fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:@".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The command that teaches `l` to `role` again, exactly (`specs/129` §8 item 3).
pub fn teach_command(role: &str, scope: &str, l: &Lesson) -> String {
    format!("delphi teach {} {}{}", role, sh_quote(&l.text()), if scope == "operator" { " --scope operator" } else { "" })
}

/// The lessons a new one overlaps, named before the yes (`specs/129` §6 item 5); nothing when there are none.
pub fn render_overlaps(overlaps: &[(String, f64, String)]) -> String {
    if overlaps.is_empty() {
        return String::new();
    }
    let mut out = String::from("\nit overlaps what the role already carries:\n");
    for (id, score, title) in overlaps {
        out.push_str(&format!("  [{}] {} ({:.0}% of the same words)\n", id, title, score * 100.0));
    }
    out.push_str("Both will stay. To merge them instead, answer no, `delphi learn unteach` the old one and teach one lesson that says both.\n");
    out
}

/// The question before the write: with an overlap, the yes also confirms that both lessons stay.
pub fn question(plain: &str, overlaps: &[(String, f64, String)]) -> String {
    if overlaps.is_empty() {
        plain.to_string()
    } else {
        format!("{} Both lessons stay.", plain)
    }
}

// ── delphi teach ─────────────────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TeachArgs {
    pub role: String,
    pub text: String,
    pub goal: Option<String>,
    pub scope: String,
}

fn teach_usage() -> String {
    "usage: delphi teach <role> \"T: <when it applies> D: <what to do> [F: <what would show it wrong>] [G: <why>]\" [--goal \"<why>\"] [--scope operator]".into()
}

/// Read the command line after `teach`.
pub fn parse_teach(args: &[String]) -> Result<TeachArgs, String> {
    if args.iter().any(|a| a == "--yes") {
        return Err("`teach` has no --yes: you type yes at the terminal, after reading what will be written".into());
    }
    let pos = positionals(args);
    let (Some(role), Some(text)) = (pos.first(), pos.get(1)) else { return Err(teach_usage()) };
    if pos.len() > 2 {
        return Err(format!("put the lesson in one pair of quotes; {}", teach_usage()));
    }
    let scope = flag(args, "--scope").unwrap_or_else(|| "workspace".into());
    if !["workspace", "operator"].contains(&scope.as_str()) {
        return Err("`--scope` is workspace or operator".into());
    }
    Ok(TeachArgs { role: role.to_string(), text: text.to_string(), goal: flag(args, "--goal"), scope })
}

/// What `teach` will write, worked out and checked before the terminal is asked anything.
#[derive(Debug, Clone)]
pub struct TeachPlan {
    pub args: TeachArgs,
    /// The lesson as it will be written: its goal attached, its words redacted.
    pub lesson: Lesson,
    pub id: String,
    pub block: String,
    pub file: String,
    pub overlaps: Vec<(String, f64, String)>,
}

fn teach_provenance(ratified_from: String) -> Provenance {
    Provenance { ratified_from, original_tier: "L5".into(), tool: "operator".into(), date: delphi_core::bundles::today(), origin: customize::ORIGIN_TERMINAL.into() }
}

/// Plan a teach: every refusal that does not need the operator happens here, and nothing is written.
pub fn plan_teach(home: &Path, ws_dir: &Path, a: &TeachArgs) -> Result<TeachPlan, String> {
    customize::check_role(home, ws_dir, &a.role)?;
    let text: String = a.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lesson = customize::redacted(&customize::with_goal(customize::parse_lesson(&text)?, a.goal.as_deref())?);
    let (id, block) = customize::lesson_preview(home, ws_dir, &a.scope, &a.role, &lesson, &teach_provenance("teach".into()))?;
    let overlaps = customize::lesson_overlaps(home, ws_dir, &a.role, &lesson);
    Ok(TeachPlan { args: a.clone(), lesson, id, block, file: customize::lesson_file(&a.scope, &a.role), overlaps })
}

/// What the operator reads before typing yes.
pub fn render_teach(p: &TeachPlan) -> String {
    let scope = if p.args.scope == "operator" { "an operator lesson: it follows you into every workspace" } else { "a workspace lesson" };
    format!(
        "teach {} {} ({})\n  goal:     {}\n  origin:   {} (typed by you)\n  written:  user_ratified, loaded into every prompt of {} from its next pack\n\nwrite to {}:\n{}\n{}",
        p.args.role,
        p.id,
        scope,
        p.lesson.goal.as_deref().unwrap_or(customize::GOAL_NONE),
        customize::ORIGIN_TERMINAL,
        p.args.role,
        p.file,
        p.block.trim_end(),
        render_overlaps(&p.overlaps)
    )
}

fn pack_hash(home: &Path, ws_dir: &Path, role: &str) -> Option<String> {
    let ws = delphi_engine::workspace::WorkspacePaths { home: home.to_path_buf(), dir: ws_dir.to_path_buf() };
    delphi_engine::mcp_packs::Packs::new(ws).compact_pack_hash(role).ok().filter(|h| !h.is_empty())
}

/// Write the planned lesson and record it. The lessons log is opened first, so a log that cannot record it stops the command
/// before the lesson is written; the write itself runs every check again.
pub fn apply_teach(home: &Path, ws_dir: &Path, p: &TeachPlan) -> Result<String, String> {
    let a = &p.args;
    let mut log = open_ledger(&ledger_dir(home, ws_dir, &a.scope))?;
    let before = pack_hash(home, ws_dir, &a.role);
    let prov = teach_provenance(format!("teach/{}", log.next_seq()));
    let kept = customize::apply_lesson(home, ws_dir, &a.scope, &a.role, &p.lesson, &prov)?;
    let payload = serde_json::json!({
        "role": a.role, "scope": a.scope, "rule": kept.id, "file": kept.file, "pin": kept.pin, "text": p.lesson.text(),
        "goal": p.lesson.goal.as_deref().unwrap_or(customize::GOAL_NONE), "origin": prov.origin, "date": prov.date,
    });
    log.append(EventEnvelope::new("lessons", TAUGHT, payload)).map_err(|e| format!("{} was written as {} but the lessons log could not record it: {}", kept.file, kept.id, e))?;
    let hash = match (before, pack_hash(home, ws_dir, &a.role)) {
        (Some(b), Some(n)) if b != n => format!(" {}'s pack_hash is now {} (was {}).", a.role, n, b),
        (_, Some(n)) => format!(" {}'s pack_hash is {}.", a.role, n),
        _ => String::new(),
    };
    Ok(format!("taught {} {} in {} as user_ratified (you typed yes); it applies from the role's next pack.{}\nUnteach it with: delphi learn unteach {} {}\n", a.role, kept.id, kept.file, hash, a.role, kept.id))
}

/// `delphi teach`: plan, then a terminal, then the operator's typed yes, then the write.
pub fn teach_run(home: &Path, ws_dir: &Path, args: &[String]) -> Result<String, String> {
    let a = parse_teach(args)?;
    let p = plan_teach(home, ws_dir, &a)?;
    if !on_terminal() {
        return Err("teach needs a terminal: a lesson is put in front of agents, and only you can type the yes; nothing was written".into());
    }
    print!("{}", render_teach(&p));
    if !crate::retro_cmd::confirm(&question("Teach it?", &p.overlaps)) {
        return Err("not confirmed; nothing was written".into());
    }
    apply_teach(home, ws_dir, &p)
}

pub fn teach_cli(home: &Path, ws_dir: &Path, args: &[String]) -> i32 {
    match teach_run(home, ws_dir, args) {
        Ok(text) => {
            print!("{}", text);
            0
        }
        Err(e) => {
            eprintln!("delphi teach: {}", e);
            1
        }
    }
}

// ── delphi learn unteach ─────────────────────────────────────────────────────────────────────────────────────────────────

/// The lesson an unteach would remove, from the ratified copy the pack reads. Writes nothing.
pub fn plan_unteach(home: &Path, ws_dir: &Path, role: &str, id: &str) -> Result<LessonEntry, String> {
    customize::check_role(home, ws_dir, role)?;
    let shown: String = id.chars().filter(|c| !c.is_control()).take(32).collect();
    customize::lessons(home, ws_dir, role).into_iter().find(|e| e.id == id).ok_or_else(|| format!("{} has no lesson `{}` (see `delphi learn lessons {}`); nothing was written", role, shown, role))
}

pub fn render_unteach(role: &str, e: &LessonEntry) -> String {
    format!(
        "unteach {} {} ({} scope)\n  {}\n  goal:   {}\n  origin: {}\n\nIt leaves the role's next pack. Its words are kept in the lessons log, so teaching them again restores it.\n",
        role,
        e.id,
        e.scope,
        e.lesson.text(),
        e.lesson.goal.as_deref().unwrap_or(customize::GOAL_NONE),
        e.origin.as_deref().unwrap_or("not recorded")
    )
}

/// Remove the lesson and record `learn.unlearned` with its id, the date, its goal and its words (`specs/129` §8 item 3). The log is
/// opened first; if it then cannot record the removal, the error carries the words so nothing is lost.
pub fn apply_unteach(home: &Path, ws_dir: &Path, role: &str, id: &str) -> Result<String, String> {
    // Found first, so a lesson that is not there opens no log.
    plan_unteach(home, ws_dir, role, id)?;
    let scope = customize::lesson_scope(home, ws_dir, role, id).ok_or_else(|| format!("{} has no lesson `{}`; nothing was written", role, id))?;
    let mut log = open_ledger(&ledger_dir(home, ws_dir, scope))?;
    let r = customize::remove_lesson(home, ws_dir, role, id)?;
    let l = &r.entry.lesson;
    let payload = serde_json::json!({
        "role": role, "scope": scope, "rule": id, "file": r.file, "pin": r.pin, "date": delphi_core::bundles::today(),
        "goal": l.goal.as_deref().unwrap_or(customize::GOAL_NONE), "text": l.text(), "origin": r.entry.origin, "ratified_from": r.entry.ratified_from,
    });
    let again = teach_command(role, scope, l);
    log.append(EventEnvelope::new("lessons", UNLEARNED, payload)).map_err(|e| format!("{} {} was removed from {} but the lessons log could not record it ({}); to restore it: {}", role, id, r.file, e, again))?;
    Ok(format!("unteached {} {}: it left {} and is not in the role's next pack. Recorded as {} in the lessons log.\nTo teach it again: {}\n", role, id, r.file, UNLEARNED, again))
}

/// `delphi learn unteach <role> <lesson id>`: a terminal and a typed yes, as `teach`.
pub fn unteach_run(home: &Path, ws_dir: &Path, args: &[String]) -> Result<String, String> {
    if args.iter().any(|a| a == "--yes") {
        return Err("`learn unteach` has no --yes: you type yes at the terminal".into());
    }
    let pos = positionals(args);
    let (Some(role), Some(id)) = (pos.get(1), pos.get(2)) else { return Err("usage: delphi learn unteach <role> <lesson id> (see `delphi learn lessons`)".into()) };
    let e = plan_unteach(home, ws_dir, role, id)?;
    if !on_terminal() {
        return Err("learn unteach needs a terminal: only you can take a lesson away from a role; nothing was written".into());
    }
    print!("{}", render_unteach(role, &e));
    if !crate::retro_cmd::confirm("Unteach it?") {
        return Err("not confirmed; nothing was written".into());
    }
    apply_unteach(home, ws_dir, role, id)
}

// ── delphi learn lessons ─────────────────────────────────────────────────────────────────────────────────────────────────

/// The roles that carry a lesson at either scope, by the ratified copies.
fn roles_with_lessons(home: &Path, ws_dir: &Path) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    for root in [home.join("operator/ratified/memory/agents"), ws_dir.join("local/ratified/memory/agents")] {
        for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
            if e.path().join("lessons.md").is_file() {
                out.insert(e.file_name().to_string_lossy().to_string());
            }
        }
    }
    out.into_iter().collect()
}

/// Whole days from `date` (`YYYY-MM-DD`) to today.
fn age_days(date: &str) -> Option<i64> {
    let then = delphi_core::time::parse_rfc3339(&format!("{}T00:00:00Z", date.get(..10)?))?;
    let now = delphi_core::time::parse_rfc3339(&format!("{}T00:00:00Z", delphi_core::bundles::today()))?;
    Some((now - then) / 86_400)
}

fn entry_json(role: &str, e: &LessonEntry) -> serde_json::Value {
    serde_json::json!({
        "role": role, "id": e.id, "scope": e.scope, "title": e.title, "text": e.lesson.text(), "goal": e.lesson.goal.as_deref().unwrap_or(customize::GOAL_NONE),
        "origin": e.origin, "date": e.date, "age_days": e.date.as_deref().and_then(age_days), "ratified_from": e.ratified_from, "tool": e.tool,
        "served": serde_json::Value::Null,
    })
}

/// `delphi learn lessons [<role>]` (`specs/129` §7): what each role carries, with its provenance and age. The counts of serves
/// arrive with S7; until then the line says they are not counted yet.
pub fn lessons_run(home: &Path, ws_dir: &Path, role: Option<&str>, json: bool) -> Result<String, String> {
    let roles = match role {
        Some(r) => {
            customize::check_role(home, ws_dir, r).map_err(|e| e.replace("; nothing was written", ""))?;
            vec![r.to_string()]
        }
        None => roles_with_lessons(home, ws_dir).into_iter().filter(|r| !customize::lessons(home, ws_dir, r).is_empty()).collect(),
    };
    let unlearned: Vec<EventEnvelope> = ledger_events(home, ws_dir).into_iter().filter(|e| e.kind == UNLEARNED).collect();
    if json {
        let rows: Vec<serde_json::Value> = roles.iter().flat_map(|r| customize::lessons(home, ws_dir, r).into_iter().map(move |e| entry_json(r, &e))).collect();
        return serde_json::to_string_pretty(&rows).map(|s| s + "\n").map_err(|e| e.to_string());
    }
    let mut out = String::new();
    for r in &roles {
        let carried = customize::lessons(home, ws_dir, r);
        out.push_str(&format!("{}: {} lesson(s){}\n", r, carried.len(), pack_hash(home, ws_dir, r).map(|h| format!(", pack_hash {}", h)).unwrap_or_default()));
        for e in &carried {
            let age = match (e.date.as_deref(), e.date.as_deref().and_then(age_days)) {
                (Some(d), Some(n)) => format!("since {} ({} day(s))", d, n),
                (Some(d), None) => format!("since {}", d),
                _ => "date not recorded".into(),
            };
            out.push_str(&format!("  {}  {:<9}  {}\n", e.id, e.scope, e.title));
            out.push_str(&format!("      {}\n", e.lesson.text()));
            out.push_str(&format!("      goal: {} · origin: {} · {} · from {}\n", e.lesson.goal.as_deref().unwrap_or(customize::GOAL_NONE), e.origin.as_deref().unwrap_or("not recorded"), age, e.ratified_from.as_deref().unwrap_or("a hand edit")));
            out.push_str("      served: not counted yet\n");
        }
        let gone: Vec<&EventEnvelope> = unlearned.iter().filter(|u| u.payload["role"].as_str() == Some(r.as_str())).collect();
        for u in gone.iter().rev().take(5) {
            let text = u.payload["text"].as_str().unwrap_or("");
            if let Some(l) = customize::parse_lesson(text).ok().filter(|l| !carried.iter().any(|e| e.lesson == *l)) {
                out.push_str(&format!("  unteached {} on {}; to teach it again: {}\n", u.payload["rule"].as_str().unwrap_or("?"), u.payload["date"].as_str().unwrap_or("?"), teach_command(r, u.payload["scope"].as_str().unwrap_or("workspace"), &l)));
            }
        }
    }
    if roles.is_empty() {
        out.push_str("no role carries a lesson yet: teach one with `delphi teach <role> \"T: … D: …\"`, or keep a note with `delphi learn keep`\n");
    } else {
        out.push_str("Unteach one with `delphi learn unteach <role> <id>` (it works while the daemon runs). Serves and repeats are counted from a later release.\n");
    }
    Ok(out)
}

#[cfg(test)]
#[path = "learn_teach_tests.rs"]
mod tests;
