//! Research that was asked for, and what became of the request (`specs/119` §2 items 3 and 4, §5 item 1).
//!
//! One line per `request_research` call in `local/research-asks.jsonl`: when, and whether the job was queued or refused because no
//! search engine is set up. No question text, no session and no caller is written: the file answers three counting questions and
//! nothing else.
//!
//! - `delphi doctor` warns about a missing engine only when research was asked for in the last 30 days ([`counts_since`]).
//! - The next session-start brief says once how often research was refused for want of an engine ([`take_unmentioned`]); the
//!   marker beside the file remembers how far that has been said.
//! - `delphi kb health` reports requests in 30 days and how many ran.
//!
//! Everything here is best effort: a line that cannot be written loses a count, never a request.

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

const FILE: &str = "local/research-asks.jsonl";
const MENTIONED: &str = "local/research-asks.mentioned";
/// Lines older than this are dropped when the file is next written past [`PRUNE_BYTES`].
const KEEP_SECS: i64 = 90 * 86_400;
const PRUNE_BYTES: u64 = 256 * 1024;
pub const THIRTY_DAYS: i64 = 30 * 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The job was accepted and queued.
    Queued,
    /// Refused: no search engine is set up.
    NoEngine,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Queued => "queued",
            Outcome::NoEngine => "no_engine",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub asked: usize,
    pub ran: usize,
    pub no_engine: usize,
}

fn file(ws_dir: &Path) -> PathBuf {
    ws_dir.join(FILE)
}

/// (unix time, outcome) of every readable line.
fn lines(ws_dir: &Path) -> Vec<(i64, String)> {
    let Ok(text) = std::fs::read_to_string(file(ws_dir)) else { return Vec::new() };
    text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter_map(|v| Some((v["unix"].as_i64()?, v["outcome"].as_str()?.to_string()))).collect()
}

/// Note one request. `now` is Unix seconds. Nothing is created unless the workspace's `local/` directory already exists.
pub fn record(ws_dir: &Path, outcome: Outcome, now: i64) {
    let path = file(ws_dir);
    if !path.parent().is_some_and(Path::is_dir) {
        return;
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > PRUNE_BYTES) {
        let kept: String = lines(ws_dir).into_iter().filter(|(at, _)| *at >= now - KEEP_SECS).map(|(at, o)| format!("{}\n", json!({"unix": at, "outcome": o}))).collect();
        let _ = std::fs::write(&path, kept);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", json!({"unix": now, "outcome": outcome.as_str()}));
    }
}

/// Requests at or after `since` (Unix seconds). Reads only; a missing file is no requests.
pub fn counts_since(ws_dir: &Path, since: i64) -> Counts {
    let mut c = Counts::default();
    for (at, outcome) in lines(ws_dir) {
        if at < since {
            continue;
        }
        c.asked += 1;
        match outcome.as_str() {
            "queued" => c.ran += 1,
            "no_engine" => c.no_engine += 1,
            _ => {}
        }
    }
    c
}

/// How many requests were refused for want of an engine since this was last asked, and remember that they have now been said. `0`
/// when there are none or when the marker cannot be written (a refusal that would be repeated in every brief is not said at all).
pub fn take_unmentioned(ws_dir: &Path) -> usize {
    let said: i64 = std::fs::read_to_string(ws_dir.join(MENTIONED)).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(i64::MIN);
    let refused: Vec<i64> = lines(ws_dir).into_iter().filter(|(at, o)| o == "no_engine" && *at > said).map(|(at, _)| at).collect();
    let Some(latest) = refused.iter().max() else { return 0 };
    match std::fs::write(ws_dir.join(MENTIONED), latest.to_string()) {
        Ok(()) => refused.len(),
        Err(_) => 0,
    }
}

/// The brief's sentence (`specs/119` §2 item 4): fixed words and a number.
pub fn brief_line(n: usize) -> String {
    format!("Research was asked for {} time{} and no search engine is set up: the operator can run `{}` at a terminal.", n, if n == 1 { "" } else { "s" }, crate::research_setup::SETUP_COMMAND)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("local")).unwrap();
        t
    }

    #[test]
    fn requests_are_counted_by_outcome_within_a_window_and_carry_no_text() {
        let t = ws();
        let now = 1_800_000_000;
        record(t.path(), Outcome::Queued, now - 40 * 86_400);
        record(t.path(), Outcome::NoEngine, now - 86_400);
        record(t.path(), Outcome::NoEngine, now - 60);
        record(t.path(), Outcome::Queued, now);
        assert_eq!(counts_since(t.path(), now - THIRTY_DAYS), Counts { asked: 3, ran: 1, no_engine: 2 });
        assert_eq!(counts_since(t.path(), 0).asked, 4);
        let text = std::fs::read_to_string(t.path().join(FILE)).unwrap();
        for l in text.lines() {
            let v: Value = serde_json::from_str(l).unwrap();
            assert_eq!(v.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["outcome", "unix"], "{l}");
        }
    }

    #[test]
    fn a_refusal_is_mentioned_once_and_a_later_one_once_more() {
        let t = ws();
        assert_eq!(take_unmentioned(t.path()), 0, "nothing was asked");
        record(t.path(), Outcome::NoEngine, 100);
        record(t.path(), Outcome::NoEngine, 200);
        record(t.path(), Outcome::Queued, 300);
        assert_eq!(take_unmentioned(t.path()), 2);
        assert_eq!(take_unmentioned(t.path()), 0, "said once");
        record(t.path(), Outcome::NoEngine, 400);
        assert_eq!(take_unmentioned(t.path()), 1);
        assert_eq!(brief_line(1), "Research was asked for 1 time and no search engine is set up: the operator can run `delphi research setup` at a terminal.");
        assert!(brief_line(17).starts_with("Research was asked for 17 times and no search engine is set up"));
    }

    #[test]
    fn nothing_is_created_where_there_is_no_workspace_and_an_old_file_is_pruned() {
        let t = tempfile::tempdir().unwrap();
        record(&t.path().join("workspaces/none"), Outcome::Queued, 1);
        assert!(std::fs::read_dir(t.path()).unwrap().next().is_none());
        assert_eq!((counts_since(t.path(), 0), take_unmentioned(t.path())), (Counts::default(), 0));

        let t = ws();
        let now = 1_800_000_000;
        let old: String = (0..12_000).map(|i| format!("{}\n", json!({"unix": now - KEEP_SECS - 10 - i, "outcome": "queued"}))).collect();
        std::fs::write(t.path().join(FILE), old).unwrap();
        record(t.path(), Outcome::Queued, now);
        assert_eq!(counts_since(t.path(), 0).asked, 1, "lines past ninety days went when the file grew large");
    }
}
