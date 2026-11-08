//! The carrier choice rule for a Panel's foreign seats (`specs/086` §6.4, built as C3b).
//!
//! Static and deterministic: no model call, no event, nothing spent, nothing started. Given the question, the stakes, the pointers and
//! what is configured and usable, it proposes which seats Delphi could run on another family, through which carrier, with or without
//! tools, and what that would cost at most. `council_frame` shows the plan so the operator's yes is informed; `council_open` still
//! refuses without `operator_confirmed`, and still judges every cap itself.
//!
//! The rule:
//! - a critique or second-opinion seat gets NO tools when the question needs no repo reading, and its verdict is labelled
//!   `unverified-against-repo`; a seat whose job is to verify a claim against the repo (the question or a pointer names files, specs or
//!   code) gets read-only tools;
//! - a family other than the host's, and other than the other proposed seats', is preferred; when none is left, the plan says so
//!   (the Panel runs labelled `homogeneous`, never refused for it unless `require_families` was asked);
//! - never more seats than `[seats] max_foreign_seats`, never a ceiling above `max_foreign_tokens`: the plan refuses and names the cap;
//! - only an endpoint that is configured, allowed and usable now is proposed; when availability could not be read, nothing is proposed.

use serde_json::{json, Value};

use crate::seat::{over_budget, Diversity, UNKNOWN, UNVERIFIED_AGAINST_REPO};

/// Where the default estimates come from. Small samples: say so wherever they are shown.
pub const ESTIMATE_SOURCE: &str = "measured 2026-11-07, small samples";

/// One configured, usable endpoint a seat could ride.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierOpt {
    pub endpoint: String,
    pub family: String,
    /// `copilot-acp`, `claude-cli`, `agy`, `codex-cli` or `api`.
    pub kind: &'static str,
}

/// What `council_frame` knows about the machine. `known: false` means the daemon could not be asked.
#[derive(Debug, Clone, Default)]
pub struct CarrierEnv {
    pub known: bool,
    pub options: Vec<CarrierOpt>,
    pub max_seats: u32,
    pub max_tokens: u64,
    /// The engine's per-seat ceiling (two calls at the configured output limit, plus input): the figure `council_open` checks.
    pub ceiling_per_seat: u64,
    pub require_families: Option<u32>,
}

/// The carrier kind of a harness endpoint (`None` harness defaults to `claude`, as the config does).
pub fn kind_of(is_harness: bool, harness: Option<&str>) -> &'static str {
    if !is_harness {
        return "api";
    }
    match harness.unwrap_or("claude") {
        "copilot" => "copilot-acp",
        "agy" => "agy",
        "codex" => "codex-cli",
        _ => "claude-cli",
    }
}

/// Typical input tokens per seat, (low, high): the measured medians, used as defaults until more data exists.
fn estimate(kind: &str) -> (u64, u64) {
    match kind {
        "copilot-acp" => (10_000, 17_000),
        "claude-cli" => (31_000, 48_000),
        "agy" => (26_000, 283_000),
        _ => (12_000, 24_000),
    }
}

/// Does the question, or a pointer, name something in the repo to be read? Files, specs, code.
pub fn needs_repo(question: &str, pointers: &[String]) -> bool {
    if !pointers.is_empty() {
        return true;
    }
    let q = question.to_lowercase();
    ["specs/", "crates/", "src/", "docs/", ".rs", ".ts", ".toml", ".md", ".py", "::", "()", "this repo", "the codebase", "the code "].iter().any(|k| q.contains(k))
        || q.match_indices("spec").any(|(i, _)| q[i + 4..].trim_start_matches(['-', ' ']).starts_with(|c: char| c.is_ascii_digit()))
}

/// Cheapest first for a seat with no tools; a tool seat prefers the carriers whose tools Delphi gates itself.
fn rank(kind: &str, tools: bool) -> u8 {
    match (tools, kind) {
        (false, "copilot-acp") | (true, "claude-cli") => 0,
        (false, "api") | (true, "api") => 1,
        (false, "claude-cli") | (true, "agy") => 2,
        (false, "agy") | (true, "copilot-acp") => 3,
        _ => 4,
    }
}

/// The plan. `host_family` is host-reported or unknown. Returns the fields `council_frame` adds: `proposed_seats`, `plan_budget`,
/// `plan_note`.
pub fn plan(question: &str, stakes: &str, pointers: &[String], host_family: Option<&str>, env: &CarrierEnv, require_families: Option<u32>) -> Value {
    let refuse = |note: String, seats: Value| {
        json!({"proposed_seats": seats, "plan_budget": Value::Null, "plan_note": note, "starts_nothing": true})
    };
    if !env.known {
        return refuse("No plan: Delphi's daemon could not be asked which endpoints are configured and usable, so no foreign seat is proposed (availability unknown). Run a Panel on your host's seats, or start Delphi and ask again.".into(), json!([]));
    }
    let repo = needs_repo(question, pointers);
    // Roles in the order they are proposed: the critique seat first; a high-stakes question adds a second opinion.
    let mut wanted: Vec<(&str, bool)> = vec![("red-team", repo)];
    if stakes == "high" {
        wanted.push(("strategist", false));
    }
    // The count cap first: say which cap, before choosing anything.
    if let Some(why) = over_budget(1, env.ceiling_per_seat, env.max_seats, env.max_tokens) {
        return refuse(format!("No foreign seat proposed. {why}"), json!([]));
    }
    let mut seats: Vec<Value> = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let mut total_low = 0u64;
    let mut total_high = 0u64;
    let mut notes: Vec<String> = Vec::new();
    for (role, tools) in wanted {
        // A cap the next seat would break stops the plan; the earlier seats stay.
        let n = seats.len() as u32 + 1;
        if let Some(why) = over_budget(n, env.ceiling_per_seat * n as u64, env.max_seats, env.max_tokens) {
            notes.push(format!("The {role} seat is not proposed: {why}"));
            break;
        }
        let mut cands: Vec<&CarrierOpt> = env.options.iter().filter(|o| Some(o.family.as_str()) != host_family).collect();
        let fresh: Vec<&CarrierOpt> = cands.iter().copied().filter(|o| !used.contains(&o.family)).collect();
        if !fresh.is_empty() {
            cands = fresh;
        } else if !used.is_empty() && !cands.is_empty() {
            notes.push(format!("No unused family remains for the {role} seat: it shares a family with another proposed seat (counts as one family)."));
        }
        cands.sort_by_key(|o| (rank(o.kind, tools), o.endpoint.clone()));
        let Some(c) = cands.first() else {
            notes.push(if host_family.is_some() {
                format!("The {role} seat is not proposed: no configured, usable endpoint is on a family other than your host's.")
            } else {
                format!("The {role} seat is not proposed: no configured, usable endpoint.")
            });
            continue;
        };
        let (lo, hi) = estimate(c.kind);
        total_low += lo;
        total_high += hi;
        used.push(c.family.clone());
        seats.push(json!({
            "role": role,
            "family": c.family,
            "endpoint": c.endpoint,
            "carrier": c.kind,
            "tools": if tools { "readonly" } else { "none" },
            "label": if tools { Value::Null } else { json!(UNVERIFIED_AGAINST_REPO) },
            "why": if tools { "its job is to verify a claim against the repo (the question or a pointer names repo files), so it gets read-only tools" } else { "a critique or second opinion that needs no repo reading, so it gets no tools; its verdict is labelled unverified-against-repo" },
            "estimate_tokens": {"low": lo, "high": hi, "source": ESTIMATE_SOURCE},
            "ceiling_tokens": env.ceiling_per_seat,
            "council_open": {"role": role, "family": c.family, "tools": if tools { "readonly" } else { "none" }},
        }));
    }
    if seats.is_empty() {
        notes.insert(0, "No foreign seat is proposed.".into());
        return refuse(notes.join(" "), json!([]));
    }
    let n = seats.len() as u32;
    let ceiling_total = env.ceiling_per_seat * n as u64;
    let planned: Vec<String> = used.clone();
    let diversity = Diversity::of(Some(host_family.unwrap_or(UNKNOWN)), &planned);
    let required = require_families.or(env.require_families);
    let unmet = required.and_then(|r| diversity.unmet(r, "by this plan (the host's family is host-reported)"));
    if diversity.homogeneous() {
        notes.push("Homogeneous: fewer than two distinct known families would sit; the ruling is labelled homogeneous.".into());
    }
    if let Some(u) = &unmet {
        notes.push(u.split(". Nothing more").next().unwrap_or(u).to_string());
    }
    if total_high > env.max_tokens {
        notes.push(format!("The high estimate ({total_high}) is above max_foreign_tokens ({}); the ceiling check passes, but a heavy seat may run past it and Round 2 would then be skipped.", env.max_tokens));
    }
    notes.push(format!("Estimates are {ESTIMATE_SOURCE}; the ceiling is the check council_open applies. Nothing is started until the operator says yes and you call council_open with foreign_seats and operator_confirmed: true."));
    json!({
        "proposed_seats": seats,
        "plan_budget": {
            "seats": n,
            "estimate_tokens": {"low": total_low, "high": total_high, "source": ESTIMATE_SOURCE},
            "ceiling_tokens_total": ceiling_total,
            "cap": {"max_foreign_seats": env.max_seats, "max_foreign_tokens": env.max_tokens},
            "fits_cap": true,
            "families": diversity.families,
            "homogeneous": diversity.homogeneous(),
            "require_families": required,
            "require_families_met": required.map(|_| unmet.is_none()),
        },
        "plan_note": notes.join(" "),
        "starts_nothing": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(endpoint: &str, family: &str, kind: &'static str) -> CarrierOpt {
        CarrierOpt { endpoint: endpoint.into(), family: family.into(), kind }
    }
    fn env(options: Vec<CarrierOpt>) -> CarrierEnv {
        CarrierEnv { known: true, options, max_seats: 2, max_tokens: 100_000, ceiling_per_seat: 40_000, require_families: None }
    }
    fn mixed() -> CarrierEnv {
        env(vec![opt("cop", "openai", "copilot-acp"), opt("cl", "claude", "claude-cli"), opt("ag", "gemini", "agy")])
    }

    #[test]
    fn an_opinion_question_gets_a_toolless_seat_on_the_cheapest_other_family() {
        let p = plan("Table or file for the retry queue?", "normal", &[], Some("claude"), &mixed(), None);
        let s = &p["proposed_seats"][0];
        assert_eq!((s["role"].as_str(), s["tools"].as_str(), s["carrier"].as_str(), s["family"].as_str()), (Some("red-team"), Some("none"), Some("copilot-acp"), Some("openai")), "{p}");
        assert_eq!(s["label"], UNVERIFIED_AGAINST_REPO);
        assert_eq!(p["plan_budget"]["estimate_tokens"]["source"], ESTIMATE_SOURCE);
        assert_eq!(p["plan_budget"]["ceiling_tokens_total"], 40_000);
    }

    #[test]
    fn a_question_naming_repo_files_gets_a_tool_seat_without_the_label() {
        for (q, ptrs) in [("Does crates/delphi-engine/src/seat.rs refuse right?", vec![]), ("Is spec 091 met?", vec![]), ("Is this sound?", vec!["- specs/091.md: the spec".to_string()])] {
            let p = plan(q, "normal", &ptrs, Some("claude"), &mixed(), None);
            let s = &p["proposed_seats"][0];
            assert_eq!(s["tools"], "readonly", "{q}: {p}");
            assert!(s["label"].is_null());
        }
        assert!(!needs_repo("Should we pick Postgres or SQLite?", &[]));
    }

    #[test]
    fn the_cap_refusal_names_the_cap() {
        let mut e = mixed();
        e.max_seats = 0;
        let p = plan("q?", "normal", &[], Some("claude"), &e, None);
        assert!(p["proposed_seats"].as_array().unwrap().is_empty());
        assert!(p["plan_note"].as_str().unwrap().contains("max_foreign_seats = 0"), "{p}");
        let mut e = mixed();
        e.max_tokens = 10;
        let p = plan("q?", "normal", &[], Some("claude"), &e, None);
        assert!(p["plan_note"].as_str().unwrap().contains("max_foreign_tokens = 10"), "{p}");
        // The second seat of a high-stakes plan is the one the cap stops.
        let mut e = mixed();
        e.max_seats = 1;
        let p = plan("q?", "high", &[], Some("claude"), &e, None);
        assert_eq!(p["proposed_seats"].as_array().unwrap().len(), 1);
        assert!(p["plan_note"].as_str().unwrap().contains("max_foreign_seats = 1"), "{p}");
    }

    #[test]
    fn an_unconfigured_endpoint_is_never_proposed_and_unknown_availability_proposes_nothing() {
        let p = plan("q?", "normal", &[], Some("claude"), &env(vec![opt("cl", "claude", "claude-cli")]), None);
        assert!(p["proposed_seats"].as_array().unwrap().is_empty());
        assert!(p["plan_note"].as_str().unwrap().contains("other than your host's"), "{p}");
        let p = plan("q?", "normal", &[], Some("claude"), &CarrierEnv::default(), None);
        assert!(p["proposed_seats"].as_array().unwrap().is_empty());
        assert!(p["plan_note"].as_str().unwrap().contains("availability unknown"), "{p}");
    }

    #[test]
    fn families_differ_when_they_can_and_a_shared_family_is_warned_about() {
        let p = plan("q?", "high", &[], Some("claude"), &mixed(), None);
        let fams: Vec<&str> = p["proposed_seats"].as_array().unwrap().iter().map(|s| s["family"].as_str().unwrap()).collect();
        assert_eq!(fams.len(), 2);
        assert_ne!(fams[0], fams[1], "{p}");
        assert_eq!(p["plan_budget"]["homogeneous"], false);
        let same = env(vec![opt("a", "gemini", "agy"), opt("b", "gemini", "api")]);
        let p = plan("q?", "high", &[], Some("gemini"), &same, None);
        assert!(p["proposed_seats"].as_array().unwrap().is_empty(), "the host's own family is never the foreign one: {p}");
        let p = plan("q?", "high", &[], Some("claude"), &same, None);
        assert_eq!(p["proposed_seats"].as_array().unwrap().len(), 2);
        assert!(p["plan_note"].as_str().unwrap().contains("shares a family"), "{p}");
        let p = plan("q?", "high", &[], None, &same, None);
        assert_eq!(p["plan_budget"]["homogeneous"], true, "an unknown host plus one foreign family is one known family: {p}");
        assert!(p["plan_note"].as_str().unwrap().contains("Homogeneous"), "{p}");
    }

    #[test]
    fn the_budget_total_is_summed_and_require_families_is_judged() {
        let p = plan("q?", "high", &[], Some("claude"), &mixed(), Some(3));
        let b = &p["plan_budget"];
        assert_eq!(b["seats"], 2);
        assert_eq!(b["ceiling_tokens_total"], 80_000);
        let lo: u64 = p["proposed_seats"].as_array().unwrap().iter().map(|s| s["estimate_tokens"]["low"].as_u64().unwrap()).sum();
        assert_eq!(b["estimate_tokens"]["low"], lo);
        assert_eq!(b["require_families_met"], true, "claude host + openai + gemini: {p}");
        let p = plan("q?", "normal", &[], Some("claude"), &mixed(), Some(3));
        assert_eq!(p["plan_budget"]["require_families_met"], false, "{p}");
        assert!(p["plan_note"].as_str().unwrap().contains("require_families = 3 is not met"), "{p}");
    }

    #[test]
    fn kinds_follow_the_harness() {
        assert_eq!([kind_of(false, None), kind_of(true, None), kind_of(true, Some("copilot")), kind_of(true, Some("agy")), kind_of(true, Some("codex"))], ["api", "claude-cli", "copilot-acp", "agy", "codex-cli"]);
    }
}
