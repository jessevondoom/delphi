//! The carrier conformance record (`specs/132` §4.1, slice C6): what `delphi endpoints test --probe` found, row by row, for one
//! endpoint, model and CLI version, in `local/carrier-conformance.json`.
//!
//! A row is `met`, `not_met` or `unmeasured`, with a short note and a count where one applies; the record never holds prompt or
//! reply text. A record is evidence for the CLI version it names only: a newer CLI is "unprobed" (C9) until probed again.
//!
//! Built here: C2 (the bare call), C5 (the served model from the CLI's own events), C6 (the tool level: at L0 the tools the stream
//! reported; for `agy` behind its gate, the gate record, folded in as this record's C6 row), C9 (slice F1: the probed version
//! against the version the CLI's coverage manifest is pinned to), and C3 when the operator runs the input ladder (`specs/131` S6,
//! `--ladder`): the largest prompt answered, which can only LOWER a seat's byte budget ([`effective_budget`]).
//! Not built here, recorded as `unmeasured`: C4 (one schema answer, not probed yet).
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
    /// When the C3 row was measured by the input ladder (`specs/131` S6), which can be earlier than `at`: a probe without the ladder
    /// keeps the last ladder's row for the same version, and the row is trusted for [`TTL_SECS`] from the ladder, not from the probe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ladder_at: Option<i64>,
}

impl Record {
    pub fn is_fresh(&self, now: i64) -> bool {
        now - self.at < TTL_SECS
    }

    pub fn row(&self, name: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.row == name)
    }

    /// Put `row` in place of the row of the same name (or add it).
    pub fn set_row(&mut self, row: Row) {
        match self.rows.iter_mut().find(|r| r.row == row.row) {
            Some(r) => *r = row,
            None => self.rows.push(row),
        }
    }

    /// The largest prompt the ladder saw answered, while the ladder's measurement is fresh: the input ceiling, at least.
    pub fn measured_ceiling(&self, now: i64) -> Option<u64> {
        let r = self.row("C3")?;
        (r.status == Status::Met && now - self.ladder_at.unwrap_or(self.at) < TTL_SECS).then_some(r.count?)
    }
}

/// The input ladder's steps, in bytes of Delphi's whole prompt (system and user text): `specs/131` §8 item 2(b).
pub const LADDER_BYTES: [u64; 4] = [100_000, 150_000, 200_000, 250_000];
/// The share of the largest size answered that a measured budget allows (`specs/131` §8 item 2(b)).
pub const MEASURED_SHARE_PERCENT: u64 = 80;
/// The system text of a ladder call.
pub const LADDER_SYSTEM: &str = "This is an input-size probe. The user text is filler, and its very last line holds one code word. Reply with only that code word, nothing else. Do not call tools.";
/// A reply longer than this is not an answer to "reply with only the code word", whatever it holds (a model that repeats the prompt
/// back would hold the code word too).
const LADDER_REPLY_MAX: usize = 200;

/// The code word of one ladder step: unguessable from the filler, different at every step.
pub fn ladder_code(nonce: u64, step: usize) -> String {
    let mut x = nonce ^ (step as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    format!("ZQ{:010X}", (x ^ (x >> 31)) & 0xFF_FFFF_FFFF)
}

/// The user text of one ladder step: plain filler lines, then the code word on the last line, sized so that [`LADDER_SYSTEM`] and this
/// text together are `total` bytes.
pub fn ladder_prompt(total: u64, code: &str) -> String {
    let last = format!("The code word is {code}.");
    let want = (total as usize).saturating_sub(LADDER_SYSTEM.len() + last.len());
    let mut text = String::with_capacity(want + last.len());
    let mut n = 0u32;
    while text.len() < want {
        n += 1;
        let line = format!("Filler line {n:06}: this line carries no instruction and nothing to remember; keep reading to the end.\n");
        text.push_str(&line[..line.len().min(want - text.len())]);
    }
    text.push_str(&last);
    text
}

/// Whether the assistant's reply (never the stream as a whole, `specs/132` §2) answered a ladder step: short, and holding the code.
pub fn ladder_answered(reply: &str, code: &str) -> bool {
    let r = reply.trim();
    r.len() <= LADDER_REPLY_MAX && r.contains(code)
}

/// The C3 row from the ladder's steps in order (size, answered), which stop at the first step not answered. `count` is the largest size
/// answered; the note names the smallest that was not. Sizes only: never the code word or the reply.
pub fn ladder_row(steps: &[(u64, bool)]) -> Row {
    let largest = steps.iter().filter(|s| s.1).map(|s| s.0).max();
    let failed = steps.iter().find(|s| !s.1).map(|s| s.0);
    match (largest, failed) {
        (None, None) => row("C3", Status::Unmeasured, "the input ladder was not run; spec 131's default budget applies", None),
        (None, Some(f)) => row("C3", Status::NotMet, format!("the input ladder's smallest step ({f} bytes) was not answered, so no ceiling is known; the budget is not lowered by this"), None),
        (Some(l), None) => row("C3", Status::Met, format!("the input ladder answered every step up to {l} bytes (assistant reply only); the ceiling is at least that"), Some(l)),
        (Some(l), Some(f)) => row("C3", Status::Met, format!("the input ladder answered {l} bytes and not {f} bytes (assistant reply only)"), Some(l)),
    }
}

/// The budget a seat on `endpoint` gets, and where it came from: the operator's `[harness.<cli>] max_prompt_bytes` or the default, lowered
/// to [`MEASURED_SHARE_PERCENT`] of the largest size the ladder saw answered on exactly the installed CLI version while that measurement is
/// fresh. A measurement only ever lowers the budget, never raises it (`specs/131` §8 item 2(b)); a record of another version is ignored.
pub fn effective_budget(configured: Option<u64>, all: &[Record], endpoint: &str, installed: Option<&str>, now: i64) -> (u64, String) {
    let base = crate::prompt_budget::budget(configured);
    let from = if configured.is_some_and(|n| n > 0) { "config" } else { "default" };
    let measured = installed.and_then(|v| all.iter().filter(|r| r.endpoint_id == endpoint && !r.cli_version.is_empty() && r.cli_version == v).filter_map(|r| r.measured_ceiling(now)).min().map(|c| (c, v)));
    match measured {
        Some((ceiling, v)) if ceiling * MEASURED_SHARE_PERCENT / 100 < base => (ceiling * MEASURED_SHARE_PERCENT / 100, format!("measured on {v}: {MEASURED_SHARE_PERCENT}% of {ceiling} bytes answered, below the {from} {base}")),
        Some((ceiling, _)) => (base, format!("{from}; the measured ceiling ({ceiling} bytes answered) does not lower it")),
        None => (base, format!("{from}, unmeasured")),
    }
}

/// The budget part of `delphi doctor`'s line.
pub fn budget_text(configured: Option<u64>, all: &[Record], endpoint: &str, installed: Option<&str>, now: i64) -> String {
    let (b, source) = effective_budget(configured, all, endpoint, installed, now);
    format!("input budget {b} bytes ({source})")
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
    rows.push(ladder_row(&[]));
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
    rows.push(pin_row(harness, cli_version));
    Record { endpoint_id: cap.endpoint_id.clone(), harness: harness.into(), model: cap.model_id.clone(), cli_version: cli_version.unwrap_or("").into(), at: cap.probed_at, tool_level: level.into(), rows, ladder_at: None }
}

/// The C3 row of a probe, with its date: the ladder's when it ran (`steps`), else the last ladder's row kept from `previous` (the record
/// for the same endpoint, model and version), else unmeasured. A probe without the ladder never drops a measurement it did not repeat.
pub fn with_ladder(mut rec: Record, steps: Option<&[(u64, bool)]>, previous: Option<&Record>) -> Record {
    match (steps, previous.and_then(|p| p.row("C3").filter(|r| r.status != Status::Unmeasured).map(|r| (r.clone(), p.ladder_at.unwrap_or(p.at))))) {
        (Some(s), _) => {
            rec.set_row(ladder_row(s));
            rec.ladder_at = (!s.is_empty()).then_some(rec.at);
        }
        (None, Some((row, at))) => {
            rec.set_row(row);
            rec.ladder_at = Some(at);
        }
        (None, None) => {}
    }
    rec
}

/// The record already saved for this endpoint, model and CLI version.
pub fn same<'a>(all: &'a [Record], rec: &Record) -> Option<&'a Record> {
    all.iter().find(|r| r.endpoint_id == rec.endpoint_id && r.model == rec.model && r.cli_version == rec.cli_version && !r.cli_version.is_empty())
}

/// C9, the version pin (`specs/132` §4 row C9, F1): the record names the version it ran on (always), and the CLI's coverage manifest is
/// pinned to that same version. A probed CLI newer than its manifest may offer surface Delphi has not classified, so the row is
/// `not_met`. It raises and lowers nothing by itself: it is the drift, said where the record is read.
fn pin_row(harness: &str, cli_version: Option<&str>) -> Row {
    use crate::coverage::{version_drift, VersionDrift};
    match version_drift(harness, cli_version.filter(|v| !v.is_empty())) {
        VersionDrift::Pinned(v) => row("C9", Status::Met, format!("the coverage manifest is pinned to {v}, the version probed"), None),
        VersionDrift::Newer { installed, pinned } => row("C9", Status::NotMet, format!("the CLI ({installed}) is newer than its coverage manifest ({pinned}): what it added is unclassified"), None),
        VersionDrift::Older { installed, pinned } => row("C9", Status::NotMet, format!("the CLI ({installed}) is older than its coverage manifest ({pinned})"), None),
        VersionDrift::Unknown => row("C9", Status::Unmeasured, "the CLI version or the manifest's pin could not be read as a version number", None),
    }
}

/// How an installed version relates to the version of the last record, in words.
fn relation(installed: &str, recorded: &str) -> &'static str {
    use crate::coverage::{compare_versions, version_number};
    match (version_number(installed), version_number(recorded)) {
        (Some(a), Some(b)) => match compare_versions(&a, &b) {
            Some(std::cmp::Ordering::Greater) => "newer than",
            Some(std::cmp::Ordering::Less) => "older than",
            _ => "other than",
        },
        _ => "other than",
    }
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
/// `configured` is the operator's `[harness.<cli>] max_prompt_bytes`; the budget printed is the one a seat gets ([`effective_budget`]).
pub fn doctor_line(all: &[Record], endpoint: &str, installed: Option<&str>, now: i64, configured: Option<u64>) -> String {
    let budget = budget_text(configured, all, endpoint, installed, now);
    let Some(v) = installed else { return format!("conformance: the CLI version could not be read, so no record applies; L0, {budget}") };
    match current(all, endpoint, v) {
        Some(r) => {
            let named = |s: Status| r.rows.iter().filter(|x| x.status == s).map(|x| x.row.as_str()).collect::<Vec<_>>().join(", ");
            let (met, failed) = (named(Status::Met), named(Status::NotMet));
            let age = if r.is_fresh(now) { format!("{}d ago", (now - r.at).max(0) / 86400) } else { "STALE, probe again".into() };
            let met = if met.is_empty() { "none".to_string() } else { met };
            let failed = if failed.is_empty() { String::new() } else { format!(", NOT met {failed}") };
            format!("conformance on {v} ({age}): met {met}{failed}; tool level {}; {budget}", r.tool_level)
        }
        None => match all.iter().filter(|r| r.endpoint_id == endpoint).max_by_key(|r| r.at) {
            Some(old) if old.cli_version.is_empty() => format!("conformance: unprobed version: {v} is installed, the last record is for an unknown version; the seat stays at L0 with {budget} until `delphi endpoints test --probe` runs again"),
            // A Claude seat's tools come from its own allow-list, never from this record; every other carrier is L0 without one.
            Some(old) => format!("conformance: unprobed version: {v} is installed, {} the last record's {}; {} until `delphi endpoints test --probe` runs again", relation(v, &old.cli_version), old.cli_version, if old.harness == "claude" { format!("no probe result applies to it; {budget}") } else { format!("the seat stays at L0 with {budget}") }),
            None => format!("conformance: not probed; run `delphi endpoints test --probe`. L0, {budget}"),
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

/// What the input ladder will cost for one harness endpoint, at most: every step, if none stops it early.
pub fn ladder_estimate_line(endpoint: &str, harness: &str, multiplier: Option<f64>) -> String {
    let steps = LADDER_BYTES.iter().map(|b| (b / 1000).to_string()).collect::<Vec<_>>().join(", ");
    let tokens = LADDER_BYTES.iter().sum::<u64>().div_ceil(4);
    let premium = match (harness, multiplier) {
        ("copilot", Some(m)) => format!(", up to {} premium request(s) ({}x a call)", m * LADDER_BYTES.len() as f64, m),
        ("copilot", None) => ", premium requests unknown (no multiplier for this model)".into(),
        _ => String::new(),
    };
    format!("{endpoint:<16} input ladder: up to {} calls of {steps} KB of filler, stopping at the first not answered; up to about {tokens} input tokens{premium}", LADDER_BYTES.len())
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
        let line = doctor_line(&all, "e", Some("1.0.112"), 300, None);
        assert!(line.contains("conformance on 1.0.112") && line.contains("met C2, C5, C6") && line.contains("tool level L0") && line.contains("input budget 150000 bytes (default, unmeasured)"), "{line}");
        let mut failed = all.clone();
        failed[1].rows[1].status = Status::NotMet;
        let line = doctor_line(&failed, "e", Some("1.0.112"), 300, None);
        assert!(line.contains("met C2, C6, NOT met C5"), "{line}");
        let moved = doctor_line(&all, "e", Some("1.0.120"), 300, None);
        assert!(moved.contains("unprobed version") && moved.contains("newer than the last record's 1.0.112") && moved.contains("L0"), "{moved}");
        assert!(doctor_line(&all, "e", Some("1.0.110"), 300, None).contains("older than the last record's 1.0.112"));
        assert!(doctor_line(&all, "other", Some("1.0.112"), 300, None).contains("not probed"));
        assert!(doctor_line(&all, "e", Some("1.0.112"), 300 + TTL_SECS, None).contains("STALE"));
        assert!(doctor_line(&all, "e", None, 300, None).contains("could not be read"));
        // A record with no version never matches.
        save(&p, &mk("", 400)).unwrap();
        assert!(current(&load(&p), "e", "").is_none());
        std::fs::write(&p, "not json").unwrap();
        assert!(load(&p).is_empty(), "a damaged file is no evidence");
    }

    /// specs/132 F1, row C9: the record says whether the probed version is the one the coverage manifest is pinned to.
    #[test]
    fn the_c9_row_compares_the_probed_version_with_the_manifest_pin() {
        let pin = crate::coverage::pinned_version("copilot").unwrap();
        let rec = |v: Option<&str>| from_probe(&cap("ok", Some("claude-haiku-4.5")), Some(&seat(&[])), None, "copilot", false, "claude", v);
        let at_pin = rec(Some(&format!("GitHub Copilot CLI {pin}.")));
        assert_eq!(status(&at_pin, "C9"), Status::Met, "{:?}", at_pin.row("C9"));
        let newer = rec(Some(&format!("GitHub Copilot CLI {pin}.7")));
        assert!(status(&newer, "C9") == Status::NotMet && newer.row("C9").unwrap().note.contains("newer than its coverage manifest"), "{:?}", newer.row("C9"));
        assert!(doctor_line(std::slice::from_ref(&newer),"e", Some(&newer.cli_version), newer.at, None).contains("NOT met C9"));
        assert!(rec(Some("0.0.1")).row("C9").unwrap().note.contains("older"));
        assert_eq!(status(&rec(None), "C9"), Status::Unmeasured, "an unread version is unmeasured, never met");
        // C9 is judged even when the call failed: the version and the manifest need no answer from a model.
        let failed = from_probe(&cap("failed", None), None, None, "copilot", false, "claude", Some(&pin));
        assert_eq!(status(&failed, "C9"), Status::Met);
    }

    #[test]
    fn the_estimate_counts_premium_requests_for_copilot_only() {
        assert!(estimate_line("cp", "copilot", "gemini-3.8-flash", 400, Some(14.0)).contains("about 100 input tokens") && estimate_line("cp", "copilot", "m", 4, Some(14.0)).contains("about 14 premium request(s)"));
        assert!(estimate_line("cp", "copilot", "m", 4, None).contains("premium requests unknown"));
        assert!(!estimate_line("ag", "agy", "m", 4, None).contains("premium"));
        assert!(ladder_estimate_line("cp", "copilot", Some(14.0)).contains("up to 56 premium request(s)") && ladder_estimate_line("ag", "agy", None).contains("up to 4 calls of 100, 150, 200, 250 KB"));
    }

    #[test]
    fn a_ladder_prompt_is_its_size_with_the_code_word_last_and_only_a_short_reply_holding_it_counts() {
        let code = ladder_code(42, 2);
        assert_ne!(code, ladder_code(42, 3));
        assert_ne!(code, ladder_code(43, 2));
        for size in LADDER_BYTES {
            let p = ladder_prompt(size, &code);
            assert_eq!((LADDER_SYSTEM.len() + p.len()) as u64, size);
            assert!(p.ends_with(&format!("The code word is {code}.")) && p.matches(&code).count() == 1);
        }
        assert!(ladder_answered(&format!("  {code}\n"), &code));
        assert!(!ladder_answered("I see only filler lines.", &code));
        // A reply that repeats the prompt back holds the code word, and is not an answer (the echo lesson of specs/132 §2).
        assert!(!ladder_answered(&ladder_prompt(10_000, &code), &code));
    }

    #[test]
    fn the_ladder_row_keeps_the_largest_size_answered_and_names_the_smallest_that_was_not() {
        let r = ladder_row(&[(100_000, true), (150_000, true), (200_000, false)]);
        assert_eq!((r.status, r.count), (Status::Met, Some(150_000)));
        assert!(r.note.contains("answered 150000 bytes and not 200000"), "{}", r.note);
        assert_eq!(ladder_row(&[(100_000, true), (250_000, true)]).count, Some(250_000));
        assert_eq!((ladder_row(&[(100_000, false)]).status, ladder_row(&[(100_000, false)]).count), (Status::NotMet, None));
        assert_eq!(ladder_row(&[]).status, Status::Unmeasured);
    }

    // specs/131 AC-14: the measured budget is 80% of the largest size answered, on the installed version only, and only ever lowers.
    #[test]
    fn a_measured_ceiling_only_lowers_the_budget_and_counts_for_its_own_version_while_fresh() {
        let mk = |v: &str, steps: &[(u64, bool)]| with_ladder(from_probe(&Capability { probed_at: 1_000, ..cap("ok", Some("gemini-3.8-flash")) }, None, None, "agy", true, "gemini", Some(v)), Some(steps), None);
        let all = vec![mk("1.5.13", &[(100_000, true), (150_000, true), (200_000, false)])];
        assert_eq!(effective_budget(None, &all, "e", Some("1.5.13"), 2_000), (120_000, "measured on 1.5.13: 80% of 150000 bytes answered, below the default 150000".into()));
        // The operator's smaller value stays; a larger one is lowered to the measurement.
        assert_eq!(effective_budget(Some(90_000), &all, "e", Some("1.5.13"), 2_000).0, 90_000);
        assert_eq!(effective_budget(Some(500_000), &all, "e", Some("1.5.13"), 2_000).0, 120_000);
        // Another version's record is ignored, and so is a stale one.
        assert_eq!(effective_budget(None, &all, "e", Some("1.6.0"), 2_000), (150_000, "default, unmeasured".into()));
        assert_eq!(effective_budget(None, &all, "e", Some("1.5.13"), 1_000 + TTL_SECS).0, 150_000);
        assert_eq!(effective_budget(None, &all, "e", None, 2_000).0, 150_000);
        // A ceiling above the budget never raises it; a ladder that answered nothing does not lower it.
        let wide = vec![mk("1.5.13", &[(100_000, true), (150_000, true), (200_000, true), (250_000, true)])];
        assert_eq!(effective_budget(None, &wide, "e", Some("1.5.13"), 2_000).0, 150_000);
        assert_eq!(effective_budget(Some(100_000), &wide, "e", Some("1.5.13"), 2_000).0, 100_000);
        assert_eq!(effective_budget(None, &[mk("1.5.13", &[(100_000, false)])], "e", Some("1.5.13"), 2_000).0, 150_000);
        // doctor prints the budget and its source.
        let line = doctor_line(&all, "e", Some("1.5.13"), 2_000, None);
        assert!(line.contains("met C2, C5, C3") && line.contains("input budget 120000 bytes (measured on 1.5.13"), "{line}");
        assert!(doctor_line(&all, "e", Some("1.6.0"), 2_000, Some(90_000)).contains("input budget 90000 bytes (config, unmeasured)"));
    }

    #[test]
    fn a_probe_without_the_ladder_keeps_the_last_ladders_row_and_its_date() {
        let base = |at: i64| from_probe(&Capability { probed_at: at, ..cap("ok", Some("m")) }, None, None, "agy", true, "gemini", Some("1.5.13"));
        let laddered = with_ladder(base(1_000), Some(&[(100_000, true), (150_000, false)]), None);
        assert_eq!(laddered.ladder_at, Some(1_000));
        let again = with_ladder(base(5_000), None, Some(&laddered));
        assert_eq!((again.row("C3").unwrap().count, again.ladder_at, again.at), (Some(100_000), Some(1_000), 5_000));
        // The measurement ages from the ladder, not from the later probe.
        assert_eq!(again.measured_ceiling(1_000 + TTL_SECS), None);
        assert_eq!(with_ladder(base(5_000), None, None).row("C3").unwrap().status, Status::Unmeasured);
        assert!(!serde_json::to_string(&laddered).unwrap().contains("ZQ"), "no code word in the record");
    }
}
