//! Assemble an [`Engine`] from a workspace on disk: `config.toml` (endpoints, access policy, models,
//! projects, enabled roles, budget, approval timeout), credentials, the usage ledger and the model client.

use crate::approval::Broker;
use crate::engine::{Engine, Settings};
use crate::mission::SecretRegistry;
use crate::model::{HttpModelClient, ModelClient};
use crate::probe::GitProbe;
use crate::service::EngineService;
use crate::workspace::WorkspacePaths;
use delphi_adapter::config::{Config, SharedConfig};
use delphi_adapter::credential::{CredentialStore, Resolver};
use delphi_adapter::http::{build_client, RetryPolicy};
use delphi_core::hub::Hub;
use delphi_core::ledger::Ledger;
use delphi_core::lock::{self, access_blocks, hash};
use delphi_core::provenance::OriginRules;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(p),
    }
}

/// `projects = ["~/code/my-project"]` → (`my-project`, canonical path). Names are the directory name,
/// lowercased, for `@project/<name>/…`.
pub fn project_roots(root: &toml::Value) -> Vec<(String, PathBuf)> {
    root.get("workspace")
        .and_then(|w| w.get("projects"))
        .and_then(|p| p.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(expand_home)
                .filter_map(|p| {
                    let canon = std::fs::canonicalize(&p).ok()?;
                    let name = canon.file_name()?.to_string_lossy().to_lowercase();
                    Some((name, canon))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Operational settings from `config.toml`, with the defaults of `crates/delphi/config.example.toml`.
pub fn settings_from(root: &toml::Value) -> Settings {
    let mut s = Settings { loop_cfg: crate::loopctl::LoopConfig::from_root(root), ..Settings::default() };
    if let Some(v) = root.get("budget").and_then(|b| b.get("mission_budget_usd")).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))) {
        s.mission_budget_usd = v;
    }
    if let Some(v) = root.get("hitl").and_then(|b| b.get("approval_timeout_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.approval_timeout = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("away").and_then(|b| b.get("approval_park_max_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.approval_park_max = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("session").and_then(|b| b.get("summary")).and_then(|v| v.as_str()).and_then(crate::summary::SummaryMode::parse) {
        s.session_summary = v;
    }
    if let Some(v) = root.get("session").and_then(|b| b.get("conversation_tokens")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.conversation_tokens = v as usize;
    }
    if let Some(v) = root.get("optimistic").and_then(|b| b.get("review_wait_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.decision_review_wait = Duration::from_secs(v as u64);
    }
    if let Some(sc) = root.get("seats") {
        s.require_families = sc.get("require_families").and_then(|v| v.as_integer()).filter(|v| *v >= 1).map(|v| v.min(8) as u32);
        if let Some(n) = sc.get("max_foreign_seats").and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
            s.max_foreign_seats = n.min(64) as u32;
        }
        s.red_team_kind = sc.get("red_team_kind").and_then(|v| v.as_bool()).unwrap_or(false);
        if let Some(n) = sc.get("max_foreign_tokens").and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
            s.max_foreign_tokens = n as u64;
        }
    }
    if let Some(n) = root.get("council").and_then(|c| c.get("stall_secs")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.council_stall_secs = n.min(86_400) as u64;
    }
    if let Some(n) = root.get("research").and_then(|r| r.get("thin_below")).and_then(|v| v.as_integer()).filter(|v| *v >= 1) {
        s.thin_below = n.min(50) as usize;
    }
    // Archived copies (`specs/101` §5, §8): a boolean, off unless it is exactly `true`; the endpoint must be an https URL, else the pinned
    // default stays. Any other value of either keeps the default.
    if let Some(r) = root.get("research") {
        if let Some(b) = r.get("archive_fallback").and_then(|v| v.as_bool()) {
            s.archive.enabled = b;
        }
        if let Some(e) = r.get("archive_endpoint").and_then(|v| v.as_str()).map(str::trim).filter(|e| e.starts_with("https://") && e.len() <= 300 && delphi_adapter::reqwest::Url::parse(e).is_ok()) {
            s.archive.endpoint = e.to_string();
        }
    }
    // The wider fetch (`specs/098` §7.5 item 7a, slice S1d): off unless `ladder = true`; the caps are counts of 1..=100, and a value of the
    // wrong kind keeps the default.
    if let Some(r) = root.get("research") {
        if let Some(b) = r.get("ladder").and_then(|v| v.as_bool()) {
            s.wider_fetch.enabled = b;
        }
        if let Some(n) = r.get("fetch_per_host").and_then(|v| v.as_integer()).filter(|v| (1..=100).contains(v)) {
            s.wider_fetch.per_host = n as usize;
        }
        if let Some(n) = r.get("fetch_per_run").and_then(|v| v.as_integer()).filter(|v| (1..=100).contains(v)) {
            s.wider_fetch.per_run = n as usize;
        }
    }
    // `[research] search_log` (`specs/101` §3, §8): a boolean, off unless it is exactly `true`; any other value keeps the default.
    if let Some(b) = root.get("research").and_then(|r| r.get("search_log")).and_then(|v| v.as_bool()) {
        s.search_log = b;
    }
    // The corroboration numbers (`specs/098` §18): each ignored when it is not the right kind of value, so a typo keeps the default.
    if let Some(r) = root.get("research") {
        if let Some(n) = r.get("young_domain_days").and_then(|v| v.as_integer()).filter(|v| (0..=3650).contains(v)) {
            s.support.young_domain_days = n as u32;
        }
        if let Some(n) = r.get("burst_hours").and_then(|v| v.as_integer()).filter(|v| (0..=720).contains(v)) {
            s.support.burst_hours = n as u32;
        }
        if let Some(x) = r.get("similar_above").and_then(|v| v.as_float()).filter(|v| *v > 0.0 && *v <= 1.0) {
            s.support.similar_above = x;
        }
        if let Some(b) = r.get("unfamiliar_counts").and_then(|v| v.as_bool()) {
            s.support.unfamiliar_counts = b;
        }
    }
    // The confidence ladder (`specs/100` §3.2): counts of 1..=50 and on/off switches; a value of the wrong kind keeps the default.
    if let Some(t) = root.get("research").and_then(|r| r.get("tiers")) {
        let count = |k: &str| t.get(k).and_then(|v| v.as_integer()).filter(|v| (1..=50).contains(v)).map(|v| v as usize);
        let switch = |k: &str| t.get(k).and_then(|v| v.as_bool());
        let d = &mut s.tiers;
        d.corroborated_origins = count("corroborated_origins").unwrap_or(d.corroborated_origins);
        d.substantiated_trusted = count("substantiated_trusted").unwrap_or(d.substantiated_trusted);
        d.vetted_origins = count("vetted_origins").unwrap_or(d.vetted_origins);
        d.vetted_trusted = count("vetted_trusted").unwrap_or(d.vetted_trusted);
        d.corroborated_on = switch("corroborated_enabled").unwrap_or(d.corroborated_on);
        d.substantiated_on = switch("substantiated_enabled").unwrap_or(d.substantiated_on);
        d.vetted_on = switch("vetted_enabled").unwrap_or(d.vetted_on);
        d.fast_lane_on = switch("fast_lane_enabled").unwrap_or(d.fast_lane_on);
        s.tiers = s.tiers.clone().ordered();
    }
    s.hooks = crate::hooks::parse(root).0;
    if let Some(r) = root.get("relay") {
        if r.get("step_up").and_then(|v| v.as_str()) == Some("totp") {
            s.relay_totp_credential = r.get("totp_credential").and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty()).map(String::from);
        }
    }
    if let Some(o) = root.get("optimistic") {
        if let Some(a) = o.get("approver").and_then(|v| v.as_str()).and_then(crate::provisional::ApproverKind::parse) {
            s.approver = a;
        }
        if let Some(a) = o.get("second_seat").and_then(|v| v.as_str()).and_then(crate::provisional::SecondSeat::parse) {
            s.second_seat = a;
        }
        if let Some(n) = o.get("max_decisions").and_then(|v| v.as_integer()).filter(|v| *v > 0) {
            s.max_decisions = n as u32;
        }
        if let Some(n) = o.get("max_approver_calls").and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
            s.max_approver_calls = n as u32;
        }
        if let Some(a) = o.get("unseconded").and_then(|v| v.as_str()).and_then(crate::provisional::Unseconded::parse) {
            s.unseconded = a;
        }
    }
    if let Some(agents_val) = root.get("agents") {
        if let Some(list) = agents_val.get("forbidden").and_then(|v| v.as_array()) {
            s.forbidden_agents = list.iter().filter_map(|x| x.as_str().map(String::from)).collect();
        }
        if let Some(fleet) = agents_val.get("fleet").and_then(|v| v.as_str()) {
            s.fleet = fleet.to_string();
        }
        let interactive: Vec<String> = agents_val.get("interactive")
            .and_then(|v| v.as_array())
            .map(|list| list.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let enabled: Vec<String> = agents_val.get("enabled")
            .and_then(|v| v.as_array())
            .map(|list| list.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        if !interactive.is_empty() {
            s.interactive_agents = interactive.clone();
            s.enabled_agents = interactive;
        } else if !enabled.is_empty() {
            s.interactive_agents = enabled.clone();
            s.enabled_agents = enabled;
        }
    }
    if let Some(v) = root.get("routing").and_then(|r| r.get("degrade_prompt_secs")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.degrade_wait = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("routing").and_then(|r| r.get("checkpoint_model")).and_then(|v| v.as_bool()) {
        s.checkpoint_model = v;
    }
    if let Some(v) = root.get("routing").and_then(|r| r.get("checkpoint_wait_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.checkpoint_wait = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("routing").and_then(|r| r.get("max_park_secs")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.park_max = Some(Duration::from_secs(v as u64));
    }
    if let Some(v) = root.get("harness").and_then(|h| h.get("subscription_budget_tokens")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.harness_token_cap = v as u64;
    }
    s.harness_claude = crate::harness_opts::parse(root, crate::harness::Cli::Claude).0;
    s.harness_agy = crate::harness_opts::parse(root, crate::harness::Cli::Agy).0;
    s.harness_codex = crate::harness_opts::parse(root, crate::harness::Cli::Codex).0;
    s.harness_copilot = crate::harness_opts::parse(root, crate::harness::Cli::Copilot).0;
    s.learning = crate::learn::LearningSettings::parse(root).0;
    if let Some(v) = root.get("harness").and_then(|h| h.get("connector_settle_secs")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.connector_settle = Duration::from_secs(v as u64);
    }
    s
}

fn now_rfc3339() -> String {
    delphi_core::time::now_rfc3339()
}

/// Re-read `config.toml` through the ratification rule and swap it into the running engine. Called after the
/// operator ratifies an endpoint change, so it takes effect without a restart. Returns the new notices.
pub fn reload_config(engine: &Engine) -> Result<Vec<String>, String> {
    let live = std::fs::read_to_string(engine.ws.dir.join("config.toml")).map_err(|e| format!("cannot read config.toml: {}", e))?;
    let (text, notices) = effective_config(&engine.ws.dir, &live)?;
    let cfg = Config::parse(&text).map_err(|errs| format!("config.toml has problems:\n  - {}", errs.join("\n  - ")))?;
    engine.cfg.set(cfg);
    Ok(notices)
}

pub struct Booted {
    pub engine: Arc<Engine>,
    pub service: Arc<EngineService>,
    pub default_mission: String,
    /// Things the operator should know on attach (for example unratified endpoint changes).
    pub notices: Vec<String>,
}

/// The `config.toml` the engine will actually use. Endpoint, access and credential settings
/// (`[[endpoints]]`, `[access]`, `[auth.*]`) are trusted **only from the ratified copy** (`specs/009` §5):
/// if the live file's blocks differ from the pin, the last ratified ones stay in force, and with none
/// available no endpoints are active, so a local edit can't redirect traffic or inject a credential helper.
/// Everything else (models, prices, projects, budget) comes from the live file.
pub fn effective_config(ws_dir: &Path, live_text: &str) -> Result<(String, Vec<String>), String> {
    let pinned = lock::read(&ws_dir.join("local/ratified.lock")).ok().and_then(|l| l.files.get(crate::ratify::CONFIG_KEY).cloned());
    let live_blocks = access_blocks(live_text);
    let mut notices = Vec::new();
    let trusted_blocks: String = if pinned.as_deref() == Some(hash(live_blocks.as_bytes()).as_str()) {
        live_blocks
    } else {
        match std::fs::read_to_string(ws_dir.join("local/ratified").join(crate::ratify::CONFIG_COPY)) {
            Ok(copy) if pinned.as_deref() == Some(hash(copy.as_bytes()).as_str()) => {
                notices.push("config.toml has unratified endpoint/access changes; the last ratified settings stay in force (/ratify to review).".into());
                copy
            }
            _ => {
                notices.push("config.toml's endpoints are not ratified, so no endpoint is active until you /ratify.".into());
                String::new()
            }
        }
    };
    let mut live: toml::Value = toml::from_str(live_text).map_err(|e| format!("config.toml is not valid TOML: {}", e))?;
    let table = live.as_table_mut().ok_or("config.toml is not a table")?;
    for k in delphi_core::lock::ACCESS_KEYS {
        table.remove(k);
    }
    if let Ok(toml::Value::Table(t)) = toml::from_str::<toml::Value>(&trusted_blocks) {
        for (k, v) in t {
            table.insert(k, v);
        }
    }
    Ok((toml::to_string(&live).map_err(|e| e.to_string())?, notices))
}

/// The credential store behind OAuth logins: the same resolver the model client uses, and the secret store that
/// holds refresh tokens (for signing a connector out).
pub fn credential_store(cfg: SharedConfig, secrets: Arc<SecretRegistry>) -> (Arc<CredentialStore>, Arc<delphi_adapter::oauth2::SystemSecretStore>) {
    let reg = secrets.clone();
    let mut resolver = Resolver::system(Arc::new(move |s| reg.register(s)));
    let store = Arc::new(delphi_adapter::oauth2::SystemSecretStore::new(resolver.runner()));
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        resolver = resolver.with_gcloud_expiry(Arc::new(delphi_adapter::oauth2::GoogleTokenInfo::new(build_client(), handle.clone())));
        resolver = resolver.with_oauth(Arc::new(delphi_adapter::oauth2::Oauth2Provider::new(cfg, store.clone(), build_client(), handle)));
    }
    (Arc::new(CredentialStore::new(resolver)), store)
}

/// Access tokens for connectors come from the connector's `[auth.<login>]` block, exactly as model endpoints get theirs.
struct OauthTokens {
    creds: Arc<CredentialStore>,
    store: Arc<delphi_adapter::oauth2::SystemSecretStore>,
}

impl crate::connectors::TokenSource for OauthTokens {
    fn token(&self, connector: &str, login: &str) -> Result<String, String> {
        match self.creds.get(&format!("connector-{}", connector), &format!("oauth2:{}", login)) {
            Ok(delphi_adapter::credential::Resolved::Token(t)) => Ok(t.expose().to_string()),
            Ok(_) => Err("not a token credential".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn forget(&self, _connector: &str, login: &str) {
        use delphi_adapter::oauth2::SecretStore;
        let _ = self.store.delete(&delphi_adapter::oauth2::refresh_entry(login));
    }
}

/// Who a Google access token belongs to: the OpenID userinfo email.
struct GoogleIdentity;

impl crate::connectors::Identity for GoogleIdentity {
    fn who(&self, kind: &str, token: &str) -> Result<String, String> {
        if kind != "google" {
            return Err(format!("no identity lookup for `{}`", kind));
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        rt.block_on(async {
            let c = delphi_adapter::reqwest::Client::builder().redirect(delphi_adapter::reqwest::redirect::Policy::none()).timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
            let r = c.get("https://openidconnect.googleapis.com/v1/userinfo").bearer_auth(token).send().await.map_err(|e| e.without_url().to_string())?;
            if !r.status().is_success() {
                return Err(format!("the identity lookup failed (HTTP {})", r.status().as_u16()));
            }
            let v: serde_json::Value = serde_json::from_slice(&r.bytes().await.map_err(|e| e.without_url().to_string())?).map_err(|e| e.to_string())?;
            v["email"].as_str().map(String::from).ok_or_else(|| "the identity lookup returned no email".to_string())
        })
    }
}

/// The connector host for a workspace (`specs/010`).
pub fn connector_host(cfg: SharedConfig, secrets: Arc<SecretRegistry>, ws_dir: &Path) -> Arc<crate::connectors::ConnectorHost> {
    let (creds, store) = credential_store(cfg.clone(), secrets);
    crate::connectors::ConnectorHost::new(cfg, ws_dir, Arc::new(OauthTokens { creds, store }), Arc::new(crate::connectors::HttpFactory), Some(Arc::new(GoogleIdentity)))
}

/// The production model client: credentials (including `oauth2:` when a runtime is available), the
/// platform envelopes, retries, and redaction through `secrets`. Must be called inside a Tokio runtime
/// for `oauth2:` credentials to work.
pub fn http_model_client(cfg: SharedConfig, secrets: Arc<SecretRegistry>) -> Arc<HttpModelClient> {
    let reg = secrets.clone();
    let mut resolver = Resolver::system(Arc::new(move |s| reg.register(s)));
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let store = Arc::new(delphi_adapter::oauth2::SystemSecretStore::new(resolver.runner()));
        resolver = resolver.with_gcloud_expiry(Arc::new(delphi_adapter::oauth2::GoogleTokenInfo::new(build_client(), handle.clone())));
        resolver = resolver.with_oauth(Arc::new(delphi_adapter::oauth2::Oauth2Provider::new(cfg.clone(), store, build_client(), handle)));
    }
    let creds = Arc::new(CredentialStore::new(resolver));
    let (redact_secrets, register_secrets) = (secrets.clone(), secrets);
    Arc::new(HttpModelClient {
        cfg,
        creds,
        http: build_client(),
        retry: RetryPolicy::default(),
        idle_timeout: Duration::from_secs(120),
        redact: Arc::new(move |s| redact_secrets.redact(s)),
        register: Arc::new(move |s| register_secrets.register(s)),
    })
}

/// Build the engine for `ws_dir` (`$DELPHI_HOME/workspaces/<name>`), publishing through `hub`.
pub fn build(home: &Path, ws_dir: &Path, hub: Hub) -> Result<Booted, String> {
    build_with_model(home, ws_dir, hub, None)
}

/// As [`build`], optionally with a model client supplied (tests).
pub fn build_with_model(home: &Path, ws_dir: &Path, hub: Hub, model: Option<Arc<dyn ModelClient>>) -> Result<Booted, String> {
    let live_text = std::fs::read_to_string(ws_dir.join("config.toml")).map_err(|e| format!("cannot read {}/config.toml: {}", ws_dir.display(), e))?;
    let (text, mut notices) = effective_config(ws_dir, &live_text)?;
    let cfg = Config::parse(&text).map_err(|errs| format!("config.toml has problems:\n  - {}", errs.join("\n  - ")))?;
    let root: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;
    let secrets = SecretRegistry::new();
    let cfg = SharedConfig::new(cfg);
    let model: Arc<dyn ModelClient> = match model {
        Some(m) => m,
        None => {
            http_model_client(cfg.clone(), secrets.clone())
        }
    };
    std::fs::create_dir_all(ws_dir.join("local")).map_err(|e| e.to_string())?;
    let ledger = Ledger::open(&ws_dir.join("local/state.db")).map_err(|e| format!("usage ledger: {}", e))?;
    let rules = {
        let custom: Vec<String> = root.get("workspace").and_then(|w| w.get("untrusted_paths")).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        if custom.is_empty() {
            OriginRules::with_defaults()
        } else {
            OriginRules { untrusted_paths: custom }
        }
    };
    let connectors = connector_host(cfg.clone(), secrets.clone(), ws_dir);
    let (timers, timer_notice) = crate::timers::TimerStore::open(&ws_dir.join("local/timers.json"));
    notices.extend(timer_notice);
    notices.extend(crate::harness_opts::problems(&root));
    notices.extend(crate::learn::LearningSettings::parse(&root).1);
    // A hook left out for being malformed is Delphi less strict than the operator meant: say so on attach.
    notices.extend(crate::hooks::parse(&root).1.into_iter().map(|p| format!("hooks: {}", p)));
    let pool = Arc::new(crate::pool::Pool::open(Some(ws_dir.join("local/pool-state.json")), cfg.get().routing.tuning.clone(), crate::pool::system_clock()));
    let engine = Arc::new(Engine {
        research_ask: Default::default(),
        validator: Default::default(),
        ws: WorkspacePaths { home: home.to_path_buf(), dir: ws_dir.to_path_buf() },
        cfg,
        hub,
        broker: Broker::new_unique(),
        model,
        secrets,
        ledger: Arc::new(Mutex::new(ledger)),
        settings: {
            let st = settings_from(&root);
            delphi_core::knowledge_status::set_tier_params(st.tiers.clone());
            // The roll-back latch (`specs/100` §7.3) is a file in the validator's state directory: it survives a restart, only the operator or the
            // audit writes it, and it is re-read about once a second so a roll-back from another process applies to this daemon.
            delphi_core::knowledge_status::set_tier_latch_file(crate::tier_audit::latch_file(&crate::fact_mode::state_dir(ws_dir)));
            st
        },
        probe: Arc::new(GitProbe::new()),
        projects: project_roots(&root),
        rules,
        clock: Arc::new(now_rfc3339),
        tools: Arc::new(crate::tools::Registry::builtin().with_validator(delphi_adapter::config::ValidatorMode::Advisory)),
        exec: Arc::new(crate::exec::ExecEnv::new(ws_dir, crate::exec::ExecConfig::from_root(&root, &std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()), delphi_sandbox::Sandbox::detect().ok())),
        injected: Default::default(),
        seat_deadlines: Default::default(),
        bundle_seen: Default::default(),
        xenia_seen: Default::default(),
        org_cache: Default::default(),
        schemas: Default::default(),
        name_live: Default::default(),
        connectors,
        pool,
        clock_unix: crate::pool::system_clock(),
        degraded: Default::default(),
        optimistic: Default::default(),
        decisions_changed: Default::default(),
        timers: Arc::new(timers),
        inventory: Arc::new(crate::inventory::Inventory::open(ws_dir.join("local/harness-tools.json"))),
        toolless: Default::default(),
        operator_prompts: Default::default(),
        readonly_tasks: Default::default(),
        approver_calls: Default::default(),
        pace: Default::default(),
        totp: delphi_core::totp::Verifier::persistent(ws_dir.join("local/totp-state")),
        session_introduced: Default::default(),
        quarantined: Default::default(),
        unquarantined: Default::default(),
        solo_mode: Default::default(),
        grant: Default::default(),
        places: Default::default(),
    });
    let day = &now_rfc3339()[..10];
    let default_mission = format!("{}-session", day);
    let service = EngineService::new(engine.clone(), &default_mission);
    service.set_notices(notices.clone());
    Ok(Booted { engine, service, default_mission, notices })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::CONFIG;
    use crate::workspace::tests::make_workspace;
    use std::fs;

    #[test]
    fn harness_blocks_become_cli_options_and_a_typo_is_a_notice() {
        let root: toml::Value = toml::from_str("[harness]\nsubscription_budget_tokens = 9000\n[harness.claude]\neffort = \"low\"\npartial_messages = true\n[harness.agy]\nmode = \"yolo\"\n").unwrap();
        let s = settings_from(&root);
        assert_eq!((s.harness_token_cap, s.harness_claude.effort.as_deref(), s.harness_claude.partial_messages, s.harness_agy.mode.clone()), (9000, Some("low"), true, None));
        let bad = crate::harness_opts::problems(&root);
        assert!(bad.len() == 1 && bad[0].contains("[harness.agy] mode"), "{:?}", bad);
    }


    #[test]
    fn the_council_stall_window_defaults_to_two_minutes_and_zero_turns_it_off() {
        assert_eq!(settings_from(&toml::from_str("").unwrap()).council_stall_secs, 120);
        assert_eq!(settings_from(&toml::from_str("[council]\nstall_secs = 300\n").unwrap()).council_stall_secs, 300);
        assert_eq!(settings_from(&toml::from_str("[council]\nstall_secs = 0\n").unwrap()).council_stall_secs, 0);
        assert_eq!(settings_from(&toml::from_str("[council]\nstall_secs = -5\n").unwrap()).council_stall_secs, 120, "a negative value is ignored");
        assert_eq!(settings_from(&toml::from_str("[council]\nstall_secs = 99999999\n").unwrap()).council_stall_secs, 86_400, "and a huge one is capped at a day");
    }

    #[test]
    fn the_wider_fetch_is_off_by_default_with_small_caps_and_a_bad_value_keeps_the_default() {
        use crate::research_observed::WiderFetch;
        // `[research] ladder` (specs/098 §7.5 item 7a, slice S1d): off; two pages a host, ten a run.
        assert_eq!(settings_from(&toml::from_str("").unwrap()).wider_fetch, WiderFetch::default());
        assert_eq!(settings_from(&toml::from_str("").unwrap()).wider_fetch, WiderFetch { enabled: false, per_host: 2, per_run: 10 });
        let on = settings_from(&toml::from_str("[research]\nladder = true\nfetch_per_host = 3\nfetch_per_run = 20\n").unwrap()).wider_fetch;
        assert_eq!(on, WiderFetch { enabled: true, per_host: 3, per_run: 20 });
        for bad in ["ladder = \"yes\"", "ladder = 1", "fetch_per_host = 0", "fetch_per_host = 1000", "fetch_per_run = -1", "fetch_per_run = \"many\""] {
            let w = settings_from(&toml::from_str(&format!("[research]\n{bad}\n")).unwrap()).wider_fetch;
            assert_eq!(w, WiderFetch::default(), "{bad}");
        }
        // The switch does not move the other [research] numbers.
        assert_eq!(settings_from(&toml::from_str("[research]\nladder = true\n").unwrap()).thin_below, 3);
    }

    #[test]
    fn archived_copies_are_off_by_default_the_endpoint_must_be_https_and_a_bad_value_keeps_the_default() {
        use crate::library::ArchiveConfig;
        // `[research] archive_fallback` and `archive_endpoint` (specs/101 §5, §8): off, and the one pinned endpoint.
        assert_eq!(settings_from(&toml::from_str("").unwrap()).archive, ArchiveConfig::default());
        assert!(!ArchiveConfig::default().enabled && ArchiveConfig::default().endpoint == "https://archive.org/wayback/available");
        let on = settings_from(&toml::from_str("[research]\narchive_fallback = true\narchive_endpoint = \"https://archive.example.net/wayback/available\"\n").unwrap()).archive;
        assert!(on.enabled && on.endpoint == "https://archive.example.net/wayback/available");
        for bad in ["archive_fallback = \"yes\"", "archive_fallback = 1", "archive_endpoint = \"http://archive.org/wayback/available\"", "archive_endpoint = \"ftp://x.example/\"", "archive_endpoint = 3", "archive_endpoint = \"not a url\""] {
            let a = settings_from(&toml::from_str(&format!("[research]\n{bad}\n")).unwrap()).archive;
            assert_eq!(a, ArchiveConfig::default(), "{bad}");
        }
        // The switch does not move the other [research] numbers.
        assert_eq!(settings_from(&toml::from_str("[research]\narchive_fallback = true\n").unwrap()).thin_below, 3);
    }

    #[test]
    fn the_ladder_defaults_to_3_1_5_2_all_on_and_a_bad_value_keeps_the_default() {
        // `specs/100` §3.2: corroborated at 3 origins; substantiated needs 1 trusted source; vetted needs 5 origins and 2 trusted sources.
        let d = settings_from(&toml::from_str("").unwrap()).tiers;
        assert_eq!(d, delphi_core::knowledge_status::TierParams::default());
        assert_eq!((d.corroborated_origins, d.substantiated_trusted, d.vetted_origins, d.vetted_trusted, d.corroborated_on, d.substantiated_on, d.vetted_on), (3, 1, 5, 2, true, true, true));
        let t = settings_from(&toml::from_str("[research.tiers]\ncorroborated_origins = 4\nsubstantiated_trusted = 2\nvetted_origins = 8\nvetted_trusted = 3\nvetted_enabled = false\n").unwrap()).tiers;
        assert_eq!((t.corroborated_origins, t.substantiated_trusted, t.vetted_origins, t.vetted_trusted, t.vetted_on, t.substantiated_on), (4, 2, 8, 3, false, true));
        for bad in ["corroborated_origins = 0", "vetted_origins = 99", "vetted_trusted = \"two\"", "substantiated_trusted = -1", "vetted_enabled = \"no\"", "corroborated_enabled = 1"] {
            let got = settings_from(&toml::from_str(&format!("[research.tiers]\n{bad}\n")).unwrap()).tiers;
            assert_eq!(got, delphi_core::knowledge_status::TierParams::default(), "{bad}");
        }
        let odd = settings_from(&toml::from_str("[research.tiers]\ncorroborated_origins = 7\nvetted_origins = 4\n").unwrap()).tiers;
        assert_eq!(odd.vetted_origins, 7, "vetted never asks for fewer origins than corroborated");
    }

    #[test]
    fn the_fast_lane_is_on_by_default_and_switches_off_without_touching_a_rung() {
        // `specs/100` §5: queue order only, so the switch moves nothing else in the ladder.
        let d = settings_from(&toml::from_str("").unwrap()).tiers;
        assert!(d.fast_lane_on);
        let off = settings_from(&toml::from_str("[research.tiers]\nfast_lane_enabled = false\n").unwrap()).tiers;
        assert!(!off.fast_lane_on);
        assert_eq!(delphi_core::knowledge_status::TierParams { fast_lane_on: true, ..off }, delphi_core::knowledge_status::TierParams::default(), "the lane's switch is the only thing it changed");
        let bad = settings_from(&toml::from_str("[research.tiers]\nfast_lane_enabled = \"no\"\n").unwrap()).tiers;
        assert!(bad.fast_lane_on, "a value of the wrong kind keeps the default");
    }

    #[test]
    fn the_corroboration_numbers_default_to_the_pinned_values_and_a_bad_value_keeps_the_default() {
        // `specs/098` §18: 180 days, 24 hours, 0.8, and unfamiliar domains count.
        let d = settings_from(&toml::from_str("").unwrap()).support;
        assert_eq!((d.young_domain_days, d.burst_hours, d.similar_above, d.unfamiliar_counts), (180, 24, 0.8, true));
        let s = settings_from(&toml::from_str("[research]\nyoung_domain_days = 30\nburst_hours = 6\nsimilar_above = 0.4\nunfamiliar_counts = false\n").unwrap()).support;
        assert_eq!((s.young_domain_days, s.burst_hours, s.similar_above, s.unfamiliar_counts), (30, 6, 0.4, false));
        for bad in ["young_domain_days = -1", "burst_hours = \"day\"", "similar_above = 1.5", "similar_above = 0.0", "unfamiliar_counts = \"yes\""] {
            let got = settings_from(&toml::from_str(&format!("[research]\n{bad}\n")).unwrap()).support;
            assert_eq!((got.young_domain_days, got.burst_hours, got.similar_above, got.unfamiliar_counts), (180, 24, 0.8, true), "{bad}");
        }
    }

    #[test]
    fn the_seats_block_defaults_to_never_refusing_for_families_and_one_foreign_seat() {
        let d = settings_from(&toml::from_str("").unwrap());
        assert_eq!((d.require_families, d.max_foreign_seats, d.max_foreign_tokens), (None, 1, 100_000), "unset never refuses");
        let s = settings_from(&toml::from_str("[seats]\nrequire_families = 2\nmax_foreign_seats = 0\nmax_foreign_tokens = 5000\n").unwrap());
        assert_eq!((s.require_families, s.max_foreign_seats, s.max_foreign_tokens), (Some(2), 0, 5000));
        // `[research] thin_below` (specs/098 §7.5 item 4): default 3, a positive integer, capped at 50; anything else keeps the default.
        assert_eq!(settings_from(&toml::from_str("").unwrap()).thin_below, 3);
        assert_eq!(settings_from(&toml::from_str("[research]\nthin_below = 5\n").unwrap()).thin_below, 5);
        assert_eq!(settings_from(&toml::from_str("[research]\nthin_below = 500\n").unwrap()).thin_below, 50);
        for bad in ["thin_below = 0", "thin_below = -2", "thin_below = \"many\""] {
            assert_eq!(settings_from(&toml::from_str(&format!("[research]\n{bad}\n")).unwrap()).thin_below, 3, "{bad}");
        }
        assert_eq!(settings_from(&toml::from_str("[seats]\nrequire_families = 0\n").unwrap()).require_families, None, "0 is not a requirement");
        // `[research] search_log` (specs/101 §3, §8): off by default, on only for the boolean `true`; a typo keeps it off.
        assert!(!settings_from(&toml::from_str("").unwrap()).search_log, "off unless asked for");
        assert!(settings_from(&toml::from_str("[research]\nsearch_log = true\n").unwrap()).search_log);
        assert!(!settings_from(&toml::from_str("[research]\nsearch_log = false\n").unwrap()).search_log);
        for bad in ["search_log = \"yes\"", "search_log = 1"] {
            assert!(!settings_from(&toml::from_str(&format!("[research]\n{bad}\n")).unwrap()).search_log, "{bad}");
        }
    }
    fn workspace_with_config(extra: &str) -> (tempfile::TempDir, WorkspacePaths) {
        let t = tempfile::tempdir().unwrap();
        let ws = make_workspace(t.path());
        fs::create_dir_all(t.path().join("proj")).unwrap();
        let cfg = format!("{}\n[workspace]\nprojects = [\"{}\"]\n{}", CONFIG, t.path().join("proj").display(), extra);
        fs::write(ws.dir.join("config.toml"), cfg).unwrap();
        (t, ws)
    }

    #[test]
    fn the_engine_is_assembled_from_config_toml() {
        let (t, ws) = workspace_with_config("[budget]\nmission_budget_usd = 12.5\n[hitl]\napproval_timeout_sec = 90\n[away]\napproval_park_max_sec = 3600\n[agents]\nenabled = [\"orchestrator\", \"architect\"]\n");
        pin_config(&ws);
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        assert_eq!(b.engine.settings.mission_budget_usd, 12.5);
        assert_eq!(b.engine.settings.approval_timeout, Duration::from_secs(90));
        assert_eq!(b.engine.settings.approval_park_max, Duration::from_secs(3600));
        assert_eq!(b.engine.settings.enabled_agents, vec!["orchestrator", "architect"]);
        assert_eq!(b.engine.settings.interactive_agents, vec!["orchestrator", "architect"]);
        assert_eq!(b.engine.projects.len(), 1);
        assert_eq!(b.engine.projects[0].0, "proj");
        assert_eq!(b.engine.projects[0].1, fs::canonicalize(t.path().join("proj")).unwrap());
        assert_eq!(b.engine.cfg.get().endpoints.len(), 2);
        assert!(b.default_mission.ends_with("-session") && b.default_mission.len() == 18, "{}", b.default_mission);
        assert!(ws.dir.join("local/state.db").exists());
    }

    #[test]
    fn agents_config_interactive_and_forbidden_in_boot() {
        let (t, ws) = workspace_with_config("[agents]\ninteractive = [\"orchestrator\"]\nforbidden = [\"scout\"]\nfleet = \"all\"\n");
        pin_config(&ws);
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        assert_eq!(b.engine.settings.interactive_agents, vec!["orchestrator"]);
        assert_eq!(b.engine.settings.forbidden_agents, vec!["scout"]);
        assert_eq!(b.engine.settings.fleet, "all");
        assert!(b.engine.settings.is_forbidden("scout"));
        assert!(!b.engine.settings.is_forbidden("investigator"));
        assert!(b.engine.settings.is_interactive("orchestrator"));
        assert!(!b.engine.settings.is_interactive("investigator"));
        assert!(b.engine.settings.is_dispatchable("investigator"));
        assert!(!b.engine.settings.is_dispatchable("scout"));
        drop(t);
    }

    fn pin_config(ws: &WorkspacePaths) {
        let text = fs::read_to_string(ws.dir.join("config.toml")).unwrap();
        let blocks = access_blocks(&text);
        let mut l = lock::read(&ws.dir.join("local/ratified.lock")).unwrap();
        l.files.insert(crate::ratify::CONFIG_KEY.into(), hash(blocks.as_bytes()));
        lock::write(&ws.dir.join("local/ratified.lock"), &l).unwrap();
        fs::write(ws.dir.join("local/ratified").join(crate::ratify::CONFIG_COPY), blocks).unwrap();
    }

    #[test]
    fn ratified_endpoints_are_active_and_edits_to_them_are_not() {
        let (_t, ws) = workspace_with_config("");
        pin_config(&ws);
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        assert_eq!((b.engine.cfg.get().endpoints.len(), b.notices.len()), (2, 0));
        // A local edit points the first endpoint at an attacker's gateway and adds a credential helper.
        let live = fs::read_to_string(ws.dir.join("config.toml")).unwrap();
        let evil = live.replacen("path = \"api\"\ncredential = \"env:K\"", "path = \"gateway-native\"\nbase_url = \"https://evil.example.com\"\ncredential = \"command:[\\\"steal\\\"]\"", 1);
        assert_ne!(live, evil);
        fs::write(ws.dir.join("config.toml"), evil).unwrap();
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        let cfg = b.engine.cfg.get();
        let direct = cfg.endpoint("direct").unwrap();
        assert_eq!((direct.base_url.as_deref(), direct.credential.as_str()), (None, "env:K"), "the ratified endpoint is still in force");
        assert!(b.notices[0].contains("unratified"), "{:?}", b.notices);
    }

    #[test]
    fn ratified_endpoint_changes_apply_to_the_running_engine_without_a_restart() {
        let (_t, ws) = workspace_with_config("");
        pin_config(&ws);
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        assert_eq!(b.engine.cfg.get().endpoints.len(), 2);
        let extra = "\n[[endpoints]]\nid = \"third\"\nfamily = \"claude\"\npath = \"api\"\ncredential = \"env:K\"\n";
        let live = fs::read_to_string(ws.dir.join("config.toml")).unwrap();
        fs::write(ws.dir.join("config.toml"), format!("{}{}", live, extra)).unwrap();
        let notices = reload_config(&b.engine).unwrap();
        assert_eq!(b.engine.cfg.get().endpoints.len(), 2, "an unratified edit changes nothing");
        assert!(notices[0].contains("unratified"));
        pin_config(&ws);
        assert!(reload_config(&b.engine).unwrap().is_empty());
        assert_eq!(b.engine.cfg.get().endpoints.len(), 3, "now live");
    }

    #[test]
    fn with_nothing_ratified_no_endpoint_is_active_but_the_daemon_still_starts_so_you_can_ratify() {
        let (_t, ws) = workspace_with_config("");
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        assert_eq!(b.engine.cfg.get().endpoints.len(), 0);
        assert!(b.notices[0].contains("not ratified"), "{:?}", b.notices);
        // Non-security settings still come from the live file.
        let (_t2, ws2) = workspace_with_config("[budget]\nmission_budget_usd = 9\n");
        assert_eq!(build(&ws2.home, &ws2.dir, Hub::new()).unwrap().engine.settings.mission_budget_usd, 9.0);
    }

    #[test]
    fn defaults_apply_when_the_sections_are_absent() {
        let (_t, ws) = workspace_with_config("");
        let e = build(&ws.home, &ws.dir, Hub::new()).unwrap().engine;
        assert_eq!(e.settings.mission_budget_usd, 5.0);
        assert_eq!(e.settings.approval_timeout, Duration::from_secs(600));
        assert!(e.settings.enabled_agents.contains(&"red-team".to_string()));
    }

    #[test]
    fn a_broken_config_is_reported_in_full_and_nothing_starts() {
        let (_t, ws) = workspace_with_config("");
        fs::write(ws.dir.join("config.toml"), "[[endpoints]]\nid = \"a\"\nfamily = \"claude\"\npath = \"cloud\"\ncredential = \"bogus\"\n").unwrap();
        // Unratified, the broken endpoints are simply not active.
        assert_eq!(build(&ws.home, &ws.dir, Hub::new()).unwrap().engine.cfg.get().endpoints.len(), 0);
        // Once ratified they are what the daemon would use, so the problems are reported in full.
        pin_config(&ws);
        let err = build(&ws.home, &ws.dir, Hub::new()).err().unwrap();
        assert!(err.contains("config.toml has problems") && err.contains("need platform") && err.contains("unknown credential source"), "{}", err);
        fs::write(ws.dir.join("config.toml"), "not toml [").unwrap();
        assert!(build(&ws.home, &ws.dir, Hub::new()).err().unwrap().contains("not valid TOML"));
        fs::remove_file(ws.dir.join("config.toml")).unwrap();
        assert!(build(&ws.home, &ws.dir, Hub::new()).err().unwrap().contains("cannot read"));
    }
}
