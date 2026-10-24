//! `delphi connectors status|test|pin` (`specs/010` §9).
//!
//! `status` is offline: what is configured, whether a login is stored, and which tool definitions are
//! ratified. `test` connects with the stored login, checks the signed-in account against the ratified one,
//! lists the server's tools and compares them with the pins. `pin` does the same and then writes the current
//! definitions as a proposal; nothing changes until the operator ratifies it (`/ratify` in the console).
//! All of it uses only the ratified configuration, never an unratified edit.

use crate::boot::{connector_host, effective_config};
use crate::connectors::{ConnectorHost, ToolState};
use crate::mission::SecretRegistry;
use delphi_adapter::config::{Config, SharedConfig};
use std::path::Path;

fn load(ws_dir: &Path) -> Result<Config, String> {
    let live = std::fs::read_to_string(ws_dir.join("config.toml")).map_err(|e| format!("cannot read config.toml: {}", e))?;
    let (text, _) = effective_config(ws_dir, &live)?;
    Config::parse(&text).map_err(|e| e.join("; "))
}

fn describe(s: &ToolState) -> String {
    match s {
        ToolState::Allowed => "ok (ratified, unchanged)".into(),
        ToolState::Unpinned => "not ratified yet (run `delphi connectors pin <name>`, then /ratify)".into(),
        ToolState::Changed { pinned, now } => format!("CHANGED since ratified; disabled. Was: {:?} Now: {:?}", pinned.chars().take(80).collect::<String>(), now.chars().take(80).collect::<String>()),
        ToolState::Missing => "not offered by the server".into(),
    }
}

/// The offline report: one block per configured connector.
pub fn status(host: &ConnectorHost, cfg: &Config) -> String {
    if cfg.connectors.is_empty() {
        return "no connectors are configured (add [connectors.<name>] to config.toml and ratify it)\n".into();
    }
    let mut out = String::new();
    for (name, c) in &cfg.connectors {
        let pins = host.ratified_pins(name);
        out.push_str(&format!("{}  {}\n    account: {}  login: {}  roles: {}\n", name, c.url, c.account.as_deref().unwrap_or("(none)"), c.login.as_deref().unwrap_or("(none needed)"), c.roles.join(", ")));
        for (tool, class) in &c.tools {
            out.push_str(&format!("    {:<24} {:<6} {}\n", tool, class.as_str(), if pins.contains_key(tool) { "pinned" } else { "not pinned" }));
        }
    }
    out
}

pub fn run(ws_dir: &Path, args: &[String]) -> i32 {
    let sub = args.first().map(String::as_str).unwrap_or("");
    let name = args.get(1).filter(|a| !a.starts_with("--")).cloned();
    if !matches!(sub, "status" | "test" | "pin") || (sub != "status" && name.is_none()) {
        eprintln!("usage: delphi connectors status | test <name> | pin <name>");
        return 2;
    }
    let cfg = match load(ws_dir) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("delphi connectors: {}", e);
            return 1;
        }
    };
    let secrets = SecretRegistry::new();
    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("delphi connectors: {}", e);
            return 1;
        }
    };
    let host = {
        let _g = rt.enter();
        connector_host(SharedConfig::new(cfg.clone()), secrets, ws_dir)
    };
    if sub == "status" {
        print!("{}", status(&host, &cfg));
        return 0;
    }
    let name = name.unwrap_or_default();
    if !cfg.connectors.contains_key(&name) {
        eprintln!("delphi connectors: no ratified connector `{}` (configured: {})", name, cfg.connectors.keys().cloned().collect::<Vec<_>>().join(", "));
        return 1;
    }
    match rt.block_on(host.refresh(&name)) {
        Ok(tools) => {
            println!("{}: connected; {} tool(s) in the catalog", name, tools.len());
            for t in &tools {
                println!("    {:<24} {:<6} {}", t.name, t.class.as_str(), describe(&t.state));
            }
            if sub == "pin" {
                match host.propose_pins(&name) {
                    Ok(n) => println!("{} definition(s) proposed. Review them in the console (`delphi chat`, then /ratify): nothing is offered to agents until you do.", n),
                    Err(e) => {
                        eprintln!("delphi connectors: {}", e);
                        return 1;
                    }
                }
            }
            let bad = tools.iter().any(|t| matches!(t.state, ToolState::Changed { .. }));
            i32::from(bad)
        }
        Err(e) => {
            eprintln!("delphi connectors: {}: {}", name, e);
            1
        }
    }
}
