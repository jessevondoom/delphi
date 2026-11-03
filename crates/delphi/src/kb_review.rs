//! `delphi kb review [show|keep|discard]`: the operator's keep-or-discard pass over research a host agent staged with
//! `propose_kb_entry` (`specs/068` §5). Keeping goes through `kb import`, so it grants inclusion and never validity;
//! discarding appends `learn.dropped`, as `delphi learn drop` does. Only the operator runs this: it is refused in an agent seat.

use delphi_core::event::EventEnvelope;
use delphi_engine::learn::{live_notes, LiveNote};
use std::path::Path;

const TRIGGER: &str = "host_kb_entry";

pub struct Entry {
    pub note: LiveNote,
    pub title: String,
    pub body: String,
    pub sources: Vec<String>,
    pub confidence: String,
}

pub fn pending(ws_dir: &Path) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    for (id, dir) in delphi_core::sessions::dirs(ws_dir) {
        let events = delphi_core::log::replay(&dir).map_err(|e| format!("could not read the log of {}: {}", id, e))?;
        for note in live_notes(&id, &events).into_iter().filter(|n| n.trigger == TRIGGER) {
            if let Some(e) = entry_of(&events, note) {
                out.push(e);
            }
        }
    }
    out.sort_by(|a, b| a.note.timestamp.cmp(&b.note.timestamp));
    Ok(out)
}

fn entry_of(events: &[EventEnvelope], note: LiveNote) -> Option<Entry> {
    let e = events.iter().find(|e| e.kind == "learn.staged" && e.payload["id"].as_str() == Some(note.id.as_str()))?;
    let k = &e.payload["kb_entry"];
    Some(Entry {
        title: k["title"].as_str()?.to_string(),
        body: k["body"].as_str()?.to_string(),
        sources: k["sources"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect(),
        confidence: k["confidence"].as_str().unwrap_or("low").to_string(),
        note,
    })
}

/// The markdown a kept entry is imported as: its body, then the sources the agent listed, so the bibliography sees them.
pub fn page_of(e: &Entry) -> String {
    let mut s = format!("# {}\n\n*Proposed by {}; kept by the operator. Not validated.*\n\n{}\n", e.title, e.note.summary.split(']').next().unwrap_or("").trim_start_matches('['), e.body);
    if !e.sources.is_empty() {
        s.push_str("\n## Sources\n\n");
        for x in &e.sources {
            s.push_str(&format!("- {}\n", x));
        }
    }
    s
}

fn render_list(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return "nothing is waiting for review: research a host agent proposes with propose_kb_entry shows up here\n".into();
    }
    let mut out = String::new();
    for e in entries {
        out.push_str(&format!("{}  {}\n    from {} · {} source(s) · stated confidence {} · {} words\n", e.note.reference(), e.title, e.note.summary.split(']').next().unwrap_or("").trim_start_matches('['), e.sources.len(), e.confidence, e.body.split_whitespace().count()));
    }
    out.push_str(&format!("{} waiting. Not searchable until kept. `delphi kb review keep --all` or `keep <ref>…`, `discard <ref>…`, `show <ref>`\n", entries.len()));
    out
}

fn pick<'a>(entries: &'a [Entry], args: &[&str]) -> Result<Vec<&'a Entry>, String> {
    if args.contains(&"--all") {
        return Ok(entries.iter().collect());
    }
    if args.is_empty() {
        return Err("name the entries (see `delphi kb review`) or give --all".into());
    }
    args.iter()
        .map(|r| {
            let hits: Vec<&Entry> = entries.iter().filter(|e| e.note.reference() == *r || e.note.id == *r).collect();
            match hits.as_slice() {
                [one] => Ok(*one),
                [] => Err(format!("no entry `{}` waiting (see `delphi kb review`)", r)),
                _ => Err(format!("`{}` is in more than one session; give <mission>/<id>", r)),
            }
        })
        .collect()
}

pub fn run_in(ws_dir: &Path, args: &[String]) -> Result<String, String> {
    let entries = pending(ws_dir)?;
    let verb = args.first().map(String::as_str).unwrap_or("list");
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    match verb {
        "list" => Ok(render_list(&entries)),
        "stats" => {
            let mut proposed = 0;
            for (_, dir) in delphi_core::sessions::dirs(ws_dir) {
                proposed += delphi_core::log::replay(&dir).map_err(|e| e.to_string())?.iter().filter(|e| e.kind == "learn.staged" && e.payload["trigger"].as_str() == Some(TRIGGER)).count();
            }
            let (pages, shown) = delphi_engine::kb_import::counts(ws_dir);
            Ok(format!("capture: {} proposed by host agents · {} waiting for review · {} decided\nlibrary: {} imported page(s), {} in default search\n", proposed, entries.len(), proposed - entries.len().min(proposed), pages, shown))
        }
        "show" => {
            let one = pick(&entries, &rest)?;
            Ok(one.iter().map(|e| format!("{}\n{}\n", e.title, page_of(e))).collect::<String>())
        }
        "keep" => {
            let mut out = String::new();
            for e in pick(&entries, &rest)? {
                let r = delphi_engine::kb::import_text(ws_dir, &format!("{}.md", e.title), &page_of(e))?;
                crate::learn_cmd::drop_note(ws_dir, &e.note)?;
                out.push_str(&r.receipt());
            }
            Ok(out)
        }
        "discard" => {
            let mut n = 0;
            for e in pick(&entries, &rest)? {
                crate::learn_cmd::drop_note(ws_dir, &e.note)?;
                n += 1;
            }
            Ok(format!("discarded {} (the events stay in the log; nothing was added to the library)\n", n))
        }
        other => Err(format!("`{}` is not a review verb (list, show, keep, discard, stats)", other)),
    }
}

pub fn cli(ws_dir: &Path, args: &[String]) -> i32 {
    match run_in(ws_dir, args) {
        Ok(s) => {
            print!("{}", s);
            0
        }
        Err(e) => {
            eprintln!("delphi kb review: {}", e);
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::log::{EventLog, LogOptions};
    use serde_json::json;

    fn stage(ws: &Path, mission: &str, id: &str, title: &str) {
        let mut log = EventLog::open(&delphi_core::sessions::dir(ws, mission), LogOptions::default()).unwrap();
        log.append(EventEnvelope::new(mission, "learn.staged", json!({"id": id, "trigger": TRIGGER, "summary": format!("[claude-code/x] KB entry: {title}"), "event_seqs": [], "tier": "L0",
            "kb_entry": {"title": title, "body": "Rotation is every 90 days. See https://wiki.example.com/v", "sources": ["https://wiki.example.com/v"], "confidence": "low"}}))).unwrap();
        log.append(EventEnvelope::new(mission, "learn.staged", json!({"id": "h9", "trigger": "host_note", "summary": "a lesson", "event_seqs": [], "tier": "L0"}))).unwrap();
    }

    #[test]
    fn review_lists_only_kb_entries_and_discard_removes_them_without_adding_anything() {
        let t = tempfile::tempdir().unwrap();
        stage(t.path(), "host-a", "h1", "Vault rotation");
        stage(t.path(), "host-b", "h1", "Other");
        let listed = run_in(t.path(), &[]).unwrap();
        assert!(listed.contains("host-a/h1") && listed.contains("host-b/h1") && listed.contains("2 waiting") && !listed.contains("a lesson"), "{listed}");
        assert!(run_in(t.path(), &["discard".into()]).is_err(), "a bare discard names nothing");
        assert!(run_in(t.path(), &["discard".into(), "h1".into()]).unwrap_err().contains("more than one"));
        run_in(t.path(), &["discard".into(), "host-a/h1".into()]).unwrap();
        let after = run_in(t.path(), &[]).unwrap();
        assert!(!after.contains("host-a/h1") && after.contains("1 waiting"), "{after}");
        assert!(!t.path().join("local/summaries").exists());
        let st = run_in(t.path(), &["stats".into()]).unwrap();
        assert!(st.contains("2 proposed") && st.contains("1 waiting") && st.contains("1 decided"), "{st}");
        let page = page_of(&pending(t.path()).unwrap()[0]);
        assert!(page.starts_with("# Other\n") && page.contains("claude-code/x") && page.contains("## Sources"), "{page}");
    }
}
