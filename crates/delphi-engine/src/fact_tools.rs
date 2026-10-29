//! The validator's tools (`specs/040` §7.2 step 1, §19.2, §24.2): requests, never writes.
//!
//! `ratify_fact` and `validate_claims` are offered only while the effective mode is not `off`
//! ([`Registry::with_validator`](crate::tools::Registry::with_validator)). Neither tool fetches, judges or writes
//! anything: each validates the shape of what the caller sent and asks the engine to log a `validator.request`
//! event. The daemon takes it from there (`fact_intake`, then `fact_ratify`), so a model cannot approve its own claim
//! and the reply it gets back says nothing but "submitted" and the id the record would have.

use crate::fact_check::Submission;
use crate::fact_intake::{normalise, MAX_CLAIMS_PER_CALL};
use crate::fact_store::fact_id;
use crate::tools::{ActionKind, Classified, ToolCtx, ToolError, ToolExec, ToolResult, ToolSpec, ToolTier};
use delphi_adapter::claude::ToolDef;

use serde_json::{json, Value};

/// The names of the validator's tools. Registered always; the engine offers and runs them only while the effective mode
/// (read at each use, so a mode change needs no restart) is not `off`.
pub const NAMES: [&str; 2] = ["ratify_fact", "validate_claims"];

/// Roles that may ask for a check. The role comes from the daemon's tool context, never from an argument.
const CALLERS: [&str; 3] = ["validator", "scout", "orchestrator"];

fn classify(tool: &str, args: &Value) -> Result<Classified, ToolError> {
    Ok(Classified { tool: tool.into(), tier: ToolTier::Green, kind: ActionKind::Read, summary: tool.into(), payload: args.clone(), pattern: tool.into() })
}

fn ratify_fact(ctx: &mut ToolCtx, args: &Value) -> ToolResult {
    if ctx.role != "validator" {
        return ToolResult::err("only the validator asks for a fact to be ratified");
    }
    // `deny_unknown_fields`: a quote, a hash or an origin key is refused, not ignored.
    let sub: Submission = match serde_json::from_value(args.clone()) {
        Ok(s) => s,
        Err(e) => return ToolResult::err(format!("a fact submission is the claim, `needed_for`, the URLs and optionally `kind`; nothing else ({})", e)),
    };
    if let Err(why) = normalise(&sub.claim) {
        return ToolResult::err(why);
    }
    let id = fact_id(&sub.claim);
    ctx.events.push(("validator.request".into(), json!({"tool": "ratify_fact", "caller": ctx.role, "claim": sub.claim, "needed_for": sub.needed_for, "urls": sub.urls, "kind": sub.kind})));
    ToolResult::ok(json!({"status": "submitted", "fact_id": id}).to_string())
}

fn validate_claims(ctx: &mut ToolCtx, args: &Value) -> ToolResult {
    if !CALLERS.contains(&ctx.role) {
        return ToolResult::err("this role may not ask for claims to be validated");
    }
    if args.as_object().is_none_or(|o| o.keys().any(|k| k != "claims")) {
        return ToolResult::err("`validate_claims` takes `claims` and nothing else: the daemon chooses the queries and the sources");
    }
    let Some(list) = args["claims"].as_array() else { return ToolResult::err("missing `claims`") };
    if list.is_empty() || list.len() > MAX_CLAIMS_PER_CALL {
        return ToolResult::err(format!("send 1 to {} claims", MAX_CLAIMS_PER_CALL));
    }
    let mut claims = Vec::new();
    for c in list {
        let Some(c) = c.as_str() else { return ToolResult::err("each claim is a sentence of text") };
        match normalise(c) {
            Ok(n) => claims.push(n),
            Err(why) => return ToolResult::err(why),
        }
    }
    let ids: Vec<Value> = claims.iter().map(|c| json!({"fact_id": fact_id(c), "status": "submitted"})).collect();
    ctx.events.push(("validator.request".into(), json!({"tool": "validate_claims", "caller": ctx.role, "claims": claims})));
    ToolResult::ok(Value::Array(ids).to_string())
}

pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            def: ToolDef {
                name: "ratify_fact".into(),
                description: "Validator only. Propose one fact for checking: `claim` (one sentence, at most 300 characters), `needed_for` (why it is wanted), `urls` (one to five pages you rely on) and optionally `kind` (`fact` or `record`). Send no quotes, hashes or origins: the daemon fetches the pages itself, reads the passages itself, asks a second model, searches for contradictions and decides. You are told only `submitted` and the id the record would have; you cannot write or approve anything.".into(),
                input_schema: json!({"type": "object", "required": ["claim", "needed_for", "urls"], "additionalProperties": false, "properties": {"claim": {"type": "string", "maxLength": 300}, "needed_for": {"type": "string", "maxLength": 200}, "urls": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 5}, "kind": {"type": "string", "enum": ["fact", "record"]}}}),
            },
            requires_write_scope: false,
            classify,
            exec: ToolExec::Local(ratify_fact),
        },
        ToolSpec {
            def: ToolDef {
                name: "validate_claims".into(),
                description: "Ask the daemon to validate claims you have noted, by claim text only (at most 20, each one plain sentence). Send no URLs, queries or search operators: the daemon chooses what to search. You are told only a status and a fact id for each; no evidence, query or timing is returned.".into(),
                input_schema: json!({"type": "object", "required": ["claims"], "additionalProperties": false, "properties": {"claims": {"type": "array", "items": {"type": "string", "maxLength": 300}, "minItems": 1, "maxItems": MAX_CLAIMS_PER_CALL}}}),
            },
            requires_write_scope: false,
            classify,
            exec: ToolExec::Local(validate_claims),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::AssumeClean;
    use crate::tools::Registry;
    use delphi_adapter::config::ValidatorMode;
    use delphi_core::pathguard::{Guard, Zones};
    use delphi_core::provenance::{Index, OriginRules, Span};
    use std::collections::HashSet;

    fn call(role: &str, tool: &str, args: Value) -> (ToolResult, Vec<(String, Value)>) {
        let r = std::path::Path::new("/nonexistent");
        let guard = Guard { zones: Zones { local: r.join("l"), team: r.join("t"), base: r.join("b"), ratified: r.join("r"), worktree: None, projects: vec![], bundles: vec![], payload: None }, role: role.into(), read_globs: vec![], write_globs: vec![], protected: HashSet::new() };
        let (mut span, mut index, rules) = (Span::root("s"), Index::new(), OriginRules::with_defaults());
        let mut ctx = ToolCtx { guard: &guard, span: &mut span, index: &mut index, rules: &rules, probe: &AssumeClean, role, mission_id: "m1", now_rfc3339: "t", stamps: vec![], events: vec![] };
        let res = Registry::builtin().with_validator(ValidatorMode::Advisory).run(&mut ctx, tool, &args);
        (res, ctx.events)
    }

    #[test]
    fn the_tools_exist_only_while_the_validator_is_not_off() {
        assert!(Registry::builtin().with_validator(ValidatorMode::Off).get("ratify_fact").is_none());
        assert!(Registry::builtin().get("validate_claims").is_none());
        for m in [ValidatorMode::Advisory, ValidatorMode::Ratify] {
            let r = Registry::builtin().with_validator(m);
            assert!(r.get("ratify_fact").is_some() && r.get("validate_claims").is_some());
        }
    }

    #[test]
    fn a_request_writes_nothing_and_a_submission_with_a_quote_or_hash_is_refused() {
        let ok = json!({"claim": "Harry Nilsson wrote the song 'One' (1968).", "needed_for": "q", "urls": ["https://example.org/a"]});
        let (r, ev) = call("validator", "ratify_fact", ok.clone());
        assert!(!r.is_error, "{}", r.content);
        assert_eq!(ev[0].0, "validator.request");
        assert!(r.content.contains("submitted") && r.content.contains("fact-") && !r.content.contains("example.org"));
        for extra in ["quote", "snapshot_sha256", "origin"] {
            let mut bad = ok.clone();
            bad[extra] = "x".into();
            assert!(call("validator", "ratify_fact", bad).0.is_error, "{}", extra);
        }
        assert!(call("builder", "ratify_fact", ok).0.is_error, "only the validator");
    }

    #[test]
    fn validate_claims_takes_claims_only_and_answers_with_status_and_id_only() {
        let (r, ev) = call("scout", "validate_claims", json!({"claims": ["Harry Nilsson wrote the song 'One' (1968)."]}));
        assert!(!r.is_error, "{}", r.content);
        let v: Value = serde_json::from_str(&r.content).unwrap();
        assert_eq!(v[0].as_object().unwrap().keys().collect::<Vec<_>>(), ["fact_id", "status"]);
        assert_eq!(ev[0].1["caller"], "scout");
        for bad in [json!({"claims": ["X is Y."], "urls": ["https://e.org"]}), json!({"claims": ["see https://evil.example"]}), json!({"claims": []}), json!({"claims": ["site:evil.example x y"]})] {
            let (r, ev) = call("scout", "validate_claims", bad);
            assert!(r.is_error && ev.is_empty());
        }
        assert!(call("builder", "validate_claims", json!({"claims": ["X is Y."]})).0.is_error);
    }
}
