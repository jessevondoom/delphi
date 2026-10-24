//! `delphi auth login|status|logout` (`specs/009` §4.1): sign in to the identity providers named by the
//! ratified `[auth.*]` blocks. Run through `delphi-core auth …`, so the `delphi` binary stays light.
//!
//! The blocks come from the same effective configuration the daemon uses, never from an unratified edit
//! of `config.toml`: a local process cannot point a login at a look-alike identity provider.

use delphi_adapter::config::{AuthFlow, Config};
use delphi_adapter::credential::SystemRunner;
use delphi_adapter::oauth2::{refresh_entry, sign_in, DeviceGrant, LoginUi, Pacing, SecretStore, SignInOptions, SystemSecretStore};
use std::io::BufRead;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

struct TerminalUi;

fn try_open(url: &str) {
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    // One argument, no shell; failure is fine because the URL is printed too.
    let _ = std::process::Command::new(program).arg(url).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

impl LoginUi for TerminalUi {
    fn device(&self, g: &DeviceGrant) {
        println!("To sign in, open  {}  and enter the code  {}", g.verification_uri, g.user_code);
        if let Some(u) = &g.verification_uri_complete {
            println!("(or open {} directly)", u);
            try_open(u);
        }
        println!("Waiting for you to approve it… (Ctrl-C to cancel)");
    }

    fn open_url(&self, url: &str) {
        println!("Open this address in your browser to sign in:\n\n  {}\n", url);
        println!("Waiting for the redirect to 127.0.0.1… (Ctrl-C to cancel; use --manual if your browser is on another machine)");
        try_open(url);
    }

    fn pasted_redirect(&self) -> Option<String> {
        println!("After approving, your browser will fail to load a 127.0.0.1 address. Paste that whole address here:");
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).ok()?;
        Some(line.trim().to_string())
    }
}

const USAGE: &str = "usage: delphi auth login <name> [--manual] | status | logout <name>";

pub fn run(ws_dir: &Path, args: &[String]) -> i32 {
    let live = match std::fs::read_to_string(ws_dir.join("config.toml")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi auth: cannot read config.toml: {}", e);
            return 1;
        }
    };
    let cfg = match crate::boot::effective_config(ws_dir, &live).and_then(|(text, _)| Config::parse(&text).map_err(|e| e.join("; "))) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("delphi auth: {}", e);
            return 1;
        }
    };
    let store = SystemSecretStore::new(Arc::new(SystemRunner));
    let name = args.get(1).filter(|a| !a.starts_with("--")).cloned();
    match (args.first().map(String::as_str), name) {
        (Some("status"), _) => {
            if cfg.auth.is_empty() {
                println!("no [auth.*] blocks are ratified for this workspace");
            }
            for (n, a) in &cfg.auth {
                let state = match a.flow {
                    AuthFlow::ClientCredentials => match a.client_secret.as_deref().map(|r| r.trim_start_matches("keychain:")).map(|k| store.get(k)) {
                        Some(Ok(Some(_))) => "client secret present",
                        _ => "client secret MISSING",
                    },
                    _ => match store.get(&refresh_entry(n)) {
                        Ok(Some(_)) => "signed in",
                        Ok(None) => "not signed in",
                        Err(_) => "keychain unavailable",
                    },
                };
                println!("{:<20} {:<20} {}", n, format!("{:?}", a.flow).to_lowercase(), state);
            }
            0
        }
        (Some("logout"), Some(n)) => {
            if !cfg.auth.contains_key(&n) {
                eprintln!("delphi auth: no [auth.{}] block", n);
                return 1;
            }
            match store.delete(&refresh_entry(&n)) {
                Ok(()) => {
                    println!("signed out of `{}` (the stored refresh token was removed; revoke it at the provider too if you suspect it leaked)", n);
                    0
                }
                Err(e) => {
                    eprintln!("delphi auth: {}", e);
                    1
                }
            }
        }
        (Some("login"), Some(n)) => {
            let Some(block) = cfg.auth.get(&n) else {
                eprintln!("delphi auth: no ratified [auth.{}] block (add it to config.toml and ratify it with `/ratify`)", n);
                return 1;
            };
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("delphi auth: {}", e);
                    return 1;
                }
            };
            let manual = args.iter().any(|a| a == "--manual");
            let client = delphi_adapter::http::build_client();
            match rt.block_on(sign_in(&client, &n, block, &store, &TerminalUi, &SignInOptions { manual, pacing: Pacing::PRODUCTION, wait: Duration::from_secs(120) })) {
                Ok(secs) => {
                    println!("signed in to `{}` (first access token valid {} min). Delphi stored a refresh token in your keychain under service `delphi`.", n, secs / 60);
                    0
                }
                Err(e) => {
                    eprintln!("delphi auth: {}", e);
                    1
                }
            }
        }
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}
