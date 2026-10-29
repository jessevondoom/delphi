//! `delphi sessions`: the central log of finished console sessions, to review later (`specs/030` §4). Read-only; names what was asked and
//! answered in the first words, the changes agents recorded, and the summary when one was written. Nothing here calls a model.

use delphi_engine::session::{self, Digest};
use std::path::Path;

pub struct Options {
    pub mission: Option<String>,
    pub json: bool,
    pub limit: usize,
    /// Every ask and answer the digest kept, not just the first few.
    pub full: bool,
}

fn day(ts: &str) -> String {
    ts.get(..16).map(|t| t.replace('T', " ")).unwrap_or_default()
}

/// The sessions, newest first, as text.
pub fn render(all: &[Digest], o: &Options) -> String {
    let mut out = String::new();
    let shown: Vec<&Digest> = all.iter().rev().filter(|d| o.mission.as_deref().is_none_or(|m| d.mission_id == m)).take(o.limit).collect();
    if shown.is_empty() {
        return "no finished sessions yet: a session is written here when you start a new one (/new) or the daemon restarts\n".into();
    }
    for d in shown {
        out.push_str(&format!("{}  {}  {}  {} ask{} · ended by {}{}{}\n", d.session_id, day(&d.started), d.mission_id, d.turns, if d.turns == 1 { "" } else { "s" }, d.ended_by, if d.tainted { " · read content that is not validated" } else { "" }, if d.escalated > 0 { format!(" · {} stopped", d.escalated) } else { String::new() }));
        if let Some(s) = &d.summary {
            out.push_str(&format!("  summary: {}\n", s));
        }
        let cap = if o.full { usize::MAX } else { 3 };
        for a in d.asks.iter().take(cap) {
            out.push_str(&format!("  asked: {}\n", a));
        }
        if o.full {
            for a in &d.answers {
                out.push_str(&format!("  answered: {}\n", a));
            }
        }
        for c in d.changes.iter().take(cap) {
            out.push_str(&format!("  recorded: {}\n", c));
        }
        out.push('\n');
    }
    out
}

pub fn cli(home: &Path, o: &Options) -> i32 {
    let all = session::every(home);
    if o.json {
        let rows: Vec<&Digest> = all.iter().rev().filter(|d| o.mission.as_deref().is_none_or(|m| d.mission_id == m)).take(o.limit).collect();
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into()));
    } else {
        print!("{}", render(&all, o));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(id: &str, mission: &str, started: &str) -> Digest {
        Digest { session_id: id.into(), mission_id: mission.into(), workspace: "w".into(), started: started.into(), ended: started.into(), ended_by: "new".into(), turns: 2, asks: vec!["pick a queue".into(), "do the second".into(), "third".into(), "fourth-one".into()], answers: vec!["Redis or Postgres".into()], tainted: false, escalated: 0, changes: vec!["10:00 builder: added claim.rs".into()], summary: None }
    }

    #[test]
    fn sessions_are_listed_newest_first_with_what_was_asked_and_the_summary_when_there_is_one() {
        let mut a = d("s_0001", "m1", "2026-10-25T10:00:00.000Z");
        a.summary = Some("They chose Postgres.".into());
        let b = d("s_0002", "m2", "2026-10-26T14:41:00.000Z");
        let o = Options { mission: None, json: false, limit: 20, full: false };
        let text = render(&[a.clone(), b.clone()], &o);
        assert!(text.find("s_0002").unwrap() < text.find("s_0001").unwrap(), "{}", text);
        assert!(text.contains("s_0002  2026-10-26 14:41  m2  2 asks · ended by new") && text.contains("  summary: They chose Postgres.") && text.contains("  asked: pick a queue") && text.contains("  recorded: 10:00 builder: added claim.rs"), "{}", text);
        assert!(!text.contains("fourth-one"), "three asks by default");
        assert!(render(&[a.clone(), b.clone()], &Options { full: true, ..o_full() }).contains("fourth-one"));
        assert!(render(&[a, b], &Options { mission: Some("m1".into()), ..o_full() }).lines().all(|l| !l.contains("s_0002")));
        assert!(render(&[], &o).contains("no finished sessions yet"));
    }

    fn o_full() -> Options {
        Options { mission: None, json: false, limit: 20, full: false }
    }
}
