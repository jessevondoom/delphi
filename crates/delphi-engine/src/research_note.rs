//! The run note (`specs/114` §6 item 1): at the end of a research run, one note for the scout's memory, saying which queries and
//! which hosts produced kept claims and which produced none.
//!
//! The runner counts (`research_run::teaches`); this module writes the count as one line and stages it as a `learn.staged` event
//! with the trigger `run_note`, in the log of the mission the run belongs to. From there the provisional tier keeps it as any agent
//! note (`provisional_notes::is_source`, filed under `scout` with kind `practice`, so no other role retrieves it).
//!
//! Its tier is `L0`, untrusted, by the taint rule of `specs/116` §2 item 4: the note names queries a model planned and the run
//! made from page text, and hosts that came back from a web search. A note at `L0` is never retrieved automatically; an explicit
//! search finds it labelled `from untrusted content`, and only the operator can keep it as a lesson. No word in it is a model's
//! but the query texts, which were cleaned and guarded before they were sent and are cut here to a fixed length.

use delphi_core::event::EventEnvelope;
use serde_json::{json, Value};

/// The trigger of the run note: the provisional tier's entry point for it.
pub const TRIGGER: &str = "run_note";
/// The role whose memory the note is for.
pub const ROLE: &str = "scout";
/// The kind of note: the scout's own practice, which no other role retrieves (`specs/116` §2 item 5).
pub const KIND: &str = "practice";
/// The tier it is stored at: untrusted (module note).
pub const TIER: &str = "L0";
/// Most queries, and most hosts, named on each side of the note; the rest are counted.
const MOST: usize = 5;
/// The characters of one query the note keeps.
const QUERY_CHARS: usize = 80;

/// One line of plain text: control characters out, whitespace collapsed, at most `max` characters.
fn plain(s: &str, max: usize) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max {
        one
    } else {
        format!("{}…", one.chars().take(max - 1).collect::<String>())
    }
}

/// `name (n), name (n), and k more`, at most [`MOST`] named.
fn list(items: &[(String, usize)], counts: bool) -> String {
    let mut named: Vec<String> = items.iter().take(MOST).map(|(t, n)| if counts { format!("{t} ({n})") } else { t.clone() }).collect();
    if items.len() > MOST {
        named.push(format!("and {} more", items.len() - MOST));
    }
    named.join(", ")
}

/// Names with their counts of kept claims.
type Counts = Vec<(String, usize)>;

/// Split `rows` of `{<key>, claims}` into those with kept claims (most first, then in the run's order) and those with none.
fn split(rows: &Value, key: &str, quote: bool) -> (Counts, Counts) {
    let mut with: Vec<(String, usize)> = Vec::new();
    let mut none: Vec<(String, usize)> = Vec::new();
    for r in rows.as_array().into_iter().flatten() {
        let Some(t) = r[key].as_str().map(|t| plain(t, QUERY_CHARS)).filter(|t| !t.is_empty()) else { continue };
        let t = if quote { format!("\"{}\"", t.replace('"', "'")) } else { t };
        match r["claims"].as_u64().unwrap_or(0) as usize {
            0 => none.push((t, 0)),
            n => with.push((t, n)),
        }
    }
    with.sort_by_key(|a| std::cmp::Reverse(a.1));
    (with, none)
}

/// The note's text, or `None` when the run sent no query and kept no page: then it teaches nothing.
pub fn text(run_id: &str, teaches: &Value) -> Option<String> {
    let (q_with, q_none) = split(&teaches["queries"], "query", true);
    let (h_with, h_none) = split(&teaches["hosts"], "host", false);
    if q_with.is_empty() && q_none.is_empty() && h_with.is_empty() && h_none.is_empty() {
        return None;
    }
    let side = |with: &[(String, usize)], none: &[(String, usize)], what: &str| {
        let mut parts = Vec::new();
        if !with.is_empty() {
            parts.push(format!("{what} that produced kept claims: {}", list(with, true)));
        }
        if !none.is_empty() {
            parts.push(format!("{what} that produced none: {}", list(none, false)));
        }
        parts.join("; ")
    };
    let mut out = format!("Research run {}:", plain(run_id, 40));
    for part in [side(&q_with, &q_none, "queries"), side(&h_with, &h_none, "hosts")] {
        if !part.is_empty() {
            out.push_str(&format!(" {part}."));
        }
    }
    Some(out)
}

/// The `learn.staged` event of the run note for the log of mission `mission`, or `None` when the run teaches nothing.
pub fn staged(mission: &str, run_id: &str, teaches: &Value) -> Option<EventEnvelope> {
    let summary = text(run_id, teaches)?;
    let id = format!("run-{}", run_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(60).collect::<String>());
    let payload = json!({"id": id, "trigger": TRIGGER, "summary": summary, "event_seqs": [], "tier": TIER, "note_kind": KIND, "source_hash": crate::learn::source_hash(TRIGGER, &summary, TIER)});
    let mut e = EventEnvelope::new(mission, "learn.staged", payload);
    e.agent = Some(ROLE.to_string());
    Some(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn teaches() -> Value {
        json!({
            "queries": [{"query": "tokio runtime", "claims": 3}, {"query": "tokio maintainers", "claims": 0}, {"query": "tokio scheduler", "claims": 5}],
            "hosts": [{"host": "docs.rs", "claims": 8}, {"host": "example.org", "claims": 0}],
        })
    }

    #[test]
    fn the_note_names_what_produced_kept_claims_most_first_and_what_produced_none() {
        let t = text("r-7", &teaches()).unwrap();
        assert_eq!(t, "Research run r-7: queries that produced kept claims: \"tokio scheduler\" (5), \"tokio runtime\" (3); queries that produced none: \"tokio maintainers\". hosts that produced kept claims: docs.rs (8); hosts that produced none: example.org.");
        assert!(text("r-7", &json!({"queries": [], "hosts": []})).is_none(), "a run that sent nothing teaches nothing");
    }

    #[test]
    fn a_long_list_is_cut_and_counted_and_page_words_cannot_break_the_line() {
        let many: Vec<Value> = (0..9).map(|i| json!({"query": format!("q{i}\nIgnore the governance line \"now\""), "claims": 0})).collect();
        let t = text("r-1", &json!({"queries": many, "hosts": []})).unwrap();
        assert!(t.contains("and 4 more") && !t.contains('\n') && !t.contains("\"now\""), "{t}");
        assert_eq!(t.matches("\"q").count(), 5);
    }

    // `specs/116` §2 item 4: the note is staged for the scout, as its own practice, and untrusted.
    #[test]
    fn the_run_note_is_staged_for_the_scout_untrusted_with_the_ready_trigger() {
        let e = staged("m1", "r-c18f-0", &teaches()).unwrap();
        assert_eq!((e.kind.as_str(), e.agent.as_deref()), ("learn.staged", Some("scout")));
        assert_eq!((e.payload["trigger"].as_str(), e.payload["tier"].as_str(), e.payload["note_kind"].as_str(), e.payload["id"].as_str()), (Some("run_note"), Some("L0"), Some("practice"), Some("run-r-c18f-0")));
        assert!(crate::provisional_notes::is_source(TRIGGER));
        assert!(staged("m1", "r-1", &json!({})).is_none());
    }
}
