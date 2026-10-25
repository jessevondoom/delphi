//! `delphi missions` and `/missions`: the missions of a workspace, newest first, so one can be reopened with
//! `delphi chat --mission <id>`. A mission is a directory under `local/missions/`; its last activity is the newest
//! event file in it. Reads files only.

use std::path::Path;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub last: Option<SystemTime>,
}

/// The newest modification time of a mission's event files, else of the mission directory itself.
fn last_activity(dir: &Path) -> Option<SystemTime> {
    let newest = std::fs::read_dir(dir.join("events")).ok()?.flatten().filter_map(|e| e.metadata().and_then(|m| m.modified()).ok()).max();
    newest.or_else(|| std::fs::metadata(dir).and_then(|m| m.modified()).ok())
}

/// Every mission of the workspace, the most recently active first (ties and unknown times by id).
pub fn list(ws_dir: &Path) -> Vec<Row> {
    let Ok(rd) = std::fs::read_dir(ws_dir.join("local/missions")) else { return Vec::new() };
    let mut rows: Vec<Row> = rd.flatten().filter(|e| e.path().is_dir()).map(|e| Row { id: e.file_name().to_string_lossy().to_string(), last: last_activity(&e.path()) }).collect();
    rows.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| b.id.cmp(&a.id)));
    rows
}

/// `just now`, `5 min ago`, `3 h ago`, `2 d ago`.
pub fn ago(then: Option<SystemTime>, now: SystemTime) -> String {
    let Some(secs) = then.and_then(|t| now.duration_since(t).ok()).map(|d: Duration| d.as_secs()) else { return "—".into() };
    match secs {
        0..=59 => "just now".into(),
        60..=3_599 => format!("{} min ago", secs / 60),
        3_600..=86_399 => format!("{} h ago", secs / 3_600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

/// The table, or the words for an empty workspace; the newest mission is the one the console opens by default.
pub fn render(rows: &[Row], now: SystemTime) -> String {
    if rows.is_empty() {
        return "no missions yet: the first one starts when you run `delphi` or `delphi run \"<task>\"`\n".into();
    }
    let width = rows.iter().map(|r| r.id.chars().count()).max().unwrap_or(0);
    let mut out = String::new();
    for (i, r) in rows.iter().enumerate() {
        out.push_str(&format!("  {:<w$}  {}{}\n", r.id, ago(r.last, now), if i == 0 { "  (latest)" } else { "" }, w = width));
    }
    out.push_str("reopen one with: delphi chat --mission <id>\n");
    out
}

/// `delphi missions`.
pub fn cli(ws_dir: &Path) -> i32 {
    print!("{}", render(&list(ws_dir), SystemTime::now()));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mission(ws: &Path, id: &str, secs_ago: u64) {
        let ev = ws.join("local/missions").join(id).join("events");
        std::fs::create_dir_all(&ev).unwrap();
        let f = ev.join("000001.jsonl");
        std::fs::write(&f, "{}\n").unwrap();
        let t = SystemTime::now() - Duration::from_secs(secs_ago);
        std::fs::File::options().write(true).open(&f).unwrap().set_modified(t).unwrap();
    }

    #[test]
    fn missions_are_listed_newest_first_with_how_long_ago_and_a_way_to_reopen_one() {
        let t = tempfile::tempdir().unwrap();
        mission(t.path(), "2026-10-23-session", 2 * 86_400 + 10);
        mission(t.path(), "2026-10-25-session", 90);
        mission(t.path(), "2026-10-24-session", 5 * 3_600);
        let rows = list(t.path());
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["2026-10-25-session", "2026-10-24-session", "2026-10-23-session"]);
        let text = render(&rows, SystemTime::now());
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].contains("2026-10-25-session") && lines[0].contains("1 min ago") && lines[0].ends_with("(latest)"), "{}", text);
        assert!(lines[1].contains("5 h ago") && lines[2].contains("2 d ago"), "{}", text);
        assert!(lines[3].contains("delphi chat --mission <id>"), "{}", text);
    }

    #[test]
    fn an_empty_workspace_says_how_a_mission_starts_and_odd_files_are_ignored() {
        let t = tempfile::tempdir().unwrap();
        assert!(render(&list(t.path()), SystemTime::now()).contains("no missions yet"));
        std::fs::create_dir_all(t.path().join("local/missions")).unwrap();
        std::fs::write(t.path().join("local/missions/stray.txt"), "x").unwrap();
        assert!(list(t.path()).is_empty(), "only directories are missions");
        assert_eq!(ago(None, SystemTime::now()), "—");
        assert_eq!(ago(Some(SystemTime::now()), SystemTime::now()), "just now");
    }
}
