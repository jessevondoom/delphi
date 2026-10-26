//! Console sessions (`specs/030`): a span of one mission's log that begins at a `session.started` event and lasts
//! until the operator says `/new` or the daemon restarts. A session is what the console shows and what the model is
//! told of the conversation so far; when one ends, a digest of it goes to the central log so it can be reviewed
//! later and offered as long-term context.

use delphi_core::event::EventEnvelope;
use delphi_core::provenance::Tier;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// `[session] conversation_tokens` default: the share of a task's context the conversation may take (bytes / 4).
pub const DEFAULT_CONVERSATION_TOKENS: usize = 6000;
/// The longest one turn is carried whole; a longer one is cut.
const TURN_CAP: usize = 3000;
/// A turn that no longer fits whole is carried as a line of this length.
const DIGEST_LINE: usize = 100;
/// Lines of a digest in the central log: the first and the last of this many each.
const DIGEST_ITEMS: usize = 6;
/// Earlier sessions of the mission offered as long-term context, and the bytes they may take together.
const EARLIER_SESSIONS: usize = 3;
const EARLIER_BYTES: usize = 1600;
/// Change-log lines of the current session carried in the preamble, and the bytes they may take.
const CHANGES_LINES: usize = 15;
const CHANGES_BYTES: usize = 2400;

/// The id of the log before any `session.started`: one implicit session, so a log from before sessions loads as today.
pub const IMPLICIT: &str = "s_0000";

/// The current session's id and its events (everything after the newest `session.started`, or the whole log).
pub fn current(events: &[EventEnvelope]) -> (String, &[EventEnvelope]) {
    match events.iter().rposition(|e| e.kind == "session.started") {
        Some(i) => (events[i].payload["session_id"].as_str().unwrap_or(IMPLICIT).to_string(), &events[i + 1..]),
        None => (IMPLICIT.to_string(), events),
    }
}

/// The id the next session gets: one more than the number started so far.
pub fn next_id(events: &[EventEnvelope]) -> String {
    format!("s_{:04}", events.iter().filter(|e| e.kind == "session.started").count() + 1)
}

/// Does this task belong to the conversation? Builds and councils carry their own objective.
fn is_conversation(e: &EventEnvelope) -> bool {
    e.kind == "task.created" && e.payload["prompt"].as_str().is_some_and(|p| !p.trim().is_empty()) && !matches!(e.payload["flow"].as_str(), Some("build") | Some("council"))
}

/// Has the operator said anything in this stretch of the log?
pub fn has_turns(session: &[EventEnvelope]) -> bool {
    session.iter().any(is_conversation)
}

/// Is there anything worth a digest in this stretch of the log: something asked, or a change an agent recorded
/// (work can be recorded in a session where the operator asked nothing, for example a timer's wake).
pub fn has_content(session: &[EventEnvelope]) -> bool {
    has_turns(session) || session.iter().any(|e| e.kind == "change.recorded")
}

fn first_line(s: &str, n: usize) -> String {
    let line = s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > n {
        line.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
    } else {
        line
    }
}

/// What a finished session leaves in the central log: enough to remember what was asked and answered, not the text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Digest {
    pub session_id: String,
    pub mission_id: String,
    pub workspace: String,
    pub started: String,
    pub ended: String,
    /// What closed it: `new` (the operator) or `restart` (the daemon).
    pub ended_by: String,
    pub turns: usize,
    pub asks: Vec<String>,
    pub answers: Vec<String>,
    /// A reply read untrusted content, so what followed ran below full trust.
    pub tainted: bool,
    pub escalated: usize,
    /// What agents recorded as finished in the session (`change.recorded`), first and last few; lines from before
    /// the change log have none.
    #[serde(default)]
    pub changes: Vec<String>,
    /// A model-written summary, when `[session] summary` is on and the session read nothing untrusted (`specs/030` §4.3). It lives on a
    /// line of its own in the central log, written after the digest, and is joined to it when the log is read.
    #[serde(default)]
    pub summary: Option<String>,
}

/// The change lines of a stretch of the log, oldest first: `HH:MM role: summary`.
fn change_lines(session: &[EventEnvelope]) -> Vec<String> {
    session
        .iter()
        .filter(|e| e.kind == "change.recorded")
        .filter_map(|e| {
            let said = e.payload["summary"].as_str()?.split_whitespace().collect::<Vec<_>>().join(" ");
            (!said.is_empty()).then(|| format!("{} {}: {}", e.timestamp.get(11..16).unwrap_or(""), e.payload["role"].as_str().unwrap_or("agent"), clip_bytes(&said, 300)))
        })
        .collect()
}

/// The first and last few lines, with a count for the ones between.
fn keep_ends(mut v: Vec<String>) -> Vec<String> {
    if v.len() > DIGEST_ITEMS * 2 {
        let hidden = v.len() - DIGEST_ITEMS * 2;
        let tail = v.split_off(v.len() - DIGEST_ITEMS);
        v.truncate(DIGEST_ITEMS);
        v.push(format!("… {} more …", hidden));
        v.extend(tail);
    }
    v
}

/// The digest of a session, or `None` if nothing was asked in it.
pub fn digest(mission: &str, workspace: &str, id: &str, ended_by: &str, session: &[EventEnvelope]) -> Option<Digest> {
    if !has_content(session) {
        return None;
    }
    let (mut asks, mut answers, mut escalated, mut tainted) = (Vec::new(), Vec::new(), 0, false);
    for e in session {
        match e.kind.as_str() {
            _ if is_conversation(e) => asks.push(first_line(e.payload["prompt"].as_str().unwrap_or(""), DIGEST_LINE)),
            "task.output" => {
                if e.payload["status"] == "escalated" {
                    escalated += 1;
                }
                answers.push(first_line(e.payload["text"].as_str().unwrap_or(""), DIGEST_LINE));
            }
            "span.taint" => tainted |= Tier::parse(e.payload["ceiling"].as_str().unwrap_or("L5")).is_some_and(|t| t < Tier::L3),
            _ => {}
        }
    }
    let turns = asks.len();
    Some(Digest {
        session_id: id.into(),
        mission_id: mission.into(),
        workspace: workspace.into(),
        started: session.first().map(|e| e.timestamp.clone()).unwrap_or_default(),
        ended: session.last().map(|e| e.timestamp.clone()).unwrap_or_default(),
        ended_by: ended_by.into(),
        turns,
        asks: keep_ends(asks),
        answers: keep_ends(answers),
        tainted,
        escalated,
        changes: keep_ends(change_lines(session)),
        summary: None,
    })
}

/// The central log: one line per finished session, across missions and workspaces, under the delphi home.
pub fn log_path(home: &Path) -> PathBuf {
    home.join("sessions.jsonl")
}

/// Append a model-written summary of a session to the central log, on a line of its own (`specs/030` §4.3). `redact` is applied first.
pub fn append_summary(home: &Path, mission: &str, session_id: &str, text: &str, model: &str, redact: &dyn Fn(&str) -> String) -> std::io::Result<()> {
    std::fs::create_dir_all(home)?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(log_path(home))?;
    writeln!(f, "{}", serde_json::json!({"kind": "summary", "mission_id": mission, "session_id": session_id, "text": redact(text), "model": model}))
}

/// Append a digest to the central log. `redact` is applied to every line first, as for any text the daemon writes.
pub fn append_central(home: &Path, d: &Digest, redact: &dyn Fn(&str) -> String) -> std::io::Result<()> {
    let mut d = d.clone();
    for l in d.asks.iter_mut().chain(d.answers.iter_mut()).chain(d.changes.iter_mut()) {
        *l = redact(l);
    }
    std::fs::create_dir_all(home)?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(log_path(home))?;
    writeln!(f, "{}", serde_json::to_string(&d).map_err(std::io::Error::other)?)
}

/// The newest `n` digests of a mission, oldest first. A line that does not parse is skipped.
pub fn earlier(home: &Path, mission: &str, n: usize) -> Vec<Digest> {
    let Ok(text) = std::fs::read_to_string(log_path(home)) else { return Vec::new() };
    let mut all: Vec<Digest> = text.lines().filter_map(|l| serde_json::from_str::<Digest>(l).ok()).filter(|d| d.mission_id == mission).collect();
    // Summaries are lines of their own, written after their digest; the newest one for a session wins.
    for l in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(l) else { continue };
        if v["kind"] != "summary" || v["mission_id"] != mission {
            continue;
        }
        if let (Some(id), Some(t)) = (v["session_id"].as_str(), v["text"].as_str()) {
            if let Some(d) = all.iter_mut().find(|d| d.session_id == id) {
                d.summary = Some(t.to_string());
            }
        }
    }
    let skip = all.len().saturating_sub(n);
    all.drain(..skip);
    all
}

/// What a task is told of the conversation, and the trust it must start under.
#[derive(Debug, Clone, PartialEq)]
pub struct Preamble {
    pub text: String,
    /// The lowest ceiling reached in the session: a reply that read untrusted content keeps that ceiling for what follows.
    pub ceiling: Tier,
    pub turns: usize,
    pub digested: usize,
    pub omitted: usize,
    /// How many earlier sessions of the mission are summarised at the top of the text.
    pub earlier: usize,
}

struct Turn {
    ask: bool,
    text: String,
}

/// The conversation so far, fitted to `budget_tokens` (bytes / 4), newest first: recent turns whole, older ones as
/// one-line digests, the oldest dropped with a count. The latest exchange is always kept. Earlier sessions of the
/// mission come first as a few digest lines. `None` when there is nothing to say.
pub fn preamble(session: &[EventEnvelope], earlier: &[Digest], budget_tokens: usize) -> Option<Preamble> {
    if budget_tokens == 0 {
        return None;
    }
    let mut turns: Vec<Turn> = Vec::new();
    let mut ceiling = Tier::L5;
    for e in session {
        if is_conversation(e) {
            turns.push(Turn { ask: true, text: e.payload["prompt"].as_str().unwrap_or("").trim().to_string() });
        } else if e.kind == "task.output" {
            let said = e.payload["text"].as_str().unwrap_or("").trim();
            let text = if e.payload["status"] == "escalated" { format!("(stopped) {}", said) } else { said.to_string() };
            turns.push(Turn { ask: false, text });
        } else if e.kind == "span.taint" {
            if let Some(t) = Tier::parse(e.payload["ceiling"].as_str().unwrap_or("L5")) {
                ceiling = ceiling.min(t);
            }
        }
    }
    let (past, past_n) = past_sessions(earlier);
    let changes = change_lines(session);
    if turns.is_empty() && past.is_empty() && changes.is_empty() {
        return None;
    }
    let budget = budget_tokens.saturating_mul(4);
    let whole = budget * 3 / 4;
    let label = |t: &Turn| if t.ask { "Operator" } else { "You" };
    let (mut used, mut digested, mut omitted) = (0usize, 0usize, 0usize);
    let mut lines: Vec<String> = Vec::new(); // newest first
    for (i, t) in turns.iter().enumerate().rev() {
        let full = format!("{}: {}", label(t), clip_bytes(&t.text, TURN_CAP));
        // The latest exchange (the last two turns) is kept whole, cut to what is left if it must be.
        let latest = i + 2 >= turns.len();
        if latest || used + full.len() <= whole {
            let line = if used + full.len() > budget { clip_bytes(&full, budget.saturating_sub(used).max(200)) } else { full };
            used += line.len() + 1;
            lines.push(line);
            continue;
        }
        let short = format!("{}: {}", label(t), first_line(&t.text, DIGEST_LINE));
        if used + short.len() + 64 < budget {
            used += short.len() + 1;
            digested += 1;
            lines.push(short);
        } else {
            omitted += 1;
        }
    }
    lines.reverse();
    let mut text = String::new();
    if !past.is_empty() {
        text.push_str(&past);
        text.push('\n');
    }
    if omitted > 0 {
        text.push_str(&format!("[{} earlier turn(s) omitted]\n", omitted));
    }
    for l in &lines {
        text.push_str(l);
        text.push('\n');
    }
    // The change log of the session (results the team recorded, newest within the byte cap), so a summary of the chat
    // knows what was done and when without replaying it.
    let mut kept: Vec<&String> = Vec::new();
    let mut size = 0;
    for l in changes.iter().rev().take(CHANGES_LINES) {
        if size + l.len() > CHANGES_BYTES {
            break;
        }
        size += l.len() + 1;
        kept.push(l);
    }
    if !kept.is_empty() {
        text.push_str(&format!("Changes recorded this session{}:\n", if kept.len() < changes.len() { format!(" (newest {} of {})", kept.len(), changes.len()) } else { String::new() }));
        for l in kept.iter().rev() {
            text.push_str(&format!("- {}\n", l));
        }
    }
    Some(Preamble { text: text.trim_end().to_string(), ceiling, turns: turns.len(), digested, omitted, earlier: past_n })
}

/// Digest lines of the mission's earlier sessions, newest last, within `EARLIER_BYTES`.
fn past_sessions(earlier: &[Digest]) -> (String, usize) {
    let mut out = String::new();
    let mut n = 0;
    for d in earlier.iter().rev().take(EARLIER_SESSIONS) {
        let asks = d.asks.iter().take(4).cloned().collect::<Vec<_>>().join("; ");
        // A summary, where there is one, says more than the first lines do; it is model text, so it is shown as context like the rest.
        let said = if let Some(s) = &d.summary { s.clone() } else if asks.is_empty() { format!("recorded {}", d.changes.iter().take(3).cloned().collect::<Vec<_>>().join("; ")) } else { format!("asked {}", asks) };
        let line = format!("Earlier session {} ({} turn(s), ended {}): {}\n", d.session_id, d.turns, d.ended.get(..10).unwrap_or(&d.ended), said);
        if out.len() + line.len() > EARLIER_BYTES {
            break;
        }
        out = line + &out;
        n += 1;
    }
    (out.trim_end().to_string(), n)
}

fn clip_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max.saturating_sub(1);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(seq: u64, kind: &str, task: &str, payload: serde_json::Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m", kind, payload);
        e.seq = seq;
        e.timestamp = format!("2026-10-25T10:00:{:02}Z", seq);
        e.task_id = Some(task.into());
        e
    }
    fn ask(seq: u64, t: &str, p: &str) -> EventEnvelope {
        ev(seq, "task.created", t, json!({"prompt": p, "target": "orchestrator"}))
    }
    fn say(seq: u64, t: &str, p: &str) -> EventEnvelope {
        ev(seq, "task.output", t, json!({"status": "done", "text": p}))
    }
    fn started(seq: u64, id: &str) -> EventEnvelope {
        ev(seq, "session.started", "", json!({"session_id": id, "reason": "new"}))
    }

    #[test]
    fn recorded_changes_reach_the_preamble_and_the_digest_and_nothing_else_of_the_work_does() {
        let change = |seq: u64, role: &str, said: &str| ev(seq, "change.recorded", "t_1", json!({"summary": said, "role": role}));
        let log = vec![ask(1, "t_1", "tidy the permission model"), change(2, "builder", "reworked the permission model to be less intrusive"), change(3, "writer", "removed ratify requirements for explicit updates"), say(4, "t_1", "done")];
        let p = preamble(&log, &[], 6000).unwrap();
        assert!(p.text.contains("Changes recorded this session:\n- 10:00 builder: reworked the permission model to be less intrusive\n- 10:00 writer: removed ratify requirements for explicit updates"), "{}", p.text);
        // Changes alone are worth saying, even before anything was asked.
        assert!(preamble(&log[1..3], &[], 6000).is_some());
        assert!(preamble(&[], &[], 6000).is_none());
        // Only the newest are carried, with a count of how many there were.
        let many: Vec<_> = (0..40).map(|i| change(i + 1, "builder", &format!("change {}", i))).collect();
        let t = preamble(&many, &[], 6000).unwrap().text;
        assert!(t.contains("(newest 15 of 40)") && t.contains("change 39") && !t.contains("change 24\n") && t.contains("change 25"), "{}", t);
        let d = digest("m", "w", "s_0001", "new", &log).unwrap();
        assert_eq!(d.changes, vec!["10:00 builder: reworked the permission model to be less intrusive", "10:00 writer: removed ratify requirements for explicit updates"]);
        // A digest line written before the change log existed still loads.
        let mut old = serde_json::to_value(&d).unwrap();
        old.as_object_mut().unwrap().remove("changes");
        assert!(serde_json::from_value::<Digest>(old).unwrap().changes.is_empty());
    }

    #[test]
    fn a_session_with_recorded_changes_but_no_questions_still_leaves_a_digest() {
        let log = vec![ev(1, "change.recorded", "t_1", json!({"summary": "rotated the timer notes", "role": "builder"}))];
        assert!(!has_turns(&log) && has_content(&log));
        let d = digest("m", "w", "s_0001", "new", &log).expect("changes alone are worth a digest");
        assert_eq!((d.turns, d.asks.len(), d.changes.len()), (0, 0, 1));
        let (line, _) = past_sessions(&[d]);
        assert!(line.contains("recorded 10:00 builder: rotated the timer notes") && !line.contains("asked"), "{}", line);
        assert!(digest("m", "w", "s_0001", "new", &[say(1, "t_1", "hi")]).is_none(), "an answer alone is still nothing");
    }

    #[test]
    fn a_session_runs_from_the_newest_start_and_a_log_without_one_is_a_single_session() {
        let log = vec![ask(1, "t_1", "a"), started(2, "s_0001"), ask(3, "t_2", "b"), started(4, "s_0002"), ask(5, "t_3", "c")];
        let (id, s) = current(&log);
        assert_eq!((id.as_str(), s.len()), ("s_0002", 1));
        assert_eq!(next_id(&log), "s_0003");
        let (id, s) = current(&log[..1]);
        assert_eq!((id.as_str(), s.len()), (IMPLICIT, 1));
        assert_eq!(next_id(&log[..1]), "s_0001");
    }

    #[test]
    fn the_latest_exchange_is_kept_whole_older_turns_shrink_to_lines_and_the_oldest_are_counted() {
        let big = "word ".repeat(400);
        let mut log = Vec::new();
        for i in 0..12u64 {
            log.push(ask(i * 2 + 1, &format!("t_{}", i), &format!("question {} {}", i, big)));
            log.push(say(i * 2 + 2, &format!("t_{}", i), &format!("answer {} {}", i, big)));
        }
        let p = preamble(&log, &[], 1500).unwrap();
        assert!(p.text.len() <= 1500 * 4, "within the budget: {}", p.text.len());
        assert!(p.text.contains("answer 11") && p.text.contains("question 11"), "the latest exchange is whole");
        assert!(p.digested > 0 && p.omitted > 0 && p.text.contains("earlier turn(s) omitted"), "{} {}", p.digested, p.omitted);
        let tiny = preamble(&log, &[], 10).unwrap();
        assert!(tiny.text.contains("answer 11"), "even a tiny budget keeps the newest turn: {}", tiny.text);
        assert!(preamble(&log, &[], 0).is_none(), "zero turns the preamble off");
        assert!(preamble(&[], &[], 100).is_none());
    }

    #[test]
    fn a_reply_that_read_untrusted_content_lowers_the_ceiling_of_what_follows() {
        let log = vec![ask(1, "t_1", "read the page"), ev(2, "span.taint", "t_1", json!({"ceiling": "L0", "cause": "docs.search"})), say(3, "t_1", "it says x")];
        assert_eq!(preamble(&log, &[], 500).unwrap().ceiling, Tier::L0);
        assert_eq!(preamble(&[ask(1, "t_1", "hi"), say(2, "t_1", "hello")], &[], 500).unwrap().ceiling, Tier::L5);
    }

    #[test]
    fn builds_and_councils_are_not_conversation() {
        let mut b = ask(1, "t_1", "build it");
        b.payload["flow"] = json!("build");
        assert!(!has_turns(&[b]));
        assert!(has_turns(&[ask(1, "t_1", "hi")]));
        assert!(digest("m", "w", "s_0001", "new", &[]).is_none(), "an empty session leaves no digest");
    }

    #[test]
    fn a_digest_keeps_the_gist_not_the_text_and_long_sessions_keep_both_ends() {
        let mut log = Vec::new();
        for i in 0..20u64 {
            log.push(ask(i * 2 + 1, &format!("t_{}", i), &format!("question {}\nmore", i)));
            log.push(say(i * 2 + 2, &format!("t_{}", i), &format!("answer {}\nmore", i)));
        }
        let d = digest("m", "w", "s_0001", "restart", &log).unwrap();
        assert_eq!((d.turns, d.ended_by.as_str()), (20, "restart"));
        assert_eq!(d.asks.len(), DIGEST_ITEMS * 2 + 1);
        assert_eq!(d.asks[0], "question 0");
        assert!(d.asks[DIGEST_ITEMS].contains("8 more"), "{:?}", d.asks);
        assert_eq!(d.asks.last().unwrap(), "question 19");
    }

    #[test]
    fn digests_go_to_one_central_log_filtered_by_mission_and_redacted() {
        let dir = std::env::temp_dir().join(format!("delphi-session-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = vec![ask(1, "t_1", "use key sk-secret"), say(2, "t_1", "ok")];
        let d = digest("m", "w", "s_0001", "new", &log).unwrap();
        append_central(&dir, &d, &|s| s.replace("sk-secret", "[redacted]")).unwrap();
        let mut other = d.clone();
        other.mission_id = "other".into();
        append_central(&dir, &other, &|s| s.to_string()).unwrap();
        let got = earlier(&dir, "m", 5);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].asks, vec!["use key [redacted]"]);
        let p = preamble(&[], &got, 500).unwrap();
        assert!(p.text.contains("Earlier session s_0001") && !p.text.contains("sk-secret"), "{}", p.text);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
