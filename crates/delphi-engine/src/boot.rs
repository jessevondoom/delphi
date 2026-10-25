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
    let mut s = Settings::default();
    if let Some(v) = root.get("budget").and_then(|b| b.get("mission_budget_usd")).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))) {
        s.mission_budget_usd = v;
    }
    if let Some(v) = root.get("hitl").and_then(|b| b.get("approval_timeout_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.approval_timeout = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("away").and_then(|b| b.get("approval_park_max_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.approval_park_max = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("optimistic").and_then(|b| b.get("review_wait_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.decision_review_wait = Duration::from_secs(v as u64);
    }
    s.hooks = crate::hooks::parse(root).0;
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
    if let Some(list) = root.get("agents").and_then(|a| a.get("enabled")).and_then(|v| v.as_array()) {
        let roles: Vec<String> = list.iter().filter_map(|x| x.as_str().map(String::from)).collect();
        if !roles.is_empty() {
            s.enabled_agents = roles;
        }
    }
    if let Some(v) = root.get("routing").and_then(|r| r.get("degrade_prompt_secs")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.degrade_wait = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("routing").and_then(|r| r.get("max_park_secs")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.park_max = Some(Duration::from_secs(v as u64));
    }
    if let Some(v) = root.get("harness").and_then(|h| h.get("subscription_budget_tokens")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.harness_token_cap = v as u64;
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
    // A hook left out for being malformed is Delphi less strict than the operator meant: say so on attach.
    notices.extend(crate::hooks::parse(&root).1.into_iter().map(|p| format!("hooks: {}", p)));
    let pool = Arc::new(crate::pool::Pool::open(Some(ws_dir.join("local/pool-state.json")), cfg.get().routing.tuning.clone(), crate::pool::system_clock()));
    let engine = Arc::new(Engine {
        ws: WorkspacePaths { home: home.to_path_buf(), dir: ws_dir.to_path_buf() },
        cfg,
        hub,
        broker: Broker::new_unique(),
        model,
        secrets,
        ledger: Arc::new(Mutex::new(ledger)),
        settings: settings_from(&root),
        probe: Arc::new(GitProbe::new()),
        projects: project_roots(&root),
        rules,
        clock: Arc::new(now_rfc3339),
        tools: Arc::new(crate::tools::Registry::builtin()),
        exec: Arc::new(crate::exec::ExecEnv::new(ws_dir, crate::exec::ExecConfig::from_root(&root, &std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()), delphi_sandbox::Sandbox::detect().ok())),
        injected: Default::default(),
        connectors,
        pool,
        clock_unix: crate::pool::system_clock(),
        degraded: Default::default(),
        optimistic: Default::default(),
        decisions_changed: Default::default(),
        timers: Arc::new(timers),
        toolless: Default::default(),
        approver_calls: Default::default(),
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
        assert_eq!(b.engine.projects.len(), 1);
        assert_eq!(b.engine.projects[0].0, "proj");
        assert_eq!(b.engine.projects[0].1, fs::canonicalize(t.path().join("proj")).unwrap());
        assert_eq!(b.engine.cfg.get().endpoints.len(), 2);
        assert!(b.default_mission.ends_with("-session") && b.default_mission.len() == 18, "{}", b.default_mission);
        assert!(ws.dir.join("local/state.db").exists());
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
