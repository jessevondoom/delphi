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
}
