//! The CLI features a `[harness.claude]` or `[harness.agy]` block switches on (`specs/021`).
//!
//! These keys sit in the ratified `[harness.*]` block, so a local edit cannot widen what a span reaches
//! (subagents, extra directories, hooks, a spend limit) without the operator ratifying it. Nothing here is
//! read from a role manifest or a model's output. A key that is absent leaves the plain launch unchanged.
//!
//! A bad value is skipped and reported by [`problems`] (shown on attach); keys this module does not know are
//! left alone, because other features share the block.

use crate::harness::{Cli, Opts, SessionMode};
use std::path::PathBuf;

const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
/// Hook kinds whose command output can allow a tool call; only Delphi's own deny hooks (`specs/018` §4) may guard one.
const ALLOW_CAPABLE_HOOKS: [&str; 2] = ["pretooluse", "permissionrequest"];
const AGY_MODES: [&str; 3] = ["plan", "accept-edits", "auto"];

/// The options for one CLI's block, with each rejected key's reason.
pub fn parse(root: &toml::Value, cli: Cli) -> (Opts, Vec<String>) {
    let mut o = Opts::default();
    let mut bad = Vec::new();
    let name = cli.program();
    let Some(t) = root.get("harness").and_then(|h| h.get(name)).and_then(|v| v.as_table()) else { return (o, bad) };
    let mut reject = |key: &str, why: &str| bad.push(format!("[harness.{}] {}: {}", name, key, why));

    if let Some(v) = t.get("effort") {
        match v.as_str().filter(|e| EFFORTS.contains(e)) {
            Some(e) => o.effort = Some(e.into()),
            None => reject("effort", "expected low, medium, high, xhigh or max"),
        }
    }
    if let Some(v) = t.get("json_schema") {
        match v.as_str().and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()).filter(|j| j.is_object()) {
            Some(j) => o.json_schema = Some(j.to_string()),
            None => reject("json_schema", "expected a JSON Schema object as a string"),
        }
    }
    if let Some(v) = t.get("session") {
        match v.as_str() {
            Some("ephemeral") => {}
            Some("persist") => o.session = SessionMode::Persist,
            _ => reject("session", "expected \"ephemeral\" or \"persist\""),
        }
    }
    if let Some(v) = t.get("add_dirs") {
        let dirs: Option<Vec<PathBuf>> = v.as_array().and_then(|a| a.iter().map(|d| d.as_str().map(PathBuf::from).filter(|p| p.is_absolute())).collect());
        match dirs {
            Some(d) => o.add_dirs = d,
            None => reject("add_dirs", "expected a list of absolute paths"),
        }
    }
    if let Some(v) = t.get("sandbox") {
        match v.as_bool() {
            Some(b) => o.sandbox = b,
            None => reject("sandbox", "expected true or false"),
        }
    }
    if let Some(v) = t.get("print_timeout_secs") {
        match v.as_integer().filter(|n| *n > 0) {
            Some(n) => o.print_timeout_secs = Some(n as u64),
            None => reject("print_timeout_secs", "expected a positive number of seconds"),
        }
    }
    match cli {
        Cli::Claude => {
            if let Some(v) = t.get("max_turns") {
                match v.as_integer().filter(|n| *n > 0) {
                    Some(n) => o.max_turns = Some(n as u32),
                    None => reject("max_turns", "expected a positive whole number"),
                }
            }
            if let Some(v) = t.get("max_budget_usd") {
                match v.as_float().or_else(|| v.as_integer().map(|n| n as f64)).filter(|x| *x > 0.0) {
                    Some(x) => o.max_budget_usd = Some(x),
                    None => reject("max_budget_usd", "expected a positive amount"),
                }
            }
            if let Some(v) = t.get("fallback_models") {
                match v.as_array().and_then(|a| a.iter().map(|m| m.as_str().map(String::from).filter(|m| !m.is_empty() && !m.contains(','))).collect::<Option<Vec<_>>>()) {
                    Some(m) => o.fallback_models = m,
                    None => reject("fallback_models", "expected a list of model names"),
                }
            }
            if let Some(v) = t.get("partial_messages") {
                match v.as_bool() {
                    Some(b) => o.partial_messages = b,
                    None => reject("partial_messages", "expected true or false"),
                }
            }
            // Tables become the JSON the CLI takes: `[harness.claude.agents.<name>]` and `[harness.claude.settings]`.
            for (key, slot) in [("agents", &mut o.agents_json), ("settings", &mut o.settings_json)] {
                if let Some(v) = t.get(key) {
                    if let Some(kind) = allow_capable_hook(key, v) {
                        reject(key, &format!("a `{}` hook can allow a tool call and is runtime-only enforceable, so it cannot be configured; guard with a `[[hooks.deny]]` hook (specs/018 018-A1)", kind));
                        continue;
                    }
                    match v.as_table().filter(|t| !t.is_empty()).and_then(|_| serde_json::to_string(v).ok()) {
                        Some(j) => *slot = Some(j),
                        None => reject(key, "expected a table"),
                    }
                }
            }
        }
        Cli::Codex => {
            // Only the levels the CLI documents for `model_reasoning_effort`; the rest stay out until probed.
            if o.effort.as_deref().is_some_and(|e| !matches!(e, "low" | "medium" | "high")) {
                o.effort = None;
                reject("effort", "expected low, medium or high for codex");
            }
        }
        Cli::Copilot => {
            // Each span runs in a scratch `COPILOT_HOME` that is not kept, and the CLI has no structured-output flag: a notice
            // for each, not a silent drop (`specs/046` §1.4).
            if o.json_schema.take().is_some() {
                reject("json_schema", "copilot has no structured-output flag; use another seat for a span that needs one");
            }
            if o.session == SessionMode::Persist {
                o.session = SessionMode::Ephemeral;
                reject("session", "a copilot span's home is a scratch directory, so it cannot be resumed; use \"ephemeral\"");
            }
        }
        Cli::Agy => {
            if let Some(v) = t.get("mode") {
                match v.as_str().filter(|m| AGY_MODES.contains(m)) {
                    Some(m) => o.mode = Some(m.into()),
                    None => reject("mode", "expected plan, accept-edits or auto"),
                }
            }
        }
    }
    (o, bad)
}

/// The first allow-capable hook kind in a `settings` table's `hooks`, or in an agent's own `hooks`.
fn allow_capable_hook(key: &str, v: &toml::Value) -> Option<String> {
    let hook_tables: Vec<&toml::Value> = match key {
        "settings" => v.get("hooks").into_iter().collect(),
        _ => v.as_table().into_iter().flat_map(|a| a.values().filter_map(|d| d.get("hooks"))).collect(),
    };
    hook_tables.iter().filter_map(|h| h.as_table()).flat_map(|h| h.keys()).find(|k| ALLOW_CAPABLE_HOOKS.contains(&k.to_lowercase().as_str())).cloned()
}

/// Every rejected key across both blocks, for the attach notices.
pub fn problems(root: &toml::Value) -> Vec<String> {
    [Cli::Claude, Cli::Agy, Cli::Codex, Cli::Copilot].into_iter().flat_map(|c| parse(root, c).1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(s: &str) -> toml::Value {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn an_absent_block_changes_nothing() {
        let (o, bad) = parse(&root("[harness]\nsubscription_budget_tokens = 5\n"), Cli::Claude);
        assert!(bad.is_empty() && o.effort.is_none() && o.session == SessionMode::Ephemeral && !o.partial_messages && o.agents_json.is_none());
    }

    #[test]
    fn claude_keys_become_options() {
        let r = root(
            r#"
[harness.claude]
effort = "high"
max_turns = 12
max_budget_usd = 2
fallback_models = ["sonnet", "haiku"]
partial_messages = true
session = "persist"
add_dirs = ["/srv/shared"]
bridge = true
[harness.claude.agents.reviewer]
description = "Reviews a diff"
prompt = "Be strict."
tools = ["Read", "Grep"]
[harness.claude.settings.hooks]
PostToolUse = [{ matcher = "Bash", hooks = [{ type = "command", command = "/usr/local/bin/log" }] }]
"#,
        );
        let (o, bad) = parse(&r, Cli::Claude);
        assert!(bad.is_empty(), "{:?}", bad);
        assert_eq!((o.effort.as_deref(), o.max_turns, o.max_budget_usd, o.fallback_models.as_slice()), (Some("high"), Some(12), Some(2.0), ["sonnet".to_string(), "haiku".to_string()].as_slice()));
        assert!(o.partial_messages && o.session == SessionMode::Persist && o.add_dirs == [PathBuf::from("/srv/shared")]);
        let agents: serde_json::Value = serde_json::from_str(o.agents_json.as_deref().unwrap()).unwrap();
        assert_eq!(agents["reviewer"]["tools"][1], "Grep");
        let settings: serde_json::Value = serde_json::from_str(o.settings_json.as_deref().unwrap()).unwrap();
        assert_eq!(settings["hooks"]["PostToolUse"][0]["matcher"], "Bash");
    }

    #[test]
    fn allow_capable_hooks_are_refused_and_named() {
        for kind in ["PreToolUse", "PermissionRequest", "pretooluse"] {
            let r = root(&format!("[harness.claude.settings.hooks]\n{} = [{{ matcher = \"Bash\", hooks = [{{ type = \"command\", command = \"/bin/gate\" }}] }}]\nStop = []\n", kind));
            let (o, bad) = parse(&r, Cli::Claude);
            assert!(o.settings_json.is_none(), "{} was forwarded", kind);
            assert!(bad.len() == 1 && bad[0].contains("settings") && bad[0].contains(kind), "{:?}", bad);
        }
        let r = root("[harness.claude.agents.r]\ndescription = \"d\"\n[harness.claude.agents.r.hooks]\nPreToolUse = []\n");
        let (o, bad) = parse(&r, Cli::Claude);
        assert!(o.agents_json.is_none() && bad.iter().any(|b| b.contains("agents")), "{:?}", bad);
    }

    #[test]
    fn empty_settings_are_unchanged() {
        let (o, bad) = parse(&root("[harness.claude.settings]\n"), Cli::Claude);
        assert!(o.settings_json.is_none() && bad.len() == 1 && bad[0].contains("expected a table"));
    }

    #[test]
    fn agy_keys_are_its_own_and_claude_only_keys_are_ignored_there() {
        let r = root("[harness.agy]\nmode = \"plan\"\nsandbox = true\nprint_timeout_secs = 90\neffort = \"low\"\nmax_turns = 3\n");
        let (o, bad) = parse(&r, Cli::Agy);
        assert!(bad.is_empty());
        assert_eq!((o.mode.as_deref(), o.sandbox, o.print_timeout_secs, o.effort.as_deref(), o.max_turns), (Some("plan"), true, Some(90), Some("low"), None));
    }

    #[test]
    fn a_config_with_these_keys_still_parses_because_the_ratified_profile_declares_them() {
        let text = format!("{}\n[harness.claude]\neffort = \"low\"\nbridge = true\n[harness.claude.agents.r]\ndescription = \"d\"\n[harness.agy]\nmode = \"plan\"\nsandbox = true\n", crate::testkit::CONFIG);
        let cfg = delphi_adapter::config::Config::parse(&text).unwrap_or_else(|e| panic!("{:?}", e));
        assert_eq!(cfg.harness["claude"].effort.as_ref().and_then(|v| v.as_str()), Some("low"));
        assert!(cfg.harness["claude"].bridge, "the bridge profile keys still work beside them");
    }

    #[test]
    fn bad_values_are_skipped_and_named() {
        let r = root("[harness.claude]\neffort = \"extreme\"\nmax_turns = 0\nadd_dirs = [\"relative\"]\nsession = \"forever\"\nagents = \"x\"\njson_schema = \"{nope\"\n[harness.agy]\nmode = \"yolo\"\n");
        let bad = problems(&r);
        for key in ["effort", "max_turns", "add_dirs", "session", "agents", "json_schema", "mode"] {
            assert!(bad.iter().any(|b| b.contains(&format!(" {}:", key))), "{} missing from {:?}", key, bad);
        }
        let (o, _) = parse(&r, Cli::Claude);
        assert!(o.effort.is_none() && o.max_turns.is_none() && o.add_dirs.is_empty() && o.session == SessionMode::Ephemeral);
    }
}
