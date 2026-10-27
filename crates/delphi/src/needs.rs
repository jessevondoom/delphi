//! `delphi needs`: which of the usage-gated and deferred roadmap items have earned their turn.
//!
//! Step 6 says each item starts only when a recorded usage need justifies it. This command is where that
//! evidence is read: counts Delphi already keeps (missions, search misses, update conflicts, the team repo),
//! plus needs the operator records on purpose (`delphi needs add <item> <why>`). It decides nothing: it
//! shows the rule, the numbers, and whether the rule is met.

use anyhow::{bail, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Automatic diff3 merging is worth building after this many customized files had to be set aside.
pub const DIFF3_CONFLICTS: usize = 3;
/// A coaching pass has material to find patterns in after this many finished tasks.
pub const RETRO_TASKS: usize = 10;

pub struct Item {
    pub id: &'static str,
    pub what: &'static str,
    pub rule: &'static str,
    pub evidence: String,
    /// `None` when nothing is measured automatically: only a recorded need can justify it.
    pub met: Option<bool>,
    pub recorded: Vec<String>,
}

const ITEMS: [(&str, &str, &str); 10] = [
    ("retro", "the /retro coaching pass and rule scores (specs/004 §4)", "at least 10 finished tasks to learn from"),
    ("diff3", "automatic three-way merge in `delphi update` (specs/006 §4)", "at least 3 customized files set aside by an update"),
    ("sync", "`delphi sync` for a shared team repo (specs/007 Plane 3)", "the team/ directory is a git repo with a remote"),
    ("watches", "ambient watches that wake an agent (specs/008 §2)", "a recorded need"),
    ("rewind", "/rewind to a checkpoint (specs/008 §3)", "a recorded need"),
    ("federation", "federated peers, planes 1 and 2 (specs/007)", "a recorded need (a second operator or machine)"),
    ("openai", "a direct OpenAI API driver", "a recorded need"),
    ("resume", "resuming a subscription CLI's own session", "a recorded need"),
    ("gemini-builder", "a Gemini Builder, if `gemini --acp` returns", "a recorded need"),
    ("xenia", "Xenia Phase 2: skills in the org store, signed invites, a pointer resolver, several stores (specs/011 §11)", "a joined store in real use: at least 3 members and a record a steward has endorsed"),
];

pub fn item_ids() -> Vec<&'static str> {
    ITEMS.iter().map(|i| i.0).collect()
}

fn lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn missions(ws: &Path) -> Vec<std::path::PathBuf> {
    fs::read_dir(ws.join("local/missions")).map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.path()).collect()).unwrap_or_default()
}

/// Finished tasks across every mission's event log.
fn finished_tasks(ws: &Path) -> usize {
    let mut n = 0;
    for m in missions(ws) {
        let Ok(rd) = fs::read_dir(m.join("events")) else { continue };
        for seg in rd.flatten() {
            n += fs::read_to_string(seg.path()).unwrap_or_default().lines().filter(|l| l.contains("\"type\":\"task.completed\"")).count();
        }
    }
    n
}

fn team_has_remote(ws: &Path) -> bool {
    let cfg = ws.join("team/.git/config");
    fs::read_to_string(cfg).map(|t| t.contains("[remote ")).unwrap_or(false)
}

/// What the joined Xenia stores show (`home` is `$DELPHI_HOME`): the evidence line, and whether the loop Phase 1
/// built is in real use. A store that cannot be read counts for nothing.
fn xenia_evidence(home: Option<&Path>, now: i64) -> (String, Option<bool>) {
    let Some(home) = home else { return ("no store is joined on this install".to_string(), Some(false)) };
    let stores = delphi_core::xenia_status::all(home, &delphi_core::xenia_sync::VerifyConfig::at(now));
    if stores.is_empty() {
        return ("no store is joined on this install (`delphi xenia join`)".to_string(), Some(false));
    }
    let mut lines = Vec::new();
    let mut used = false;
    for (r, st) in &stores {
        match st {
            Ok(st) => {
                let members = st.roster.as_ref().map(|x| x.members).unwrap_or(0);
                used |= members >= 3 && st.l2 >= 1;
                lines.push(format!("{}: {} member(s), {} record(s) endorsed (L2), {} at L1", r.id(), members, st.l2, st.l1));
            }
            Err(e) => lines.push(format!("{}: cannot be read ({})", r.id(), e)),
        }
    }
    (lines.join("; "), Some(used))
}

pub fn report_in(ws: &Path, home: Option<&Path>) -> Vec<Item> {
    report_at(ws, home, delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(0))
}

/// [`report_in`] with the clock given (a freshness statement is judged against it).
pub fn report_at(ws: &Path, home: Option<&Path>, now: i64) -> Vec<Item> {
    let recorded = lines(&ws.join("local/needs.jsonl"));
    let conflicts = lines(&ws.join("local/update-conflicts.jsonl")).len();
    let tasks = finished_tasks(ws);
    ITEMS
        .iter()
        .map(|(id, what, rule)| {
            let mine: Vec<String> = recorded.iter().filter(|r| r["item"] == *id).map(|r| r["reason"].as_str().unwrap_or("").to_string()).collect();
            let (evidence, met) = match *id {
                "retro" => (format!("{} finished task(s)", tasks), Some(tasks >= RETRO_TASKS)),
                "diff3" => (format!("{} customized file(s) set aside so far", conflicts), Some(conflicts >= DIFF3_CONFLICTS)),
                "xenia" => xenia_evidence(home, now),
                "sync" => (if team_has_remote(ws) { "team/ has a git remote".to_string() } else { "team/ has no git remote (single operator)".to_string() }, Some(team_has_remote(ws))),
                _ => ("nothing is measured for this one".to_string(), None),
            };
            // A need the operator wrote down counts for the items that have no automatic signal, and is shown for the rest.
            let met = match met {
                None => Some(!mine.is_empty()),
                m => m,
            };
            Item { id, what, rule, evidence, met, recorded: mine }
        })
        .collect()
}

pub fn render(items: &[Item]) -> String {
    let mut out = String::from("Roadmap items waiting on evidence. A rule being met means it is worth discussing, not that it is scheduled.\n\n");
    for i in items {
        let mark = match i.met {
            Some(true) => "MET   ",
            Some(false) => "not yet",
            None => "-",
        };
        out.push_str(&format!("{:<8}{:<15}{}\n    rule:     {}\n    evidence: {}\n", mark, i.id, i.what, i.rule, i.evidence));
        for r in &i.recorded {
            out.push_str(&format!("    you wrote: {}\n", r));
        }
    }
    out.push_str("\nRecord a need on purpose:  delphi needs add <item> <why>\n");
    out
}

pub fn add(ws: &Path, item: &str, reason: &str) -> Result<()> {
    if !item_ids().contains(&item) {
        bail!("unknown item `{}` (one of: {})", item, item_ids().join(", "));
    }
    if reason.trim().is_empty() {
        bail!("say why: `delphi needs add {} <what happened that you needed it for>`", item);
    }
    use std::io::Write;
    fs::create_dir_all(ws.join("local"))?;
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut f = fs::OpenOptions::new().create(true).append(true).open(ws.join("local/needs.jsonl"))?;
    writeln!(f, "{}", serde_json::json!({"t": t, "item": item, "reason": reason.chars().take(500).collect::<String>()}))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by<'a>(items: &'a [Item], id: &str) -> &'a Item {
        items.iter().find(|i| i.id == id).unwrap()
    }

    #[test]
    fn a_fresh_workspace_has_met_nothing() {
        let d = tempfile::tempdir().unwrap();
        let items = report_in(d.path(), None);
        assert!(items.iter().all(|i| i.met == Some(false)), "{:?}", items.iter().map(|i| (i.id, i.met)).collect::<Vec<_>>());
        assert!(render(&items).contains("not yet") && render(&items).contains("delphi needs add"));
    }

    #[test]
    fn finished_tasks_and_set_aside_files_are_counted_from_what_is_already_kept() {
        let d = tempfile::tempdir().unwrap();
        let seg = d.path().join("local/missions/m1/events");
        fs::create_dir_all(&seg).unwrap();
        let line = "{\"seq\":1,\"type\":\"task.completed\",\"payload\":{}}\n";
        fs::write(seg.join("000001.jsonl"), line.repeat(10)).unwrap();
        fs::write(d.path().join("local/update-conflicts.jsonl"), "{\"file\":\"a\"}\n{\"file\":\"b\"}\n{\"file\":\"c\"}\n").unwrap();
        let items = report_in(d.path(), None);
        assert_eq!((by(&items, "retro").met, by(&items, "diff3").met), (Some(true), Some(true)));
        assert_eq!(by(&items, "retro").evidence, "10 finished task(s)");
    }

    #[test]
    fn a_shared_team_repo_is_what_justifies_sync() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("team/.git")).unwrap();
        fs::write(d.path().join("team/.git/config"), "[core]\n").unwrap();
        assert_eq!(by(&report_in(d.path(), None), "sync").met, Some(false));
        fs::write(d.path().join("team/.git/config"), "[core]\n[remote \"origin\"]\n\turl = x\n").unwrap();
        assert_eq!(by(&report_in(d.path(), None), "sync").met, Some(true));
    }

    #[test]
    fn xenia_is_justified_by_a_joined_store_in_real_use_or_by_a_recorded_need() {
        use delphi_core::xenia_fixture::{endorse, now, records as rk, u, Fx};
        let ws = tempfile::tempdir().unwrap();
        assert_eq!(by(&report_in(ws.path(), None), "xenia").met, Some(false));
        let t = tempfile::tempdir().unwrap();
        let fx0 = Fx::with_cast(t.path());
        let m = rk::memory_with(&fx0.id("mia"), &u(0), 1, "active", "v", "payments", "internal_empirical");
        let e = endorse(&fx0.id("sam"), 10, &m, 1, "payments");
        // Joined but nothing endorsed yet: not in real use.
        let (fx, _) = Fx::installed_on(fx0, &[&m]);
        let home = fx.dir.join("home");
        let items = report_at(ws.path(), Some(&home), now());
        assert_eq!(by(&items, "xenia").met, Some(false));
        assert!(by(&items, "xenia").evidence.contains("acme/acme-main: 8 member(s), 0 record(s) endorsed"), "{}", by(&items, "xenia").evidence);
        // A steward endorses it: three or more members and an endorsed record is the loop working.
        fx.put(&e);
        let head = fx.commit("endorse");
        fx.fresh_at(&head, now() - 1800, 7 * 86400);
        fx.sync();
        let items = report_at(ws.path(), Some(&home), now());
        assert_eq!(by(&items, "xenia").met, Some(true), "{}", by(&items, "xenia").evidence);
        assert!(item_ids().contains(&"xenia"));
    }

    #[test]
    fn items_without_a_measure_are_justified_only_by_a_need_you_recorded() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(by(&report_in(d.path(), None), "rewind").met, Some(false));
        assert!(add(d.path(), "rewind", "").is_err() && add(d.path(), "nonsense", "why").is_err());
        add(d.path(), "rewind", "a build went wrong at 3am and I wanted the worktree from before").unwrap();
        let items = report_in(d.path(), None);
        assert_eq!(by(&items, "rewind").met, Some(true));
        assert_eq!(by(&items, "rewind").recorded.len(), 1);
        assert!(render(&items).contains("you wrote: a build went wrong"));
        assert_eq!(by(&items, "watches").met, Some(false), "other items are unaffected");
    }
}
