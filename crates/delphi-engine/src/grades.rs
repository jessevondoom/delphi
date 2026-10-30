//! The record of promote and nerf (`specs/056` §4). A grade lives in the ratified `[grades]` table; this file keeps the
//! `agent.graded` records of who proposed which change and why, in `<ws>/local/grades.jsonl`, outside any mission (a grade
//! belongs to the workspace). It is the operator's journal: no span can write it.

use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

const FILE: &str = "local/grades.jsonl";

/// One `agent.graded {role, kind, from, to, reason}` record. `state` is `proposed` until `/ratify` applies it.
pub fn record(ws_dir: &Path, role: &str, kind: Option<&str>, from: i32, to: i32, reason: &str, at: i64) -> std::io::Result<()> {
    let rec = json!({"type": "agent.graded", "role": role, "kind": kind.unwrap_or(""), "from": from, "to": to, "reason": reason, "state": "proposed", "at": at});
    std::fs::create_dir_all(ws_dir.join("local"))?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(ws_dir.join(FILE))?;
    writeln!(f, "{rec}")
}

pub fn history(ws_dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(ws_dir.join(FILE)).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// The measured results for a role, all models together, as "accepted/runs"; `None` when nothing is recorded.
pub fn evidence_line(ws_dir: &Path, role: &str) -> Option<String> {
    let e = crate::assign::read_evidence(ws_dir, role);
    let (runs, accepted) = e.iter().fold((0, 0), |(r, a), x| (r + x.runs, a + x.accepted));
    (runs > 0).then(|| format!("{accepted}/{runs} accepted"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_kept_and_read_back() {
        let d = tempfile::tempdir().unwrap();
        record(d.path(), "builder", Some("docs"), 0, -1, "14 of 14 first-pass", 5).unwrap();
        let h = history(d.path());
        assert_eq!((h[0]["type"].as_str(), h[0]["kind"].as_str(), h[0]["from"].as_i64(), h[0]["to"].as_i64()), (Some("agent.graded"), Some("docs"), Some(0), Some(-1)));
    }
}
