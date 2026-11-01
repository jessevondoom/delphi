//! `delphi changelog`: what the team recorded as finished in a mission (`record_change`, `specs/032`), printed as
//! Markdown for a pull request description, or as JSON lines. Reads the mission's event files only.

use delphi_engine::changelog::{entries, markdown, Filter};
use std::path::Path;

pub struct Options {
    pub mission: Option<String>,
    pub filter: Filter,
    pub json: bool,
}

/// The text to print for a workspace, or what is wrong with the request.
pub fn render(ws_dir: &Path, opts: &Options) -> Result<String, String> {
    let id = match &opts.mission {
        Some(m) => m.clone(),
        None => crate::missions::list(ws_dir).into_iter().next().map(|r| r.id).ok_or("no missions yet: nothing has been recorded")?,
    };
    // A mission id is a directory name; refuse anything that could point elsewhere.
    if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
        return Err(format!("`{}` is not a mission id (see `delphi missions`)", id));
    }
    let dir = delphi_core::sessions::dir(ws_dir, &id);
    if !dir.is_dir() {
        return Err(format!("no mission `{}` in this workspace (see `delphi missions`)", id));
    }
    let events = delphi_core::log::replay(&dir).map_err(|e| format!("could not read the mission log: {}", e))?;
    let found = entries(&events, &opts.filter);
    if opts.json {
        let mut out = String::new();
        for e in &found {
            out.push_str(&serde_json::to_string(e).map_err(|e| e.to_string())?);
            out.push('\n');
        }
        return Ok(out);
    }
    Ok(markdown(&id, &found))
}

pub fn cli(ws_dir: &Path, opts: &Options) -> i32 {
    match render(ws_dir, opts) {
        Ok(text) => {
            print!("{}", text);
            0
        }
        Err(e) => {
            eprintln!("delphi changelog: {}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::event::EventEnvelope;
    use serde_json::json;

    fn write_mission(ws: &Path, id: &str, events: &[(&str, serde_json::Value)]) {
        let dir = delphi_core::sessions::dir(ws, id);
        let mut log = delphi_core::log::EventLog::open(&dir, delphi_core::log::LogOptions::default()).unwrap();
        for (kind, payload) in events {
            log.append(EventEnvelope::new(id, kind, payload.clone())).unwrap();
        }
    }

    #[test]
    fn prints_the_latest_mission_as_markdown_or_json_and_refuses_bad_ids() {
        let t = tempfile::tempdir().unwrap();
        assert!(render(t.path(), &Options { mission: None, filter: Filter::default(), json: false }).unwrap_err().contains("no missions yet"));
        write_mission(t.path(), "m1", &[("change.recorded", json!({"summary": "added a timeout", "role": "builder", "files": ["a.rs"]})), ("session.started", json!({"session_id": "s_0001"})), ("change.recorded", json!({"summary": "removed ratify requirements", "role": "writer"}))]);
        let md = render(t.path(), &Options { mission: None, filter: Filter::default(), json: false }).unwrap();
        assert!(md.starts_with("# Changes in m1\n") && md.contains("## Session s_0000") && md.contains("## Session s_0001") && md.contains("- added a timeout _(builder,") && md.contains("`a.rs`"), "{}", md);
        let one = render(t.path(), &Options { mission: Some("m1".into()), filter: Filter { session: Some("s_0001".into()), since: None }, json: true }).unwrap();
        assert_eq!(one.lines().count(), 1);
        assert_eq!(serde_json::from_str::<serde_json::Value>(one.trim()).unwrap()["summary"], "removed ratify requirements");
        for bad in ["../x", "a/b", ".hidden", ""] {
            assert!(render(t.path(), &Options { mission: Some(bad.into()), filter: Filter::default(), json: false }).is_err(), "{}", bad);
        }
        assert!(render(t.path(), &Options { mission: Some("nope".into()), filter: Filter::default(), json: false }).unwrap_err().contains("no mission `nope`"));
    }
}
