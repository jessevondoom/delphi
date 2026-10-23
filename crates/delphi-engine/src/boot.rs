//! Assemble an [`Engine`] from a workspace on disk: `config.toml` (endpoints, access policy, models,
//! projects, enabled roles, budget, approval timeout), credentials, the usage ledger and the model client.

use crate::approval::Broker;
use crate::engine::{Engine, Settings};
use crate::mission::SecretRegistry;
use crate::model::{HttpModelClient, ModelClient};
use crate::probe::GitProbe;
use crate::service::EngineService;
use crate::workspace::WorkspacePaths;
use delphi_adapter::config::Config;
use delphi_adapter::credential::{CredentialStore, Resolver};
use delphi_adapter::http::{build_client, RetryPolicy};
use delphi_core::hub::Hub;
use delphi_core::ledger::Ledger;
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

/// Operational settings from `config.toml`, with the defaults of `templates/config.example.toml`.
pub fn settings_from(root: &toml::Value) -> Settings {
    let mut s = Settings::default();
    if let Some(v) = root.get("budget").and_then(|b| b.get("mission_budget_usd")).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))) {
        s.mission_budget_usd = v;
    }
    if let Some(v) = root.get("hitl").and_then(|b| b.get("approval_timeout_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.approval_timeout = Duration::from_secs(v as u64);
    }
    if let Some(list) = root.get("agents").and_then(|a| a.get("enabled")).and_then(|v| v.as_array()) {
        let roles: Vec<String> = list.iter().filter_map(|x| x.as_str().map(String::from)).collect();
        if !roles.is_empty() {
            s.enabled_agents = roles;
        }
    }
    s
}

fn now_rfc3339() -> String {
    delphi_core::time::now_rfc3339()
}

pub struct Booted {
    pub engine: Arc<Engine>,
    pub service: Arc<EngineService>,
    pub default_mission: String,
}

/// Build the engine for `ws_dir` (`$DELPHI_HOME/workspaces/<name>`), publishing through `hub`.
pub fn build(home: &Path, ws_dir: &Path, hub: Hub) -> Result<Booted, String> {
    build_with_model(home, ws_dir, hub, None)
}

/// As [`build`], optionally with a model client supplied (tests).
pub fn build_with_model(home: &Path, ws_dir: &Path, hub: Hub, model: Option<Arc<dyn ModelClient>>) -> Result<Booted, String> {
    let text = std::fs::read_to_string(ws_dir.join("config.toml")).map_err(|e| format!("cannot read {}/config.toml: {}", ws_dir.display(), e))?;
    let cfg = Config::parse(&text).map_err(|errs| format!("config.toml has problems:\n  - {}", errs.join("\n  - ")))?;
    let root: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;
    let secrets = SecretRegistry::new();
    let cfg = Arc::new(cfg);
    let model: Arc<dyn ModelClient> = match model {
        Some(m) => m,
        None => {
            let reg = secrets.clone();
            let creds = Arc::new(CredentialStore::new(Resolver::system(Arc::new(move |s| reg.register(s)))));
            let (redact_secrets, register_secrets) = (secrets.clone(), secrets.clone());
            Arc::new(HttpModelClient {
                cfg: cfg.clone(),
                creds,
                http: build_client(),
                retry: RetryPolicy::default(),
                idle_timeout: Duration::from_secs(120),
                redact: Arc::new(move |s| redact_secrets.redact(s)),
                register: Arc::new(move |s| register_secrets.register(s)),
            })
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
    let engine = Arc::new(Engine {
        ws: WorkspacePaths { home: home.to_path_buf(), dir: ws_dir.to_path_buf() },
        cfg,
        hub,
        broker: Broker::new("ap_"),
        model,
        secrets,
        ledger: Arc::new(Mutex::new(ledger)),
        settings: settings_from(&root),
        probe: Arc::new(GitProbe::new()),
        projects: project_roots(&root),
        rules,
        clock: Arc::new(now_rfc3339),
    });
    let day = &now_rfc3339()[..10];
    let default_mission = format!("{}-session", day);
    let service = EngineService::new(engine.clone(), &default_mission);
    Ok(Booted { engine, service, default_mission })
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
        let (t, ws) = workspace_with_config("[budget]\nmission_budget_usd = 12.5\n[hitl]\napproval_timeout_sec = 90\n[agents]\nenabled = [\"orchestrator\", \"architect\"]\n");
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        assert_eq!(b.engine.settings.mission_budget_usd, 12.5);
        assert_eq!(b.engine.settings.approval_timeout, Duration::from_secs(90));
        assert_eq!(b.engine.settings.enabled_agents, vec!["orchestrator", "architect"]);
        assert_eq!(b.engine.projects.len(), 1);
        assert_eq!(b.engine.projects[0].0, "proj");
        assert_eq!(b.engine.projects[0].1, fs::canonicalize(t.path().join("proj")).unwrap());
        assert_eq!(b.engine.cfg.endpoints.len(), 2);
        assert!(b.default_mission.ends_with("-session") && b.default_mission.len() == 18, "{}", b.default_mission);
        assert!(ws.dir.join("local/state.db").exists());
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
        let err = build(&ws.home, &ws.dir, Hub::new()).err().unwrap();
        assert!(err.contains("config.toml has problems") && err.contains("need platform") && err.contains("bogus"), "{}", err);
        fs::write(ws.dir.join("config.toml"), "not toml [").unwrap();
        assert!(build(&ws.home, &ws.dir, Hub::new()).err().unwrap().contains("not valid TOML"));
        fs::remove_file(ws.dir.join("config.toml")).unwrap();
        assert!(build(&ws.home, &ws.dir, Hub::new()).err().unwrap().contains("cannot read"));
    }
}
