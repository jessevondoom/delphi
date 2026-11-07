//! Seat receipts and the family table (`specs/091` SR-1, SR-2).
//!
//! A seat is one run of one model on one carrier: a harness span (`harness_run.rs`) or the API turns of a span (`engine.rs`).
//! When it ends, the daemon writes a `seat.receipt` event that says what was asked for, what the carrier reported was served,
//! which family that is, what the seat was offered, and a hash of the prompt. It holds no text and no arguments.
//!
//! * The served model is what the carrier **reported** (a stream field), marked `carrier_claimed`: a router or alias can serve
//!   something else. It is never read from the model's own words.
//! * The family comes from the ratified table ([`FamilyTable`]), by the served model, not by the carrier or endpoint
//!   (GOVERNANCE.md H-001): the same model through two carriers is one family. A model the table does not map, or none reported,
//!   is `unknown`, and `unknown` is not diversity.
//! * Only the daemon writes this event kind. No host method appends events of a kind the host names.

use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

/// The shipped table (`model_families.toml`).
pub const SHIPPED_FAMILIES: &str = include_str!("model_families.toml");
/// A workspace's own ratified replacement, beside its `config.toml`.
pub const FAMILIES_OVERRIDE: &str = "model-families.toml";
/// The event kind. Written by the daemon only.
pub const RECEIPT_KIND: &str = "seat.receipt";
/// The family of a model the table does not map, or that no carrier reported.
pub const UNKNOWN: &str = "unknown";

/// `expires` may be written as a TOML date or as a quoted string.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
enum Day {
    Date(toml::value::Datetime),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawEntry {
    pattern: String,
    family: String,
    owner: String,
    expires: Day,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawTable {
    #[serde(default)]
    #[allow(dead_code)]
    version: u32,
    #[serde(default, rename = "family")]
    families: Vec<RawEntry>,
}

/// One row of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct FamilyEntry {
    pub pattern: String,
    pub family: String,
    pub owner: String,
    /// `YYYY-MM-DD`.
    pub expires: String,
}

impl FamilyEntry {
    pub fn is_expired(&self, today: &str) -> bool {
        self.expires.as_str() < today
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FamilyTable {
    pub entries: Vec<FamilyEntry>,
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() }) && (1..=12).contains(&s[5..7].parse::<u32>().unwrap_or(0)) && (1..=31).contains(&s[8..10].parse::<u32>().unwrap_or(0))
}

/// `*` matches any run of characters; everything else matches itself, ignoring ASCII case.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t) = (pattern.to_ascii_lowercase(), text.to_ascii_lowercase());
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == t;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !t.starts_with(first) || t.len() < first.len() + last.len() || !t.ends_with(last) {
        return false;
    }
    let mut at = first.len();
    let end = t.len() - last.len();
    for mid in &parts[1..parts.len() - 1] {
        match t[at..end].find(mid) {
            Some(i) => at += i + mid.len(),
            None => return false,
        }
    }
    true
}

impl FamilyTable {
    pub fn parse(text: &str) -> Result<FamilyTable, String> {
        let raw: RawTable = toml::from_str(text).map_err(|e| format!("model families: {e}"))?;
        let mut entries = Vec::new();
        for (i, r) in raw.families.into_iter().enumerate() {
            let at = format!("model families: entry {} (`{}`)", i + 1, r.pattern);
            let expires = match r.expires {
                Day::Date(d) => d.to_string(),
                Day::Text(s) => s,
            };
            if r.pattern.trim().is_empty() {
                return Err(format!("model families: entry {} has an empty pattern", i + 1));
            }
            let fam_ok = !r.family.is_empty() && r.family.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            if !fam_ok || r.family == UNKNOWN {
                return Err(format!("{at} has family `{}`; use a lowercase name such as `claude` (`{UNKNOWN}` is reserved for a model the table does not map)", r.family));
            }
            if r.owner.trim().is_empty() {
                return Err(format!("{at} has no owner; name who renews it"));
            }
            if !is_date(&expires) {
                return Err(format!("{at} has expires `{expires}`; use a date such as 2027-02-05"));
            }
            entries.push(FamilyEntry { pattern: r.pattern, family: r.family, owner: r.owner, expires });
        }
        Ok(FamilyTable { entries })
    }

    pub fn shipped() -> FamilyTable {
        FamilyTable::parse(SHIPPED_FAMILIES).expect("the shipped family table parses (a test pins this)")
    }

    /// The workspace's replacement if it has one, else the shipped table. A replacement that does not parse is an error
    /// (`delphi doctor` reports it); the daemon then falls back to the shipped table, never to a guess.
    pub fn load(ws_dir: &Path) -> Result<FamilyTable, String> {
        match std::fs::read_to_string(ws_dir.join(FAMILIES_OVERRIDE)) {
            Ok(t) => FamilyTable::parse(&t),
            Err(_) => Ok(FamilyTable::shipped()),
        }
    }

    /// The first entry that maps this served model.
    pub fn entry_for(&self, model: &str) -> Option<&FamilyEntry> {
        self.entries.iter().find(|e| glob(&e.pattern, model))
    }

    /// Entries past their `expires` date: they still apply, and `delphi doctor` says so.
    pub fn expired(&self, today: &str) -> Vec<&FamilyEntry> {
        self.entries.iter().filter(|e| e.is_expired(today)).collect()
    }
}

/// What the receipt says was served.
#[derive(Debug, Clone, PartialEq)]
pub struct Served {
    /// As the carrier reported it; `None` when it reported nothing.
    pub model: Option<String>,
    pub family: String,
    /// `table` when an entry mapped the model, else `unknown`.
    pub family_source: &'static str,
}

/// The family of the model the carrier reported. Nothing reported, or no entry: `unknown`.
pub fn resolve(table: &FamilyTable, reported: Option<&str>) -> Served {
    let model = reported.map(str::trim).filter(|m| !m.is_empty());
    match model.and_then(|m| table.entry_for(m)) {
        Some(e) => Served { model: model.map(String::from), family: e.family.clone(), family_source: "table" },
        None => Served { model: model.map(String::from), family: UNKNOWN.into(), family_source: "unknown" },
    }
}

/// `blake3:<hex>` of the prompt text as sent. The two parts are length-prefixed so that moving text between them changes the hash.
pub fn prompt_hash(system: &str, user: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(&(system.len() as u64).to_le_bytes());
    h.update(system.as_bytes());
    h.update(user.as_bytes());
    format!("blake3:{}", h.finalize().to_hex())
}

/// What the seat was run on and offered.
#[derive(Debug, Clone, PartialEq)]
pub struct Carrier {
    /// `claude`, `agy`, `codex`, `copilot`, or `api`.
    pub kind: String,
    /// The CLI's version when it is known; `None` otherwise (no carrier reports it to a span today).
    pub cli_version: Option<String>,
    /// The tool names the carrier said it offered the model.
    pub tools_offered: Vec<String>,
    pub dispatch_allowed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    pub seat_id: String,
    pub requested_endpoint: String,
    pub requested_model: String,
    pub served: Served,
    pub carrier: Carrier,
    /// What Delphi granted, by name: never arguments.
    pub tool_grant: Value,
    pub prompt_hash: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    /// `ok`, `error`, or `model_mismatch`.
    pub outcome: String,
}

impl Receipt {
    pub fn to_value(&self) -> Value {
        json!({
            "seat_id": self.seat_id,
            "requested": {"endpoint": self.requested_endpoint, "model": self.requested_model},
            "served": {"model": self.served.model, "carrier_claimed": true, "family": self.served.family, "family_source": self.served.family_source},
            "carrier": {"kind": self.carrier.kind, "cli_version": self.carrier.cli_version, "tools_offered": self.carrier.tools_offered, "dispatch_allowed": self.carrier.dispatch_allowed},
            "tool_grant": self.tool_grant,
            "prompt_hash": self.prompt_hash,
            "tokens_in": self.tokens_in,
            "tokens_out": self.tokens_out,
            "outcome": self.outcome,
        })
    }
}

/// The API turns of one span, folded per (endpoint, requested model, served model).
#[derive(Debug, Clone, PartialEq)]
struct ApiSeat {
    endpoint: String,
    model: String,
    served: Option<String>,
    provider: String,
    tools: Vec<String>,
    prompt_hash: String,
    tokens_in: u64,
    tokens_out: u64,
}

/// A span's seat bookkeeping: the numbering of its seats and the API turns waiting for the span's end.
#[derive(Debug, Default)]
pub struct SeatBook {
    n: u32,
    api: Vec<ApiSeat>,
}

impl SeatBook {
    /// The id of the next seat of this span.
    pub fn next_id(&mut self, span_id: &str) -> String {
        self.n += 1;
        format!("{span_id}.s{}", self.n)
    }

    /// One finished API turn.
    #[allow(clippy::too_many_arguments)]
    pub fn note_api(&mut self, endpoint: &str, model: &str, served: Option<&str>, provider: &str, tools: &[String], prompt_hash: &str, tokens_in: u64, tokens_out: u64) {
        if let Some(s) = self.api.iter_mut().find(|s| s.endpoint == endpoint && s.model == model && s.served.as_deref() == served) {
            s.tokens_in += tokens_in;
            s.tokens_out += tokens_out;
            return;
        }
        self.api.push(ApiSeat { endpoint: endpoint.into(), model: model.into(), served: served.map(String::from), provider: provider.into(), tools: tools.to_vec(), prompt_hash: prompt_hash.into(), tokens_in, tokens_out });
    }

    /// The API seats of the span, as receipts, and forget them.
    pub fn take_api_receipts(&mut self, span_id: &str, table: &FamilyTable, outcome: &str) -> Vec<(String, Receipt)> {
        let seats = std::mem::take(&mut self.api);
        let mut out = Vec::new();
        for s in seats {
            self.n += 1;
            let dispatch = s.tools.iter().any(|t| t == "dispatch_agent");
            let receipt = Receipt {
                seat_id: format!("{span_id}.s{}", self.n),
                requested_endpoint: s.endpoint.clone(),
                requested_model: s.model,
                served: resolve(table, s.served.as_deref()),
                carrier: Carrier { kind: "api".into(), cli_version: None, tools_offered: s.tools.clone(), dispatch_allowed: dispatch },
                tool_grant: json!({"builtin": "none", "native": [], "bridge": s.tools, "provider": s.provider}),
                prompt_hash: s.prompt_hash,
                tokens_in: s.tokens_in,
                tokens_out: s.tokens_out,
                outcome: outcome.into(),
            };
            out.push((s.endpoint, receipt));
        }
        out
    }
}

/// Which families actually sat (`specs/091` SR-3), from the receipts' SERVED family, never the requested one.
/// `unknown` is counted apart and never makes a family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diversity {
    /// Distinct known families, sorted.
    pub families: Vec<String>,
    /// Seats whose served family is `unknown`.
    pub unknown: usize,
}

impl Diversity {
    /// `host` is the host's own family when it is known (a host-reported guess, since a host seat has no receipt); `served` is the
    /// served family of each receipt that counts.
    pub fn of(host: Option<&str>, served: &[String]) -> Diversity {
        let mut families: Vec<String> = Vec::new();
        let mut unknown = 0;
        for f in host.into_iter().chain(served.iter().map(String::as_str)) {
            if f == UNKNOWN || f.is_empty() {
                unknown += 1;
            } else if !families.iter().any(|x| x == f) {
                families.push(f.to_string());
            }
        }
        families.sort();
        Diversity { families, unknown }
    }

    /// Fewer than two distinct known families sat: the label `homogeneous` (H-008).
    pub fn homogeneous(&self) -> bool {
        self.families.len() < 2
    }

    /// `None` when `required` is met; else the fixed sentence for a caller who asked for it (SR-3).
    pub fn unmet(&self, required: u32, when: &str) -> Option<String> {
        if self.families.len() as u32 >= required {
            return None;
        }
        let sat = if self.families.is_empty() { "none known".to_string() } else { self.families.join(", ") };
        let unknown = match self.unknown {
            0 => String::new(),
            n => format!("; {n} seat(s) had a family the table does not map (`unknown` never counts)"),
        };
        Some(format!(
            "require_families = {required} is not met {when}: known families {sat}{unknown}. Nothing more was spent. To retry, make the other seat's family known (the host's `model`, or a served model in model-families.toml) or lower require_families; leave it unset to run labelled homogeneous."
        ))
    }
}

/// The served family of every receipt of `task_id` that ended `ok`, and the tokens those seats used (in plus out).
pub fn served_for_task(events: &[delphi_core::event::EventEnvelope], task_id: &str) -> (Vec<String>, u64) {
    let mut fams = Vec::new();
    let mut seen: Vec<(Option<String>, String)> = Vec::new();
    let mut tokens = 0;
    for e in events.iter().filter(|e| e.kind == RECEIPT_KIND && e.task_id.as_deref() == Some(task_id)) {
        tokens += e.payload["tokens_in"].as_u64().unwrap_or(0) + e.payload["tokens_out"].as_u64().unwrap_or(0);
        if e.payload["outcome"] == "ok" {
            // One entry per seat (role) and family: a seat's two rounds are two receipts but one seat.
            let entry = (e.agent.clone(), e.payload["served"]["family"].as_str().unwrap_or(UNKNOWN).to_string());
            if !seen.contains(&entry) {
                fams.push(entry.1.clone());
                seen.push(entry);
            }
        }
    }
    (fams, tokens)
}

/// The label a seat's verdict carries when no receipt of it shows a tool that could read the repo (`specs/086` §7).
pub const UNVERIFIED_AGAINST_REPO: &str = "unverified-against-repo";

/// True when this receipt's carrier was granted nothing that can read a repository: no built-in, no native tool, no Delphi bridge
/// tool. A Copilot ACP seat counts as tool-less whatever it reports (`specs/086` §6.4). A receipt that says nothing about tools
/// counts as tool-less too: the label errs towards "unverified".
pub fn receipt_toolless(p: &Value) -> bool {
    if p["carrier"]["kind"].as_str() == Some("copilot") {
        return true;
    }
    let g = &p["tool_grant"];
    g["builtin"].as_str().is_none_or(|b| b == "none") && g["native"].as_array().is_none_or(|a| a.is_empty()) && g["bridge"].as_array().is_none_or(|a| a.is_empty())
}

/// One seat Delphi ran, as its receipts say (`specs/086` §6.4, §7).
#[derive(Debug, Clone, PartialEq)]
pub struct SeatSummary {
    pub role: String,
    pub served_model: Option<String>,
    pub family: String,
    pub carrier: String,
    pub tools_offered: Vec<String>,
    /// At least one receipt ended `ok`.
    pub sat: bool,
    /// It sat, and none of its `ok` receipts shows a tool that could read the repo.
    pub unverified_against_repo: bool,
    pub tokens: u64,
}

impl SeatSummary {
    pub fn to_value(&self) -> Value {
        json!({"seat": self.role, "served_model": self.served_model, "family": self.family, "carrier": self.carrier, "tools_offered": self.tools_offered, "sat": self.sat, "unverified_against_repo": self.unverified_against_repo, "tokens": self.tokens})
    }
}

/// The seats of `task_id` from its receipts, one per role, in the order they first appear. Text is never read: receipts hold none.
pub fn seat_summaries(events: &[delphi_core::event::EventEnvelope], task_id: &str) -> Vec<SeatSummary> {
    let mut out: Vec<SeatSummary> = Vec::new();
    for e in events.iter().filter(|e| e.kind == RECEIPT_KIND && e.task_id.as_deref() == Some(task_id)) {
        let role = e.agent.clone().unwrap_or_default();
        let ok = e.payload["outcome"] == "ok";
        let tokens = e.payload["tokens_in"].as_u64().unwrap_or(0) + e.payload["tokens_out"].as_u64().unwrap_or(0);
        let tools: Vec<String> = e.payload["carrier"]["tools_offered"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect();
        let at = match out.iter().position(|s| s.role == role) {
            Some(i) => i,
            None => {
                out.push(SeatSummary { role: role.clone(), served_model: None, family: UNKNOWN.into(), carrier: String::new(), tools_offered: Vec::new(), sat: false, unverified_against_repo: false, tokens: 0 });
                out.len() - 1
            }
        };
        let s = &mut out[at];
        s.tokens += tokens;
        // The seat is described by its first receipt that sat (the first at all while none has).
        if ok && !s.sat {
            s.sat = true;
            s.served_model = e.payload["served"]["model"].as_str().map(String::from);
            s.family = e.payload["served"]["family"].as_str().unwrap_or(UNKNOWN).to_string();
            s.carrier = e.payload["carrier"]["kind"].as_str().unwrap_or("").to_string();
            s.unverified_against_repo = true;
        }
        if ok {
            for t in tools {
                if !s.tools_offered.contains(&t) {
                    s.tools_offered.push(t);
                }
            }
            if !receipt_toolless(&e.payload) {
                s.unverified_against_repo = false;
            }
        }
    }
    out
}

/// The count budget on foreign seats (`specs/091` SR-4), per Council: `None` when the plan fits, else the fixed sentence.
/// It names the cap and never echoes the question.
pub fn over_budget(seats: u32, tokens: u64, max_seats: u32, max_tokens: u64) -> Option<String> {
    if seats > max_seats {
        return Some(format!("foreign-seat budget: {seats} foreign seat(s) asked, the cap is [seats] max_foreign_seats = {max_seats} per Council. Nothing was spent. Open the Panel without foreign_seat, or raise the cap in config.toml."));
    }
    if seats > 0 && tokens > max_tokens {
        return Some(format!("foreign-seat budget: the foreign seat's ceiling is {tokens} tokens, the cap is [seats] max_foreign_tokens = {max_tokens} per Council. Nothing was spent. Open the Panel without foreign_seat, or raise the cap in config.toml."));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_table_parses_and_maps_the_models_seen_through_each_carrier() {
        let t = FamilyTable::shipped();
        for (model, family) in [
            ("claude-haiku-4.5", "claude"),
            ("claude-sonnet-5.5", "claude"),
            ("claude-sonnet-5-5-thinking", "claude"),
            ("claude-opus-5-5-medium", "claude"),
            ("gpt-6-luna", "openai"),
            ("gpt-6.1-terra", "openai"),
            ("gpt-oss-120b-medium", "openai"),
            ("gemini-4-argon", "gemini"),
            ("gemini-4-argon-high", "gemini"),
            ("gemini-3.8-flash-low", "gemini"),
            ("Claude-Future-9", "claude"),
        ] {
            assert_eq!(resolve(&t, Some(model)).family, family, "{model}");
        }
        assert!(t.entries.iter().all(|e| e.owner == "operator"));
    }

    #[test]
    fn an_unmapped_or_unreported_model_is_unknown() {
        let t = FamilyTable::shipped();
        assert_eq!(resolve(&t, Some("mystery-model-7")), Served { model: Some("mystery-model-7".into()), family: "unknown".into(), family_source: "unknown" });
        assert_eq!(resolve(&t, None), Served { model: None, family: "unknown".into(), family_source: "unknown" });
        assert_eq!(resolve(&t, Some("  ")).family, "unknown");
    }

    #[test]
    fn the_same_model_through_two_carriers_is_one_family() {
        let t = FamilyTable::shipped();
        // Copilot writes dots, agy writes dashes and a variant: one lineage.
        let (a, b) = (resolve(&t, Some("claude-sonnet-5.5")), resolve(&t, Some("claude-sonnet-5-5-thinking")));
        assert_eq!(a.family, b.family);
        assert_eq!(a.family_source, "table");
        // A vendor's model through an aggregator and through its own API.
        assert_eq!(resolve(&t, Some("gemini-4-argon")).family, resolve(&t, Some("gemini-4-argon-high")).family);
    }

    #[test]
    fn globs_match_by_wildcard_only() {
        assert!(glob("a*c", "abc") && glob("a*c", "ac") && glob("*b*", "abc") && glob("abc", "ABC") && glob("a*b*c", "axxbyyc"));
        assert!(!glob("a*c", "ab") && !glob("abc", "abcd") && !glob("a*b*c", "axxcyyb") && !glob("ab*bc", "abc"));
    }

    #[test]
    fn a_malformed_table_is_refused_with_a_message_that_names_the_entry() {
        let e = FamilyTable::parse("[[family]]\npattern = \"x-*\"\nfamily = \"unknown\"\nowner = \"o\"\nexpires = \"2027-01-01\"\n").unwrap_err();
        assert!(e.contains("entry 1") && e.contains("reserved"), "{e}");
        let e = FamilyTable::parse("[[family]]\npattern = \"x-*\"\nfamily = \"Claude\"\nowner = \"o\"\nexpires = \"2027-01-01\"\n").unwrap_err();
        assert!(e.contains("lowercase"), "{e}");
        let e = FamilyTable::parse("[[family]]\npattern = \"x-*\"\nfamily = \"x\"\nowner = \"\"\nexpires = \"2027-01-01\"\n").unwrap_err();
        assert!(e.contains("no owner"), "{e}");
        let e = FamilyTable::parse("[[family]]\npattern = \"x-*\"\nfamily = \"x\"\nowner = \"o\"\nexpires = \"soon\"\n").unwrap_err();
        assert!(e.contains("expires `soon`"), "{e}");
        let e = FamilyTable::parse("[[family]]\npattern = \"x-*\"\nfamily = \"x\"\nowner = \"o\"\n").unwrap_err();
        assert!(e.starts_with("model families:") && e.contains("expires"), "{e}");
        assert!(FamilyTable::parse("not toml [").unwrap_err().starts_with("model families:"));
        // A bare TOML date is accepted as well as a quoted one.
        assert_eq!(FamilyTable::parse("[[family]]\npattern = \"x-*\"\nfamily = \"x\"\nowner = \"o\"\nexpires = 2027-01-01\n").unwrap().entries[0].expires, "2027-01-01");
    }

    #[test]
    fn a_workspace_file_replaces_the_shipped_table_and_a_bad_one_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(FamilyTable::load(dir.path()).unwrap(), FamilyTable::shipped());
        std::fs::write(dir.path().join(FAMILIES_OVERRIDE), "[[family]]\npattern = \"zed-*\"\nfamily = \"zed\"\nowner = \"me\"\nexpires = \"2030-01-01\"\n").unwrap();
        let t = FamilyTable::load(dir.path()).unwrap();
        assert_eq!(resolve(&t, Some("zed-1")).family, "zed");
        assert_eq!(resolve(&t, Some("claude-sonnet-5.5")).family, "unknown", "a replacement replaces");
        std::fs::write(dir.path().join(FAMILIES_OVERRIDE), "oops [").unwrap();
        assert!(FamilyTable::load(dir.path()).is_err());
    }

    #[test]
    fn an_expired_entry_still_applies_and_is_listed_for_doctor() {
        let t = FamilyTable::parse("[[family]]\npattern = \"old-*\"\nfamily = \"old\"\nowner = \"o\"\nexpires = \"2020-01-01\"\n[[family]]\npattern = \"new-*\"\nfamily = \"new\"\nowner = \"o\"\nexpires = \"2099-01-01\"\n").unwrap();
        assert_eq!(resolve(&t, Some("old-1")).family, "old");
        let x = t.expired("2026-11-07");
        assert_eq!(x.len(), 1);
        assert_eq!(x[0].pattern, "old-*");
        assert!(FamilyTable::shipped().expired("2026-11-07").is_empty(), "the shipped entries are in date");
    }

    #[test]
    fn the_prompt_hash_depends_on_the_text_and_where_the_split_falls() {
        assert_eq!(prompt_hash("a", "b"), prompt_hash("a", "b"));
        assert_ne!(prompt_hash("a", "b"), prompt_hash("a", "c"));
        assert_ne!(prompt_hash("ab", "c"), prompt_hash("a", "bc"));
        assert!(prompt_hash("a", "b").starts_with("blake3:"));
    }

    #[test]
    fn a_receipt_names_everything_and_carries_no_prompt_text() {
        let secret = "SECRET-PROMPT-TEXT-9917";
        let r = Receipt {
            seat_id: "s1.s1".into(),
            requested_endpoint: "ep".into(),
            requested_model: "claude-sonnet-5.5".into(),
            served: resolve(&FamilyTable::shipped(), Some("gpt-6-luna")),
            carrier: Carrier { kind: "copilot".into(), cli_version: None, tools_offered: vec!["view".into()], dispatch_allowed: false },
            tool_grant: json!({"builtin": "none"}),
            prompt_hash: prompt_hash("sys", secret),
            tokens_in: 10,
            tokens_out: 3,
            outcome: "model_mismatch".into(),
        };
        let v = r.to_value();
        assert_eq!((v["requested"]["model"].as_str(), v["served"]["model"].as_str(), v["served"]["family"].as_str(), v["served"]["family_source"].as_str(), v["served"]["carrier_claimed"].as_bool()), (Some("claude-sonnet-5.5"), Some("gpt-6-luna"), Some("openai"), Some("table"), Some(true)));
        assert_eq!((v["tokens_in"].as_u64(), v["tokens_out"].as_u64(), v["carrier"]["dispatch_allowed"].as_bool()), (Some(10), Some(3), Some(false)));
        assert!(!v.to_string().contains(secret), "the hash stands for the text; the text is not there");
    }

    // specs/091 §5: "no event of that kind comes from a host". The host-facing modules name no event of this kind, so a host's
    // call cannot write one; only `harness_run.rs` and `engine.rs` do, in the daemon.
    #[test]
    fn no_host_facing_module_writes_a_seat_receipt() {
        for (name, src) in [("host_rpc.rs", include_str!("host_rpc.rs")), ("mcp_host.rs", include_str!("mcp_host.rs")), ("mcp_daemon.rs", include_str!("mcp_daemon.rs")), ("mcp_wire.rs", include_str!("mcp_wire.rs"))] {
            assert!(!src.contains("seat.receipt") && !src.contains("RECEIPT_KIND"), "{name} must not write seat receipts");
        }
        let writers: Vec<&str> = [("harness_run.rs", include_str!("harness_run.rs")), ("engine.rs", include_str!("engine.rs"))].iter().filter(|(_, s)| s.contains("RECEIPT_KIND")).map(|(n, _)| *n).collect();
        assert_eq!(writers, ["harness_run.rs", "engine.rs"]);
    }

    #[test]
    fn api_turns_fold_into_one_receipt_per_model_and_the_ids_are_numbered() {
        let mut b = SeatBook::default();
        assert_eq!(b.next_id("sp"), "sp.s1");
        b.note_api("ep", "m", Some("m-served"), "anthropic", &["read_file".into()], "blake3:x", 10, 2);
        b.note_api("ep", "m", Some("m-served"), "anthropic", &["read_file".into()], "blake3:x", 5, 1);
        b.note_api("ep", "m", None, "anthropic", &[], "blake3:x", 1, 1);
        let out = b.take_api_receipts("sp", &FamilyTable::shipped(), "ok");
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].1.tokens_in, out[0].1.tokens_out, out[0].1.seat_id.as_str()), (15, 3, "sp.s2"));
        assert_eq!(out[1].1.served.family, "unknown", "an API answer that did not say what served it");
        assert!(b.take_api_receipts("sp", &FamilyTable::shipped(), "ok").is_empty());
    }

    // specs/091 SR-3: what sat is read from the served family; unknown is not diversity.
    fn fams(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn one_known_family_is_homogeneous_and_two_are_not() {
        assert!(Diversity::of(Some("claude"), &fams(&["claude"])).homogeneous());
        assert!(Diversity::of(None, &fams(&["gemini"])).homogeneous());
        let d = Diversity::of(Some("claude"), &fams(&["gemini"]));
        assert_eq!((d.families.clone(), d.homogeneous(), d.unknown), (fams(&["claude", "gemini"]), false, 0));
    }

    #[test]
    fn unknown_never_counts_toward_diversity() {
        let d = Diversity::of(Some("claude"), &fams(&["unknown"]));
        assert_eq!((d.families.clone(), d.unknown, d.homogeneous()), (fams(&["claude"]), 1, true));
        let none = Diversity::of(None, &fams(&["unknown", "unknown"]));
        assert_eq!((none.families.len(), none.unknown), (0, 2));
        let msg = d.unmet(2, "by the receipts").unwrap();
        assert!(msg.contains("require_families = 2") && msg.contains("claude") && msg.contains("1 seat(s)") && msg.contains("unknown"), "{msg}");
    }

    #[test]
    fn the_same_model_through_two_carriers_counts_once() {
        let t = FamilyTable::shipped();
        let served: Vec<String> = ["claude-sonnet-5.5", "claude-sonnet-5-5-thinking"].iter().map(|m| resolve(&t, Some(m)).family).collect();
        let d = Diversity::of(None, &served);
        assert_eq!(d.families, fams(&["claude"]));
        assert!(d.unmet(2, "x").is_some());
    }

    #[test]
    fn a_requirement_needs_known_families_and_a_met_one_is_silent() {
        assert!(Diversity::of(None, &[]).unmet(1, "x").is_some(), "even one known family is needed when asked for");
        assert!(Diversity::of(Some("claude"), &fams(&["gemini"])).unmet(2, "x").is_none());
    }

    #[test]
    fn the_budget_names_its_cap_and_never_echoes_a_question() {
        assert!(over_budget(1, 40_000, 1, 100_000).is_none());
        assert!(over_budget(0, 999_999, 0, 1).is_none(), "no foreign seat, nothing to cap");
        let a = over_budget(1, 10, 0, 100_000).unwrap();
        assert!(a.contains("max_foreign_seats = 0") && a.contains("Nothing was spent"), "{a}");
        let b = over_budget(1, 50_000, 1, 40_000).unwrap();
        assert!(b.contains("max_foreign_tokens = 40000") && b.contains("50000"), "{b}");
    }

    #[test]
    fn receipts_of_a_task_count_by_served_family_once_per_seat_and_only_when_ok() {
        let ev = |task: &str, agent: &str, family: &str, outcome: &str| {
            let mut e = delphi_core::event::EventEnvelope::new("m", RECEIPT_KIND, json!({"served": {"family": family}, "tokens_in": 10, "tokens_out": 5, "outcome": outcome}));
            e.task_id = Some(task.into());
            e.agent = Some(agent.into());
            e
        };
        let log = vec![ev("t", "red-team", "gemini", "ok"), ev("t", "red-team", "gemini", "ok"), ev("t", "architect", "claude", "model_mismatch"), ev("other", "x", "openai", "ok")];
        let (served, tokens) = served_for_task(&log, "t");
        assert_eq!((served, tokens), (fams(&["gemini"]), 45), "two rounds of one seat are one seat; a refused seat did not sit; its tokens still count toward the budget");
    }

    // specs/091 SR-3: the ruling is labelled from the receipts' served family, not from the request.
    #[test]
    fn a_panel_ruling_lists_the_families_that_sat_and_says_so_when_a_required_count_was_not_met() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("councils")).unwrap();
        std::fs::write(dir.path().join("councils/001-x.md"), "## Decision\n\nx\n").unwrap();
        let ev = |kind: &str, payload: serde_json::Value, agent: Option<&str>| {
            let mut e = delphi_core::event::EventEnvelope::new("m", kind, payload);
            e.task_id = Some("t-1".into());
            e.agent = agent.map(String::from);
            e
        };
        let receipt = |family: &str| json!({"served": {"family": family}, "tokens_in": 1, "tokens_out": 1, "outcome": "ok"});
        let view = |served: &str, required: Option<u64>| {
            let log = vec![
                delphi_core::event::EventEnvelope::new("m", crate::host_rpc::COUNCIL_EVENT, json!({"task_id": "t-1", "venue": "panel"})),
                ev("council.started", json!({"venue": "panel", "host_family": "claude", "require_families": required, "foreign": {"role": "red-team", "family": "gemini"}}), None),
                ev(RECEIPT_KIND, receipt(served), Some("red-team")),
                ev(&format!("council.{}", "verdict"), json!({"decision": "d", "path": "missions/m/councils/001-x.md", "options": [], "steps": [], "homogeneous": false}), None),
                ev("task.completed", json!({"status": "done", "reason": ""}), None),
            ];
            crate::host_rpc::council_view(&log, dir.path(), "t-1").unwrap()
        };
        // Requested gemini and served gemini: two families, not homogeneous, requirement met.
        let v = view("gemini", Some(2));
        assert_eq!((v["ruling"]["families"].clone(), v["ruling"]["homogeneous"].clone(), v["ruling"]["require_families_met"].clone()), (json!(["claude", "gemini"]), json!(false), json!(true)), "{v}");
        assert!(v["display"].as_str().unwrap().contains("2 families: claude, gemini"));
        // The carrier served a model the table does not map: the request does not matter, unknown is not diversity.
        let v = view("unknown", Some(2));
        let card = v["display"].as_str().unwrap();
        assert_eq!((v["ruling"]["families"].clone(), v["ruling"]["homogeneous"].clone(), v["ruling"]["require_families_met"].clone()), (json!(["claude"]), json!(true), json!(false)), "{v}");
        assert!(card.contains("1 family: claude, 1 unknown (not counted) (homogeneous)") && card.contains("require_families = 2 was NOT met"), "{card}");
        // The same model through a second carrier is the host's own family again: one family.
        let v = view("claude", None);
        assert_eq!((v["ruling"]["families"].clone(), v["ruling"]["homogeneous"].clone(), v["ruling"]["require_families_met"].clone()), (json!(["claude"]), json!(true), json!(null)), "{v}");
    }

    // specs/086 §7: the label that travels with a tool-less seat's verdict.
    #[test]
    fn a_seat_with_no_repo_reading_tool_in_any_receipt_is_unverified_against_the_repo() {
        let rcpt = |agent: &str, carrier: &str, bridge: Value, outcome: &str| {
            let mut e = delphi_core::event::EventEnvelope::new("m", RECEIPT_KIND, json!({"served": {"model": "m-1", "family": "gemini"}, "carrier": {"kind": carrier, "tools_offered": bridge.clone()}, "tool_grant": {"builtin": "none", "native": [], "bridge": bridge}, "tokens_in": 4, "tokens_out": 2, "outcome": outcome}));
            e.task_id = Some("t".into());
            e.agent = Some(agent.into());
            e
        };
        let log = vec![
            rcpt("a", "api", json!(["read_file"]), "ok"),
            rcpt("a", "api", json!([]), "ok"),
            rcpt("b", "api", json!([]), "ok"),
            rcpt("c", "copilot", json!(["view"]), "ok"),
            rcpt("d", "api", json!(["read_file"]), "error"),
            rcpt("e", "api", json!([]), "ok"),
        ];
        let s = seat_summaries(&log, "t");
        let flag = |r: &str| s.iter().find(|x| x.role == r).map(|x| (x.sat, x.unverified_against_repo)).unwrap();
        assert_eq!(flag("a"), (true, false), "tools in Round 1 are enough, though Round 2 had none");
        assert_eq!(flag("b"), (true, true), "no tool anywhere");
        assert_eq!(flag("c"), (true, true), "a Copilot ACP seat is tool-less whatever it lists");
        assert_eq!(flag("d"), (false, false), "a seat that did not sit carries no verdict to label");
        assert_eq!(s.iter().find(|x| x.role == "a").unwrap().tokens, 12);
        assert_eq!(s.len(), 5, "one entry per seat, in order of first receipt");
        let v = s[0].to_value();
        assert_eq!((v["family"].as_str(), v["served_model"].as_str(), v["carrier"].as_str()), (Some("gemini"), Some("m-1"), Some("api")));
        // A receipt that says nothing of tools counts as tool-less.
        assert!(receipt_toolless(&json!({"served": {}})));
        assert!(!receipt_toolless(&json!({"tool_grant": {"builtin": "none", "native": ["Read"], "bridge": []}})));
    }
}
