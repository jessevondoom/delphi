//! `delphi auth login|status|logout` (`specs/009` §4.1): sign in to the identity providers named by the
//! ratified `[auth.*]` blocks. Run through `delphi-core auth …`, so the `delphi` binary stays light.
//!
//! The blocks come from the same effective configuration the daemon uses, never from an unratified edit
//! of `config.toml`: a local process cannot point a login at a look-alike identity provider.

use delphi_adapter::config::{AuthBlock, AuthFlow, Config};
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

const GOOGLE_ISSUER: &str = "https://accounts.google.com";
const GUIDE: &str = "missions/2026-10-23-foundational-design/google-oauth-client.md in the Delphi repository";

/// What to tell the operator when `login <name>` finds no ratified block. Google is the one provider Delphi
/// can say more about, because it ships no client and every Google login starts with creating one.
fn missing_block(name: &str, configured: &[&String]) -> String {
    let have = if configured.is_empty() { "none are ratified".to_string() } else { format!("ratified: {}", configured.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")) };
    if name == "google" {
        format!(
            "delphi auth: no ratified [auth.google] block, so there is no Google OAuth client to sign in with ({}).\n  Delphi ships no client id. Create a Desktop-app OAuth client in your own Google Cloud project, store its secret in the keychain, add an [auth.google] block to config.toml (flow = \"loopback_pkce\", issuer = \"{}\", client_id, client_secret = \"keychain:google-client-secret\", authorization_params = {{ access_type = \"offline\", prompt = \"consent\" }}), then /ratify it.\n  Step by step: {}",
            have, GOOGLE_ISSUER, GUIDE
        )
    } else {
        format!("delphi auth: no ratified [auth.{}] block ({}); add it to config.toml and ratify it with `/ratify`", name, have)
    }
}

/// Checks that a block can plausibly complete a login, before a browser is opened. Errors name the fix;
/// warnings are printed and the login goes on.
pub fn preflight(name: &str, a: &AuthBlock) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    if a.client_id.trim().is_empty() || a.client_id.contains(['<', '>']) {
        return Err(format!("[auth.{}] client_id is a placeholder; put the real client id from your provider's console there and ratify the change", name));
    }
    if a.issuer.as_deref().map(|i| i.trim_end_matches('/')) == Some(GOOGLE_ISSUER) {
        if !a.client_id.ends_with(".apps.googleusercontent.com") {
            return Err(format!("[auth.{}] client_id `{}` is not a Google OAuth client id (they end in `.apps.googleusercontent.com`); copy it from Google Cloud console > APIs & Services > Credentials. See {}", name, a.client_id, GUIDE));
        }
        if a.flow == AuthFlow::LoopbackPkce {
            if a.client_secret.is_none() {
                return Err(format!("[auth.{}] has no client_secret: Google's Desktop-app clients still require one at the token endpoint. Store it in the keychain and set client_secret = \"keychain:<name>\". See {}", name, GUIDE));
            }
            if a.authorization_params.get("access_type").map(String::as_str) != Some("offline") {
                return Err(format!("[auth.{}] needs authorization_params = {{ access_type = \"offline\", prompt = \"consent\" }}: without access_type=offline Google issues no refresh token, so Delphi could not keep you signed in", name));
            }
        }
        if a.flow == AuthFlow::DeviceCode && a.scopes.iter().any(|s| s.contains("/auth/drive") && !s.ends_with("/drive.file") && !s.ends_with("/drive.appdata") || s.contains("/auth/gmail") || s.contains("/auth/calendar") || s.contains("/auth/cloud-platform")) {
            warnings.push(format!("[auth.{}] uses the device_code flow, which Google restricts to a few scopes (openid, email, profile, drive.file, drive.appdata, youtube); Drive read, Gmail, Calendar and cloud-platform need flow = \"loopback_pkce\" and will be refused here", name));
        }
    }
    Ok(warnings)
}

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
                eprintln!("{}", missing_block(&n, &cfg.auth.keys().collect::<Vec<_>>()));
                return 1;
            };
            match preflight(&n, block) {
                Ok(warnings) => warnings.iter().for_each(|w| eprintln!("delphi auth: warning: {}", w)),
                Err(e) => {
                    eprintln!("delphi auth: {}", e);
                    return 1;
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn google(extra: &str) -> AuthBlock {
        let text = format!("[auth.google]\nflow = \"loopback_pkce\"\nissuer = \"https://accounts.google.com\"\nclient_id = \"123-abc.apps.googleusercontent.com\"\nclient_secret = \"keychain:google-client-secret\"\nauthorization_params = {{ access_type = \"offline\", prompt = \"consent\" }}\n{}", extra);
        Config::parse(&text).unwrap().auth["google"].clone()
    }

    #[test]
    fn a_complete_google_block_passes_without_warnings() {
        assert_eq!(preflight("google", &google("scopes = [\"https://www.googleapis.com/auth/drive.readonly\"]")), Ok(vec![]));
    }

    #[test]
    fn google_login_without_a_usable_client_stops_with_the_fix() {
        let mut a = google("");
        a.client_id = "<your client id>".into();
        assert!(preflight("google", &a).unwrap_err().contains("placeholder"));
        a.client_id = "not-a-google-id".into();
        assert!(preflight("google", &a).unwrap_err().contains(".apps.googleusercontent.com"));
        let mut a = google("");
        a.client_secret = None;
        assert!(preflight("google", &a).unwrap_err().contains("client_secret"));
        let mut a = google("");
        a.authorization_params.clear();
        assert!(preflight("google", &a).unwrap_err().contains("access_type"));
    }

    #[test]
    fn google_device_flow_warns_about_restricted_scopes() {
        let mut a = google("scopes = [\"https://www.googleapis.com/auth/drive.readonly\"]");
        a.flow = AuthFlow::DeviceCode;
        let w = preflight("google", &a).unwrap();
        assert!(w.len() == 1 && w[0].contains("loopback_pkce"), "{:?}", w);
        a.scopes = vec!["https://www.googleapis.com/auth/drive.file".into()];
        assert!(preflight("google", &a).unwrap().is_empty());
    }

    #[test]
    fn a_missing_google_block_explains_how_to_make_a_client() {
        let msg = missing_block("google", &[]);
        for needle in ["no Google OAuth client", "Desktop-app", "keychain:google-client-secret", "access_type", "/ratify", "google-oauth-client.md"] {
            assert!(msg.contains(needle), "missing `{}` in {}", needle, msg);
        }
        let other = "corp".to_string();
        let msg = missing_block("okta", &[&other]);
        assert!(msg.contains("ratified: corp") && !msg.contains("Google"), "{}", msg);
    }
}
