//! The carrier conformance record (`specs/132` §4.1, slice C6): what `delphi endpoints test --probe` found, row by row, for one
//! endpoint, model and CLI version, in `local/carrier-conformance.json`.
//!
//! A row is `met`, `not_met` or `unmeasured`, with a short note and a count where one applies; the record never holds prompt or
//! reply text. A record is evidence for the CLI version it names only: a newer CLI is "unprobed" (C9) until probed again.
//!
//! Built here: C2 (the bare call), C5 (the served model from the CLI's own events), C6 (the tool level: at L0 the tools the stream
//! reported; for `agy` behind its gate, the gate record, folded in as this record's C6 row). Not built here, recorded as
//! `unmeasured` with the reason: C3 (the input ladder is spec 131 S6) and C4 (one schema answer, not probed yet).
//! Nothing reads this record to grant anything: `local/agy-gate.json` stays what a span reads for the agy gate.

use crate::capabilities::{Capability, GateRecord};
use crate::inventory::SeatRecord;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// How long a record is trusted, as the agy gate record.
pub const TTL_SECS: i64 = crate::capabilities::GATE_TTL_SECS;
/// The file, under the workspace.
pub const FILE: &str = "local/carrier-conformance.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Met,
    NotMet,
    Unmeasured,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Met => "met",
            Status::NotMet => "not_met",
            Status::Unmeasured => "unmeasured",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// `C2`, `C3`, ...
    pub row: String,
    pub status: Status,
    /// Why, in Delphi's words (never the model's).
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub endpoint_id: String,
    pub harness: String,
    pub model: String,
    /// `<cli> --version`, first line; empty when it could not be read (then no version can match it).
    pub cli_version: String,
    pub at: i64,
    /// `L0` (no tools) or `L1` (read-only Delphi tools behind Delphi's gate), as declared by the profile.
    pub tool_level: String,
    pub rows: Vec<Row>,
}

impl Record {
    pub fn is_fresh(&self, now: i64) -> bool {
        now - self.at < TTL_SECS
    }

    pub fn row(&self, name: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.row == name)
    }
}

fn row(name: &str, status: Status, note: impl Into<String>, count: Option<u64>) -> Row {
    Row { row: name.into(), status, note: note.into(), count }
}

/// The tool level a profile declares for a carrier: `agy` and `claude` with Delphi's bridge on are L1; every other carrier is L0
/// until spec 132 C8 (Copilot L1) or X5 (Codex) is built and probed.
pub fn declared_level(harness: &str, bridge: bool) -> &'static str {
    match harness {
        "agy" | "claude" if bridge => "L1",
        _ => "L0",
    }
}

/// The record for one probed harness endpoint, from what the probe saw, dated when the probe ran. The served model is the one the
/// CLI reported in its own events (`cap.served_model`).
pub fn from_probe(cap: &Capability, seat: Option<&SeatRecord>, gate: Option<&GateRecord>, harness: &str, bridge: bool, family: &str, cli_version: Option<&str>) -> Record {
    let level = declared_level(harness, bridge);
    let mut rows = Vec::new();
    // C2: the bare call answered. Only the class of failure is kept: the CLI's text may quote the reply.
    let c2 = cap.auth == "ok";
    rows.push(if c2 { row("C2", Status::Met, "the bare call answered", None) } else { row("C2", Status::NotMet, format!("the bare call failed ({})", cap.auth.split(':').next().unwrap_or("failed")), None) });
    // C5: the served model, from the events. A carrier that reports none is `unknown`, never assumed to be the configured one.
    rows.push(match (c2, cap.served_model.as_deref()) {
        (false, _) => row("C5", Status::Unmeasured, "not reached: C2 failed", None),
        (true, None) => row("C5", Status::NotMet, "the CLI reported no served model, so its family is unknown", None),
        (true, Some(m)) => {
            let got = delphi_adapter::config::family_of_model(m);
            if m.eq_ignore_ascii_case(&cap.model_id) || got.as_deref() == Some(family) {
                row("C5", Status::Met, format!("served `{}`, family {}", m, got.as_deref().unwrap_or(family)), None)
            } else {
                row("C5", Status::NotMet, format!("served `{}` (family {}) where `{}` of family {} was asked for", m, got.as_deref().unwrap_or("unknown"), cap.model_id, family), None)
            }
        }
    });
    rows.push(row("C3", Status::Unmeasured, "the input ladder is spec 131 S6, not built; spec 131's default budget applies", None));
    rows.push(row("C4", Status::Unmeasured, "the schema answer is not probed yet", None));
    // C6: the tool level.
    rows.push(match (c2, level, harness, gate, seat) {
        (false, ..) => row("C6", Status::Unmeasured, "not reached: C2 failed", None),
        (true, "L1", "agy", Some(g), _) if g.verified && g.served && g.side_effects.is_empty() => row("C6", Status::Met, "L1: the agy gate was watched to deny every kind of tool tried (agy gate record)", Some(g.kinds.len() as u64)),
        (true, "L1", "agy", Some(g), _) => row("C6", Status::NotMet, if g.side_effects.is_empty() { "L1: the agy gate is unproven (agy gate record)" } else { "L1: something ran although the agy gate should have stopped it" }, Some(g.kinds.len() as u64)),
        (true, "L1", ..) => row("C6", Status::Unmeasured, "L1: one allowed read, one refused write and one refused native tool are not probed for this carrier yet", None),
        (true, _, _, _, None) => row("C6", Status::Unmeasured, "L0: the CLI sent no start-up event listing its tools", None),
        (true, _, _, _, Some(s)) if s.tools.is_empty() => row("C6", Status::Met, "L0: the stream reported no tools offered", Some(0)),
        (true, _, _, _, Some(s)) => row("C6", Status::NotMet, format!("L0: the stream reported tools offered ({})", s.tools.join(", ")), Some(s.tools.len() as u64)),
    });
    Record { endpoint_id: cap.endpoint_id.clone(), harness: harness.into(), model: cap.model_id.clone(), cli_version: cli_version.unwrap_or("").into(), at: cap.probed_at, tool_level: level.into(), rows }
}

pub fn load(path: &Path) -> Vec<Record> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Replace the record for the same endpoint, model and CLI version; keep the others (an older version's record stays, and is
/// never read for a newer one).
pub fn save(path: &Path, rec: &Record) -> std::io::Result<()> {
    let mut all: Vec<Record> = load(path).into_iter().filter(|r| !(r.endpoint_id == rec.endpoint_id && r.model == rec.model && r.cli_version == rec.cli_version)).collect();
    all.push(rec.clone());
    all.sort_by(|a, b| (&a.endpoint_id, &a.model, &a.cli_version).cmp(&(&b.endpoint_id, &b.model, &b.cli_version)));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    delphi_core::lock::write_atomic(path, serde_json::to_string_pretty(&all).expect("serializes").as_bytes())
}

/// The newest record for this endpoint on exactly this CLI version.
pub fn current<'a>(all: &'a [Record], endpoint: &str, cli_version: &str) -> Option<&'a Record> {
    all.iter().filter(|r| r.endpoint_id == endpoint && !r.cli_version.is_empty() && r.cli_version == cli_version).max_by_key(|r| r.at)
}

/// `delphi doctor`'s line for one harness endpoint (§4.1 step 5): version, rows met, the budget and its source, the tool level,
/// and "unprobed version" when the CLI moved since the last record.
pub fn doctor_line(all: &[Record], endpoint: &str, installed: Option<&str>, now: i64) -> String {
    const BUDGET: &str = "input budget: none enforced yet (spec 131 S2 not built), ceiling unmeasured";
    let Some(v) = installed else { return format!("conformance: the CLI version could not be read, so no record applies; L0, {BUDGET}") };
    match current(all, endpoint, v) {
        Some(r) => {
            let named = |s: Status| r.rows.iter().filter(|x| x.status == s).map(|x| x.row.as_str()).collect::<Vec<_>>().join(", ");
            let (met, failed) = (named(Status::Met), named(Status::NotMet));
            let age = if r.is_fresh(now) { format!("{}d ago", (now - r.at).max(0) / 86400) } else { "STALE, probe again".into() };
            let met = if met.is_empty() { "none".to_string() } else { met };
            let failed = if failed.is_empty() { String::new() } else { format!(", NOT met {failed}") };
            format!("conformance on {v} ({age}): met {met}{failed}; tool level {}; {BUDGET}", r.tool_level)
        }
        None => match all.iter().filter(|r| r.endpoint_id == endpoint).max_by_key(|r| r.at) {
            Some(old) => format!("conformance: unprobed version: {v} is installed, the last record is for {}; the seat stays at L0 with spec 131's default budget until `delphi endpoints test --probe` runs again", if old.cli_version.is_empty() { "an unknown version" } else { &old.cli_version }),
            None => format!("conformance: not probed; run `delphi endpoints test --probe`. L0, {BUDGET}"),
        },
    }
}

/// What one probe of a harness endpoint will cost, printed before anything runs (§4.1 step 2): calls, Delphi's own prompt in
/// tokens (the CLI adds its own system text, which is not counted here), and for Copilot the premium requests from the catalog
/// multiplier. The cost class is display only.
pub fn estimate_line(endpoint: &str, harness: &str, model: &str, prompt_bytes: usize, multiplier: Option<f64>) -> String {
    let tokens = prompt_bytes.div_ceil(4);
    let premium = match (harness, multiplier) {
        ("copilot", Some(m)) => format!(", about {} premium request(s) ({}x)", m, m),
        ("copilot", None) => ", premium requests unknown (no multiplier for this model)".into(),
        _ => String::new(),
    };
    format!("{endpoint:<16} {harness} `{model}`: 1 call, about {tokens} input tokens of Delphi's prompt plus the CLI's own{premium}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(auth: &str, served: Option<&str>) -> Capability {
        Capability { endpoint_id: "e".into(), model_id: "claude-haiku-4.5".into(), path: "harness".into(), degraded: false, probed_at: 5, auth: auth.into(), usage_reported: None, tool_calling: None, reasoning: None, signature_round_trip: None, cache: String::new(), tokens_used: 0, cost_usd: None, notes: vec![], served_model: served.map(Into::into) }
    }

    fn seat(tools: &[&str]) -> SeatRecord {
        SeatRecord { endpoint_id: "e".into(), harness: "copilot".into(), model: "m".into(), tools: tools.iter().map(|t| t.to_string()).collect(), mcp_servers: vec![], bridge: false, seen_unix: 5 }
    }

    fn status(r: &Record, name: &str) -> Status {
        r.row(name).unwrap().status
    }

    #[test]
    fn rows_are_judged_from_what_the_cli_reported_and_hold_no_reply_text() {
        let r = from_probe(&cap("ok", Some("claude-haiku-4.5")), Some(&seat(&[])), None, "copilot", true, "claude", Some("GitHub Copilot CLI 1.0.112."));
        assert_eq!((status(&r, "C2"), status(&r, "C5"), status(&r, "C6"), status(&r, "C3"), status(&r, "C4")), (Status::Met, Status::Met, Status::Met, Status::Unmeasured, Status::Unmeasured));
        assert_eq!((r.tool_level.as_str(), r.cli_version.as_str()), ("L0", "GitHub Copilot CLI 1.0.112."), "Copilot is L0 whatever its approval bridge says");
        // A failed call keeps only the class of failure, never the CLI's text (which may quote the reply).
        let r = from_probe(&cap("failed: the secret reply text", None), None, None, "copilot", false, "claude", Some("v"));
        assert_eq!((status(&r, "C2"), status(&r, "C5"), status(&r, "C6")), (Status::NotMet, Status::Unmeasured, Status::Unmeasured));
        assert!(!serde_json::to_string(&r).unwrap().contains("secret"));
        // No served model is unknown, never the configured one; another family's model is a mismatch.
        assert_eq!(status(&from_probe(&cap("ok", None), None, None, "copilot", false, "claude", Some("v")), "C5"), Status::NotMet);
        let r = from_probe(&cap("ok", Some("gpt-6-luna")), None, None, "copilot", false, "claude", Some("v"));
        assert!(status(&r, "C5") == Status::NotMet && r.row("C5").unwrap().note.contains("openai"), "{:?}", r.row("C5"));
        // At L0, tools in the stream fail C6.
        let r = from_probe(&cap("ok", Some("claude-haiku-4.5")), Some(&seat(&["shell", "write"])), None, "codex", false, "claude", Some("v"));
        assert_eq!((status(&r, "C6"), r.row("C6").unwrap().count), (Status::NotMet, Some(2)));
    }

    #[test]
    fn the_agy_gate_record_is_the_agy_c6_row() {
        let gate = |verified: bool, side: &[&str]| GateRecord { endpoint_id: "e".into(), agy_version: "1.4.7".into(), at: 1, attempted: vec![], kinds: vec!["shell".into(), "write".into()], served: true, side_effects: side.iter().map(|s| s.to_string()).collect(), verified, builder_ready: false };
        let c = cap("ok", Some("gemini-3.8-flash"));
        let r = from_probe(&c, None, Some(&gate(true, &[])), "agy", true, "gemini", Some("1.4.7"));
        assert_eq!((r.tool_level.as_str(), status(&r, "C6"), r.row("C6").unwrap().count), ("L1", Status::Met, Some(2)));
        let r = from_probe(&c, None, Some(&gate(false, &["conf-write.txt was created"])), "agy", true, "gemini", Some("1.4.7"));
        assert!(status(&r, "C6") == Status::NotMet && r.row("C6").unwrap().note.contains("something ran"));
        let r = from_probe(&c, None, None, "claude", true, "gemini", Some("1"));
        assert_eq!((r.tool_level.as_str(), status(&r, "C6")), ("L1", Status::Unmeasured));
    }

    #[test]
    fn a_record_counts_for_its_own_cli_version_only_and_doctor_says_so() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(FILE);
        let mk = |v: &str, at: i64| from_probe(&Capability { probed_at: at, ..cap("ok", Some("claude-haiku-4.5")) }, Some(&seat(&[])), None, "copilot", false, "claude", Some(v));
        save(&p, &mk("1.0.105", 100)).unwrap();
        save(&p, &mk("1.0.112", 200)).unwrap();
        save(&p, &mk("1.0.112", 300)).unwrap();
        let all = load(&p);
        assert_eq!(all.len(), 2, "one record per endpoint, model and version");
        assert_eq!(current(&all, "e", "1.0.112").unwrap().at, 300);
        let line = doctor_line(&all, "e", Some("1.0.112"), 300);
        assert!(line.contains("conformance on 1.0.112") && line.contains("met C2, C5, C6") && line.contains("tool level L0") && line.contains("spec 131 S2 not built"), "{line}");
        let mut failed = all.clone();
        failed[1].rows[1].status = Status::NotMet;
        let line = doctor_line(&failed, "e", Some("1.0.112"), 300);
        assert!(line.contains("met C2, C6, NOT met C5"), "{line}");
        let moved = doctor_line(&all, "e", Some("1.0.120"), 300);
        assert!(moved.contains("unprobed version") && moved.contains("1.0.112") && moved.contains("L0"), "{moved}");
        assert!(doctor_line(&all, "other", Some("1.0.112"), 300).contains("not probed"));
        assert!(doctor_line(&all, "e", Some("1.0.112"), 300 + TTL_SECS).contains("STALE"));
        assert!(doctor_line(&all, "e", None, 300).contains("could not be read"));
        // A record with no version never matches.
        save(&p, &mk("", 400)).unwrap();
        assert!(current(&load(&p), "e", "").is_none());
        std::fs::write(&p, "not json").unwrap();
        assert!(load(&p).is_empty(), "a damaged file is no evidence");
    }

    #[test]
    fn the_estimate_counts_premium_requests_for_copilot_only() {
        assert!(estimate_line("cp", "copilot", "gemini-3.8-flash", 400, Some(14.0)).contains("about 100 input tokens") && estimate_line("cp", "copilot", "m", 4, Some(14.0)).contains("about 14 premium request(s)"));
        assert!(estimate_line("cp", "copilot", "m", 4, None).contains("premium requests unknown"));
        assert!(!estimate_line("ag", "agy", "m", 4, None).contains("premium"));
    }
}
