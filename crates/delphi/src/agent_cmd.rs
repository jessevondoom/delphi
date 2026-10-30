//! `delphi agent promote|nerf|grades` (`specs/056` §4): move the class of model a role is assigned.
//!
//! A grade lives in the ratified `[grades]` table of `config.toml`. `promote` and `nerf` edit the live file, so the change is a
//! pending Red-tier proposal that shows its diff and does nothing until the operator `/ratify`s it; the daemon reads `[grades]`
//! only from the ratified copy. Delphi may suggest a grade; it never applies one. The commands are refused inside an agent seat.

use delphi_adapter::config::{Config, GRADE_LIMIT};
use delphi_engine::{grades, ratify};
use std::path::Path;
use toml_edit::{value, DocumentMut, Item, Table};

/// The live text with `role` (and `kind`) moved by `delta` within the limit: `(new text, from, to)`.
pub fn change(live: &str, role: &str, kind: Option<&str>, delta: i32) -> Result<(String, i32, i32), String> {
    let mut doc: DocumentMut = live.parse().map_err(|e| format!("config.toml is not valid TOML: {e}"))?;
    if !doc.contains_key("grades") {
        doc["grades"] = Item::Table(Table::new());
    }
    let grades = doc["grades"].as_table_mut().ok_or("[grades] must be a table")?;
    let current = match grades.get(role) {
        Some(Item::Value(v)) if v.is_integer() => (v.as_integer().unwrap_or(0) as i32, None),
        Some(Item::Table(t)) => (t.get("default").and_then(|v| v.as_integer()).unwrap_or(0) as i32, Some(t.clone())),
        None => (0, None),
        Some(_) => return Err(format!("[grades] {role} must be a whole number or a table")),
    };
    let from = match (kind, &current.1) {
        (Some(k), Some(t)) => t.get(k).and_then(|v| v.as_integer()).map(|n| n as i32).unwrap_or(current.0),
        _ => current.0,
    };
    let to = (from + delta).clamp(-GRADE_LIMIT, GRADE_LIMIT);
    if to == from {
        return Err(format!("{role}{} is already at {from}, the limit of ±{GRADE_LIMIT}", kind.map(|k| format!(" ({k})")).unwrap_or_default()));
    }
    match kind {
        None => match grades.get_mut(role) {
            Some(Item::Table(t)) => t["default"] = value(to as i64),
            _ => grades[role] = value(to as i64),
        },
        Some(k) => {
            if !matches!(grades.get(role), Some(Item::Table(_))) {
                let mut t = Table::new();
                if current.0 != 0 {
                    t["default"] = value(current.0 as i64);
                }
                grades.insert(role, Item::Table(t));
            }
            grades[role][k] = value(to as i64);
        }
    }
    Ok((doc.to_string(), from, to))
}

fn known_role(ws_dir: &Path, role: &str) -> bool {
    delphi_engine::models::ClassRules::load(ws_dir).map(|r| r.roles.contains_key(role)).unwrap_or(false) || ["base", "core", "team"].iter().any(|z| ws_dir.join(z).join("agents").join(role).is_dir())
}

fn grades_block(text: &str) -> String {
    let v: toml::Value = toml::from_str(text).unwrap_or(toml::Value::Table(Default::default()));
    v.get("grades").map(|g| format!("[grades]\n{}", toml::to_string(g).unwrap_or_default())).unwrap_or_default()
}

pub fn cli(ws_dir: &Path, args: &[String], now: i64) -> i32 {
    let pos: Vec<&str> = {
        let mut out = Vec::new();
        let mut skip = false;
        for a in args.iter().skip(2) {
            if skip {
                skip = false;
            } else if matches!(a.as_str(), "--kind" | "--reason" | "--workspace" | "--home") {
                skip = true;
            } else if !a.starts_with("--") {
                out.push(a.as_str());
            }
        }
        out
    };
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    match (args.get(1).map(String::as_str), pos.as_slice()) {
        (Some(verb @ ("promote" | "nerf")), [role]) => {
            if crate::xenia_terminal::inside_an_agent_seat() {
                eprintln!("delphi agent {verb}: refused; a grade is the operator's to propose and this process was started by an agent seat");
                return 2;
            }
            if !known_role(ws_dir, role) {
                eprintln!("delphi agent {verb}: no role `{role}` (see `delphi capabilities`)");
                return 2;
            }
            let kind = flag("--kind");
            let path = ws_dir.join("config.toml");
            let live = std::fs::read_to_string(&path).unwrap_or_default();
            let (new, from, to) = match change(&live, role, kind.as_deref(), if verb == "promote" { 1 } else { -1 }) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("delphi agent {verb}: {e}");
                    return 1;
                }
            };
            let reason = flag("--reason").unwrap_or_default();
            if let Err(e) = std::fs::write(&path, &new) {
                eprintln!("delphi agent {verb}: could not write config.toml: {e}");
                return 4;
            }
            let _ = grades::record(ws_dir, role, kind.as_deref(), from, to, &reason, now);
            println!("{}", ratify::line_diff(&grades_block(&live), &grades_block(&new)));
            println!("{role}{}: grade {from} -> {to}. This is a proposal: nothing changes until you /ratify it.", kind.map(|k| format!(" ({k})")).unwrap_or_default());
            0
        }
        (Some("grades"), []) => {
            let live = std::fs::read_to_string(ws_dir.join("config.toml")).unwrap_or_default();
            let effective = delphi_engine::boot::effective_config(ws_dir, &live).ok().and_then(|(t, _)| Config::parse(&t).ok()).unwrap_or_default();
            let live_cfg = Config::parse(&live).unwrap_or_default();
            let mut roles: Vec<&String> = effective.grades.roles.keys().chain(live_cfg.grades.roles.keys()).collect();
            roles.sort();
            roles.dedup();
            if roles.is_empty() {
                println!("no grades set; every role is at 0. `delphi agent promote <role>` or `nerf <role>` proposes one.");
            }
            for r in roles {
                let (a, b) = (effective.grades.roles.get(r).cloned().unwrap_or_default(), live_cfg.grades.roles.get(r).cloned().unwrap_or_default());
                let pending = if a != b { format!("  (pending /ratify: {})", b.role) } else { String::new() };
                let kinds: String = a.kinds.iter().map(|(k, n)| format!(" {k}={n}")).collect();
                println!("{r}: {}{kinds}{pending}  [{}]", a.role, grades::evidence_line(ws_dir, r).unwrap_or_else(|| "no measured results yet".into()));
            }
            0
        }
        _ => {
            eprintln!("usage: delphi agent promote|nerf <role> [--kind <task kind>] [--reason <text>] | grades [--workspace <name>]");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_edits_only_the_grades_table_and_stays_in_bounds() {
        let (t, from, to) = change("[workspace]\nname = \"w\" # mine\n", "architect", None, 1).unwrap();
        assert_eq!((from, to), (0, 1));
        assert!(t.contains("name = \"w\" # mine") && t.contains("[grades]") && t.contains("architect = 1"), "{t}");
        let (t, _, _) = change(&t, "builder", Some("docs"), -1).unwrap();
        let g = Config::parse(&t).unwrap().grades;
        assert_eq!((g.of("architect", None), g.of("builder", Some("docs")), g.of("builder", None)), (1, -1, 0));
        let (t, from, to) = change(&t, "architect", None, 1).unwrap();
        assert_eq!((from, to), (1, 2));
        assert!(change(&t, "architect", None, 1).is_err(), "no further than the limit");
    }

    #[test]
    fn a_role_grade_survives_becoming_a_table() {
        let (t, _, _) = change("[grades]\nbuilder = -1\n", "builder", Some("docs"), -1).unwrap();
        let g = Config::parse(&t).unwrap().grades;
        assert_eq!((g.of("builder", None), g.of("builder", Some("docs"))), (-1, -2));
    }

    // specs/056 §8: a promote does nothing until ratified, and a plain edit of the live file (an agent's, say) cannot move a grade.
    #[test]
    fn a_proposed_grade_is_inert_until_it_is_ratified() {
        use delphi_core::lock::{self, access_blocks, hash, LockFile};
        let ws = tempfile::tempdir().unwrap();
        let ratified = "[grades]\narchitect = 1\n";
        std::fs::create_dir_all(ws.path().join("local/ratified")).unwrap();
        let blocks = access_blocks(ratified);
        std::fs::write(ws.path().join("local/ratified").join(ratify::CONFIG_COPY), &blocks).unwrap();
        let mut lockfile = LockFile::default();
        lockfile.files.insert(ratify::CONFIG_KEY.into(), hash(blocks.as_bytes()));
        lock::write(&ws.path().join("local/ratified.lock"), &lockfile).unwrap();

        let (proposed, _, to) = change(ratified, "architect", None, 1).unwrap();
        assert_eq!(to, 2);
        let (eff, notices) = delphi_engine::boot::effective_config(ws.path(), &proposed).unwrap();
        assert_eq!(Config::parse(&eff).map(|c| c.grades.of("architect", None)).unwrap_or(-9), 1, "the ratified grade stays in force");
        assert!(notices.iter().any(|n| n.contains("unratified")), "{notices:?}");
        let (eff, _) = delphi_engine::boot::effective_config(ws.path(), ratified).unwrap();
        assert_eq!(Config::parse(&eff).map(|c| c.grades.of("architect", None)).unwrap_or(-9), 1);
    }
}
