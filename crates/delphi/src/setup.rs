//! `delphi setup`: one guided pass from a fresh install to a working workspace.
//!
//! It detects what is already on the machine (the `claude` and `agy` CLIs and whether they are signed in,
//! `gcloud`, the project's toolchain), asks only for what it cannot find, opens the right web pages, takes
//! credentials with hidden input straight into the keychain (never a file, never the screen), writes the
//! workspace `config.toml`, shows you the access change for ratification, and proves each endpoint with a
//! tiny call. Run it again any time: it skips what is done. `delphi setup --check` changes nothing.
//!
//! Everything that touches the world goes through [`Env`] and [`Ui`], so the whole flow is tested with
//! scripted answers and a fake machine.

use crate::home::{self, InitOptions};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, Item, Table};

/// The machine: programs, the keychain, the network.
pub trait Env {
    /// (exit code, stdout) of a program, or `None` if it cannot be started.
    fn run(&self, program: &str, args: &[&str]) -> Option<(i32, String)>;
    /// Run a program attached to the terminal (a login). True if it exited 0.
    fn run_tty(&self, program: &str, args: &[&str]) -> bool;
    /// Store a secret under Delphi's own keychain service. The value is never logged.
    fn secret_set(&self, name: &str, secret: &str) -> Result<(), String>;
    /// (status, body) of an HTTPS GET with headers.
    fn http_get(&self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, String), String>;
}

/// The person.
pub trait Ui {
    fn say(&mut self, text: &str);
    fn yes_no(&mut self, question: &str, default: bool) -> bool;
    fn line(&mut self, question: &str, default: &str) -> String;
    /// Hidden input; the answer is never echoed.
    fn secret(&mut self, question: &str) -> String;
    fn open_url(&mut self, url: &str);
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Detected {
    pub claude: Option<ClaudeState>,
    pub agy_version: Option<String>,
    pub codex: Option<CodexState>,
    pub copilot_version: Option<String>,
    pub gcloud: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexState {
    pub version: String,
    /// `codex login status` exits 0 when signed in (it prints to stderr, which is not read).
    pub signed_in: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeState {
    pub version: String,
    pub logged_in: bool,
    pub plan: String,
}

pub fn detect(env: &dyn Env) -> Detected {
    let claude = env.run("claude", &["--version"]).map(|(_, v)| {
        let status = env.run("claude", &["auth", "status", "--json"]).and_then(|(_, o)| serde_json::from_str::<Value>(&o).ok());
        ClaudeState { version: v.trim().to_string(), logged_in: status.as_ref().is_some_and(|s| s["loggedIn"] == true), plan: status.as_ref().and_then(|s| s["subscriptionType"].as_str()).unwrap_or("").to_string() }
    });
    let codex = env.run("codex", &["--version"]).map(|(_, v)| CodexState { version: v.trim().trim_start_matches("codex-cli").trim().to_string(), signed_in: matches!(env.run("codex", &["login", "status"]), Some((0, _))) });
    let copilot_version = env.run("copilot", &["--version"]).and_then(|(_, v)| {
        v.lines().next().and_then(|l| l.split_whitespace().find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))).map(|w| w.trim_end_matches('.').to_string())
    });
    Detected { claude, codex, agy_version: env.run("agy", &["--version"]).map(|(_, v)| v.trim().to_string()), copilot_version, gcloud: env.run("gcloud", &["--version"]).is_some() }
}

/// What the project's own toolchain suggests for the merge gates and the commands that need no approval.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectKind {
    pub name: &'static str,
    pub test: Vec<&'static str>,
    pub check: Option<Vec<&'static str>>,
    pub allow: Vec<&'static str>,
}

pub fn project_kind(root: &Path) -> Option<ProjectKind> {
    let has = |f: &str| root.join(f).exists();
    if has("Cargo.toml") {
        Some(ProjectKind { name: "Rust", test: vec!["cargo", "test"], check: Some(vec!["cargo", "clippy"]), allow: vec!["cargo test*", "cargo build*", "cargo check*", "cargo clippy*", "cargo fmt*"] })
    } else if has("package.json") {
        Some(ProjectKind { name: "Node", test: vec!["npm", "test"], check: None, allow: vec!["npm test*", "npm run test*", "npm run lint*", "npm run build*"] })
    } else if has("go.mod") {
        Some(ProjectKind { name: "Go", test: vec!["go", "test", "./..."], check: Some(vec!["go", "vet", "./..."]), allow: vec!["go test*", "go build*", "go vet*"] })
    } else if has("pyproject.toml") || has("requirements.txt") || has("setup.py") {
        Some(ProjectKind { name: "Python", test: vec!["python3", "-m", "pytest"], check: None, allow: vec!["python3 -m pytest*", "pytest*"] })
    } else {
        None
    }
}

// ── editing config.toml without losing the operator's comments ──────────────

fn endpoints(doc: &mut DocumentMut) -> &mut ArrayOfTables {
    if !doc.contains_key("endpoints") {
        doc["endpoints"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    doc["endpoints"].as_array_of_tables_mut().expect("endpoints is an array of tables")
}

fn has_endpoint(doc: &DocumentMut, id: &str) -> bool {
    doc.get("endpoints").and_then(|e| e.as_array_of_tables()).is_some_and(|a| a.iter().any(|t| t.get("id").and_then(|i| i.as_str()) == Some(id)))
}

fn str_array(items: &[&str]) -> Array {
    let mut a = Array::new();
    for i in items {
        a.push(*i);
    }
    a
}

fn add_to_array(table: &mut Table, key: &str, id: &str) {
    if !table.contains_key(key) {
        table[key] = value(Array::new());
    }
    if let Some(a) = table[key].as_array_mut() {
        if !a.iter().any(|v| v.as_str() == Some(id)) {
            a.push(id);
        }
    }
}

/// Make `id` allowed and last in the order for its family (an existing order is kept; this endpoint joins it).
fn allow_endpoint(doc: &mut DocumentMut, family: &str, id: &str) {
    if !doc.contains_key("access") {
        doc["access"] = Item::Table(Table::new());
    }
    let access = doc["access"].as_table_mut().expect("access is a table");
    add_to_array(access, "allowed_endpoints", id);
    if !access.contains_key("order") {
        access["order"] = Item::Table(Table::new());
        access["order"].as_table_mut().expect("order table").set_implicit(false);
    }
    if let Some(order) = access["order"].as_table_like_mut() {
        if !order.contains_key(family) {
            order.insert(family, value(Array::new()));
        }
        if let Some(a) = order.get_mut(family).and_then(|v| v.as_array_mut()) {
            if !a.iter().any(|v| v.as_str() == Some(id)) {
                a.push(id);
            }
        }
    }
}

fn models_table(entries: &[(&str, &str)]) -> toml_edit::InlineTable {
    let mut t = toml_edit::InlineTable::new();
    for (k, v) in entries {
        t.insert(*k, (*v).into());
    }
    t
}

fn add_endpoint(doc: &mut DocumentMut, family: &str, id: &str, fields: &[(&str, &str)], models: &[(&str, &str)]) {
    if has_endpoint(doc, id) {
        return;
    }
    let mut t = Table::new();
    t["id"] = value(id);
    t["family"] = value(family);
    for (k, v) in fields {
        t[*k] = value(*v);
    }
    if !models.is_empty() {
        t["models"] = value(models_table(models));
    }
    endpoints(doc).push(t);
    allow_endpoint(doc, family, id);
}

fn set_catalog(doc: &mut DocumentMut, model: &str, max_output: u64, input_usd: f64, output_usd: f64) {
    if !doc.contains_key("models") {
        doc["models"] = Item::Table(Table::new());
        doc["models"].as_table_mut().expect("models").set_implicit(true);
    }
    let models = doc["models"].as_table_mut().expect("models is a table");
    if !models.contains_key("catalog") {
        models["catalog"] = Item::Table(Table::new());
        models["catalog"].as_table_mut().expect("catalog").set_implicit(true);
    }
    let catalog = models["catalog"].as_table_mut().expect("catalog is a table");
    if catalog.contains_key(model) {
        return;
    }
    let mut t = Table::new();
    t["max_output_tokens"] = value(max_output as i64);
    t["input_usd_per_mtok"] = value(input_usd);
    t["output_usd_per_mtok"] = value(output_usd);
    catalog[model] = Item::Table(t);
}

/// Fill in `[exec]` only where the operator has not already.
fn exec_defaults(doc: &mut DocumentMut, kind: &Option<ProjectKind>) {
    if !doc.contains_key("exec") {
        doc["exec"] = Item::Table(Table::new());
    }
    let exec = doc["exec"].as_table_mut().expect("exec is a table");
    if !exec.contains_key("sandbox") {
        exec["sandbox"] = value("off");
    }
    if let Some(k) = kind {
        if !exec.contains_key("allow") {
            exec["allow"] = value(str_array(&k.allow));
        }
        if !exec.contains_key("test_command") {
            exec["test_command"] = value(str_array(&k.test));
        }
        if let (Some(c), false) = (&k.check, exec.contains_key("check_command")) {
            exec["check_command"] = value(str_array(c));
        }
    }
}

fn set_family(doc: &mut DocumentMut, tier_models: &[(&str, &str)], family: &str) {
    // `[models.<family>]` placeholders from the template (`<claude fast model id>`) are replaced; real values are kept.
    if !doc.contains_key("models") {
        doc["models"] = Item::Table(Table::new());
        doc["models"].as_table_mut().expect("models").set_implicit(true);
    }
    let models = doc["models"].as_table_mut().expect("models is a table");
    if !models.contains_key(family) {
        models[family] = Item::Table(Table::new());
    }
    let t = models[family].as_table_mut().expect("family table");
    for (tier, m) in tier_models {
        let current = t.get(tier).and_then(|v| v.as_str()).unwrap_or("");
        if current.is_empty() || current.starts_with('<') {
            t[*tier] = value(*m);
        }
    }
}

/// From `agy models` (`id<TAB>name` lines): a cheap, a middle and a strong model.
pub fn pick_agy_models(listing: &str) -> Option<[String; 3]> {
    let ids: Vec<&str> = listing.lines().filter_map(|l| l.split('\t').next()).map(str::trim).filter(|i| i.starts_with("gemini-")).collect();
    let find = |needle: &[&str]| ids.iter().find(|i| needle.iter().all(|n| i.contains(n))).map(|s| s.to_string());
    let fast = find(&["flash", "low"]).or_else(|| find(&["flash"]))?;
    let mid = find(&["flash", "medium"]).or_else(|| find(&["flash"]))?;
    let top = find(&["argon", "medium"]).or_else(|| find(&["pro"])).unwrap_or_else(|| mid.clone());
    Some([fast, mid, top])
}

/// From the Gemini API model list: the newest-looking flash-lite/flash/pro that can `generateContent`.
pub fn pick_gemini_models(body: &str) -> Option<[String; 3]> {
    let v: Value = serde_json::from_str(body).ok()?;
    let mut names: Vec<String> = v["models"]
        .as_array()?
        .iter()
        .filter(|m| m["supportedGenerationMethods"].as_array().is_some_and(|a| a.iter().any(|x| x == "generateContent")))
        .filter_map(|m| m["name"].as_str().map(|n| n.trim_start_matches("models/").to_string()))
        .filter(|n| n.starts_with("gemini-") && !n.contains("image") && !n.contains("embedding") && !n.contains("tts") && !n.contains("live") && !n.contains("preview"))
        .collect();
    names.sort();
    names.reverse(); // lexicographic: higher version numbers first
    let first = |needle: &str, not: &str| names.iter().find(|n| n.contains(needle) && (not.is_empty() || !n.contains(not))).cloned();
    let flash = first("flash", "lite")?;
    let fast = first("flash-lite", "").unwrap_or_else(|| flash.clone());
    let top = first("pro", "").unwrap_or_else(|| flash.clone());
    Some([fast, flash, top])
}

/// From the Anthropic model list: the newest haiku, sonnet and opus.
pub fn pick_claude_models(body: &str) -> Option<[String; 3]> {
    let v: Value = serde_json::from_str(body).ok()?;
    let mut ids: Vec<(String, String)> = v["data"].as_array()?.iter().filter_map(|m| Some((m["created_at"].as_str().unwrap_or("").to_string(), m["id"].as_str()?.to_string()))).collect();
    ids.sort();
    ids.reverse();
    let newest = |needle: &str| ids.iter().find(|(_, id)| id.contains(needle)).map(|(_, id)| id.clone());
    let sonnet = newest("sonnet")?;
    Some([newest("haiku").unwrap_or_else(|| sonnet.clone()), sonnet.clone(), newest("opus").unwrap_or(sonnet)])
}

// ── the flow ────────────────────────────────────────────────────────────────

pub struct Opts {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub workspace: Option<String>,
    /// Take the defaults without asking (never credentials).
    pub yes: bool,
    /// Report only.
    pub check: bool,
}

/// What `run` ended up doing, for the summary and for tests.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub added: Vec<String>,
    pub ratified: bool,
}

fn report_detection(ui: &mut dyn Ui, d: &Detected) {
    ui.say("What I found on this machine:");
    match &d.claude {
        Some(c) if c.logged_in => ui.say(&format!("  ✓ Claude Code {} — signed in ({} plan)", c.version, if c.plan.is_empty() { "?" } else { &c.plan })),
        Some(c) => ui.say(&format!("  • Claude Code {} — installed, not signed in", c.version)),
        None => ui.say("  • Claude Code — not installed (https://claude.com/claude-code)"),
    }
    match &d.agy_version {
        Some(v) => ui.say(&format!("  ✓ agy (Gemini subscription CLI) {} — installed", v)),
        None => ui.say("  • agy (Antigravity CLI) — not installed"),
    }
    match &d.codex {
        Some(c) if c.signed_in => ui.say(&format!("  ✓ Codex CLI {} — signed in", c.version)),
        Some(c) => ui.say(&format!("  • Codex CLI {} — installed, not signed in (`codex login`)", c.version)),
        None => {}
    }
    if let Some(v) = &d.copilot_version {
        ui.say(&format!("  ✓ GitHub Copilot CLI {} — installed", v));
    }
    ui.say(&format!("  {} gcloud — {}", if d.gcloud { "✓" } else { "•" }, if d.gcloud { "installed" } else { "not installed" }));
}

fn open_first(ui: &mut dyn Ui, what: &str, url: &str) {
    ui.say(&format!("  Opening {} — {}", what, url));
    ui.open_url(url);
}

/// Claude through the operator's own `claude` CLI: nothing to paste.
fn claude_harness(ui: &mut dyn Ui, env: &dyn Env, d: &Detected, doc: &mut DocumentMut) -> Option<&'static str> {
    if has_endpoint(doc, "claude-sub") {
        return None;
    }
    let c = d.claude.as_ref()?;
    if !c.logged_in {
        if !ui.yes_no("Claude Code is installed but not signed in. Sign in now (opens your browser)?", true) || !env.run_tty("claude", &["auth", "login"]) {
            ui.say("  Skipped Claude. Run `claude auth login`, then `delphi setup` again.");
            return None;
        }
    } else if !ui.yes_no("Use your Claude subscription through the `claude` CLI? (no key needed)", true) {
        return None;
    }
    add_endpoint(doc, "claude", "claude-sub", &[("path", "harness"), ("harness", "claude"), ("credential", "harness-login"), ("cost_model", "subscription")], &[("fast", "haiku"), ("workhorse", "sonnet"), ("frontier", "opus")]);
    Some("claude-sub")
}

/// From `codex debug models` (JSON): the models the CLI lists for the signed-in account, best-ranked first.
/// Hidden ones (internal reviewers and the like) are left out.
pub fn pick_codex_models(json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return vec![] };
    let mut listed: Vec<(i64, String)> = v["models"].as_array().into_iter().flatten().filter(|m| m["visibility"] == "list").filter_map(|m| Some((m["priority"].as_i64().unwrap_or(i64::MAX), m["slug"].as_str()?.to_string()))).collect();
    listed.sort();
    listed.into_iter().map(|(_, s)| s).collect()
}

/// ChatGPT subscription through `codex`: read-only roles.
fn codex_harness(ui: &mut dyn Ui, env: &dyn Env, d: &Detected, doc: &mut DocumentMut) -> Option<&'static str> {
    if has_endpoint(doc, "codex-sub") || !d.codex.as_ref().is_some_and(|c| c.signed_in) {
        return None;
    }
    if !ui.yes_no("Use your ChatGPT subscription through `codex`? (read-only roles like reviewers and Council seats; its output is treated as untrusted)", true) {
        return None;
    }
    // Every tier gets the CLI's first listed model: which one is cheap or strong is not stated, so it is left for the operator to edit.
    let models = env.run("codex", &["debug", "models"]).map(|(_, o)| pick_codex_models(&o)).unwrap_or_default();
    let Some(m) = models.first() else {
        ui.say("  `codex` listed no models for this account; skipped. Run `codex debug models` to see why.");
        return None;
    };
    add_endpoint(doc, "openai", "codex-sub", &[("path", "harness"), ("harness", "codex"), ("credential", "harness-login"), ("cost_model", "subscription")], &[("fast", m), ("workhorse", m), ("frontier", m)]);
    Some("codex-sub")
}

/// Gemini subscription through `agy`: reviewers, Council seats, and Builders once verified with `delphi endpoints test --probe`.
fn agy_harness(ui: &mut dyn Ui, env: &dyn Env, d: &Detected, doc: &mut DocumentMut) -> Option<&'static str> {
    if has_endpoint(doc, "agy-sub") || d.agy_version.is_none() {
        return None;
    }
    if !ui.yes_no("Use your Gemini subscription through `agy`? (reviewers, Council seats, and Builders once verified with `delphi endpoints test --probe`)", true) {
        return None;
    }
    let listing = env.run("agy", &["models"]).map(|(_, o)| o).unwrap_or_default();
    let [fast, mid, top] = pick_agy_models(&listing).unwrap_or_else(|| ["gemini-3.8-flash-low".into(), "gemini-3.8-flash-medium".into(), "gemini-3.8-flash-high".into()]);
    add_endpoint(doc, "gemini", "agy-sub", &[("path", "harness"), ("harness", "agy"), ("credential", "harness-login"), ("cost_model", "subscription")], &[("fast", &fast), ("workhorse", &mid), ("frontier", &top)]);
    Some("agy-sub")
}

/// GitHub Copilot subscription through `copilot`: Builders and reviewers; file edits and commands judged via ACP approval bridge.
fn copilot_harness(ui: &mut dyn Ui, _env: &dyn Env, d: &Detected, doc: &mut DocumentMut) -> Option<&'static str> {
    if has_endpoint(doc, "copilot-sub") || d.copilot_version.is_none() {
        return None;
    }
    if !ui.yes_no("Use your GitHub Copilot subscription through `copilot`? (Builders and reviewers; Delphi's ACP approval bridge judges file edits and commands)", true) {
        return None;
    }
    add_endpoint(doc, "claude", "copilot-sub", &[("path", "harness"), ("harness", "copilot"), ("credential", "harness-login"), ("cost_model", "subscription")], &[("fast", "claude-haiku-4.5"), ("workhorse", "claude-sonnet-5.5"), ("frontier", "claude-opus-5.5")]);
    Some("copilot-sub")
}

/// A local Ollama server: slow, but it costs nothing and never runs out, so it is declared last and the pool falls to it
/// when the paid families are exhausted. Nothing leaves the machine.
fn ollama_local(ui: &mut dyn Ui, env: &dyn Env, doc: &mut DocumentMut) -> Option<&'static str> {
    if has_endpoint(doc, "ollama") {
        return None;
    }
    let (status, body) = env.http_get("http://localhost:11434/api/tags", &[]).ok()?;
    if status != 200 {
        return None;
    }
    let models = pick_ollama_models(&body);
    let first = models.first()?;
    if !ui.yes_no("Ollama is running here. Use it as a slow local fallback for when your other models run out? (free; nothing leaves this machine)", true) {
        return None;
    }
    let model = ui.line(&format!("  Ollama model ({} installed)", models.join(", ")), first);
    let model = model.trim();
    if model.is_empty() {
        return None;
    }
    add_endpoint(doc, "local", "ollama", &[("path", "gateway-compat"), ("base_url", "http://localhost:11434"), ("credential", "none"), ("cost_model", "local")], &[("fast", model), ("workhorse", model), ("frontier", model)]);
    set_catalog(doc, model, 8192, 0.0, 0.0);
    Some("ollama")
}

/// Installed Ollama models that can chat (embedding models are left out), in the server's order.
fn pick_ollama_models(tags: &str) -> Vec<String> {
    let v: Value = serde_json::from_str(tags).unwrap_or(Value::Null);
    v["models"].as_array().into_iter().flatten().filter_map(|m| m["name"].as_str()).filter(|n| !n.contains("embed")).map(String::from).collect()
}

/// A Gemini API key from AI Studio: one page, one paste.
fn gemini_key(ui: &mut dyn Ui, env: &dyn Env, doc: &mut DocumentMut) -> Result<Option<&'static str>> {
    if has_endpoint(doc, "gemini-key") {
        return Ok(None);
    }
    if !ui.yes_no("Add a Gemini API key from Google AI Studio? (free tier available; one page, one paste)", false) {
        return Ok(None);
    }
    open_first(ui, "AI Studio", "https://aistudio.google.com/apikey");
    ui.say("  Create a key, copy it, come back here and paste it. It will not be shown.");
    let key = ui.secret("Gemini API key");
    if key.trim().len() < 20 {
        ui.say("  That does not look like a key; skipped.");
        return Ok(None);
    }
    let key = key.trim();
    let (status, body) = env.http_get("https://generativelanguage.googleapis.com/v1beta/models?pageSize=200", &[("x-goog-api-key", key)]).map_err(|e| anyhow!("could not reach Google: {}", e))?;
    if status != 200 {
        ui.say(&format!("  Google rejected the key (HTTP {}); nothing was saved.", status));
        return Ok(None);
    }
    env.secret_set("gemini-api-key", key).map_err(|e| anyhow!("could not store the key in your keychain: {}", e))?;
    let [fast, mid, top] = pick_gemini_models(&body).unwrap_or_else(|| ["gemini-flash-lite-latest".into(), "gemini-flash-latest".into(), "gemini-pro-latest".into()]);
    ui.say(&format!("  Key works and is stored in your keychain. Models: {} / {} / {}", fast, mid, top));
    add_endpoint(doc, "gemini", "gemini-key", &[("path", "api"), ("credential", "keychain:gemini-api-key")], &[("fast", &fast), ("workhorse", &mid), ("frontier", &top)]);
    prices(ui, doc, &[&fast, &mid, &top], "https://ai.google.dev/pricing");
    Ok(Some("gemini-key"))
}

/// An Anthropic API key.
fn anthropic_key(ui: &mut dyn Ui, env: &dyn Env, doc: &mut DocumentMut) -> Result<Option<&'static str>> {
    if has_endpoint(doc, "anthropic-key") {
        return Ok(None);
    }
    if !ui.yes_no("Add an Anthropic API key? (metered billing; not your Claude subscription)", false) {
        return Ok(None);
    }
    open_first(ui, "the Anthropic console", "https://console.anthropic.com/settings/keys");
    let key = ui.secret("Anthropic API key");
    let key = key.trim();
    if !key.starts_with("sk-ant-") {
        ui.say("  That does not look like an Anthropic key (they start with sk-ant-); skipped.");
        return Ok(None);
    }
    let (status, body) = env.http_get("https://api.anthropic.com/v1/models?limit=100", &[("x-api-key", key), ("anthropic-version", "2023-06-01")]).map_err(|e| anyhow!("could not reach Anthropic: {}", e))?;
    if status != 200 {
        ui.say(&format!("  Anthropic rejected the key (HTTP {}); nothing was saved.", status));
        return Ok(None);
    }
    env.secret_set("anthropic-api-key", key).map_err(|e| anyhow!("could not store the key in your keychain: {}", e))?;
    let [fast, mid, top] = pick_claude_models(&body).ok_or_else(|| anyhow!("the model list had no Claude models"))?;
    ui.say(&format!("  Key works and is stored in your keychain. Models: {} / {} / {}", fast, mid, top));
    add_endpoint(doc, "claude", "anthropic-key", &[("path", "api"), ("credential", "keychain:anthropic-api-key")], &[("fast", &fast), ("workhorse", &mid), ("frontier", &top)]);
    prices(ui, doc, &[&fast, &mid, &top], "https://www.anthropic.com/pricing");
    Ok(Some("anthropic-key"))
}

/// Delphi reserves budget before each metered call, so each model needs prices. A blank price is not quietly
/// turned into a near-zero one (that would switch the budget off for the model): the operator is asked whether it
/// is really free, and if not the model is left without a price, which Delphi refuses to call until one is added.
fn prices(ui: &mut dyn Ui, doc: &mut DocumentMut, models: &[&str], url: &str) {
    ui.say(&format!("  Delphi needs a price per model to reserve budget (see {}). Enter dollars per million tokens.", url));
    let mut seen: Vec<&str> = Vec::new();
    for m in models {
        if seen.contains(m) {
            continue;
        }
        seen.push(m);
        let parse = |s: String| s.trim().trim_start_matches('$').parse::<f64>().ok().filter(|v| *v > 0.0);
        let input = parse(ui.line(&format!("  {} input $/Mtok", m), ""));
        let output = parse(ui.line(&format!("  {} output $/Mtok", m), ""));
        if let (Some(i), Some(o)) = (input, output) {
            set_catalog(doc, m, 64_000, i, o);
        } else if ui.yes_no(&format!("  No price entered for {}. Treat it as free (the budget will then not limit it; right for a free tier)?", m), false) {
            set_catalog(doc, m, 64_000, input.unwrap_or(0.000_001), output.unwrap_or(0.000_001));
        } else {
            ui.say(&format!("  {} is left without a price: Delphi will not call it until you add [models.catalog.\"{}\"] prices to config.toml.", m, m));
        }
    }
}

/// Google OAuth: the advanced path. Two Google forms cannot be automated, so this walks through them.
fn google_oauth(ui: &mut dyn Ui, env: &dyn Env, d: &Detected, doc: &mut DocumentMut) -> Result<Option<&'static str>> {
    if doc.get("auth").and_then(|a| a.get("google")).is_some() {
        return Ok(None);
    }
    if !ui.yes_no("Set up Google sign-in for the Gemini API (advanced; the key above is simpler)?", false) {
        return Ok(None);
    }
    let mut project = ui.line("Google Cloud project ID (leave blank to create one)", "");
    if project.trim().is_empty() {
        let suggested = format!("delphi-{}", std::process::id() % 100_000);
        if d.gcloud && ui.yes_no(&format!("Create project `{}` with gcloud?", suggested), true) && env.run("gcloud", &["projects", "create", &suggested]).is_some_and(|(c, _)| c == 0) {
            project = suggested;
        } else {
            open_first(ui, "project creation", "https://console.cloud.google.com/projectcreate");
            project = ui.line("Project ID you created", "");
        }
    }
    let project = project.trim().to_string();
    if project.is_empty() {
        return Ok(None);
    }
    let enable = format!("https://console.cloud.google.com/apis/library/generativelanguage.googleapis.com?project={}", project);
    if d.gcloud && env.run("gcloud", &["services", "enable", "generativelanguage.googleapis.com", "--project", &project]).is_some_and(|(c, _)| c == 0) {
        ui.say("  Enabled the Generative Language API with gcloud.");
    } else {
        open_first(ui, "the API page; click Enable", &enable);
        ui.yes_no("Enabled it?", true);
    }
    open_first(ui, "the consent screen; choose External and add yourself as a test user", &format!("https://console.cloud.google.com/auth/audience?project={}", project));
    ui.yes_no("Done with the consent screen?", true);
    open_first(ui, "client creation; choose Desktop app", &format!("https://console.cloud.google.com/auth/clients/create?project={}", project));
    let client_id = ui.line("Client ID (ends in .apps.googleusercontent.com)", "");
    if !client_id.trim().ends_with(".apps.googleusercontent.com") {
        ui.say("  That is not a client ID; skipped.");
        return Ok(None);
    }
    let secret = ui.secret("Client secret");
    if secret.trim().is_empty() {
        ui.say("  No secret; skipped.");
        return Ok(None);
    }
    env.secret_set("google-client-secret", secret.trim()).map_err(|e| anyhow!("could not store the secret in your keychain: {}", e))?;
    let mut auth = Table::new();
    auth["flow"] = value("loopback_pkce");
    auth["issuer"] = value("https://accounts.google.com");
    auth["client_id"] = value(client_id.trim());
    auth["client_secret"] = value("keychain:google-client-secret");
    auth["scopes"] = value(str_array(&["https://www.googleapis.com/auth/cloud-platform"]));
    let mut params = toml_edit::InlineTable::new();
    params.insert("access_type", "offline".into());
    params.insert("prompt", "consent".into());
    auth["authorization_params"] = value(params);
    if !doc.contains_key("auth") {
        doc["auth"] = Item::Table(Table::new());
        doc["auth"].as_table_mut().expect("auth").set_implicit(true);
    }
    doc["auth"]["google"] = Item::Table(auth);
    add_endpoint(doc, "gemini", "gemini-oauth", &[("path", "api"), ("credential", "oauth2:google"), ("project", &project)], &[("fast", "gemini-flash-lite-latest"), ("workhorse", "gemini-flash-latest"), ("frontier", "gemini-pro-latest")]);
    ui.say("  After ratification, `delphi auth login google` signs you in (a browser opens).");
    Ok(Some("gemini-oauth"))
}

/// Put the access change in front of the operator and ratify it on their say-so.
pub(crate) fn ratify_config(ui: &mut dyn Ui, home: &Path, ws_dir: &Path, yes: bool) -> bool {
    use delphi_engine::ratify;
    let pending: Vec<_> = ratify::list(home, ws_dir).into_iter().filter(|p| p.logical.starts_with("config.toml")).collect();
    if pending.is_empty() {
        return true;
    }
    let mut all = true;
    for p in pending {
        ui.say(&format!("\nModel access changed — this is what you are about to trust ({}):\n{}", p.summary, p.diff));
        // `--yes` never ratifies: trust in what a model may reach is the one answer that is not defaulted.
        if yes {
            ui.say("  Not ratified (--yes does not ratify). Review it and run `/ratify` in the console.");
            all = false;
        } else if ui.yes_no("Ratify this (Delphi will only talk to the endpoints listed here)?", true) {
            match p.live_blake3.as_deref().map(|h| ratify::apply(home, ws_dir, &p.id, h)) {
                Some(Ok(_)) => ui.say("  Ratified."),
                other => {
                    ui.say(&format!("  Could not ratify: {:?}", other.map(|r| r.map(|_| ()))));
                    all = false;
                }
            }
        } else {
            ui.say("  Not ratified; it stays a pending proposal (`/ratify` in the console).");
            all = false;
        }
    }
    all
}

pub fn run(ui: &mut dyn Ui, env: &dyn Env, o: &Opts) -> Result<Outcome> {
    ui.say("Delphi setup\n");
    let detected = detect(env);
    report_detection(ui, &detected);
    let root = home::git_root(&o.cwd);
    let kind = root.as_deref().and_then(project_kind);
    if let (Some(r), Some(k)) = (&root, &kind) {
        ui.say(&format!("  ✓ project: {} ({})", r.display(), k.name));
    } else if root.is_none() {
        ui.say("  • no Git repository detected; Delphi can still run here using the default workspace.");
    }

    // Use the matching project workspace, or the persistent default workspace when none matches.
    let ws_name = match home::resolve_workspace(&o.home, &o.cwd, o.workspace.clone()) {
        home::Resolve::Found(n) => n,
        home::Resolve::NoneRegistered(_) | home::Resolve::NotInGitRepo => {
            let default = crate::start::DEFAULT_WORKSPACE;
            let dir = o.home.join("workspaces").join(default);
            if dir.join("config.toml").exists() {
                default.to_string()
            } else if o.check {
                ui.say("\nNo default workspace yet. Run `delphi` to create one, then resume setup.");
                return Ok(Outcome::default());
            } else {
                let ws = home::init(&InitOptions { home: o.home.clone(), workspace: default.into(), projects: vec![], theme: "greek".into() })?;
                ui.say(&format!("  Created default workspace `{}`.", ws.name));
                ws.name
            }
        }
        home::Resolve::Ambiguous(v) => return Err(anyhow!("several workspaces register this project ({}); pass --workspace", v.join(", "))),
        home::Resolve::NoSuchWorkspace(n) => return Err(anyhow!("no workspace named `{}`", n)),
    };
    let ws_dir = o.home.join("workspaces").join(&ws_name);
    if o.check {
        return Ok(Outcome::default());
    }

    let cfg_path = ws_dir.join("config.toml");
    let mut doc: DocumentMut = std::fs::read_to_string(&cfg_path)?.parse().map_err(|e| anyhow!("config.toml is not valid TOML: {}", e))?;
    let mut added: Vec<String> = Vec::new();

    ui.say("\nHow do you want to reach models?");
    if let Some(id) = claude_harness(ui, env, &detected, &mut doc) {
        set_family(&mut doc, &[("fast", "haiku"), ("workhorse", "sonnet"), ("frontier", "opus")], "claude");
        added.push(id.into());
    }
    if let Some(id) = agy_harness(ui, env, &detected, &mut doc) {
        added.push(id.into());
    }
    if let Some(id) = codex_harness(ui, env, &detected, &mut doc) {
        added.push(id.into());
    }
    if let Some(id) = copilot_harness(ui, env, &detected, &mut doc) {
        added.push(id.into());
    }
    // A step that fails late (after its secret is already in the keychain) must not throw away the endpoints the
    // earlier steps set up: the config is written first and the error is returned after.
    let mut late: Option<anyhow::Error> = None;
    if !o.yes {
        for i in 0..3 {
            let step = match i {
                0 => gemini_key(ui, env, &mut doc),
                1 => anthropic_key(ui, env, &mut doc),
                _ => google_oauth(ui, env, &detected, &mut doc),
            };
            match step {
                Ok(Some(id)) => added.push(id.into()),
                Ok(None) => {}
                Err(e) => {
                    late = Some(e);
                    break;
                }
            }
        }
    }
    if let Some(e) = late {
        std::fs::write(&cfg_path, doc.to_string())?;
        ui.say(&format!("\nStopped: {}\nWhat was set up before that is saved ({}), but not yet ratified: run `delphi setup` again to continue.", e, if added.is_empty() { "nothing".to_string() } else { added.join(", ") }));
        return Err(e);
    }
    // After every paid endpoint, so the pool reaches it last.
    if let Some(id) = ollama_local(ui, env, &mut doc) {
        added.push(id.into());
    }
    // Both families available: have Gemini review what Claude builds (no author-family self-review, [H-008]).
    let has_claude = has_endpoint(&doc, "claude-sub") || has_endpoint(&doc, "anthropic-key");
    let has_gemini = has_endpoint(&doc, "agy-sub") || has_endpoint(&doc, "gemini-key") || has_endpoint(&doc, "gemini-oauth");
    if has_claude && has_gemini && doc.get("access").and_then(|a| a.get("role_family")).is_none() && (o.yes || ui.yes_no("Have Gemini do the Red-Team audits of what Claude builds (a second opinion from a different model family)?", true)) {
        doc["access"]["role_family"] = value({
            let mut t = toml_edit::InlineTable::new();
            t.insert("red-team", "gemini".into());
            t
        });
    }
    // `[exec]` is one of the ratified access blocks (`lock::ACCESS_KEYS`): these defaults are shown in the diff
    // below and are inactive until the operator ratifies them.
    exec_defaults(&mut doc, &kind);
    std::fs::write(&cfg_path, doc.to_string())?;
    if added.is_empty() && !has_claude && !has_gemini {
        ui.say("\nNothing to connect yet. Sign in to Claude Code (`claude auth login`), install agy, or run `delphi setup` again and choose a key.");
    }

    let ratified = ratify_config(ui, &o.home, &ws_dir, o.yes);
    ui.say(&format!("\nDone. {}", if added.is_empty() { "No new endpoints were added.".to_string() } else { format!("Added: {}.", added.join(", ")) }));
    ui.say("Next:\n  delphi endpoints test --probe     one tiny call per endpoint, to prove it works\n  delphi                            the console; try `/build implement @specs/… in @project/<name>`");
    Ok(Outcome { added, ratified })
}

// ── the real machine and terminal ───────────────────────────────────────────

pub struct RealEnv;

impl Env for RealEnv {
    fn run(&self, program: &str, args: &[&str]) -> Option<(i32, String)> {
        let out = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
        Some((out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned()))
    }

    fn run_tty(&self, program: &str, args: &[&str]) -> bool {
        std::process::Command::new(program).args(args).status().map(|s| s.success()).unwrap_or(false)
    }

    fn secret_set(&self, name: &str, secret: &str) -> Result<(), String> {
        use delphi_adapter::oauth2::SecretStore;
        delphi_adapter::oauth2::SystemSecretStore::new(std::sync::Arc::new(delphi_adapter::credential::SystemRunner)).set(name, secret)
    }

    fn http_get(&self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, String), String> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        let client = delphi_adapter::http::build_client();
        rt.block_on(async {
            let mut req = client.get(url).timeout(std::time::Duration::from_secs(20));
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            let resp = req.send().await.map_err(|e| e.without_url().to_string())?;
            let status = resp.status().as_u16();
            let body = resp.text().await.map_err(|e| e.without_url().to_string())?;
            Ok((status, body))
        })
    }
}

/// The terminal. With `auto` (`--yes`) every question takes its default and nothing is printed for it.
pub struct TerminalUi {
    pub auto: bool,
}

impl TerminalUi {
    fn read_line() -> String {
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
        s.trim_end_matches(['\n', '\r']).to_string()
    }
}

impl Ui for TerminalUi {
    fn say(&mut self, text: &str) {
        println!("{}", text);
    }

    fn yes_no(&mut self, question: &str, default: bool) -> bool {
        use std::io::Write;
        if self.auto {
            return default;
        }
        print!("{} [{}] ", question, if default { "Y/n" } else { "y/N" });
        let _ = std::io::stdout().flush();
        match Self::read_line().trim().to_lowercase().as_str() {
            "" => default,
            a => a.starts_with('y'),
        }
    }

    fn line(&mut self, question: &str, default: &str) -> String {
        use std::io::Write;
        if self.auto {
            return default.to_string();
        }
        if default.is_empty() { print!("{}: ", question) } else { print!("{} [{}]: ", question, default) };
        let _ = std::io::stdout().flush();
        let a = Self::read_line();
        if a.trim().is_empty() { default.to_string() } else { a.trim().to_string() }
    }

    fn secret(&mut self, question: &str) -> String {
        use std::io::Write;
        print!("{} (hidden): ", question);
        let _ = std::io::stdout().flush();
        // SAFETY: plain termios calls on standard input; the original settings are restored before returning.
        let saved = unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) == 0 {
                let mut quiet = t;
                quiet.c_lflag &= !(libc::ECHO as libc::tcflag_t);
                libc::tcsetattr(0, libc::TCSANOW, &quiet);
                Some(t)
            } else {
                None
            }
        };
        let a = Self::read_line();
        if let Some(t) = saved {
            // SAFETY: restoring what tcgetattr returned.
            unsafe {
                libc::tcsetattr(0, libc::TCSANOW, &t);
            }
        }
        println!();
        a
    }

    fn open_url(&mut self, url: &str) {
        let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = std::process::Command::new(program).arg(url).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, VecDeque};

    #[derive(Default)]
    struct FakeEnv {
        programs: BTreeMap<String, String>,
        logged_in: bool,
        stored: RefCell<Vec<(String, String)>>,
        http: BTreeMap<String, (u16, String)>,
        /// A keychain name whose `secret_set` fails.
        failing_secret: &'static str,
        tty_runs: RefCell<Vec<String>>,
        /// The exit code of `codex login status`.
        codex_login_code: i32,
    }

    impl Env for FakeEnv {
        fn run(&self, program: &str, args: &[&str]) -> Option<(i32, String)> {
            let out = self.programs.get(&format!("{} {}", program, args.first().copied().unwrap_or("")))?;
            if program == "claude" && args.first() == Some(&"auth") {
                return Some((0, format!("{{\"loggedIn\":{},\"subscriptionType\":\"pro\",\"email\":\"private@example.com\"}}", self.logged_in)));
            }
            if program == "codex" && args.first() == Some(&"login") {
                return Some((self.codex_login_code, String::new()));
            }
            Some((0, out.clone()))
        }
        fn run_tty(&self, program: &str, args: &[&str]) -> bool {
            self.tty_runs.borrow_mut().push(format!("{} {}", program, args.join(" ")));
            true
        }
        fn secret_set(&self, name: &str, secret: &str) -> Result<(), String> {
            if name == self.failing_secret {
                return Err("the keychain is locked".into());
            }
            self.stored.borrow_mut().push((name.into(), secret.into()));
            Ok(())
        }
        fn http_get(&self, url: &str, _h: &[(&str, &str)]) -> Result<(u16, String), String> {
            self.http.iter().find(|(k, _)| url.starts_with(k.as_str())).map(|(_, v)| v.clone()).ok_or_else(|| "offline".into())
        }
    }

    #[derive(Default)]
    struct Script {
        yes: VecDeque<bool>,
        lines: VecDeque<String>,
        secrets: VecDeque<String>,
        said: Vec<String>,
        opened: Vec<String>,
        asked: Vec<String>,
    }

    impl Ui for Script {
        fn say(&mut self, t: &str) {
            self.said.push(t.to_string());
        }
        fn yes_no(&mut self, q: &str, default: bool) -> bool {
            self.asked.push(q.to_string());
            self.yes.pop_front().unwrap_or(default)
        }
        fn line(&mut self, q: &str, default: &str) -> String {
            self.asked.push(q.to_string());
            self.lines.pop_front().unwrap_or_else(|| default.to_string())
        }
        fn secret(&mut self, q: &str) -> String {
            self.asked.push(q.to_string());
            self.secrets.pop_front().unwrap_or_default()
        }
        fn open_url(&mut self, u: &str) {
            self.opened.push(u.to_string());
        }
    }

    fn machine(claude: bool, logged_in: bool, agy: bool) -> FakeEnv {
        let mut e = FakeEnv { logged_in, ..Default::default() };
        if claude {
            e.programs.insert("claude --version".into(), "2.1.310 (Claude Code)\n".into());
            e.programs.insert("claude auth".into(), String::new());
        }
        if agy {
            e.programs.insert("agy --version".into(), "1.4.6\n".into());
            e.programs.insert("agy models".into(), "gemini-4-argon-medium\tArgon\ngemini-3.8-flash-high\tFlash High\ngemini-3.8-flash-medium\tFlash\ngemini-3.8-flash-low\tFlash Low\nclaude-opus-5-5-low\tOpus\n".into());
        }
        e
    }

    fn project(dir: &Path, cargo: bool) -> PathBuf {
        let p = dir.join("proj");
        std::fs::create_dir_all(&p).unwrap();
        std::process::Command::new("git").args(["init", "-q"]).current_dir(&p).status().unwrap();
        if cargo {
            std::fs::write(p.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        }
        std::fs::canonicalize(&p).unwrap()
    }

    fn opts(t: &Path, proj: &Path) -> Opts {
        Opts { home: t.join("home"), cwd: proj.to_path_buf(), workspace: None, yes: false, check: false }
    }

    fn config(t: &Path) -> String {
        let ws = std::fs::read_dir(t.join("home/workspaces")).unwrap().next().unwrap().unwrap().path();
        std::fs::read_to_string(ws.join("config.toml")).unwrap()
    }

    #[test]
    fn detection_reads_the_clis_own_status_and_the_project_kind_picks_the_gate_commands() {
        let d = detect(&machine(true, true, true));
        assert_eq!(d.claude, Some(ClaudeState { version: "2.1.310 (Claude Code)".into(), logged_in: true, plan: "pro".into() }));
        assert_eq!(d.agy_version.as_deref(), Some("1.4.6"));
        assert!(!d.gcloud);
        assert_eq!(detect(&machine(false, false, false)), Detected::default());
        let t = tempfile::tempdir().unwrap();
        let k = project_kind(&project(t.path(), true)).unwrap();
        assert_eq!((k.name, k.test.join(" ")), ("Rust", "cargo test".to_string()));
        assert_eq!(project_kind(&project(tempfile::tempdir().unwrap().path(), false)), None);
    }

    fn codex_machine(logged_in: bool) -> FakeEnv {
        let mut e = FakeEnv::default();
        e.programs.insert("codex --version".into(), "codex-cli 0.165.0\n".into());
        e.programs.insert("codex login".into(), String::new());
        e.programs.insert("codex debug".into(), r#"{"models":[{"slug":"hidden","visibility":"hide","priority":1},{"slug":"gpt-b","visibility":"list","priority":9},{"slug":"gpt-a","visibility":"list","priority":5}]}"#.into());
        e.codex_login_code = if logged_in { 0 } else { 1 };
        e
    }

    #[test]
    fn a_signed_in_codex_is_detected_and_offered_as_a_read_only_seat_on_its_first_listed_model() {
        let d = detect(&codex_machine(true));
        assert_eq!(d.codex, Some(CodexState { version: "0.165.0".into(), signed_in: true }));
        assert_eq!(detect(&codex_machine(false)).codex.map(|c| c.signed_in), Some(false));
        assert_eq!(pick_codex_models(r#"{"models":[{"slug":"x","visibility":"hide"}]}"#), Vec::<String>::new());
        assert!(pick_codex_models("not json").is_empty());
        let mut doc = DocumentMut::new();
        let mut ui = Script::default();
        assert_eq!(codex_harness(&mut ui, &codex_machine(true), &d, &mut doc), Some("codex-sub"));
        let text = doc.to_string();
        assert!(text.contains("harness = \"codex\"") && text.contains("family = \"openai\"") && text.contains("credential = \"harness-login\""), "{}", text);
        assert!(text.matches("gpt-a").count() == 3 && !text.contains("gpt-b") && !text.contains("hidden"), "{}", text);
        // Signed out: nothing is offered and nothing is asked.
        let mut ui2 = Script::default();
        assert_eq!(codex_harness(&mut ui2, &codex_machine(false), &detect(&codex_machine(false)), &mut DocumentMut::new()), None);
        assert!(ui2.asked.is_empty());
    }

    #[test]
    fn copilot_is_detected_and_offered_with_acp_bridge_for_builders() {
        let mut e = FakeEnv::default();
        e.programs.insert("copilot --version".into(), "GitHub Copilot CLI 1.0.105.\n".into());
        let d = detect(&e);
        assert_eq!(d.copilot_version.as_deref(), Some("1.0.105"));
        let mut doc = DocumentMut::new();
        let mut ui = Script::default();
        assert_eq!(copilot_harness(&mut ui, &e, &d, &mut doc), Some("copilot-sub"));
        let text = doc.to_string();
        assert!(text.contains("harness = \"copilot\"") && text.contains("family = \"claude\"") && text.contains("credential = \"harness-login\""), "{}", text);
        assert!(text.contains("claude-sonnet-5.5"), "{}", text);
    }

    #[test]
    fn the_model_pickers_choose_sensible_tiers_from_real_shaped_listings() {
        let agy = "gemini-4-argon-low\tA\ngemini-4-argon-medium\tB\ngemini-3.8-flash-high\tC\ngemini-3.8-flash-medium\tD\ngemini-3.8-flash-low\tE\nclaude-opus-5-5-low\tF\n";
        assert_eq!(pick_agy_models(agy).unwrap(), ["gemini-3.8-flash-low", "gemini-3.8-flash-medium", "gemini-4-argon-medium"]);
        let g = r#"{"models":[{"name":"models/gemini-2.5-flash","supportedGenerationMethods":["generateContent"]},{"name":"models/gemini-2.5-flash-lite","supportedGenerationMethods":["generateContent"]},{"name":"models/gemini-2.5-pro","supportedGenerationMethods":["generateContent"]},{"name":"models/gemini-embedding-001","supportedGenerationMethods":["embedContent"]},{"name":"models/gemini-2.5-flash-image","supportedGenerationMethods":["generateContent"]}]}"#;
        assert_eq!(pick_gemini_models(g).unwrap(), ["gemini-2.5-flash-lite", "gemini-2.5-flash", "gemini-2.5-pro"]);
        let c = r#"{"data":[{"id":"claude-haiku-4-5","created_at":"2025-10-01"},{"id":"claude-sonnet-4-5","created_at":"2025-09-29"},{"id":"claude-sonnet-5-5","created_at":"2026-09-01"},{"id":"claude-opus-5-5","created_at":"2026-09-02"}]}"#;
        assert_eq!(pick_claude_models(c).unwrap(), ["claude-haiku-4-5", "claude-sonnet-5-5", "claude-opus-5-5"]);
    }

    #[test]
    fn a_signed_in_machine_gets_a_working_workspace_with_no_pasting_and_the_change_is_ratified() {
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), true);
        let env = machine(true, true, true);
        let mut ui = Script::default();
        let out = run(&mut ui, &env, &opts(t.path(), &proj)).unwrap();
        assert_eq!(out.added, ["claude-sub", "agy-sub"]);
        assert!(out.ratified);
        let cfg = config(t.path());
        let parsed = delphi_adapter::config::Config::parse(&cfg).expect("the written config is valid");
        assert!(parsed.is_allowed("claude-sub") && parsed.is_allowed("agy-sub"));
        assert_eq!(parsed.family_for_role("red-team"), "gemini", "a second family audits the first");
        assert!(cfg.contains("test_command = [\"cargo\", \"test\"]") && cfg.contains("allow = [\"cargo test*\""), "{}", cfg);
        assert!(cfg.contains("# DELPHI WORKSPACE CONFIGURATION TEMPLATE"), "the operator's comments survive the edit");
        assert!(env.stored.borrow().is_empty(), "subscriptions need no secret");
        // The access change really is ratified: the daemon's effective config sees the endpoints.
        let ws = t.path().join("home/workspaces").read_dir().unwrap().next().unwrap().unwrap().path();
        let (eff, _) = delphi_engine::boot::effective_config(&ws, &cfg).unwrap();
        assert!(eff.contains("claude-sub") && eff.contains("agy-sub"));
        // Nothing was written into the project.
        assert!(!proj.join(".delphi").exists());
    }

    #[test]
    fn a_running_ollama_becomes_the_last_resort_local_endpoint() {
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), true);
        let mut env = machine(true, true, false);
        env.http.insert("http://localhost:11434/api/tags".into(), (200, r#"{"models":[{"name":"nomic-embed-text:latest"},{"name":"qwen3:8b"}]}"#.into()));
        let out = run(&mut Script::default(), &env, &opts(t.path(), &proj)).unwrap();
        assert_eq!(out.added, ["claude-sub", "ollama"]);
        let cfg = config(t.path());
        let parsed = delphi_adapter::config::Config::parse(&cfg).expect("the written config is valid");
        let ids: Vec<_> = parsed.pool_entries().iter().map(|k| (k.endpoint_id.clone(), k.model_id.clone())).collect();
        assert_eq!(ids.last().unwrap(), &("ollama".to_string(), "qwen3:8b".to_string()), "{:?}", ids);
        assert!(parsed.problems().iter().all(|p| !p.contains("ollama")), "a free local model needs no prices: {:?}", parsed.problems());
        // Re-running changes nothing.
        let before = config(t.path());
        assert!(run(&mut Script::default(), &env, &opts(t.path(), &proj)).unwrap().added.is_empty());
        assert_eq!(config(t.path()), before);
        // Not running: not offered.
        let t2 = tempfile::tempdir().unwrap();
        let proj2 = project(t2.path(), true);
        assert_eq!(run(&mut Script::default(), &machine(true, true, false), &opts(t2.path(), &proj2)).unwrap().added, ["claude-sub"]);
    }

    #[test]
    fn running_it_again_changes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), true);
        let env = machine(true, true, true);
        run(&mut Script::default(), &env, &opts(t.path(), &proj)).unwrap();
        let before = config(t.path());
        let out = run(&mut Script::default(), &env, &opts(t.path(), &proj)).unwrap();
        assert!(out.added.is_empty() && out.ratified);
        assert_eq!(config(t.path()), before);
    }

    #[test]
    fn a_signed_out_claude_is_offered_a_login_and_declining_skips_it() {
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), false);
        let env = machine(true, false, false);
        let mut ui = Script::default();
        let out = run(&mut ui, &env, &opts(t.path(), &proj)).unwrap();
        assert_eq!(out.added, ["claude-sub"]);
        assert_eq!(env.tty_runs.borrow().as_slice(), ["claude auth login"]);
        let t2 = tempfile::tempdir().unwrap();
        let proj2 = project(t2.path(), false);
        let mut no = Script { yes: VecDeque::from([false]), ..Default::default() };
        let out = run(&mut no, &env, &opts(t2.path(), &proj2)).unwrap();
        assert!(out.added.is_empty());
        assert!(no.said.iter().any(|s| s.contains("claude auth login")));
    }

    #[test]
    fn a_gemini_key_is_validated_stored_in_the_keychain_and_never_written_to_config_or_the_screen() {
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), false);
        let mut env = machine(false, false, false);
        env.http.insert("https://generativelanguage.googleapis.com/v1beta/models".into(), (200, r#"{"models":[{"name":"models/gemini-2.5-flash","supportedGenerationMethods":["generateContent"]},{"name":"models/gemini-2.5-pro","supportedGenerationMethods":["generateContent"]}]}"#.into()));
        let key = "AIzaSyFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE0";
        let mut ui = Script { yes: VecDeque::from([true, false, false]), secrets: VecDeque::from([key.to_string()]), lines: VecDeque::from(["0.30".into(), "2.50".into(), "1.25".into(), "10.00".into()]), ..Default::default() };
        let out = run(&mut ui, &env, &opts(t.path(), &proj)).unwrap();
        assert_eq!(out.added, ["gemini-key"]);
        assert_eq!(env.stored.borrow().as_slice(), [("gemini-api-key".to_string(), key.to_string())]);
        let cfg = config(t.path());
        assert!(!cfg.contains(key) && cfg.contains("keychain:gemini-api-key"));
        assert!(!ui.said.iter().any(|s| s.contains(key)) && !ui.asked.iter().any(|s| s.contains(key)));
        assert_eq!(ui.opened, ["https://aistudio.google.com/apikey"]);
        let parsed = delphi_adapter::config::Config::parse(&cfg).unwrap();
        let flash = parsed.catalog_entry("gemini-2.5-flash").expect("priced");
        assert!(flash.has_prices() && flash.input_usd_per_mtok == 0.30 && flash.output_usd_per_mtok == 2.50);
        assert!(parsed.problems().is_empty(), "{:?}", parsed.problems());
    }

    const GEMINI_MODELS: &str = r#"{"models":[{"name":"models/gemini-2.5-flash","supportedGenerationMethods":["generateContent"]},{"name":"models/gemini-2.5-pro","supportedGenerationMethods":["generateContent"]}]}"#;

    #[test]
    fn a_blank_price_is_not_quietly_made_free() {
        let key = "AIzaSyFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE0";
        // Declined: the unpriced model is left out of the catalog (so Delphi refuses to call it), the priced one is kept.
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), false);
        let mut env = machine(false, false, false);
        env.http.insert("https://generativelanguage.googleapis.com/v1beta/models".into(), (200, GEMINI_MODELS.into()));
        let mut ui = Script { yes: VecDeque::from([true, false, false, false]), secrets: VecDeque::from([key.to_string()]), lines: VecDeque::from(["".into(), "".into(), "1.25".into(), "10.00".into()]), ..Default::default() };
        run(&mut ui, &env, &opts(t.path(), &proj)).unwrap();
        let parsed = delphi_adapter::config::Config::parse(&config(t.path())).unwrap();
        assert!(parsed.catalog_entry("gemini-2.5-flash").is_none() && parsed.catalog_entry("gemini-2.5-pro").is_some_and(|e| e.has_prices()));
        assert!(ui.said.iter().any(|s| s.contains("left without a price")));
        assert!(parsed.problems().iter().any(|p| p.contains("gemini-2.5-flash")), "the gap is reported, not hidden: {:?}", parsed.problems());
        // Confirmed free: a near-zero placeholder, on purpose.
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), false);
        let mut ui = Script { yes: VecDeque::from([true, true, false, false]), secrets: VecDeque::from([key.to_string()]), lines: VecDeque::from(["".into(), "".into(), "1.25".into(), "10.00".into()]), ..Default::default() };
        run(&mut ui, &env, &opts(t.path(), &proj)).unwrap();
        let parsed = delphi_adapter::config::Config::parse(&config(t.path())).unwrap();
        assert!(parsed.catalog_entry("gemini-2.5-flash").is_some_and(|e| e.has_prices() && e.input_usd_per_mtok < 0.001));
    }

    #[test]
    fn a_step_that_fails_late_keeps_the_endpoints_set_up_before_it() {
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), false);
        let mut env = machine(false, false, false);
        env.failing_secret = "anthropic-api-key";
        env.http.insert("https://generativelanguage.googleapis.com/v1beta/models".into(), (200, GEMINI_MODELS.into()));
        env.http.insert("https://api.anthropic.com/v1/models".into(), (200, r#"{"data":[{"id":"claude-sonnet-x","created_at":"2026-01-01"}]}"#.into()));
        let mut ui = Script {
            yes: VecDeque::from([true, true]),
            secrets: VecDeque::from(["AIzaSyFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE0".to_string(), "sk-ant-fakefakefakefake".into()]),
            lines: VecDeque::from(["0.30".into(), "2.50".into(), "1.25".into(), "10.00".into()]),
            ..Default::default()
        };
        let err = run(&mut ui, &env, &opts(t.path(), &proj)).unwrap_err();
        assert!(err.to_string().contains("keychain"), "{}", err);
        let cfg = config(t.path());
        assert!(cfg.contains("gemini-key") && cfg.contains("gemini-2.5-flash"), "the earlier endpoint survives: {}", cfg);
        assert!(!cfg.contains("anthropic-key"));
        assert!(ui.said.iter().any(|s| s.contains("saved") && s.contains("gemini-key")));
    }

    #[test]
    fn a_rejected_or_malformed_key_saves_nothing() {
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), false);
        let mut env = machine(false, false, false);
        env.http.insert("https://generativelanguage.googleapis.com/v1beta/models".into(), (400, "API key not valid".into()));
        let mut ui = Script { yes: VecDeque::from([true, false, false]), secrets: VecDeque::from(["AIza-this-key-is-bad-bad-bad".to_string()]), ..Default::default() };
        let out = run(&mut ui, &env, &opts(t.path(), &proj)).unwrap();
        assert!(out.added.is_empty() && env.stored.borrow().is_empty());
        assert!(ui.said.iter().any(|s| s.contains("rejected the key (HTTP 400)")));
        let mut short = Script { yes: VecDeque::from([true, false, false]), secrets: VecDeque::from(["x".to_string()]), ..Default::default() };
        let t2 = tempfile::tempdir().unwrap();
        let proj2 = project(t2.path(), false);
        assert!(run(&mut short, &env, &opts(t2.path(), &proj2)).unwrap().added.is_empty());
    }

    #[test]
    fn the_google_oauth_path_stores_the_secret_and_writes_a_valid_auth_block() {
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), false);
        let env = machine(false, false, false);
        let mut ui = Script {
            yes: VecDeque::from([false, false, true]),
            lines: VecDeque::from(["my-gcp-project".into(), "1234-abcd.apps.googleusercontent.com".into()]),
            secrets: VecDeque::from(["GOCSPX-fakesecret".to_string()]),
            ..Default::default()
        };
        let out = run(&mut ui, &env, &opts(t.path(), &proj)).unwrap();
        assert_eq!(out.added, ["gemini-oauth"]);
        assert_eq!(env.stored.borrow().as_slice(), [("google-client-secret".to_string(), "GOCSPX-fakesecret".to_string())]);
        let cfg = config(t.path());
        assert!(!cfg.contains("GOCSPX"));
        let parsed = delphi_adapter::config::Config::parse(&cfg).expect("valid");
        let a = &parsed.auth["google"];
        assert_eq!((a.client_secret.as_deref(), a.authorization_params.get("access_type").map(String::as_str)), (Some("keychain:google-client-secret"), Some("offline")));
        assert_eq!(parsed.endpoint("gemini-oauth").unwrap().project.as_deref(), Some("my-gcp-project"));
        assert!(ui.opened.iter().any(|u| u.contains("generativelanguage.googleapis.com?project=my-gcp-project")));
        assert!(ui.opened.iter().any(|u| u.contains("/auth/clients/create?project=my-gcp-project")));
    }

    #[test]
    fn check_mode_changes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), true);
        let mut o = opts(t.path(), &proj);
        o.check = true;
        run(&mut Script::default(), &machine(true, true, true), &o).unwrap();
        assert!(!t.path().join("home").exists(), "no workspace is created by --check");
    }

    #[test]
    fn declining_ratification_leaves_a_pending_proposal_not_a_trusted_change() {
        let t = tempfile::tempdir().unwrap();
        let proj = project(t.path(), false);
        let mut ui = Script { yes: VecDeque::from([true, true, true, false, false, false, false]), ..Default::default() };
        // claude yes, agy yes, red-team-gemini yes, then ratify no
        let env = machine(true, true, true);
        let mut o = opts(t.path(), &proj);
        o.yes = false;
        ui.yes = VecDeque::from([true, true, false, false, false, true, false]);
        let out = run(&mut ui, &env, &o).unwrap();
        assert!(!out.ratified);
        let ws = t.path().join("home/workspaces").read_dir().unwrap().next().unwrap().unwrap().path();
        let (eff, notices) = delphi_engine::boot::effective_config(&ws, &config(t.path())).unwrap();
        assert!(!eff.contains("claude-sub"), "an unratified endpoint is not active");
        assert!(notices.iter().any(|n| n.contains("ratif")));
    }
}
