//! What Delphi knows of each model a carrier serves (`specs/132` §5.1 to §5.3, slice C3).
//!
//! Two sources, both DATA and ADVICE, never a grant: nothing here allows an endpoint, a model or a tool, and nothing here raises a
//! seat's trust.
//! * The **seed table** (`model_capabilities.toml`, shipped): what was measured on 2026-11-20 of each latest-generation model (vendor
//!   and family, Copilot multiplier and price category, prompt budget in tokens, bytes per token, JSON-in-text behaviour). It is the
//!   default until a live read replaces it. Spec 131 S2 (the per-carrier input budget) is not built; it can read
//!   [`SeedModel::prompt_budget_tokens`].
//! * The **Copilot catalog**: the account's own model list, read through `copilot --acp` `session/new` (`models.availableModels[]`,
//!   each with `modelId` and `_meta {copilotUsage, copilotEnablement, copilotPriceCategory}`), cached in
//!   `local/copilot-catalog.json` with the CLI version it was read on. The survey read it free (no prompt is sent).
//!
//! **The cost class is a display and a cost input only.** It is the vendor's multiplier, read and labelled; it never decides a
//! model's tier, class or capability, never moves a seat, and Delphi never lowers a seat to a cheaper model by itself.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// The shipped seed table.
pub const SHIPPED_SEED: &str = include_str!("model_capabilities.toml");
/// The cache of the account's Copilot catalog, in the workspace's `local/`.
pub const CATALOG_FILE: &str = "copilot-catalog.json";
/// A cached catalog older than this is read again.
pub const CATALOG_MAX_AGE_SECS: i64 = 24 * 3600;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SeedModel {
    pub id: String,
    pub carrier: String,
    pub vendor: String,
    pub family: String,
    /// Copilot's premium-request multiplier (0 where the carrier has none).
    pub multiplier: f64,
    pub price_category: String,
    pub prompt_budget_tokens: u64,
    pub bytes_per_token: f64,
    /// The prompt size at which delivery of the last line was checked (0: not paid-tested).
    pub tail_tested_tokens: u64,
    pub json_in_text: String,
    /// Where the carrier reported the prompt's tokens (`input`, `cache_write`, `both`); empty when the survey did not record it.
    #[serde(default)]
    pub token_accounting: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SeedCarrier {
    pub name: String,
    #[serde(default)]
    pub silent_input_cut_tokens: Option<u64>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Seed {
    pub version: u32,
    pub measured: String,
    pub copilot_cli: String,
    pub agy_cli: String,
    #[serde(default, rename = "carrier")]
    pub carriers: Vec<SeedCarrier>,
    #[serde(default, rename = "model")]
    pub models: Vec<SeedModel>,
}

impl Seed {
    pub fn parse(text: &str) -> Result<Seed, String> {
        toml::from_str(text).map_err(|e| format!("model capabilities: {e}"))
    }

    pub fn shipped() -> Seed {
        Seed::parse(SHIPPED_SEED).expect("the shipped seed table parses (a test pins this)")
    }

    pub fn model(&self, carrier: &str, id: &str) -> Option<&SeedModel> {
        self.models.iter().find(|m| m.carrier == carrier && m.id == id)
    }
}

/// The vendor's multiplier as a label (`specs/132` §5.2): `low` under 1x, `standard` at 1x, `premium` over 1x. Display and cost
/// input only; see the module note.
pub fn cost_class(multiplier: f64) -> &'static str {
    if multiplier < 1.0 {
        "low"
    } else if multiplier == 1.0 {
        "standard"
    } else {
        "premium"
    }
}

/// One model of the account's catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    /// `copilotUsage` as a number (`"0.33x"` is 0.33); `None` when the catalog gives none (`auto`).
    pub multiplier: Option<f64>,
    pub price_category: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub cli_version: String,
    pub read_unix: i64,
    pub models: Vec<CatalogModel>,
    /// The version the ACP server itself reported (`initialize` → `agentInfo.version`), which can differ from `--version`
    /// (`specs/132` §17.1, §17.2 item 5). `None` when it gave none, or for a cache written before C9.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_version: Option<String>,
}

impl Catalog {
    pub fn model(&self, id: &str) -> Option<&CatalogModel> {
        self.models.iter().find(|m| m.id == id)
    }

    /// Fresh enough, and read on this CLI version.
    pub fn usable_for(&self, cli_version: &str, now: i64) -> bool {
        self.cli_version == cli_version && now - self.read_unix <= CATALOG_MAX_AGE_SECS
    }
}

/// The models of a `session/new` answer (its `result`, or the whole JSON-RPC line). `auto` is not a model and is left out.
pub fn parse_session_new(v: &Value) -> Vec<CatalogModel> {
    let r = if v.get("result").is_some() { &v["result"] } else { v };
    r["models"]["availableModels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["modelId"].as_str().filter(|s| !s.is_empty() && *s != "auto")?;
            let meta = &m["_meta"];
            let multiplier = meta["copilotUsage"].as_str().and_then(|u| u.trim().trim_end_matches(['x', 'X']).parse::<f64>().ok());
            Some(CatalogModel { id: id.into(), multiplier, price_category: meta["copilotPriceCategory"].as_str().unwrap_or("").into(), enabled: meta["copilotEnablement"].as_str().is_none_or(|e| e == "enabled") })
        })
        .collect()
}

/// What is known of one configured model: its multiplier and cost class, for display and as a Panel plan's cost input only (never a
/// tier, class or capability decision), and whether a fresh catalog lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCost {
    pub model: String,
    /// The vendor's multiplier as shown (`"0.33x"`, `"1x"`, `"14x"`); `None` when neither the catalog nor the seed table gives one.
    pub multiplier: Option<String>,
    pub class: Option<&'static str>,
    /// `Some(false)`: a fresh catalog lacks the model or lists it disabled, so the tier is unusable. `None`: no fresh catalog to judge by.
    pub in_catalog: Option<bool>,
}

/// The model's cost from `fresh` (a catalog the caller found usable for the installed CLI) or, failing that, from the seed table.
pub fn model_cost(fresh: Option<&Catalog>, model: &str) -> ModelCost {
    let listed = fresh.and_then(|c| c.model(model));
    let multiplier = listed.and_then(|m| m.multiplier).or_else(|| Seed::shipped().model("copilot", model).map(|m| m.multiplier).filter(|m| *m > 0.0));
    ModelCost { model: model.into(), multiplier: multiplier.map(|m| format!("{m}x")), class: multiplier.map(cost_class), in_catalog: fresh.map(|_| listed.is_some_and(|m| m.enabled)) }
}

/// Why a seat on this endpoint's tier cannot run, judged by a fresh catalog: its model is missing from the account's list, or listed
/// but not enabled. Never a swap to another model: the tier is unusable and the caller says so.
pub fn refusal(fresh: &Catalog, endpoint: &str, tier: &str, model: &str) -> Option<String> {
    match fresh.model(model) {
        Some(m) if m.enabled => None,
        Some(_) => Some(format!("endpoint `{endpoint}`: its {tier} model `{model}` is listed but not enabled in this account's Copilot catalog (read on {}), so the {tier} tier is unusable; enable it in the account or change the endpoint's `models`", fresh.cli_version)),
        None => Some(format!("endpoint `{endpoint}`: its {tier} model `{model}` is not in this account's Copilot catalog (read on {}), so the {tier} tier is unusable; Delphi never swaps in another model. Change the endpoint's `models`", fresh.cli_version)),
    }
}

/// The cached catalog, when it was read on `cli_version` and is young enough.
pub fn fresh(local_dir: &Path, cli_version: &str, now: i64) -> Option<Catalog> {
    load(local_dir).filter(|c| c.usable_for(cli_version, now))
}

/// The cached catalog if fresh; otherwise `read` once (a `session/new` answer), cache what it returns, and use that. `None` when
/// neither works.
pub fn refresh(local_dir: &Path, cli_version: &str, now: i64, read: &dyn Fn() -> Option<Value>) -> Option<Catalog> {
    if let Some(c) = fresh(local_dir, cli_version, now) {
        return Some(c);
    }
    let answer = read()?;
    let models = parse_session_new(&answer);
    if models.is_empty() {
        return None;
    }
    let c = Catalog { cli_version: cli_version.into(), read_unix: now, models, acp_version: acp_version(&answer) };
    let _ = save(local_dir, &c);
    Some(c)
}

/// The ACP server's own version from a live read (the `agentInfo` [`read_live`] carries over from `initialize`).
pub fn acp_version(answer: &Value) -> Option<String> {
    answer["agentInfo"]["version"].as_str().or_else(|| answer["result"]["agentInfo"]["version"].as_str()).map(|v| v.trim().chars().take(40).collect::<String>()).filter(|v| !v.is_empty())
}

/// The bare version number in a `--version` line (`GitHub Copilot CLI 1.0.114.` gives `1.0.114`).
pub fn version_number(line: &str) -> Option<String> {
    line.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_ascii_digit())).find(|w| w.contains('.') && w.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))).map(String::from)
}

/// `delphi doctor`'s line when the ACP server reported another version than `--version` (`specs/132` §17.2 item 5): the seat runs the
/// ACP one, and that is the version Delphi records. `None` when they agree or either is unknown.
pub fn version_mismatch(cli_version_line: &str, acp: Option<&str>) -> Option<String> {
    let (cli, acp) = (version_number(cli_version_line)?, version_number(acp?)?);
    (cli != acp).then(|| format!("`copilot --version` says {cli} but its ACP server reports {acp}: a seat runs {acp}, and that is the version Delphi records for it. Probe results for {cli} do not describe the seat"))
}

pub fn cache_path(local_dir: &Path) -> PathBuf {
    local_dir.join(CATALOG_FILE)
}

pub fn load(local_dir: &Path) -> Option<Catalog> {
    serde_json::from_str(&std::fs::read_to_string(cache_path(local_dir)).ok()?).ok()
}

pub fn save(local_dir: &Path, c: &Catalog) -> std::io::Result<()> {
    std::fs::create_dir_all(local_dir)?;
    std::fs::write(cache_path(local_dir), serde_json::to_string_pretty(c).unwrap_or_default())
}

/// Read the catalog live: `copilot --acp`, `initialize`, then `session/new` with no prompt, in a scratch working directory and
/// `COPILOT_HOME` (the login is in the OS keychain, so it still authenticates; nothing lands in `~/.copilot`). The answer's
/// `result` is returned; the process is stopped right after. No prompt is ever sent.
pub fn read_live(program: &str, timeout: Duration) -> Option<Value> {
    let scratch = std::env::temp_dir().join(format!("delphi-copilot-catalog-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).ok()?;
    let out = read_in(program, &scratch, timeout);
    let _ = std::fs::remove_dir_all(&scratch);
    out
}

fn read_in(program: &str, scratch: &Path, timeout: Duration) -> Option<Value> {
    let mut child = Command::new(program)
        .args(["--acp", "--no-custom-instructions", "--disable-builtin-mcps", "--no-auto-update", "--disable-client-telemetry", "--no-ask-user", "--no-experimental", "--no-bash-env"])
        .current_dir(scratch)
        .env("COPILOT_HOME", scratch.join(".copilot-home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if tx.send(v).is_err() {
                    break;
                }
            }
        }
    });
    let send = |stdin: &mut std::process::ChildStdin, v: Value| writeln!(stdin, "{}", v).and_then(|_| stdin.flush());
    let mut out: Option<Value> = None;
    let mut agent_info = Value::Null;
    let cwd = scratch.display().to_string();
    if send(&mut stdin, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": 1, "clientCapabilities": {}}})).is_ok() {
        let deadline = std::time::Instant::now() + timeout;
        let mut asked = false;
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            let Ok(m) = rx.recv_timeout(left) else { break };
            match m["id"].as_i64() {
                Some(1) if !asked => {
                    asked = true;
                    agent_info = m["result"]["agentInfo"].clone();
                    if send(&mut stdin, json!({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": cwd, "mcpServers": []}})).is_err() {
                        break;
                    }
                }
                Some(2) => {
                    out = m.get("result").cloned();
                    // The ACP server's own version travels with the answer (`specs/132` §17.2 item 5).
                    if let (Some(o), false) = (out.as_mut().and_then(|v| v.as_object_mut()), agent_info.is_null()) {
                        o.insert("agentInfo".into(), agent_info.clone());
                    }
                    break;
                }
                _ => {}
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seat::{resolve, FamilyTable};

    /// The latest generation per model line (operator, 2026-11-20). An older generation of a listed line must not be added.
    const SUPPORTED: [&str; 17] = ["claude-sonnet-5.5", "claude-haiku-5.5", "claude-fable-5.1", "claude-opus-5.5", "gpt-6.2-sol", "gpt-6.2-astra", "gpt-6.1-terra", "gpt-6-luna", "gpt-5.3-codex", "gemini-4-argon", "gemini-4-carbon", "gemini-4-hydrogen", "gemini-4-neon", "gemini-3.8-flash", "kimi-k3", "mai-code-2", "mai-code-2-flash"];
    const OLDER: [&str; 10] = ["claude-sonnet-5", "claude-haiku-4.5", "claude-fable-5", "claude-opus-4.8-fast", "gpt-6.1-sol", "gpt-6-sol", "gpt-5.6-sol", "gpt-5.6-luna", "gpt-6-astra", "mai-code-1.1-flash"];

    #[test]
    fn the_acp_version_is_kept_and_a_mismatch_with_the_cli_version_is_said() {
        let mut answer = session_new();
        answer["agentInfo"] = json!({"name": "Copilot", "version": "1.0.107"});
        assert_eq!(acp_version(&answer).as_deref(), Some("1.0.107"));
        let d = tempfile::tempdir().unwrap();
        let c = refresh(d.path(), "GitHub Copilot CLI 1.0.114.", 1000, &|| Some(answer.clone())).unwrap();
        assert_eq!(c.acp_version.as_deref(), Some("1.0.107"));
        assert_eq!(load(d.path()).unwrap().acp_version.as_deref(), Some("1.0.107"), "the cache keeps it");
        assert_eq!(version_number("GitHub Copilot CLI 1.0.114."), Some("1.0.114".into()));
        let line = version_mismatch("GitHub Copilot CLI 1.0.114.", Some("1.0.107")).unwrap();
        assert!(line.contains("1.0.114") && line.contains("1.0.107"), "{line}");
        assert_eq!(version_mismatch("GitHub Copilot CLI 1.0.114.", Some("1.0.114")), None);
        assert_eq!(version_mismatch("GitHub Copilot CLI 1.0.114.", None), None);
        // A cache written before C9 has no field and still loads.
        let old: Catalog = serde_json::from_str(r#"{"cli_version":"v","read_unix":0,"models":[]}"#).unwrap();
        assert_eq!(old.acp_version, None);
    }

    #[test]
    fn the_seed_table_holds_the_seventeen_copilot_models_and_no_older_generation() {
        let s = Seed::shipped();
        let copilot: Vec<&str> = s.models.iter().filter(|m| m.carrier == "copilot").map(|m| m.id.as_str()).collect();
        assert_eq!(copilot.len(), 17);
        assert!(SUPPORTED.iter().all(|id| copilot.contains(id)), "{copilot:?}");
        assert!(OLDER.iter().all(|id| !s.models.iter().any(|m| m.id == *id)));
        assert!(s.model("agy", "gpt-oss-120b-medium").is_some_and(|m| m.family == "openai"));
        assert_eq!(s.carriers.iter().find(|c| c.name == "agy").and_then(|c| c.silent_input_cut_tokens), Some(47_000));
        assert_eq!(s.model("copilot", "gemini-4-hydrogen").map(|m| m.prompt_budget_tokens), Some(983_040));
        assert!(s.models.iter().all(|m| m.prompt_budget_tokens > 0));
    }

    /// Every row's family is the one the family table resolves for its id: kimi and mai are their own families, nothing is `unknown`.
    #[test]
    fn every_seed_family_is_the_family_tables() {
        let t = FamilyTable::shipped();
        for m in Seed::shipped().models {
            assert_eq!(resolve(&t, Some(&m.id)).family, m.family, "{}", m.id);
        }
        assert_eq!(resolve(&t, Some("kimi-k3")).family, "kimi");
        assert_eq!(resolve(&t, Some("mai-code-2-flash")).family, "mai");
    }

    #[test]
    fn the_cost_class_is_the_multipliers_label() {
        assert_eq!((cost_class(0.33), cost_class(1.0), cost_class(14.0), cost_class(15.0)), ("low", "standard", "premium", "premium"));
    }

    /// A `session/new` answer shaped like the survey's (catalog-raw.json, trimmed).
    pub(crate) fn session_new() -> Value {
        json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "s", "models": {"availableModels": [
            {"modelId": "auto", "name": "Auto", "description": "Let Copilot pick the best model"},
            {"modelId": "claude-sonnet-5.5", "name": "Claude Sonnet 5.5", "_meta": {"copilotUsage": "1x", "copilotEnablement": "enabled", "copilotPriceCategory": "medium"}},
            {"modelId": "claude-haiku-4.5", "name": "Claude Haiku 4.5", "_meta": {"copilotUsage": "0.33x", "copilotEnablement": "enabled", "copilotPriceCategory": "low"}},
            {"modelId": "gemini-3.8-flash", "name": "Gemini 3.8 Flash", "_meta": {"copilotUsage": "14x", "copilotEnablement": "enabled", "copilotPriceCategory": "low"}},
            {"modelId": "kimi-k3", "name": "Kimi K3", "_meta": {"copilotUsage": "1x", "copilotEnablement": "disabled", "copilotPriceCategory": "medium"}}
        ]}}})
    }

    #[test]
    fn a_session_new_answer_becomes_the_catalog_without_auto() {
        let m = parse_session_new(&session_new());
        assert_eq!(m.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["claude-sonnet-5.5", "claude-haiku-4.5", "gemini-3.8-flash", "kimi-k3"]);
        assert_eq!(m[1].multiplier, Some(0.33));
        assert_eq!(cost_class(m[2].multiplier.unwrap()), "premium", "14x");
        assert!(!m[3].enabled);
        assert!(parse_session_new(&json!({"result": {}})).is_empty());
    }

    #[test]
    fn the_cache_keeps_its_cli_version_and_age() {
        let d = tempfile::tempdir().unwrap();
        let c = Catalog { cli_version: "1.0.112".into(), read_unix: 1000, models: parse_session_new(&session_new()), acp_version: None };
        save(d.path(), &c).unwrap();
        let back = load(d.path()).unwrap();
        assert_eq!(back, c);
        assert!(back.usable_for("1.0.112", 1000 + 3600));
        assert!(!back.usable_for("1.0.113", 1000), "another CLI version is not reused");
        assert!(!back.usable_for("1.0.112", 1000 + CATALOG_MAX_AGE_SECS + 1));
    }

    /// The cost comes from the fresh catalog, else the seed table; only a fresh catalog can call a model missing.
    #[test]
    fn a_models_cost_and_presence_come_from_the_fresh_catalog_then_the_seed() {
        let c = Catalog { cli_version: "v".into(), read_unix: 0, models: parse_session_new(&session_new()), acp_version: None };
        let g = model_cost(Some(&c), "gemini-3.8-flash");
        assert_eq!((g.multiplier.as_deref(), g.class, g.in_catalog), (Some("14x"), Some("premium"), Some(true)));
        assert_eq!(model_cost(Some(&c), "kimi-k3").in_catalog, Some(false), "listed but disabled");
        let argon = model_cost(Some(&c), "gemini-4-argon");
        assert_eq!((argon.multiplier.as_deref(), argon.class, argon.in_catalog), (Some("1x"), Some("standard"), Some(false)), "missing, cost from the seed");
        assert_eq!(model_cost(None, "gemini-4-argon").in_catalog, None);
        assert_eq!(model_cost(None, "no-such-model").class, None);
        assert!(refusal(&c, "e", "fast", "claude-sonnet-5.5").is_none());
        assert!(refusal(&c, "e", "fast", "gemini-4-argon").unwrap().contains("fast model `gemini-4-argon` is not in this account's Copilot catalog"));
        assert!(refusal(&c, "e", "frontier", "kimi-k3").unwrap().contains("not enabled"));
    }

    #[test]
    fn refresh_reads_once_and_then_uses_the_cache() {
        let d = tempfile::tempdir().unwrap();
        let reads = std::cell::Cell::new(0);
        let read = || {
            reads.set(reads.get() + 1);
            Some(session_new())
        };
        assert_eq!(refresh(d.path(), "1.0.112", 1000, &read).unwrap().models.len(), 4);
        assert_eq!(refresh(d.path(), "1.0.112", 2000, &read).unwrap().read_unix, 1000);
        assert_eq!(reads.get(), 1);
        assert_eq!(refresh(d.path(), "1.0.113", 2000, &read).unwrap().cli_version, "1.0.113", "a new CLI version is read again");
        assert!(refresh(tempfile::tempdir().unwrap().path(), "1.0.112", 0, &|| None).is_none());
    }

    /// The live read with a stand-in `copilot --acp`: it answers `initialize`, then `session/new`; no prompt is sent.
    #[test]
    fn the_live_read_takes_session_new_and_sends_no_prompt() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let seen = d.path().join("seen");
        let answer = session_new().to_string().replace('\'', "");
        let script = format!("#!/bin/sh\necho \"$*\" > {s}\nread a\necho '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nread b\necho \"$b\" >> {s}\necho '{answer}'\nsleep 5\n", s = seen.display());
        let prog = d.path().join("copilot");
        std::fs::write(&prog, script).unwrap();
        std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
        let r = read_live(prog.to_str().unwrap(), Duration::from_secs(5)).expect("an answer");
        assert_eq!(parse_session_new(&r).len(), 4);
        let s = std::fs::read_to_string(&seen).unwrap();
        assert!(s.starts_with("--acp ") && s.contains("session/new") && !s.contains("session/prompt"), "{s}");
    }
}
