//! The change log read back from a mission's event log (`specs/032`): every `change.recorded` event with the session it
//! happened in, filtered, and rendered as Markdown for a pull request description. Pure: events in, text out.

use delphi_core::event::EventEnvelope;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub session: String,
    pub timestamp: String,
    pub role: String,
    pub task: String,
    pub summary: String,
    pub files: Vec<String>,
}

/// What to keep: one session, and/or only entries at or after a time (an RFC 3339 timestamp or just its date).
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub session: Option<String>,
    pub since: Option<String>,
}

/// The recorded changes of a mission, oldest first, each with the session in force when it was recorded.
pub fn entries(events: &[EventEnvelope], filter: &Filter) -> Vec<Entry> {
    let mut session = crate::session::IMPLICIT.to_string();
    let mut out = Vec::new();
    for e in events {
        if e.kind == "session.started" {
            session = e.payload["session_id"].as_str().unwrap_or(crate::session::IMPLICIT).to_string();
        } else if e.kind == "change.recorded" {
            let summary = e.payload["summary"].as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
            if summary.is_empty() || filter.session.as_ref().is_some_and(|s| *s != session) || filter.since.as_ref().is_some_and(|t| e.timestamp.as_str() < t.as_str()) {
                continue;
            }
            let files = e.payload["files"].as_array().map(|a| a.iter().filter_map(|f| f.as_str()).map(String::from).collect()).unwrap_or_default();
            out.push(Entry { session: session.clone(), timestamp: e.timestamp.clone(), role: e.payload["role"].as_str().unwrap_or("agent").to_string(), task: e.task_id.clone().unwrap_or_default(), summary, files });
        }
    }
    out
}

/// Markdown, grouped by session in the order they happened. Each line is the agent's sentence, then who, which task
/// and when, then the files; ready to paste into a pull request.
pub fn markdown(mission: &str, entries: &[Entry]) -> String {
    if entries.is_empty() {
        return format!("No changes were recorded in mission {}.\n", mission);
    }
    let mut out = format!("# Changes in {}\n", mission);
    let mut current = "";
    for e in entries {
        if e.session != current {
            current = &e.session;
            out.push_str(&format!("\n## Session {} ({})\n\n", e.session, e.timestamp.get(..16).unwrap_or(&e.timestamp).replace('T', " ")));
        }
        let mut meta = vec![e.role.clone()];
        if !e.task.is_empty() {
            meta.push(e.task.clone());
        }
        meta.push(e.timestamp.get(11..16).unwrap_or("").to_string());
        out.push_str(&format!("- {} _({})_", e.summary.trim_end_matches('.'), meta.join(", ")));
        if !e.files.is_empty() {
            out.push_str(&format!(" — {}", e.files.iter().map(|f| format!("`{}`", f)).collect::<Vec<_>>().join(", ")));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(seq: u64, kind: &str, task: &str, ts: &str, payload: serde_json::Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m", kind, payload);
        e.seq = seq;
        e.timestamp = ts.into();
        e.task_id = Some(task.into());
        e
    }
    fn log() -> Vec<EventEnvelope> {
        vec![
            ev(1, "change.recorded", "t_1", "2026-10-25T09:00:00Z", json!({"summary": "added a timeout", "role": "builder", "files": ["a.rs"]})),
            ev(2, "session.started", "", "2026-10-25T10:00:00Z", json!({"session_id": "s_0001", "reason": "new"})),
            ev(3, "change.recorded", "t_2", "2026-10-25T10:21:00Z", json!({"summary": "reworked the permission model.\n", "role": "builder"})),
            ev(4, "change.recorded", "t_2", "2026-10-25T10:22:00Z", json!({"summary": "  ", "role": "builder"})),
            ev(5, "task.output", "t_2", "2026-10-25T10:23:00Z", json!({"text": "done"})),
            ev(6, "change.recorded", "t_3", "2026-10-26T08:00:00Z", json!({"summary": "removed ratify requirements", "role": "writer", "files": ["x.md", "y.md"]})),
        ]
    }

    #[test]
    fn entries_carry_the_session_they_were_recorded_in_and_skip_empty_ones() {
        let all = entries(&log(), &Filter::default());
        assert_eq!(all.iter().map(|e| (e.session.as_str(), e.summary.as_str())).collect::<Vec<_>>(), [("s_0000", "added a timeout"), ("s_0001", "reworked the permission model."), ("s_0001", "removed ratify requirements")]);
        assert_eq!(entries(&log(), &Filter { session: Some("s_0001".into()), since: None }).len(), 2);
        assert_eq!(entries(&log(), &Filter { session: None, since: Some("2026-10-26".into()) }).len(), 1, "a date alone is a valid `since`");
        assert_eq!(entries(&log(), &Filter { session: Some("s_0001".into()), since: Some("2026-10-25T10:30".into()) }).len(), 1);
        assert!(entries(&log(), &Filter { session: Some("s_0009".into()), since: None }).is_empty());
    }

    #[test]
    fn markdown_groups_by_session_and_is_ready_for_a_pull_request() {
        let md = markdown("mission-1", &entries(&log(), &Filter::default()));
        let want = "# Changes in mission-1\n\n## Session s_0000 (2026-10-25 09:00)\n\n- added a timeout _(builder, t_1, 09:00)_ — `a.rs`\n\n## Session s_0001 (2026-10-25 10:21)\n\n- reworked the permission model _(builder, t_2, 10:21)_\n- removed ratify requirements _(writer, t_3, 08:00)_ — `x.md`, `y.md`\n";
        assert_eq!(md, want);
        assert_eq!(markdown("m", &[]), "No changes were recorded in mission m.\n");
    }
}
