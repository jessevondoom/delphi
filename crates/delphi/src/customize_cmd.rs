//! `delphi customize note|rule|skill|profile`: the operator writes to the stores directly (`specs/031` §3.5, slice L5).
//!
//! Each verb needs a terminal: a rule, a skill or a profile line is put in front of agents, so a script or an agent's
//! shell cannot write one (the same rule as `delphi retro accept`). A rule, a skill and a profile line also show what
//! will be written and ask for a typed `yes` (`--yes` skips only that question). A rule goes through the checks a
//! ratified proposal gets; the operator's own wording does not make it exempt from the no-relaxation test or the caps.
//! `note` stages a note at tier `L5`: it is kept for review and never loaded into a prompt.

use delphi_core::event::EventEnvelope;
use delphi_core::log::EventLog;
use delphi_engine::customize;
use std::path::{Path, PathBuf};

pub struct Options {
    pub home: PathBuf,
}

const VALUE_FLAGS: [&str; 10] = ["--file", "--workspace", "--home", "--scope", "--why", "--replace", "--remove", "--agents", "--description", "--mission"];

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn positionals(args: &[String]) -> Vec<&str> {
    args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !VALUE_FLAGS.contains(&args[*i - 1].as_str()))).map(|(_, a)| a.as_str()).collect()
}

fn on_terminal() -> bool {
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

fn need_terminal(verb: &str) -> Result<(), String> {
    if on_terminal() {
        Ok(())
    } else {
        Err(format!("{} needs a terminal: what it writes is put in front of agents, and a script cannot write it for you", verb))
    }
}

fn sure(args: &[String], question: &str) -> Result<(), String> {
    if args.iter().any(|a| a == "--yes") || crate::retro_cmd::confirm(question) {
        Ok(())
    } else {
        Err("not confirmed; nothing was changed".into())
    }
}

/// The first mission, newest first, whose log is free to write: a daemon holds the one it is working in.
fn open_any_log(ws_dir: &Path, only: Option<&str>) -> Result<(String, EventLog), String> {
    let ids: Vec<String> = match only {
        Some(m) => vec![m.to_string()],
        None => crate::missions::list(ws_dir).into_iter().map(|r| r.id).collect(),
    };
    if ids.is_empty() {
        return Err("there is no mission to attach a note to yet; start one with `delphi run \"<task>\"`, or write a rule or profile line instead".into());
    }
    let mut last = String::new();
    for id in ids {
        if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') || !ws_dir.join("local/missions").join(&id).is_dir() {
            return Err(format!("no mission `{}` in this workspace (see `delphi missions`)", id));
        }
        match crate::retro_cmd::open_log(ws_dir, &id) {
            Ok(log) => return Ok((id, log)),
            Err(e) => last = e,
        }
    }
    Err(last)
}

pub fn note(ws_dir: &Path, text: &str, mission: Option<&str>) -> Result<String, String> {
    customize::note_event("m", text, 1)?;
    let (id, mut log) = open_any_log(ws_dir, mission)?;
    let seq = log.next_seq();
    let mut e: EventEnvelope = customize::note_event(&id, text, seq)?;
    e.agent = None;
    log.append(e).map_err(|e| format!("could not write to the log of {}: {}", id, e))?;
    Ok(format!("noted as {}/n{}: kept for review in `delphi learn list`, never loaded into a prompt\n", id, seq))
}

pub fn rule(ws_dir: &Path, opts: &Options, args: &[String], text: Option<&str>) -> Result<String, String> {
    let scope = flag(args, "--scope").unwrap_or_else(|| "workspace".into());
    let why = flag(args, "--why").unwrap_or_default();
    let (op, target) = match (flag(args, "--replace"), flag(args, "--remove")) {
        (Some(_), Some(_)) => return Err("use --replace or --remove, not both".into()),
        (Some(t), None) => ("merge", t),
        (None, Some(t)) => ("demote", t),
        (None, None) => ("add", String::new()),
    };
    let p = customize::rule_change(op, &scope, text.unwrap_or(""), &why, &target)?;
    let blocked = customize::rule_problems(&opts.home, ws_dir, &p);
    if !blocked.is_empty() {
        return Err(format!("it cannot be written as it stands:\n{}", blocked.iter().map(|b| format!("  - {}\n", b)).collect::<String>()));
    }
    need_terminal("customize rule")?;
    println!("{} a {} rule{}: {}", op, p.scope, if target.is_empty() { String::new() } else { format!(" ({})", target) }, p.text);
    sure(args, "Write it?")?;
    let done = customize::apply_rule(&opts.home, ws_dir, &p)?;
    Ok(format!("written to {} and ratified (you wrote it); it applies from the next task\n", done.file))
}

pub fn profile(ws_dir: &Path, opts: &Options, args: &[String], line: Option<&str>) -> Result<String, String> {
    let path = opts.home.join("operator/profile.md");
    if let Some(file) = flag(args, "--file") {
        need_terminal("customize profile")?;
        let new = std::fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {}", file, e))?;
        sure(args, &format!("Replace your profile with {} ({} bytes)?", file, new.len()))?;
        customize::edit_profile(&opts.home, ws_dir, |_| new.clone())?;
        return Ok("profile replaced and ratified (you wrote it)\n".into());
    }
    let Some(line) = line else {
        return std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {}", path.display(), e));
    };
    need_terminal("customize profile")?;
    println!("add to your profile: {}", line);
    sure(args, "Write it?")?;
    let text = customize::edit_profile(&opts.home, ws_dir, |c| customize::with_line(c, line))?;
    let over = text.len().saturating_sub(delphi_engine::workspace::PROFILE_SUMMARY_BYTES);
    Ok(format!("added to your profile and ratified (you wrote it){}\n", if over > 0 { format!("; only the first {} bytes reach a prompt, so move what matters most up", delphi_engine::workspace::PROFILE_SUMMARY_BYTES) } else { String::new() }))
}

pub fn skill(ws_dir: &Path, opts: &Options, args: &[String], dir: &Path) -> Result<String, String> {
    let name = crate::skills_cmd::dir_name(dir);
    let text = std::fs::read_to_string(dir.join("SKILL.md")).map_err(|e| format!("cannot read {}/SKILL.md: {}", dir.display(), e))?;
    let agents: Vec<String> = flag(args, "--agents").map(|a| a.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    let imported = crate::skills_cmd::import_text(&text, &name, &crate::skills_cmd::ImportOpts { agents, description: flag(args, "--description") }).map_err(|errs| errs.iter().map(|e| format!("  - {}\n", e)).collect::<String>()).map_err(|e| format!("`{}` cannot be written:\n{}", name, e))?;
    let relax = delphi_bundle::no_relaxation(&imported.text);
    if !relax.is_empty() {
        return Err(format!("`{}` reads as an instruction that relaxes a rule (`{}`); the no-relaxation test refuses it", name, relax[0].1));
    }
    if let Some(existing) = crate::skills_cmd::find_skill(ws_dir, &name) {
        return Err(format!("a skill named `{}` already exists ({}); use another directory name", name, existing.display()));
    }
    let pkg = crate::skills_cmd::package_files(dir)?;
    need_terminal("customize skill")?;
    println!("skill `{}`: {} ({} other file(s))", name, imported.text.lines().find(|l| l.starts_with("description:")).unwrap_or(""), pkg.files.len());
    for n in &imported.notes {
        println!("  note: {}", n);
    }
    for s in &pkg.skipped {
        println!("  not copied: {}", s);
    }
    sure(args, "Write it?")?;
    customize::install_skill(&opts.home, ws_dir, &name, &imported.text, &pkg.files)?;
    Ok(format!("skill `{}` written to operator/skills/ and ratified (you wrote it)\n", name))
}

pub fn cli(ws_dir: &Path, args: &[String], opts: &Options) -> i32 {
    let pos = positionals(args);
    let Some(verb) = pos.first().copied() else {
        eprintln!("usage: delphi customize note \"<text>\" | rule \"<text>\" [--scope workspace|operator] [--why \"…\"] [--replace <ID> | --remove <ID>] | skill <dir> --agents <role,role> | profile [\"<line>\" | --file <path>]");
        return 2;
    };
    let result = match verb {
        "note" => match pos.get(1) {
            Some(t) => need_terminal("customize note").and_then(|_| note(ws_dir, t, flag(args, "--mission").as_deref())),
            None => Err("note needs the text: delphi customize note \"<what to remember>\"".into()),
        },
        "rule" if pos.get(1).is_some() || flag(args, "--remove").is_some() => rule(ws_dir, opts, args, pos.get(1).copied()),
        "rule" => Err("rule needs the text: delphi customize rule \"<the rule>\" (or --remove <ID>)".into()),
        "skill" => match pos.get(1) {
            Some(d) => skill(ws_dir, opts, args, Path::new(d)),
            None => Err("skill needs a directory holding SKILL.md: delphi customize skill <dir> --agents <role,role>".into()),
        },
        "profile" => profile(ws_dir, opts, args, pos.get(1).copied()),
        other => Err(format!("`{}` is not a customize verb (note, rule, skill, profile)", other)),
    };
    match result {
        Ok(text) => {
            print!("{}", text);
            0
        }
        Err(e) => {
            eprintln!("delphi customize: {}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::log::replay;

    fn ws_with_mission() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("local/missions/m1");
        std::fs::create_dir_all(&dir).unwrap();
        let mut log = EventLog::open(&dir, delphi_core::log::LogOptions::default()).unwrap();
        log.append(EventEnvelope::new("m1", "mission.started", serde_json::json!({}))).unwrap();
        d
    }

    #[test]
    fn a_note_is_staged_at_l5_and_listed_by_the_same_reader_as_every_other_note() {
        let d = ws_with_mission();
        let said = note(d.path(), "Prefer small commits.", None).unwrap();
        assert!(said.contains("m1/n2"), "{}", said);
        let events = replay(&d.path().join("local/missions/m1")).unwrap();
        let live = delphi_engine::learn::live_notes("m1", &events);
        assert_eq!((live.len(), live[0].tier.as_str(), live[0].trigger.as_str(), live[0].summary.as_str()), (1, "L5", "operator_note", "Prefer small commits."));
        assert!(note(d.path(), "x", Some("../m1")).is_err() && note(d.path(), "x", Some("nope")).is_err());
    }

    #[test]
    fn a_note_with_no_mission_says_what_to_do_instead() {
        let d = tempfile::tempdir().unwrap();
        assert!(note(d.path(), "x", None).unwrap_err().contains("no mission"));
    }

    #[test]
    fn a_script_cannot_write_a_rule_a_skill_or_a_profile_line() {
        // The tests run without a terminal, which is the case being checked.
        let d = tempfile::tempdir().unwrap();
        let o = Options { home: d.path().to_path_buf() };
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(rule(d.path(), &o, &args(&["rule", "Run the formatter first."]), Some("Run the formatter first.")).unwrap_err().contains("needs a terminal"));
        assert!(profile(d.path(), &o, &args(&["profile", "--yes"]), Some("I like tabs")).unwrap_err().contains("needs a terminal"), "--yes does not stand in for a terminal");
        assert!(cli(d.path(), &args(&["note", "x"]), &o) == 1);
    }

    #[test]
    fn a_rule_that_relaxes_governance_is_refused_before_anything_is_asked() {
        let d = tempfile::tempdir().unwrap();
        let o = Options { home: d.path().to_path_buf() };
        let e = rule(d.path(), &o, &["rule".to_string()], Some("Skip the approval cards when in a hurry.")).unwrap_err();
        assert!(e.contains("relaxes"), "{}", e);
    }
}
