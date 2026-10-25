//! OAuth2 sign-in and token refresh (`specs/009` §4.1–§4.2).
//!
//! * Flows: `device_code` (RFC 8628), `client_credentials`, and `loopback_pkce` (authorization code with
//!   PKCE, redirected to `127.0.0.1`). Delphi ships no client ids; they come from `[auth.<name>]`.
//! * Only a **refresh token** is ever stored, in Delphi's own keychain entry (`service=delphi`, name
//!   `oauth2.<auth-name>`). Access tokens live in memory and are renewed before expiry.
//! * A revoked or expired refresh token (`invalid_grant`) deletes the entry and yields
//!   [`CredentialError::NeedsReauth`]; a network failure does not (the sign-in is still good).
//! * Subscription logins (a Claude or Gemini plan) are not OAuth clients Delphi may use; those stay with
//!   the official CLIs (`specs/009` §4.2 invariant 2).

use crate::config::{AuthBlock, AuthFlow, SharedConfig};
use crate::credential::{CredentialError, Runner, Secret};
use crate::sigv4::uri_encode;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const DEFAULT_LIFETIME_SECS: i64 = 3600;
const SKEW_SECS: i64 = 60;
const MAX_RESPONSE: usize = 1 << 20;

#[derive(Debug, PartialEq, Eq)]
pub enum OauthError {
    /// The provider refused with an OAuth error code (`invalid_grant`, `access_denied`, …).
    Rejected { code: String, description: String },
    /// The device code or the loopback wait ran out.
    Expired,
    Http { status: u16, message: String },
    Transport(String),
    Malformed(String),
}

impl fmt::Display for OauthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OauthError::Rejected { code, description } if description.is_empty() => write!(f, "the identity provider refused ({})", code),
            OauthError::Rejected { code, description } => write!(f, "the identity provider refused ({}): {}", code, description),
            OauthError::Expired => write!(f, "the sign-in was not completed in time"),
            OauthError::Http { status, message } => write!(f, "identity provider HTTP {}: {}", status, message),
            OauthError::Transport(m) => write!(f, "could not reach the identity provider: {}", m),
            OauthError::Malformed(m) => write!(f, "unexpected identity provider response: {}", m),
        }
    }
}

impl std::error::Error for OauthError {}

impl OauthError {
    /// The stored sign-in is no good any more; the operator must sign in again.
    pub fn needs_reauth(&self) -> bool {
        matches!(self, OauthError::Rejected { code, .. } if code == "invalid_grant")
    }
}

/// Tokens from the token endpoint. `Debug` never shows a value.
pub struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    pub expires_in: Option<i64>,
}

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tokens(<redacted>, refresh: {}, expires_in: {:?})", self.refresh.is_some(), self.expires_in)
    }
}

/// Endpoint URLs after OIDC discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub token: String,
    pub device: Option<String>,
    pub authorization: Option<String>,
}

fn url_ok(u: &str) -> bool {
    u.starts_with("https://") || crate::config::is_loopback(u)
}

fn form(params: &[(&str, &str)]) -> String {
    params.iter().map(|(k, v)| format!("{}={}", uri_encode(k), uri_encode(v))).collect::<Vec<_>>().join("&")
}

async fn read_body(mut resp: reqwest::Response) -> Result<(u16, String), OauthError> {
    let status = resp.status().as_u16();
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(transport)? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_RESPONSE {
            return Err(OauthError::Malformed("response too large".into()));
        }
    }
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

fn transport(e: reqwest::Error) -> OauthError {
    OauthError::Transport(e.without_url().to_string())
}

/// An OAuth error body (`{"error": "...", "error_description": "..."}`), if that is what this is.
fn oauth_error(body: &str) -> Option<OauthError> {
    let v: Value = serde_json::from_str(body).ok()?;
    let code = v["error"].as_str()?.to_string();
    Some(OauthError::Rejected { code, description: v["error_description"].as_str().unwrap_or("").chars().take(300).collect() })
}

async fn post_form(client: &reqwest::Client, url: &str, params: &[(&str, &str)]) -> Result<Value, OauthError> {
    let resp = client.post(url).header("content-type", "application/x-www-form-urlencoded").header("accept", "application/json").body(form(params)).send().await.map_err(transport)?;
    let (status, body) = read_body(resp).await?;
    // Providers answer 400 (and sometimes 401) with an OAuth error object; surface that, not the status.
    if let Some(e) = oauth_error(&body) {
        return Err(e);
    }
    if !(200..300).contains(&status) {
        return Err(OauthError::Http { status, message: body.chars().take(200).collect() });
    }
    serde_json::from_str(&body).map_err(|_| OauthError::Malformed("not JSON".into()))
}

fn tokens(v: &Value) -> Result<Tokens, OauthError> {
    let access = v["access_token"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| OauthError::Malformed("no access_token".into()))?;
    Ok(Tokens { access: access.to_string(), refresh: v["refresh_token"].as_str().filter(|s| !s.is_empty()).map(String::from), expires_in: v["expires_in"].as_i64().or_else(|| v["expires_in"].as_str().and_then(|s| s.parse().ok())) })
}

/// Resolve the endpoints, using OIDC discovery only for what the block does not give.
pub async fn endpoints(client: &reqwest::Client, a: &AuthBlock) -> Result<Endpoints, OauthError> {
    let (mut token, mut device, mut authorization) = (a.token_url.clone(), a.device_authorization_url.clone(), a.authorization_url.clone());
    let need_discovery = token.is_none() || (a.flow == AuthFlow::DeviceCode && device.is_none()) || (a.flow == AuthFlow::LoopbackPkce && authorization.is_none());
    if need_discovery {
        let issuer = a.issuer.as_deref().ok_or_else(|| OauthError::Malformed("no issuer to discover endpoints from".into()))?;
        let resp = client.get(format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'))).header("accept", "application/json").send().await.map_err(transport)?;
        let (status, body) = read_body(resp).await?;
        if status != 200 {
            return Err(OauthError::Http { status, message: "OIDC discovery failed".into() });
        }
        let doc: Value = serde_json::from_str(&body).map_err(|_| OauthError::Malformed("discovery document is not JSON".into()))?;
        // OIDC Discovery §4.3: the document must name the issuer it was fetched for, or a look-alike host
        // (or a redirect to one) could hand us its own token endpoint.
        match doc["issuer"].as_str() {
            Some(named) if named.trim_end_matches('/') == issuer.trim_end_matches('/') => {}
            Some(_) => return Err(OauthError::Malformed("the discovery document names a different issuer than the configured one".into())),
            None => return Err(OauthError::Malformed("the discovery document names no issuer".into())),
        }
        let pick = |k: &str| doc[k].as_str().map(String::from);
        token = token.or_else(|| pick("token_endpoint"));
        device = device.or_else(|| pick("device_authorization_endpoint"));
        authorization = authorization.or_else(|| pick("authorization_endpoint"));
    }
    let token = token.ok_or_else(|| OauthError::Malformed("no token endpoint".into()))?;
    for u in [Some(&token), device.as_ref(), authorization.as_ref()].into_iter().flatten() {
        if !url_ok(u) {
            return Err(OauthError::Malformed(format!("endpoint `{}` is not https", u.split('?').next().unwrap_or(""))));
        }
    }
    Ok(Endpoints { token, device, authorization })
}

fn base_params<'a>(a: &'a AuthBlock, secret: Option<&'a str>) -> Vec<(&'a str, &'a str)> {
    let mut p = vec![("client_id", a.client_id.as_str())];
    if let Some(s) = secret {
        p.push(("client_secret", s));
    }
    p
}

pub async fn refresh(client: &reqwest::Client, ep: &Endpoints, a: &AuthBlock, client_secret: Option<&str>, refresh_token: &str) -> Result<Tokens, OauthError> {
    let mut p = vec![("grant_type", "refresh_token"), ("refresh_token", refresh_token)];
    p.extend(base_params(a, client_secret));
    tokens(&post_form(client, &ep.token, &p).await?)
}

pub async fn client_credentials(client: &reqwest::Client, ep: &Endpoints, a: &AuthBlock, client_secret: &str) -> Result<Tokens, OauthError> {
    let scope = a.scopes.join(" ");
    let mut p = vec![("grant_type", "client_credentials")];
    if !scope.is_empty() {
        p.push(("scope", &scope));
    }
    if let Some(aud) = &a.audience {
        p.push(("audience", aud));
    }
    p.extend(base_params(a, Some(client_secret)));
    tokens(&post_form(client, &ep.token, &p).await?)
}

// ── Device code ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceGrant {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

pub async fn device_start(client: &reqwest::Client, ep: &Endpoints, a: &AuthBlock) -> Result<DeviceGrant, OauthError> {
    let url = ep.device.as_deref().ok_or_else(|| OauthError::Malformed("no device authorization endpoint".into()))?;
    let scope = a.scopes.join(" ");
    let mut p = vec![("client_id", a.client_id.as_str())];
    if !scope.is_empty() {
        p.push(("scope", &scope));
    }
    if let Some(aud) = &a.audience {
        p.push(("audience", aud));
    }
    let v = post_form(client, url, &p).await?;
    let s = |k: &str| v[k].as_str().map(String::from);
    let missing = |k: &str| OauthError::Malformed(format!("device response has no {}", k));
    Ok(DeviceGrant {
        device_code: s("device_code").ok_or_else(|| missing("device_code"))?,
        user_code: s("user_code").ok_or_else(|| missing("user_code"))?,
        verification_uri: s("verification_uri").or_else(|| s("verification_url")).ok_or_else(|| missing("verification_uri"))?,
        verification_uri_complete: s("verification_uri_complete"),
        expires_in: v["expires_in"].as_u64().unwrap_or(600),
        interval: v["interval"].as_u64().unwrap_or(5),
    })
}

/// How patiently to poll. Production waits at least a second and adds five on `slow_down` (RFC 8628);
/// tests use zero.
#[derive(Debug, Clone, Copy)]
pub struct Pacing {
    pub floor: Duration,
    pub slow_down_step: Duration,
}

impl Pacing {
    pub const PRODUCTION: Pacing = Pacing { floor: Duration::from_secs(1), slow_down_step: Duration::from_secs(5) };
    pub const INSTANT: Pacing = Pacing { floor: Duration::ZERO, slow_down_step: Duration::ZERO };
}

/// Consecutive failures to reach the token endpoint (or 5xx/429 answers) before polling gives up. A
/// pending authorization is not a failure and does not count.
const MAX_TRANSIENT_POLLS: u32 = 5;

fn transient(e: &OauthError) -> bool {
    matches!(e, OauthError::Transport(_)) || matches!(e, OauthError::Http { status, .. } if *status == 429 || *status >= 500)
}

/// Poll until the operator approves. `client_secret` is sent when the client has one (confidential clients,
/// and Google's, refuse the device-code exchange without it). A network blip or a 5xx answer is retried at
/// the polling interval (RFC 8628 §3.5); only a run of them ends the sign-in.
pub async fn device_poll(client: &reqwest::Client, ep: &Endpoints, a: &AuthBlock, client_secret: Option<&str>, g: &DeviceGrant, pacing: Pacing) -> Result<Tokens, OauthError> {
    let deadline = Instant::now() + Duration::from_secs(g.expires_in);
    let mut interval = Duration::from_secs(g.interval).max(pacing.floor);
    let mut failures = 0;
    loop {
        tokio::time::sleep(interval).await;
        if Instant::now() >= deadline {
            return Err(OauthError::Expired);
        }
        let mut p = vec![("grant_type", "urn:ietf:params:oauth:grant-type:device_code"), ("device_code", g.device_code.as_str())];
        p.extend(base_params(a, client_secret));
        match post_form(client, &ep.token, &p).await {
            Ok(v) => return tokens(&v),
            Err(OauthError::Rejected { code, .. }) if code == "authorization_pending" => failures = 0,
            Err(OauthError::Rejected { code, .. }) if code == "slow_down" => {
                failures = 0;
                interval += pacing.slow_down_step;
            }
            Err(OauthError::Rejected { code, .. }) if code == "expired_token" => return Err(OauthError::Expired),
            Err(e) if transient(&e) => {
                failures += 1;
                if failures >= MAX_TRANSIENT_POLLS {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }
}

// ── Loopback PKCE ───────────────────────────────────────────────────────────

/// 32 random bytes from the operating system.
pub fn random_bytes(n: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).map_err(|e| format!("no system randomness: {}", e))?;
    Ok(buf)
}

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    /// 256 bits; a redirect without it is never accepted.
    pub state: String,
}

impl Pkce {
    pub fn generate() -> Result<Pkce, String> {
        let verifier = URL_SAFE_NO_PAD.encode(random_bytes(32)?);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Ok(Pkce { verifier, challenge, state: URL_SAFE_NO_PAD.encode(random_bytes(32)?) })
    }
}

pub fn authorization_url(ep: &Endpoints, a: &AuthBlock, redirect_uri: &str, pkce: &Pkce) -> Result<String, OauthError> {
    let base = ep.authorization.as_deref().ok_or_else(|| OauthError::Malformed("no authorization endpoint".into()))?;
    let scope = a.scopes.join(" ");
    let mut p = vec![("response_type", "code"), ("client_id", a.client_id.as_str()), ("redirect_uri", redirect_uri), ("state", pkce.state.as_str()), ("code_challenge", pkce.challenge.as_str()), ("code_challenge_method", "S256")];
    if !scope.is_empty() {
        p.push(("scope", &scope));
    }
    if let Some(aud) = &a.audience {
        p.push(("audience", aud));
    }
    // Operator-supplied parameters never override the ones that make the flow safe.
    const OWN: [&str; 6] = ["response_type", "client_id", "redirect_uri", "state", "code_challenge", "code_challenge_method"];
    for (k, v) in a.authorization_params.iter().filter(|(k, _)| !OWN.contains(&k.as_str())) {
        p.push((k, v));
    }
    Ok(format!("{}{}{}", base, if base.contains('?') { '&' } else { '?' }, form(&p)))
}

pub async fn exchange_code(client: &reqwest::Client, ep: &Endpoints, a: &AuthBlock, client_secret: Option<&str>, code: &str, redirect_uri: &str, verifier: &str) -> Result<Tokens, OauthError> {
    let mut p = vec![("grant_type", "authorization_code"), ("code", code), ("redirect_uri", redirect_uri), ("code_verifier", verifier)];
    p.extend(base_params(a, client_secret));
    tokens(&post_form(client, &ep.token, &p).await?)
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_of(target: &str) -> HashMap<String, String> {
    let q = target.split_once('?').map(|(_, q)| q).unwrap_or(target);
    q.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (pct_decode(k), pct_decode(v))).collect()
}

/// The authorization code in a redirect target (`/callback?code=…&state=…` or a whole pasted URL), after
/// checking `state`.
pub fn code_from_redirect(target: &str, state: &str) -> Result<String, OauthError> {
    let q = query_of(target);
    if q.get("state").map(|s| s.as_str()) != Some(state) {
        return Err(OauthError::Malformed("the redirect's state does not match this sign-in".into()));
    }
    if let Some(e) = q.get("error") {
        return Err(OauthError::Rejected { code: e.clone(), description: q.get("error_description").cloned().unwrap_or_default() });
    }
    q.get("code").filter(|c| !c.is_empty()).cloned().ok_or_else(|| OauthError::Malformed("the redirect carries no code".into()))
}

/// Bind the loopback listener. A fixed `port` is the one registered with the provider.
pub async fn loopback_bind(port: Option<u16>) -> Result<(TcpListener, u16), OauthError> {
    let l = TcpListener::bind(("127.0.0.1", port.unwrap_or(0))).await.map_err(|e| OauthError::Transport(format!("cannot listen on 127.0.0.1: {}", e)))?;
    let p = l.local_addr().map_err(|e| OauthError::Transport(e.to_string()))?.port();
    Ok((l, p))
}

/// Why a loopback request is not the provider's redirect.
fn browser_navigation_problem(head: &str, port: u16) -> Option<&'static str> {
    let mut lines = head.lines();
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    if parts.next() != Some("GET") {
        return Some("method");
    }
    if !parts.next().unwrap_or("").starts_with("/callback") {
        return Some("path");
    }
    let header = |name: &str| lines.clone().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string()));
    // DNS rebinding: the Host must be the loopback address we bound.
    match header("host") {
        Some(h) if h == format!("127.0.0.1:{}", port) || h == format!("localhost:{}", port) => {}
        _ => return Some("host"),
    }
    // A redirect is a top-level navigation. A script on some page that fetches this port is not.
    if header("origin").is_some() {
        return Some("origin");
    }
    if header("sec-fetch-mode").is_some_and(|m| m != "navigate") || header("sec-fetch-dest").is_some_and(|d| d != "document") {
        return Some("fetch-metadata");
    }
    None
}

/// Wait for the provider's redirect; unrelated or forged requests are answered 400 and ignored.
pub async fn loopback_wait(listener: TcpListener, port: u16, state: &str, timeout: Duration) -> Result<String, OauthError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let (mut sock, _) = match tokio::time::timeout_at(deadline, listener.accept()).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return Err(OauthError::Transport(e.to_string())),
            Err(_) => return Err(OauthError::Expired),
        };
        let mut buf = vec![0u8; 8192];
        let mut n = 0;
        // Read the request head (bounded, with its own short timeout so an idle connection can't stall us).
        let head = loop {
            match tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf[n..])).await {
                Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break None,
                Ok(Ok(k)) => n += k,
            }
            if let Some(i) = buf[..n].windows(4).position(|w| w == b"\r\n\r\n") {
                break Some(String::from_utf8_lossy(&buf[..i]).into_owned());
            }
            if n == buf.len() {
                break None;
            }
        };
        let reply = |status: &str, body: &str| format!("HTTP/1.1 {}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\ncache-control: no-store\r\n\r\n{}", status, body.len(), body);
        let Some(head) = head else {
            let _ = sock.write_all(reply("400 Bad Request", "bad request").as_bytes()).await;
            continue;
        };
        if browser_navigation_problem(&head, port).is_some() {
            let _ = sock.write_all(reply("400 Bad Request", "not the sign-in redirect").as_bytes()).await;
            continue;
        }
        let target = head.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("");
        match code_from_redirect(target, state) {
            Ok(code) => {
                let _ = sock.write_all(reply("200 OK", "<!doctype html><title>Delphi</title><p>Signed in. You can close this tab and return to the terminal.").as_bytes()).await;
                return Ok(code);
            }
            Err(e @ OauthError::Rejected { .. }) => {
                let _ = sock.write_all(reply("200 OK", "<!doctype html><title>Delphi</title><p>The sign-in was refused. Return to the terminal.").as_bytes()).await;
                return Err(e);
            }
            // A wrong state is a stale tab or a forged request: not this sign-in.
            Err(_) => {
                let _ = sock.write_all(reply("400 Bad Request", "not this sign-in").as_bytes()).await;
            }
        }
    }
}

// ── Stored sign-ins ─────────────────────────────────────────────────────────

/// Delphi's own keychain entries (`service=delphi`).
pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>, String>;
    fn set(&self, name: &str, value: &str) -> Result<(), String>;
    fn delete(&self, name: &str) -> Result<(), String>;
}

pub fn refresh_entry(auth_name: &str) -> String {
    format!("oauth2.{}", auth_name)
}

/// The macOS Keychain or the Linux Secret Service. Values never appear on a command line: they travel on
/// the helper's standard input.
pub struct SystemSecretStore {
    runner: Arc<dyn Runner>,
}

impl SystemSecretStore {
    pub fn new(runner: Arc<dyn Runner>) -> SystemSecretStore {
        SystemSecretStore { runner }
    }
}

/// Token characters only; anything else could be read as syntax by the `security -i` command parser.
fn storable(v: &str) -> bool {
    !v.is_empty() && v.len() <= 8192 && v.chars().all(|c| c.is_ascii_alphanumeric() || "._~+/=:-".contains(c))
}

impl SecretStore for SystemSecretStore {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        let argv: Vec<String> = if cfg!(target_os = "macos") {
            ["security", "find-generic-password", "-s", "delphi", "-a", name, "-w"].iter().map(|s| s.to_string()).collect()
        } else {
            ["secret-tool", "lookup", "service", "delphi", "name", name].iter().map(|s| s.to_string()).collect()
        };
        let out = self.runner.run(&argv, Duration::from_secs(20))?;
        let v = out.stdout.trim_end_matches(['\n', '\r']).to_string();
        Ok(if out.status == 0 && !v.is_empty() { Some(v) } else { None })
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        if !storable(value) || !storable(name) {
            return Err("the token contains characters this keychain writer will not pass to the system tool".into());
        }
        let (program, args, stdin): (&str, Vec<&str>, String) = if cfg!(target_os = "macos") {
            ("security", vec!["-i"], format!("add-generic-password -U -s delphi -a {} -w {}\n", name, value))
        } else {
            ("secret-tool", vec!["store", "--label=Delphi", "service", "delphi", "name", name], value.to_string())
        };
        let mut child = std::process::Command::new(program).args(&args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().map_err(|e| format!("cannot run {}: {}", program, e))?;
        child.stdin.take().expect("piped").write_all(stdin.as_bytes()).map_err(|e| e.to_string())?;
        let status = child.wait().map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err(format!("{} could not store the entry", program)) }
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        let argv: Vec<String> = if cfg!(target_os = "macos") {
            ["security", "delete-generic-password", "-s", "delphi", "-a", name].iter().map(|s| s.to_string()).collect()
        } else {
            ["secret-tool", "clear", "service", "delphi", "name", name].iter().map(|s| s.to_string()).collect()
        };
        self.runner.run(&argv, Duration::from_secs(20)).map(|_| ())
    }
}

/// Asks Google how long an access token has left (the `tokeninfo` endpoint; the token travels in a POST
/// body to Google, the party that issued it, never in a URL). Any failure is `None`, and the caller assumes
/// a short life.
pub struct GoogleTokenInfo {
    http: reqwest::Client,
    handle: tokio::runtime::Handle,
    url: String,
}

impl GoogleTokenInfo {
    pub fn new(http: reqwest::Client, handle: tokio::runtime::Handle) -> GoogleTokenInfo {
        GoogleTokenInfo { http, handle, url: "https://oauth2.googleapis.com/tokeninfo".into() }
    }

    #[cfg(test)]
    fn at(mut self, url: String) -> GoogleTokenInfo {
        self.url = url;
        self
    }
}

impl crate::credential::ExpiryProbe for GoogleTokenInfo {
    fn seconds_left(&self, access_token: &str) -> Option<i64> {
        let ask = async {
            let v = tokio::time::timeout(Duration::from_secs(10), post_form(&self.http, &self.url, &[("access_token", access_token)])).await.ok()?.ok()?;
            v["expires_in"].as_i64().or_else(|| v["expires_in"].as_str().and_then(|s| s.parse().ok()))
        };
        // From a thread of our own, so this is safe whether or not the caller is inside the runtime.
        std::thread::scope(|s| s.spawn(|| self.handle.block_on(ask)).join().ok().flatten())
    }
}

/// How `delphi auth login` talks to the operator.
pub trait LoginUi {
    fn device(&self, grant: &DeviceGrant);
    fn open_url(&self, url: &str);
    /// `--manual`: the pasted redirect URL.
    fn pasted_redirect(&self) -> Option<String>;
}

fn client_secret_of(a: &AuthBlock, store: &dyn SecretStore) -> Result<Option<String>, String> {
    let Some(r) = &a.client_secret else { return Ok(None) };
    let name = r.strip_prefix("keychain:").unwrap_or(r);
    store.get(name)?.map(Some).ok_or_else(|| {
        let add = if cfg!(target_os = "macos") { format!("security add-generic-password -s delphi -a {} -w", name) } else { format!("secret-tool store --label=Delphi service delphi name {}", name) };
        format!("no keychain entry `{}` for the client secret; add it with `{}` (it prompts for the value)", name, add)
    })
}

#[derive(Debug, Clone, Copy)]
pub struct SignInOptions {
    /// Paste the redirect URL instead of listening on loopback.
    pub manual: bool,
    pub pacing: Pacing,
    /// How long to wait for the loopback redirect.
    pub wait: Duration,
}

/// Run the interactive sign-in for `name` and keep the refresh token. Returns the lifetime of the first
/// access token for display.
pub async fn sign_in(client: &reqwest::Client, name: &str, a: &AuthBlock, store: &dyn SecretStore, ui: &dyn LoginUi, o: &SignInOptions) -> Result<i64, String> {
    let SignInOptions { manual, pacing, wait } = *o;
    let ep = endpoints(client, a).await.map_err(|e| e.to_string())?;
    let secret = client_secret_of(a, store)?;
    let t = match a.flow {
        AuthFlow::ClientCredentials => return Err("client_credentials needs no sign-in; it authenticates with its keychain secret".into()),
        AuthFlow::DeviceCode => {
            let g = device_start(client, &ep, a).await.map_err(|e| e.to_string())?;
            ui.device(&g);
            device_poll(client, &ep, a, secret.as_deref(), &g, pacing).await.map_err(|e| e.to_string())?
        }
        AuthFlow::LoopbackPkce => {
            let pkce = Pkce::generate()?;
            let (listener, port) = loopback_bind(a.redirect_port).await.map_err(|e| e.to_string())?;
            let redirect = format!("http://127.0.0.1:{}/callback", port);
            let url = authorization_url(&ep, a, &redirect, &pkce).map_err(|e| e.to_string())?;
            ui.open_url(&url);
            let code = if manual {
                drop(listener);
                let pasted = ui.pasted_redirect().ok_or("no redirect URL was pasted")?;
                code_from_redirect(&pasted, &pkce.state)
            } else {
                loopback_wait(listener, port, &pkce.state, wait).await
            }
            .map_err(|e| e.to_string())?;
            exchange_code(client, &ep, a, secret.as_deref(), &code, &redirect, &pkce.verifier).await.map_err(|e| e.to_string())?
        }
    };
    let refresh = t.refresh.ok_or("the provider issued no refresh token (check that the client allows offline access)")?;
    store.set(&refresh_entry(name), &refresh)?;
    Ok(t.expires_in.unwrap_or(DEFAULT_LIFETIME_SECS))
}

/// The access token for an `oauth2:<name>` credential, refreshed on demand.
pub trait OauthSource: Send + Sync {
    fn access_token(&self, name: &str) -> Result<Secret, CredentialError>;
}

pub struct Oauth2Provider {
    cfg: SharedConfig,
    store: Arc<dyn SecretStore>,
    http: reqwest::Client,
    handle: tokio::runtime::Handle,
    cache: Mutex<HashMap<String, (String, i64)>>,
    /// One refresh at a time: two spans needing a token must not both spend the refresh token.
    flight: Mutex<()>,
    /// Rotated refresh tokens the keychain refused. The provider has already killed the old one, so this
    /// copy is the only live sign-in until the keychain accepts it; it is retried on every refresh and
    /// never leaves memory.
    unsaved: Mutex<HashMap<String, String>>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl Oauth2Provider {
    pub fn new(cfg: SharedConfig, store: Arc<dyn SecretStore>, http: reqwest::Client, handle: tokio::runtime::Handle) -> Oauth2Provider {
        let now: Arc<dyn Fn() -> i64 + Send + Sync> = Arc::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0));
        Oauth2Provider { cfg, store, http, handle, cache: Mutex::new(HashMap::new()), flight: Mutex::new(()), unsaved: Mutex::new(HashMap::new()), now }
    }

    #[cfg(test)]
    fn with_clock(mut self, now: Arc<dyn Fn() -> i64 + Send + Sync>) -> Oauth2Provider {
        self.now = now;
        self
    }

    fn reauth(name: &str) -> CredentialError {
        CredentialError::NeedsReauth { endpoint: name.to_string() }
    }
}

impl OauthSource for Oauth2Provider {
    fn access_token(&self, name: &str) -> Result<Secret, CredentialError> {
        let now = (self.now)();
        if let Some((tok, exp)) = self.cache.lock().unwrap().get(name) {
            if *exp - SKEW_SECS > now {
                return Ok(Secret::new(tok.clone(), Some(*exp)));
            }
        }
        let _one = self.flight.lock().unwrap();
        // Another caller may have refreshed while we waited.
        if let Some((tok, exp)) = self.cache.lock().unwrap().get(name) {
            if *exp - SKEW_SECS > now {
                return Ok(Secret::new(tok.clone(), Some(*exp)));
            }
        }
        let cfg = self.cfg.get();
        let a = cfg.auth.get(name).ok_or_else(|| CredentialError::Failed(format!("no [auth.{}] block", name)))?.clone();
        let secret = client_secret_of(&a, &*self.store).map_err(CredentialError::Failed)?;
        let entry = refresh_entry(name);
        let fetch = async {
            let ep = endpoints(&self.http, &a).await?;
            match a.flow {
                AuthFlow::ClientCredentials => client_credentials(&self.http, &ep, &a, secret.as_deref().unwrap_or("")).await,
                _ => {
                    let held = self.unsaved.lock().unwrap().get(name).cloned();
                    let stored = match held {
                        Some(rt) => Some(rt),
                        None => self.store.get(&entry).map_err(OauthError::Transport)?,
                    };
                    let Some(rt) = stored else { return Err(OauthError::Rejected { code: "invalid_grant".into(), description: "not signed in".into() }) };
                    refresh(&self.http, &ep, &a, secret.as_deref(), &rt).await
                }
            }
        };
        match self.handle.block_on(fetch) {
            Ok(t) => {
                // Providers may rotate the refresh token; the old one is dead the moment this returns. If the
                // keychain will not take the new one, the access token we already hold is still good: keep both
                // in memory rather than throw the sign-in away, and try the keychain again next time.
                let pending = t.refresh.clone().or_else(|| self.unsaved.lock().unwrap().get(name).cloned());
                if let Some(rt) = pending {
                    match self.store.set(&entry, &rt) {
                        Ok(()) => {
                            self.unsaved.lock().unwrap().remove(name);
                        }
                        Err(_) => {
                            self.unsaved.lock().unwrap().insert(name.to_string(), rt);
                        }
                    }
                }
                let exp = now + t.expires_in.unwrap_or(DEFAULT_LIFETIME_SECS);
                self.cache.lock().unwrap().insert(name.to_string(), (t.access.clone(), exp));
                Ok(Secret::new(t.access, Some(exp)))
            }
            Err(e) if e.needs_reauth() => {
                self.cache.lock().unwrap().remove(name);
                self.unsaved.lock().unwrap().remove(name);
                let _ = self.store.delete(&entry);
                Err(Self::reauth(name))
            }
            Err(e) => Err(CredentialError::Failed(format!("oauth2 `{}`: {}", name, e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

    #[derive(Default)]
    struct Mem(Mutex<HashMap<String, String>>);

    impl SecretStore for Mem {
        fn get(&self, n: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(n).cloned())
        }
        fn set(&self, n: &str, v: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(n.into(), v.into());
            Ok(())
        }
        fn delete(&self, n: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(n);
            Ok(())
        }
    }

    type Handler = Box<dyn Fn(&str, &HashMap<String, String>) -> (u16, String) + Send + Sync>;

    /// A scripted identity provider: path + decoded form body → (status, JSON).
    async fn idp(h: Handler) -> (String, Arc<Mutex<Vec<(String, HashMap<String, String>)>>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let log = Arc::new(Mutex::new(Vec::new()));
        let (log2, h) = (log.clone(), Arc::new(h));
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let (log, h) = (log2.clone(), h.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let head_end = loop {
                        let n = s.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let path = head.lines().next().unwrap_or("").split_whitespace().nth(1).unwrap_or("").to_string();
                    let want: usize = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse().ok())).unwrap_or(0);
                    while buf.len() < head_end + want {
                        let n = s.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let params = query_of(&String::from_utf8_lossy(&buf[head_end..]));
                    log.lock().unwrap().push((path.clone(), params.clone()));
                    let (status, body) = h(&path, &params);
                    let out = format!("HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", status, body.len(), body);
                    let _ = s.write_all(out.as_bytes()).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        (base, log)
    }

    fn block(base: &str, flow: &str, extra: &str) -> AuthBlock {
        let text = format!("[auth.x]\nflow = \"{}\"\ntoken_url = \"{}/token\"\ndevice_authorization_url = \"{}/device\"\nauthorization_url = \"{}/authorize\"\nclient_id = \"cid\"\nscopes = [\"a\", \"b c\"]\n{}", flow, base, base, base, extra);
        Config::parse(&text).unwrap().auth["x"].clone()
    }

    fn client() -> reqwest::Client {
        crate::http::build_client()
    }

    fn json(v: serde_json::Value) -> (u16, String) {
        (200, v.to_string())
    }

    #[test]
    fn config_rejects_unsafe_auth_blocks() {
        let bad = "[auth.a]\nflow = \"client_credentials\"\ntoken_url = \"http://idp.example.com/t\"\nclient_id = \"\"\nclient_secret = \"hunter2\"\n[auth.b]\nflow = \"device_code\"\ntoken_url = \"https://i/t\"\nclient_id = \"c\"\n[[endpoints]]\nid = \"e\"\nfamily = \"claude\"\npath = \"api\"\ncredential = \"oauth2:ghost\"\n";
        let errs = Config::parse(bad).unwrap_err().join("\n");
        for needle in ["must be https", "needs a client_id", "client_secret must be a keychain reference", "device_code flow needs", "no [auth.ghost] block"] {
            assert!(errs.contains(needle), "missing `{}` in:\n{}", needle, errs);
        }
    }

    #[tokio::test]
    async fn the_device_flow_polls_through_pending_and_slow_down() {
        let polls = Arc::new(AtomicUsize::new(0));
        let p2 = polls.clone();
        let (base, log) = idp(Box::new(move |path, form| match path {
            "/device" => json(serde_json::json!({"device_code": "DC", "user_code": "ABCD-1234", "verification_uri": "https://idp/activate", "expires_in": 60, "interval": 0})),
            "/token" => match p2.fetch_add(1, Ordering::SeqCst) {
                0 => (400, r#"{"error":"authorization_pending"}"#.into()),
                1 => (400, r#"{"error":"slow_down"}"#.into()),
                _ => {
                    assert_eq!(form["device_code"], "DC");
                    json(serde_json::json!({"access_token": "AT", "refresh_token": "RT", "expires_in": 1800}))
                }
            },
            _ => (404, "{}".into()),
        }))
        .await;
        let a = block(&base, "device_code", "");
        let ep = endpoints(&client(), &a).await.unwrap();
        let g = device_start(&client(), &ep, &a).await.unwrap();
        assert_eq!((g.user_code.as_str(), g.verification_uri.as_str()), ("ABCD-1234", "https://idp/activate"));
        let t = device_poll(&client(), &ep, &a, None, &g, Pacing::INSTANT).await.unwrap();
        assert_eq!((t.access.as_str(), t.refresh.as_deref(), t.expires_in), ("AT", Some("RT"), Some(1800)));
        assert_eq!(polls.load(Ordering::SeqCst), 3);
        let start = &log.lock().unwrap()[0].1;
        assert_eq!((start["client_id"].as_str(), start["scope"].as_str()), ("cid", "a b c"));
    }

    #[tokio::test]
    async fn a_denied_or_expired_device_flow_stops() {
        let (base, _) = idp(Box::new(|path, _| if path == "/token" { (400, r#"{"error":"access_denied","error_description":"no"}"#.into()) } else { (404, "{}".into()) })).await;
        let a = block(&base, "device_code", "");
        let ep = endpoints(&client(), &a).await.unwrap();
        let g = DeviceGrant { device_code: "DC".into(), user_code: "U".into(), verification_uri: "v".into(), verification_uri_complete: None, expires_in: 60, interval: 0 };
        match device_poll(&client(), &ep, &a, None, &g, Pacing::INSTANT).await.unwrap_err() {
            OauthError::Rejected { code, .. } => assert_eq!(code, "access_denied"),
            e => panic!("{:?}", e),
        }
    }

    fn grant() -> DeviceGrant {
        DeviceGrant { device_code: "DC".into(), user_code: "U".into(), verification_uri: "v".into(), verification_uri_complete: None, expires_in: 60, interval: 0 }
    }

    #[tokio::test]
    async fn device_polling_sends_the_client_secret() {
        let (base, log) = idp(Box::new(|_, _| json(serde_json::json!({"access_token": "AT", "refresh_token": "RT"})))).await;
        let a = block(&base, "device_code", "");
        let ep = endpoints(&client(), &a).await.unwrap();
        device_poll(&client(), &ep, &a, Some("S3CRET"), &grant(), Pacing::INSTANT).await.unwrap();
        let sent = log.lock().unwrap()[0].1.clone();
        assert_eq!((sent["client_secret"].as_str(), sent["client_id"].as_str()), ("S3CRET", "cid"));
    }

    #[tokio::test]
    async fn device_polling_survives_a_transient_failure_but_not_a_run_of_them() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let (base, _) = idp(Box::new(move |_, _| match n2.fetch_add(1, Ordering::SeqCst) {
            0 => (503, "unavailable".into()),
            1 => (400, r#"{"error":"authorization_pending"}"#.into()),
            2 => (502, "bad gateway".into()),
            _ => json(serde_json::json!({"access_token": "AT"})),
        }))
        .await;
        let a = block(&base, "device_code", "");
        let ep = endpoints(&client(), &a).await.unwrap();
        let t = device_poll(&client(), &ep, &a, None, &grant(), Pacing::INSTANT).await.unwrap();
        assert_eq!(t.access, "AT");
        // A provider that stays down ends the sign-in with the real error.
        let (base, log) = idp(Box::new(|_, _| (503, "down".into()))).await;
        let a = block(&base, "device_code", "");
        let ep = endpoints(&client(), &a).await.unwrap();
        let e = device_poll(&client(), &ep, &a, None, &grant(), Pacing::INSTANT).await.unwrap_err();
        assert!(matches!(e, OauthError::Http { status: 503, .. }), "{:?}", e);
        assert_eq!(log.lock().unwrap().len(), MAX_TRANSIENT_POLLS as usize);
        // A connection that cannot be made at all is transient too, and ends the same way.
        let a = block("http://127.0.0.1:1", "device_code", "");
        let ep = endpoints(&client(), &a).await.unwrap();
        assert!(matches!(device_poll(&client(), &ep, &a, None, &grant(), Pacing::INSTANT).await.unwrap_err(), OauthError::Transport(_)));
    }

    #[tokio::test]
    async fn discovery_must_name_the_configured_issuer() {
        let (base, _) = idp(Box::new(|_, _| json(serde_json::json!({"issuer": "https://accounts.evil.example", "token_endpoint": "https://i/t"})))).await;
        let mut a = block(&base, "loopback_pkce", "");
        a.token_url = None;
        a.authorization_url = None;
        a.issuer = Some(base.clone());
        let err = endpoints(&client(), &a).await.unwrap_err();
        assert!(matches!(err, OauthError::Malformed(ref m) if m.contains("different issuer")), "{:?}", err);
        // No issuer in the document is as bad.
        let (base, _) = idp(Box::new(|_, _| json(serde_json::json!({"token_endpoint": "https://i/t", "authorization_endpoint": "https://i/a"})))).await;
        a.issuer = Some(base);
        let err = endpoints(&client(), &a).await.unwrap_err();
        assert!(matches!(err, OauthError::Malformed(ref m) if m.contains("no issuer")), "{:?}", err);
    }

    /// A discovery server whose document names itself (plus `suffix`, e.g. a trailing slash) as its issuer.
    async fn discovery_server(suffix: &str, token_endpoint: &str) -> String {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let doc = serde_json::json!({"issuer": format!("{}{}", base, suffix), "token_endpoint": token_endpoint, "authorization_endpoint": "https://i/a", "device_authorization_endpoint": "https://i/d"}).to_string();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let doc = doc.clone();
                tokio::spawn(async move {
                    let mut tmp = [0u8; 2048];
                    let _ = s.read(&mut tmp).await;
                    let _ = s.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", doc.len(), doc).as_bytes()).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        base
    }

    #[tokio::test]
    async fn discovery_accepts_the_matching_issuer_with_or_without_a_trailing_slash() {
        for suffix in ["", "/"] {
            let base = discovery_server(suffix, "https://i/t").await;
            let mut a = block(&base, "loopback_pkce", "");
            a.token_url = None;
            a.authorization_url = None;
            a.issuer = Some(base);
            assert_eq!(endpoints(&client(), &a).await.unwrap().token, "https://i/t");
        }
    }

    #[tokio::test]
    async fn discovery_fills_what_the_block_omits_and_refuses_plain_http_elsewhere() {
        let base = discovery_server("", "http://evil.example.com/t").await;
        let mut a = block(&base, "device_code", "");
        a.token_url = None;
        a.issuer = Some(base.clone());
        let err = endpoints(&client(), &a).await.unwrap_err();
        assert!(matches!(err, OauthError::Malformed(ref m) if m.contains("not https")), "{:?}", err);
    }

    #[tokio::test]
    async fn pkce_login_through_the_loopback_listener() {
        let (base, log) = idp(Box::new(|path, form| match path {
            "/token" => {
                assert_eq!((form["grant_type"].as_str(), form["code"].as_str()), ("authorization_code", "THE-CODE"));
                json(serde_json::json!({"access_token": "AT", "refresh_token": "RT-1", "expires_in": 3600}))
            }
            _ => (404, "{}".into()),
        }))
        .await;
        let a = block(&base, "loopback_pkce", "");
        let store = Mem::default();
        struct Ui(Mutex<Option<String>>);
        impl LoginUi for Ui {
            fn device(&self, _: &DeviceGrant) {}
            fn open_url(&self, url: &str) {
                *self.0.lock().unwrap() = Some(url.to_string());
            }
            fn pasted_redirect(&self) -> Option<String> {
                None
            }
        }
        let ui = Arc::new(Ui(Mutex::new(None)));
        // The "browser": waits for the URL, then follows the redirect with a bad request first, then the real one.
        let ui2 = ui.clone();
        let browser = tokio::spawn(async move {
            let url = loop {
                if let Some(u) = ui2.0.lock().unwrap().clone() {
                    break u;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            };
            let q = query_of(&url);
            assert_eq!((q["code_challenge_method"].as_str(), q["response_type"].as_str()), ("S256", "code"));
            assert_eq!(q["state"].len(), 43, "256 bits of state");
            let redirect = q["redirect_uri"].clone();
            let port: u16 = redirect.split(':').next_back().unwrap().split('/').next().unwrap().parse().unwrap();
            let send = |host: String, target: String, extra: &'static str| async move {
                let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                s.write_all(format!("GET {} HTTP/1.1\r\nHost: {}\r\n{}\r\n", target, host, extra).as_bytes()).await.unwrap();
                let mut out = String::new();
                s.read_to_string(&mut out).await.unwrap();
                out.lines().next().unwrap_or("").to_string()
            };
            let me = format!("127.0.0.1:{}", port);
            assert!(send("evil.example.com".into(), "/callback?code=X&state=y".into(), "").await.contains("400"), "rebinding host");
            assert!(send(me.clone(), "/callback?code=STOLEN&state=wrong".into(), "").await.contains("400"), "wrong state");
            assert!(send(me.clone(), format!("/callback?code=STOLEN&state={}", q["state"]), "Sec-Fetch-Mode: no-cors\r\n").await.contains("400"), "script fetch");
            assert!(send(me.clone(), format!("/callback?code=STOLEN&state={}", q["state"]), "Origin: https://evil.example\r\n").await.contains("400"), "has an Origin");
            assert!(send(me, format!("/callback?code=THE-CODE&state={}", q["state"]), "Sec-Fetch-Mode: navigate\r\nSec-Fetch-Dest: document\r\nSec-Fetch-Site: cross-site\r\n").await.contains("200"));
        });
        let secs = sign_in(&client(), "x", &a, &store, &*ui, &SignInOptions { manual: false, pacing: Pacing::INSTANT, wait: Duration::from_secs(10) }).await.unwrap();
        browser.await.unwrap();
        assert_eq!(secs, 3600);
        assert_eq!(store.get("oauth2.x").unwrap().as_deref(), Some("RT-1"));
        let exch = log.lock().unwrap().last().unwrap().1.clone();
        assert!(!exch["code_verifier"].is_empty() && exch["redirect_uri"].starts_with("http://127.0.0.1:"), "the verifier proves the same client");
    }

    #[test]
    fn extra_authorization_parameters_are_added_but_cannot_replace_the_safe_ones() {
        let mut a = block("http://127.0.0.1:1", "loopback_pkce", "");
        a.authorization_params = [("access_type".to_string(), "offline".to_string()), ("prompt".to_string(), "consent".to_string()), ("state".to_string(), "attacker".to_string()), ("code_challenge".to_string(), "evil".to_string())].into();
        let ep = Endpoints { token: "http://127.0.0.1:1/token".into(), device: None, authorization: Some("http://127.0.0.1:1/authorize".into()) };
        let pk = Pkce::generate().unwrap();
        let url = authorization_url(&ep, &a, "http://127.0.0.1:9/callback", &pk).unwrap();
        let q = query_of(&url);
        assert_eq!((q["access_type"].as_str(), q["prompt"].as_str()), ("offline", "consent"));
        assert_eq!((q["state"].as_str(), q["code_challenge"].as_str()), (pk.state.as_str(), pk.challenge.as_str()));
    }

    #[test]
    fn only_https_or_a_true_loopback_host_is_an_acceptable_endpoint() {
        for ok in ["https://idp.example.com/t", "http://127.0.0.1:8080/t", "http://localhost/t"] {
            assert!(url_ok(ok), "{}", ok);
        }
        for bad in ["http://127.0.0.1.evil.com/t", "http://localhost.evil.com/t", "http://idp.example.com/t", "ftp://x"] {
            assert!(!url_ok(bad), "{}", bad);
        }
    }

    #[test]
    fn a_pasted_redirect_is_checked_against_state() {
        assert_eq!(code_from_redirect("http://127.0.0.1:9/callback?code=C%2B1&state=S", "S").unwrap(), "C+1");
        assert!(code_from_redirect("http://127.0.0.1:9/callback?code=C&state=other", "S").is_err());
        assert!(matches!(code_from_redirect("?error=access_denied&state=S", "S"), Err(OauthError::Rejected { .. })));
    }

    fn provider(base: &str, flow: &str, extra: &str, store: Arc<Mem>, now: Arc<AtomicI64>) -> Oauth2Provider {
        let cfg = Config::parse(&format!("[auth.x]\nflow = \"{}\"\ntoken_url = \"{}/token\"\ndevice_authorization_url = \"{}/device\"\nclient_id = \"cid\"\n{}", flow, base, base, extra)).unwrap();
        Oauth2Provider::new(SharedConfig::new(cfg), store, client(), tokio::runtime::Handle::current()).with_clock(Arc::new(move || now.load(Ordering::SeqCst)))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_provider_refreshes_before_expiry_and_stores_a_rotated_token() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let (base, log) = idp(Box::new(move |_, form| {
            let i = n2.fetch_add(1, Ordering::SeqCst);
            assert_eq!(form["grant_type"], "refresh_token");
            json(serde_json::json!({"access_token": format!("AT{}", i), "refresh_token": format!("RT{}", i + 2), "expires_in": 300}))
        }))
        .await;
        let store = Arc::new(Mem::default());
        store.set("oauth2.x", "RT1").unwrap();
        let now = Arc::new(AtomicI64::new(1_000));
        let p = Arc::new(provider(&base, "device_code", "", store.clone(), now.clone()));
        let run = |p: Arc<Oauth2Provider>| tokio::task::spawn_blocking(move || p.access_token("x"));
        let a = run(p.clone()).await.unwrap().unwrap();
        assert_eq!((a.expose(), a.expires_at), ("AT0", Some(1_300)));
        assert_eq!(store.get("oauth2.x").unwrap().as_deref(), Some("RT2"), "the rotated refresh token replaces the old one");
        assert_eq!(run(p.clone()).await.unwrap().unwrap().expose(), "AT0", "cached while valid");
        now.store(1_250, Ordering::SeqCst); // within the 60 s skew of expiry
        let b = run(p.clone()).await.unwrap().unwrap();
        assert_eq!(b.expose(), "AT1");
        assert_eq!(log.lock().unwrap()[1].1["refresh_token"], "RT2");
        assert_eq!(n.load(Ordering::SeqCst), 2);
    }

    /// A keychain that refuses writes while `locked`.
    struct Lockable {
        inner: Mem,
        locked: std::sync::atomic::AtomicBool,
    }

    impl SecretStore for Lockable {
        fn get(&self, n: &str) -> Result<Option<String>, String> {
            self.inner.get(n)
        }
        fn set(&self, n: &str, v: &str) -> Result<(), String> {
            if self.locked.load(Ordering::SeqCst) {
                return Err("keychain is locked".into());
            }
            self.inner.set(n, v)
        }
        fn delete(&self, n: &str) -> Result<(), String> {
            self.inner.delete(n)
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_rotated_refresh_token_the_keychain_refuses_does_not_lose_the_sign_in() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let (base, log) = idp(Box::new(move |_, _| {
            let i = n2.fetch_add(1, Ordering::SeqCst);
            json(serde_json::json!({"access_token": format!("AT{}", i), "refresh_token": format!("RT{}", i + 2), "expires_in": 300}))
        }))
        .await;
        let store = Arc::new(Lockable { inner: Mem::default(), locked: std::sync::atomic::AtomicBool::new(true) });
        store.inner.set("oauth2.x", "RT1").unwrap();
        let now = Arc::new(AtomicI64::new(1_000));
        let cfg = Config::parse(&format!("[auth.x]\nflow = \"device_code\"\ntoken_url = \"{}/token\"\ndevice_authorization_url = \"{}/device\"\nclient_id = \"cid\"\n", base, base)).unwrap();
        let now2 = now.clone();
        let p = Arc::new(Oauth2Provider::new(SharedConfig::new(cfg), store.clone(), client(), tokio::runtime::Handle::current()).with_clock(Arc::new(move || now2.load(Ordering::SeqCst))));
        let run = |p: Arc<Oauth2Provider>| tokio::task::spawn_blocking(move || p.access_token("x"));
        // The write fails, but the token the provider just issued is returned, not an error.
        assert_eq!(run(p.clone()).await.unwrap().unwrap().expose(), "AT0");
        assert_eq!(store.inner.get("oauth2.x").unwrap().as_deref(), Some("RT1"), "the keychain still holds the dead token");
        // The next refresh uses the rotated token from memory, not the dead one in the keychain.
        now.store(1_250, Ordering::SeqCst);
        assert_eq!(run(p.clone()).await.unwrap().unwrap().expose(), "AT1");
        assert_eq!(log.lock().unwrap()[1].1["refresh_token"], "RT2");
        // Once the keychain accepts writes, the newest token lands there.
        store.locked.store(false, Ordering::SeqCst);
        now.store(1_500, Ordering::SeqCst);
        assert_eq!(run(p.clone()).await.unwrap().unwrap().expose(), "AT2");
        assert_eq!(log.lock().unwrap()[2].1["refresh_token"], "RT3");
        assert_eq!(store.inner.get("oauth2.x").unwrap().as_deref(), Some("RT4"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_revoked_refresh_token_means_sign_in_again_but_a_network_error_does_not() {
        let (base, _) = idp(Box::new(|_, _| (400, r#"{"error":"invalid_grant","error_description":"revoked"}"#.into()))).await;
        let store = Arc::new(Mem::default());
        store.set("oauth2.x", "RT1").unwrap();
        let p = Arc::new(provider(&base, "device_code", "", store.clone(), Arc::new(AtomicI64::new(0))));
        let p1 = p.clone();
        let e = tokio::task::spawn_blocking(move || p1.access_token("x")).await.unwrap().unwrap_err();
        assert_eq!(e, CredentialError::NeedsReauth { endpoint: "x".into() });
        assert_eq!(store.get("oauth2.x").unwrap(), None, "the dead token is removed so it is not retried");
        // Never signed in: same answer.
        let p2 = p.clone();
        assert!(matches!(tokio::task::spawn_blocking(move || p2.access_token("x")).await.unwrap(), Err(CredentialError::NeedsReauth { .. })));
        // A provider that is down is a transient failure and keeps the sign-in.
        store.set("oauth2.x", "RT9").unwrap();
        let down = Arc::new(provider("http://127.0.0.1:1", "device_code", "", store.clone(), Arc::new(AtomicI64::new(0))));
        let e = tokio::task::spawn_blocking(move || down.access_token("x")).await.unwrap().unwrap_err();
        assert!(matches!(e, CredentialError::Failed(_)), "{:?}", e);
        assert_eq!(store.get("oauth2.x").unwrap().as_deref(), Some("RT9"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn client_credentials_uses_the_keychain_secret_and_never_stores_a_refresh_token() {
        let (base, log) = idp(Box::new(|_, f| {
            assert_eq!((f["grant_type"].as_str(), f["client_secret"].as_str(), f["audience"].as_str()), ("client_credentials", "S3CRET", "gw"));
            json(serde_json::json!({"access_token": "AT", "expires_in": 120}))
        }))
        .await;
        let store = Arc::new(Mem::default());
        store.set("corp-secret", "S3CRET").unwrap();
        let p = Arc::new(provider(&base, "client_credentials", "client_secret = \"keychain:corp-secret\"\naudience = \"gw\"\nscopes = [\"llm.invoke\"]", store.clone(), Arc::new(AtomicI64::new(10))));
        let s = tokio::task::spawn_blocking(move || p.access_token("x")).await.unwrap().unwrap();
        assert_eq!((s.expose(), s.expires_at), ("AT", Some(130)));
        assert_eq!(log.lock().unwrap()[0].1["scope"], "llm.invoke");
        assert_eq!(store.get("oauth2.x").unwrap(), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn token_info_reports_the_real_lifetime_and_nothing_on_failure() {
        use crate::credential::ExpiryProbe;
        let (base, log) = idp(Box::new(|_, f| if f["access_token"] == "good" { json(serde_json::json!({"expires_in": "1742", "aud": "x"})) } else { (400, r#"{"error_description":"Invalid Value"}"#.into()) })).await;
        let probe = Arc::new(GoogleTokenInfo::new(client(), tokio::runtime::Handle::current()).at(format!("{}/tokeninfo", base)));
        let p = probe.clone();
        let (good, bad) = tokio::task::spawn_blocking(move || (p.seconds_left("good"), p.seconds_left("bad"))).await.unwrap();
        assert_eq!((good, bad), (Some(1742), None));
        assert_eq!(log.lock().unwrap()[0].0, "/tokeninfo", "the token goes in the body, not the path");
        let down = Arc::new(GoogleTokenInfo::new(client(), tokio::runtime::Handle::current()).at("http://127.0.0.1:1/tokeninfo".into()));
        assert_eq!(tokio::task::spawn_blocking(move || down.seconds_left("good")).await.unwrap(), None);
    }

    #[test]
    fn only_plain_token_characters_are_passed_to_the_keychain_tool() {
        assert!(storable("1//0gAbC-d_e.f~g+h/i=j:k"));
        for bad in ["", "a b", "a\"b", "a;b", "a\nb", "a'b", "a\\b", "$(x)"] {
            assert!(!storable(bad), "{:?}", bad);
        }
    }

    #[test]
    fn tokens_never_print() {
        let t = Tokens { access: "AT-SECRET".into(), refresh: Some("RT-SECRET".into()), expires_in: Some(5) };
        let s = format!("{:?}", t);
        assert!(!s.contains("SECRET"), "{}", s);
    }
}
