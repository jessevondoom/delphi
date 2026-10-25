//! Credential sources and resolution (`specs/009` §4).
//!
//! Invariants enforced here:
//! 1. **Never on disk:** nothing is written to a file. The only writes are Delphi's own keychain entries
//!    for OAuth refresh tokens (`oauth2.rs`); every other keychain entry is only ever *read*.
//! 2. **Never another tool's:** secrets come from Delphi's own keychain entries (`service=delphi`),
//!    the environment, a configured helper, or the official `aws` / `gcloud` CLIs asked to mint a
//!    token. Delphi never opens another tool's session or cache files.
//! 3. **Never in agent context:** values are returned to the transport layer only.
//! 4. **Literal redaction:** every resolved value is passed to the `register` callback so the event
//!    redactor masks it for its lifetime. Error messages never contain helper output or stderr.
//! 5. **Expiry mid-task:** a credential that can no longer be refreshed yields
//!    [`CredentialError::NeedsReauth`]; callers pause and never fail over outside the policy.

use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialRef {
    Keychain(String),
    Env(String),
    Aws(String),
    GcloudAdc,
    GcloudSa(String),
    Oauth2(String),
    Command(Vec<String>),
    HarnessLogin,
}

impl CredentialRef {
    pub fn parse(s: &str) -> Result<CredentialRef, String> {
        let nonempty = |v: &str, what: &str| if v.trim().is_empty() { Err(format!("a `{}:` credential needs a {}", s.split(':').next().unwrap_or(""), what)) } else { Ok(v.to_string()) };
        if s == "gcloud-adc" {
            return Ok(CredentialRef::GcloudAdc);
        }
        if s == "harness-login" {
            return Ok(CredentialRef::HarnessLogin);
        }
        match s.split_once(':') {
            Some(("keychain", v)) => Ok(CredentialRef::Keychain(nonempty(v, "name")?)),
            Some(("env", v)) => Ok(CredentialRef::Env(nonempty(v, "variable name")?)),
            Some(("aws", v)) => Ok(CredentialRef::Aws(nonempty(v, "profile")?)),
            Some(("gcloud-sa", v)) => Ok(CredentialRef::GcloudSa(nonempty(v, "keychain reference")?)),
            Some(("oauth2", v)) => Ok(CredentialRef::Oauth2(nonempty(v, "[auth.<name>] block name")?)),
            Some(("command", v)) => {
                let argv = parse_argv(v).map_err(|e| format!("credential `command:` {}", e))?;
                Ok(CredentialRef::Command(argv))
            }
            _ => Err("unknown credential source (expected keychain:, env:, aws:, gcloud-adc, gcloud-sa:, oauth2:, command: or harness-login); the value is not shown in case it is a pasted secret".to_string()),
        }
    }
}

/// `["helper","--flag"]` as JSON, or whitespace-separated words with double quotes. Never a shell.
fn parse_argv(v: &str) -> Result<Vec<String>, String> {
    let v = v.trim();
    let argv: Vec<String> = if v.starts_with('[') {
        serde_json::from_str(v).map_err(|e| format!("argv is not a JSON string array: {}", e))?
    } else {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_quote = false;
        let mut has = false;
        for c in v.chars() {
            match c {
                '"' => {
                    in_quote = !in_quote;
                    has = true;
                }
                c if c.is_whitespace() && !in_quote => {
                    if has {
                        out.push(std::mem::take(&mut cur));
                        has = false;
                    }
                }
                c => {
                    cur.push(c);
                    has = true;
                }
            }
        }
        if in_quote {
            return Err("unterminated quote in argv".into());
        }
        if has {
            out.push(cur);
        }
        out
    };
    if argv.is_empty() || argv[0].is_empty() {
        return Err("argv is empty".into());
    }
    Ok(argv)
}

/// A bearer token or API key. `Debug` never shows the value.
#[derive(Clone)]
pub struct Secret {
    value: String,
    pub expires_at: Option<i64>,
}

impl Secret {
    pub fn new(value: String, expires_at: Option<i64>) -> Secret {
        Secret { value, expires_at }
    }

    /// The raw value; only transport code should call this.
    pub fn expose(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret(<redacted>, expires_at: {:?})", self.expires_at)
    }
}

#[derive(Clone)]
pub struct AwsCredentials {
    pub access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    pub expires_at: Option<i64>,
}

impl AwsCredentials {
    pub fn new(access_key_id: String, secret_access_key: String, session_token: Option<String>, expires_at: Option<i64>) -> AwsCredentials {
        AwsCredentials { access_key_id, secret_access_key, session_token, expires_at }
    }

    pub fn secret_access_key(&self) -> &str {
        &self.secret_access_key
    }

    pub fn session_token(&self) -> Option<&str> {
        self.session_token.as_deref()
    }
}

impl fmt::Debug for AwsCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AwsCredentials(<redacted>, expires_at: {:?})", self.expires_at)
    }
}

#[derive(Debug, Clone)]
pub enum Resolved {
    Token(Secret),
    Aws(AwsCredentials),
    /// `harness-login`: the official CLI authenticates itself; Delphi holds nothing.
    None,
}

impl Resolved {
    pub fn expires_at(&self) -> Option<i64> {
        match self {
            Resolved::Token(s) => s.expires_at,
            Resolved::Aws(a) => a.expires_at,
            Resolved::None => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CredentialError {
    NotFound(String),
    Failed(String),
    Unsupported(String),
    /// The credential expired and could not be refreshed: pause and ask the operator to sign in again.
    NeedsReauth { endpoint: String },
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialError::NotFound(m) | CredentialError::Failed(m) | CredentialError::Unsupported(m) => write!(f, "{}", m),
            CredentialError::NeedsReauth { endpoint } => write!(f, "re-authenticate {}", endpoint),
        }
    }
}

impl std::error::Error for CredentialError {}

pub struct RunOutput {
    pub status: i32,
    pub stdout: String,
    /// Stderr is never surfaced, only its size.
    pub stderr_len: usize,
}

pub trait Runner: Send + Sync {
    fn run(&self, argv: &[String], timeout: Duration) -> Result<RunOutput, String>;
}

/// Runs the program directly (no shell) with stdin closed and a hard timeout.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, argv: &[String], timeout: Duration) -> Result<RunOutput, String> {
        use std::io::Read;
        use std::process::{Command, Stdio};
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run `{}`: {}", argv[0], e.kind()))?;
        let mut out = child.stdout.take().unwrap();
        let mut err = child.stderr.take().unwrap();
        let t_out = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = out.read_to_string(&mut s);
            s
        });
        let t_err = std::thread::spawn(move || {
            let mut s = Vec::new();
            let _ = err.read_to_end(&mut s);
            s.len()
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("`{}` timed out after {}s", argv[0], timeout.as_secs()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(15)),
                Err(e) => return Err(format!("waiting for `{}`: {}", argv[0], e.kind())),
            }
        };
        Ok(RunOutput { status: status.code().unwrap_or(-1), stdout: t_out.join().unwrap_or_default(), stderr_len: t_err.join().unwrap_or(0) })
    }
}

type EnvFn = dyn Fn(&str) -> Option<String> + Send + Sync;
type RegisterFn = dyn Fn(&str) + Send + Sync;
type NowFn = dyn Fn() -> i64 + Send + Sync;

pub struct Resolver {
    runner: Arc<dyn Runner>,
    env: Arc<EnvFn>,
    register: Arc<RegisterFn>,
    now: Arc<NowFn>,
    timeout: Duration,
    oauth: Option<Arc<dyn crate::oauth2::OauthSource>>,
    gcloud_expiry: Option<Arc<dyn ExpiryProbe>>,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Deserialize)]
struct HelperOutput {
    token: String,
    #[serde(default)]
    expires_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AwsExport {
    access_key_id: String,
    secret_access_key: String,
    #[serde(default)]
    session_token: Option<String>,
    #[serde(default)]
    expiration: Option<String>,
}

/// What to assume about a gcloud token when its real lifetime cannot be learned. `gcloud` does not print
/// one, and a token is usually good for an hour, but a short guess only costs another `gcloud` call while
/// a long one gets an expired token sent to the provider.
const GCLOUD_UNKNOWN_LIFETIME_SECS: i64 = 5 * 60;

/// Learns how many seconds an access token has left, from the issuer.
pub trait ExpiryProbe: Send + Sync {
    fn seconds_left(&self, access_token: &str) -> Option<i64>;
}

impl Resolver {
    pub fn new(runner: Arc<dyn Runner>, env: Arc<EnvFn>, register: Arc<RegisterFn>, now: Arc<NowFn>) -> Resolver {
        Resolver { runner, env, register, now, timeout: Duration::from_secs(20), oauth: None, gcloud_expiry: None }
    }

    /// Ask this probe how long a `gcloud-adc` token lives, instead of guessing.
    pub fn with_gcloud_expiry(mut self, probe: Arc<dyn ExpiryProbe>) -> Resolver {
        self.gcloud_expiry = Some(probe);
        self
    }

    /// Serve `oauth2:<name>` credentials from this source.
    pub fn with_oauth(mut self, oauth: Arc<dyn crate::oauth2::OauthSource>) -> Resolver {
        self.oauth = Some(oauth);
        self
    }

    pub fn runner(&self) -> Arc<dyn Runner> {
        self.runner.clone()
    }

    /// Real process environment, real subprocesses; `register` receives every resolved secret.
    pub fn system(register: Arc<RegisterFn>) -> Resolver {
        Resolver::new(Arc::new(SystemRunner), Arc::new(|k| std::env::var(k).ok()), register, Arc::new(unix_now))
    }

    pub fn now(&self) -> i64 {
        (self.now)()
    }

    fn secret(&self, value: String, expires_at: Option<i64>) -> Resolved {
        (self.register)(&value);
        Resolved::Token(Secret::new(value, expires_at))
    }

    fn run(&self, argv: &[String], what: &str) -> Result<String, CredentialError> {
        let out = self.runner.run(argv, self.timeout).map_err(CredentialError::Failed)?;
        if out.status != 0 {
            // Stderr may hold anything; report only that it was produced.
            return Err(CredentialError::Failed(format!("{} failed (exit status {}, {} bytes on stderr suppressed)", what, out.status, out.stderr_len)));
        }
        Ok(out.stdout)
    }

    pub fn resolve(&self, r: &CredentialRef) -> Result<Resolved, CredentialError> {
        match r {
            CredentialRef::HarnessLogin => Ok(Resolved::None),
            CredentialRef::Env(var) => match (self.env)(var) {
                Some(v) if !v.is_empty() => Ok(self.secret(v, None)),
                _ => Err(CredentialError::NotFound(format!("environment variable {} is not set", var))),
            },
            CredentialRef::Keychain(name) => {
                let argv: Vec<String> = if cfg!(target_os = "macos") {
                    ["security", "find-generic-password", "-s", "delphi", "-a", name, "-w"].iter().map(|s| s.to_string()).collect()
                } else {
                    ["secret-tool", "lookup", "service", "delphi", "name", name].iter().map(|s| s.to_string()).collect()
                };
                let out = self.runner.run(&argv, self.timeout).map_err(CredentialError::Failed)?;
                let value = out.stdout.trim_end_matches(['\n', '\r']).to_string();
                if out.status != 0 || value.is_empty() {
                    return Err(CredentialError::NotFound(format!("no keychain entry `{}` for service `delphi` (add it with your keychain tool; Delphi never writes secrets to files)", name)));
                }
                Ok(self.secret(value, None))
            }
            CredentialRef::Command(argv) => {
                let stdout = self.run(argv, "credential helper")?;
                let parsed: HelperOutput =
                    serde_json::from_str(stdout.trim()).map_err(|_| CredentialError::Failed("credential helper output was not {\"token\": \"...\", \"expires_at\": \"...\"}".into()))?;
                if parsed.token.is_empty() {
                    return Err(CredentialError::Failed("credential helper returned an empty token".into()));
                }
                let expires = match parsed.expires_at.as_deref() {
                    None => None,
                    Some(t) => Some(delphi_core::time::parse_rfc3339(t).ok_or_else(|| CredentialError::Failed("credential helper expires_at is not RFC 3339".into()))?),
                };
                Ok(self.secret(parsed.token, expires))
            }
            CredentialRef::Aws(profile) => {
                let argv: Vec<String> = ["aws", "configure", "export-credentials", "--profile", profile, "--format", "process"].iter().map(|s| s.to_string()).collect();
                let stdout = self.run(&argv, "`aws configure export-credentials` (run `aws sso login` if your session expired)")?;
                let e: AwsExport = serde_json::from_str(stdout.trim()).map_err(|_| CredentialError::Failed("`aws configure export-credentials` output was not the expected JSON".into()))?;
                (self.register)(&e.access_key_id);
                (self.register)(&e.secret_access_key);
                if let Some(t) = &e.session_token {
                    (self.register)(t);
                }
                let expires = e.expiration.as_deref().and_then(delphi_core::time::parse_rfc3339);
                Ok(Resolved::Aws(AwsCredentials::new(e.access_key_id, e.secret_access_key, e.session_token, expires)))
            }
            CredentialRef::GcloudAdc => {
                let argv: Vec<String> = ["gcloud", "auth", "application-default", "print-access-token"].iter().map(|s| s.to_string()).collect();
                let stdout = self.run(&argv, "`gcloud auth application-default print-access-token` (run `gcloud auth application-default login`)")?;
                let token = stdout.trim().to_string();
                if token.is_empty() {
                    return Err(CredentialError::Failed("gcloud returned an empty access token".into()));
                }
                let left = self.gcloud_expiry.as_ref().and_then(|p| p.seconds_left(&token)).filter(|s| *s > 0).unwrap_or(GCLOUD_UNKNOWN_LIFETIME_SECS);
                Ok(self.secret(token, Some(self.now() + left)))
            }
            CredentialRef::GcloudSa(_) => Err(CredentialError::Unsupported("`gcloud-sa:` service-account keys (JWT signing) are not implemented yet; use `gcloud-adc` with GOOGLE_APPLICATION_CREDENTIALS or workload identity".into())),
            CredentialRef::Oauth2(name) => {
                let source = self.oauth.as_ref().ok_or_else(|| CredentialError::Unsupported("`oauth2:` credentials need the OAuth provider, which this process did not set up".into()))?;
                let t = source.access_token(name)?;
                Ok(self.secret(t.expose().to_string(), t.expires_at))
            }
        }
    }
}

/// Resolves on demand and caches expiring credentials, refreshing before they lapse. Static
/// credentials (env, keychain) are re-read each call so rotation takes effect immediately.
pub struct CredentialStore {
    resolver: Resolver,
    cache: Mutex<HashMap<String, Resolved>>,
    /// One renewal per credential at a time: two spans that find a token expired must not both mint one.
    flights: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// Refresh this many seconds before expiry.
    skew: i64,
}

impl CredentialStore {
    pub fn new(resolver: Resolver) -> CredentialStore {
        CredentialStore { resolver, cache: Mutex::new(HashMap::new()), flights: Mutex::new(HashMap::new()), skew: 60 }
    }

    fn fresh(&self, key: &str) -> (Option<Resolved>, Option<Resolved>) {
        let now = self.resolver.now();
        let cached = self.cache.lock().unwrap().get(key).cloned();
        let usable = cached.clone().filter(|c| c.expires_at().map(|e| e - self.skew > now).unwrap_or(false));
        (usable, cached)
    }

    pub fn get(&self, endpoint_id: &str, credential: &str) -> Result<Resolved, CredentialError> {
        let parsed = CredentialRef::parse(credential).map_err(CredentialError::Failed)?;
        let key = format!("{}|{}", endpoint_id, credential);
        if let (Some(c), _) = self.fresh(&key) {
            return Ok(c);
        }
        let flight = self.flights.lock().unwrap().entry(key.clone()).or_default().clone();
        let _one = flight.lock().unwrap();
        // Another caller may have renewed it while we waited.
        let (usable, cached) = self.fresh(&key);
        if let Some(c) = usable {
            return Ok(c);
        }
        match self.resolver.resolve(&parsed) {
            Ok(fresh) => {
                if fresh.expires_at().is_some() {
                    self.cache.lock().unwrap().insert(key, fresh.clone());
                }
                Ok(fresh)
            }
            Err(CredentialError::NeedsReauth { .. }) => {
                self.cache.lock().unwrap().remove(&key);
                Err(CredentialError::NeedsReauth { endpoint: endpoint_id.to_string() })
            }
            // The renewal failed, but what we hold has not lapsed (we only renew early, by the skew): keep
            // using it, so a blip in `gcloud`, the network or the provider does not cost a working sign-in.
            Err(CredentialError::Failed(_)) | Err(CredentialError::NotFound(_)) if cached.as_ref().is_some_and(|c| c.expires_at().is_some_and(|e| e > self.resolver.now())) => Ok(cached.unwrap()),
            // We held an expiring credential that has lapsed and can't renew it: the operator must sign in again.
            Err(CredentialError::Failed(_)) | Err(CredentialError::NotFound(_)) if cached.is_some() => {
                self.cache.lock().unwrap().remove(&key);
                Err(CredentialError::NeedsReauth { endpoint: endpoint_id.to_string() })
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    struct Fake {
        calls: Mutex<Vec<Vec<String>>>,
        replies: Mutex<Vec<Result<RunOutput, String>>>,
    }

    impl Fake {
        fn new(replies: Vec<Result<RunOutput, String>>) -> Arc<Fake> {
            Arc::new(Fake { calls: Mutex::new(vec![]), replies: Mutex::new(replies) })
        }
    }

    impl Runner for Fake {
        fn run(&self, argv: &[String], _t: Duration) -> Result<RunOutput, String> {
            self.calls.lock().unwrap().push(argv.to_vec());
            self.replies.lock().unwrap().remove(0)
        }
    }

    fn ok(stdout: &str) -> Result<RunOutput, String> {
        Ok(RunOutput { status: 0, stdout: stdout.into(), stderr_len: 0 })
    }

    fn resolver(f: Arc<Fake>, env: Vec<(&'static str, &'static str)>, now: Arc<AtomicI64>) -> (Resolver, Arc<Mutex<Vec<String>>>) {
        let registered = Arc::new(Mutex::new(Vec::new()));
        let reg = registered.clone();
        let r = Resolver::new(
            f,
            Arc::new(move |k| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())),
            Arc::new(move |v| reg.lock().unwrap().push(v.to_string())),
            Arc::new(move || now.load(Ordering::SeqCst)),
        );
        (r, registered)
    }

    #[test]
    fn parses_every_source() {
        use CredentialRef::*;
        assert_eq!(CredentialRef::parse("keychain:anthropic"), Ok(Keychain("anthropic".into())));
        assert_eq!(CredentialRef::parse("env:ANTHROPIC_API_KEY"), Ok(Env("ANTHROPIC_API_KEY".into())));
        assert_eq!(CredentialRef::parse("aws:work-sso"), Ok(Aws("work-sso".into())));
        assert_eq!(CredentialRef::parse("gcloud-adc"), Ok(GcloudAdc));
        assert_eq!(CredentialRef::parse("gcloud-sa:sa-key"), Ok(GcloudSa("sa-key".into())));
        assert_eq!(CredentialRef::parse("oauth2:corp-okta"), Ok(Oauth2("corp-okta".into())));
        assert_eq!(CredentialRef::parse("harness-login"), Ok(HarnessLogin));
        assert_eq!(CredentialRef::parse(r#"command:["corp-token","--json"]"#), Ok(Command(vec!["corp-token".into(), "--json".into()])));
        assert_eq!(CredentialRef::parse(r#"command:corp-token --scope "llm invoke""#), Ok(Command(vec!["corp-token".into(), "--scope".into(), "llm invoke".into()])));
        for bad in ["", "keychain:", "env:", "command:", "command:[1,2]", "plaintext-key", "ssh:thing"] {
            assert!(CredentialRef::parse(bad).is_err(), "{}", bad);
        }
    }

    #[test]
    fn env_values_are_registered_with_the_redactor_and_never_shown() {
        let (r, reg) = resolver(Fake::new(vec![]), vec![("K", "sk-super-secret-value-123")], Arc::new(AtomicI64::new(0)));
        let got = r.resolve(&CredentialRef::Env("K".into())).unwrap();
        assert_eq!(reg.lock().unwrap().as_slice(), ["sk-super-secret-value-123"]);
        assert!(!format!("{:?}", got).contains("super-secret"));
        assert!(matches!(r.resolve(&CredentialRef::Env("MISSING".into())), Err(CredentialError::NotFound(_))));
    }

    #[test]
    fn keychain_uses_delphis_own_service_entry() {
        let f = Fake::new(vec![ok("key-from-keychain\n")]);
        let (r, reg) = resolver(f.clone(), vec![], Arc::new(AtomicI64::new(0)));
        match r.resolve(&CredentialRef::Keychain("anthropic".into())).unwrap() {
            Resolved::Token(s) => assert_eq!(s.expose(), "key-from-keychain"),
            other => panic!("{:?}", other),
        }
        let argv = f.calls.lock().unwrap()[0].clone();
        assert!(argv.contains(&"delphi".to_string()) && argv.contains(&"anthropic".to_string()), "{:?}", argv);
        assert_eq!(reg.lock().unwrap().len(), 1);
        let f = Fake::new(vec![Ok(RunOutput { status: 44, stdout: String::new(), stderr_len: 30 })]);
        let (r, _) = resolver(f, vec![], Arc::new(AtomicI64::new(0)));
        assert!(matches!(r.resolve(&CredentialRef::Keychain("nope".into())), Err(CredentialError::NotFound(_))));
    }

    #[test]
    fn helper_output_is_parsed_and_stderr_and_garbage_never_leak() {
        let f = Fake::new(vec![ok(r#"{"token":"helper-token-abcdef123456","expires_at":"2026-10-23T12:00:00Z"}"#), ok("here is a secret: hunter2"), Ok(RunOutput { status: 3, stdout: "partial-secret".into(), stderr_len: 99 })]);
        let (r, reg) = resolver(f, vec![], Arc::new(AtomicI64::new(0)));
        let c = CredentialRef::Command(vec!["h".into()]);
        match r.resolve(&c).unwrap() {
            Resolved::Token(s) => {
                assert_eq!(s.expose(), "helper-token-abcdef123456");
                assert_eq!(s.expires_at, delphi_core::time::parse_rfc3339("2026-10-23T12:00:00Z"));
            }
            other => panic!("{:?}", other),
        }
        assert_eq!(reg.lock().unwrap().as_slice(), ["helper-token-abcdef123456"]);
        for _ in 0..2 {
            let msg = r.resolve(&c).unwrap_err().to_string();
            assert!(!msg.contains("hunter2") && !msg.contains("partial-secret"), "{}", msg);
        }
    }

    #[test]
    fn aws_credentials_come_from_the_official_cli_and_secrets_are_registered() {
        let json = r#"{"Version":1,"AccessKeyId":"AKIAEXAMPLE","SecretAccessKey":"aws-secret-key-0123456789","SessionToken":"aws-session-token-xyz","Expiration":"2026-10-23T12:00:00Z"}"#;
        let f = Fake::new(vec![ok(json)]);
        let (r, reg) = resolver(f.clone(), vec![], Arc::new(AtomicI64::new(0)));
        match r.resolve(&CredentialRef::Aws("work-sso".into())).unwrap() {
            Resolved::Aws(a) => {
                assert_eq!((a.access_key_id.as_str(), a.secret_access_key(), a.session_token()), ("AKIAEXAMPLE", "aws-secret-key-0123456789", Some("aws-session-token-xyz")));
                assert!(a.expires_at.is_some());
                assert!(!format!("{:?}", a).contains("aws-secret"));
            }
            other => panic!("{:?}", other),
        }
        assert_eq!(f.calls.lock().unwrap()[0], ["aws", "configure", "export-credentials", "--profile", "work-sso", "--format", "process"]);
        assert_eq!(reg.lock().unwrap().len(), 3); // key id, secret key, session token
    }

    #[test]
    fn gcloud_adc_and_unsupported_sources() {
        let f = Fake::new(vec![ok("ya29.token-value-12345\n")]);
        let (r, _) = resolver(f.clone(), vec![], Arc::new(AtomicI64::new(1_000)));
        match r.resolve(&CredentialRef::GcloudAdc).unwrap() {
            Resolved::Token(s) => assert_eq!((s.expose(), s.expires_at), ("ya29.token-value-12345", Some(1_000 + GCLOUD_UNKNOWN_LIFETIME_SECS)), "with no way to ask, assume a short life"),
            other => panic!("{:?}", other),
        }
        assert_eq!(f.calls.lock().unwrap()[0], ["gcloud", "auth", "application-default", "print-access-token"]);
        assert!(matches!(r.resolve(&CredentialRef::Oauth2("x".into())), Err(CredentialError::Unsupported(_))));
        assert!(matches!(r.resolve(&CredentialRef::GcloudSa("x".into())), Err(CredentialError::Unsupported(_))));
        assert!(matches!(r.resolve(&CredentialRef::HarnessLogin), Ok(Resolved::None)));
    }

    struct Probe(Option<i64>);

    impl ExpiryProbe for Probe {
        fn seconds_left(&self, token: &str) -> Option<i64> {
            assert_eq!(token, "ya29.token-value-12345");
            self.0
        }
    }

    #[test]
    fn a_gcloud_token_expires_when_the_issuer_says_not_when_we_guess() {
        let now = Arc::new(AtomicI64::new(1_000));
        for (reported, want) in [(Some(3_599), 1_000 + 3_599), (Some(1_200), 1_000 + 1_200), (None, 1_000 + GCLOUD_UNKNOWN_LIFETIME_SECS), (Some(0), 1_000 + GCLOUD_UNKNOWN_LIFETIME_SECS)] {
            let f = Fake::new(vec![ok("ya29.token-value-12345\n")]);
            let (r, _) = resolver(f, vec![], now.clone());
            let r = r.with_gcloud_expiry(Arc::new(Probe(reported)));
            match r.resolve(&CredentialRef::GcloudAdc).unwrap() {
                Resolved::Token(s) => assert_eq!(s.expires_at, Some(want), "{:?}", reported),
                other => panic!("{:?}", other),
            }
        }
    }

    #[test]
    fn a_failed_renewal_keeps_a_token_that_has_not_lapsed_and_only_a_lapsed_one_needs_reauth() {
        let now = Arc::new(AtomicI64::new(0));
        let fail = || Ok(RunOutput { status: 1, stdout: String::new(), stderr_len: 5 });
        let f = Fake::new(vec![ok(r#"{"token":"short-lived-token-1234","expires_at":"1970-01-01T00:10:00Z"}"#), fail(), fail()]);
        let (r, _) = resolver(f.clone(), vec![], now.clone());
        let store = CredentialStore::new(r);
        let cred = r#"command:["h"]"#;
        store.get("gw", cred).unwrap();
        now.store(580, Ordering::SeqCst); // inside the refresh skew, 20 s before the token really lapses
        match store.get("gw", cred).unwrap() {
            Resolved::Token(s) => assert_eq!(s.expose(), "short-lived-token-1234"),
            other => panic!("{:?}", other),
        }
        assert_eq!(f.calls.lock().unwrap().len(), 2, "renewal was attempted");
        // The token is still held: a later success replaces it, and a failure after the real expiry is a reauth.
        now.store(601, Ordering::SeqCst);
        assert_eq!(store.get("gw", cred).unwrap_err(), CredentialError::NeedsReauth { endpoint: "gw".into() });
    }

    #[test]
    fn concurrent_callers_share_one_renewal() {
        let now = Arc::new(AtomicI64::new(10_000));
        let tok = ok(&format!(r#"{{"token":"one-aaaaaaaaaaaaaaaa","expires_at":"{}"}}"#, delphi_core::time::format_unix_millis(10_000 + 3_600, 0)));
        let f = Fake::new(vec![tok]);
        let (r, _) = resolver(f.clone(), vec![], now);
        let store = Arc::new(CredentialStore::new(r));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let s = store.clone();
                std::thread::spawn(move || s.get("gw", r#"command:["h"]"#).map(|r| matches!(r, Resolved::Token(_))))
            })
            .collect();
        for t in threads {
            assert_eq!(t.join().unwrap(), Ok(true));
        }
        assert_eq!(f.calls.lock().unwrap().len(), 1, "one helper run, not eight");
    }

    #[test]
    fn expiring_credentials_are_cached_then_refreshed_before_they_lapse() {
        let now = Arc::new(AtomicI64::new(10_000));
        let tok = |n: &str, exp: i64| ok(&format!(r#"{{"token":"{}-aaaaaaaaaaaaaaaa","expires_at":"{}"}}"#, n, delphi_core::time::format_unix_millis(exp, 0)));
        let f = Fake::new(vec![tok("one", 10_000 + 3_600), tok("two", 10_000 + 7_200)]);
        let (r, _) = resolver(f.clone(), vec![], now.clone());
        let store = CredentialStore::new(r);
        let cred = r#"command:["h"]"#;
        let first = store.get("gw", cred).unwrap();
        let again = store.get("gw", cred).unwrap();
        assert_eq!(f.calls.lock().unwrap().len(), 1, "cached while valid");
        assert!(matches!((&first, &again), (Resolved::Token(a), Resolved::Token(b)) if a.expose() == b.expose()));
        now.store(10_000 + 3_600 - 30, Ordering::SeqCst); // inside the 60 s refresh skew
        match store.get("gw", cred).unwrap() {
            Resolved::Token(s) => assert!(s.expose().starts_with("two")),
            other => panic!("{:?}", other),
        }
        assert_eq!(f.calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_credential_that_cannot_be_renewed_needs_reauthentication_not_failover() {
        let now = Arc::new(AtomicI64::new(0));
        let f = Fake::new(vec![ok(r#"{"token":"short-lived-token-1234","expires_at":"1970-01-01T00:10:00Z"}"#), Ok(RunOutput { status: 1, stdout: String::new(), stderr_len: 5 })]);
        let (r, _) = resolver(f, vec![], now.clone());
        let store = CredentialStore::new(r);
        store.get("corp-gateway", r#"command:["h"]"#).unwrap();
        now.store(1_000, Ordering::SeqCst);
        assert_eq!(store.get("corp-gateway", r#"command:["h"]"#).unwrap_err(), CredentialError::NeedsReauth { endpoint: "corp-gateway".into() });
        assert_eq!(CredentialError::NeedsReauth { endpoint: "corp-gateway".into() }.to_string(), "re-authenticate corp-gateway");
    }

    #[test]
    fn the_system_runner_execs_without_a_shell_and_enforces_timeouts() {
        let out = SystemRunner.run(&["/bin/echo".into(), r#"{"token":"x"}"#.into()], Duration::from_secs(5)).unwrap();
        assert_eq!((out.status, out.stdout.trim()), (0, r#"{"token":"x"}"#));
        // No shell: metacharacters are plain arguments.
        let out = SystemRunner.run(&["/bin/echo".into(), "a; echo injected".into()], Duration::from_secs(5)).unwrap();
        assert_eq!(out.stdout.trim(), "a; echo injected");
        let t0 = Instant::now();
        assert!(SystemRunner.run(&["/bin/sleep".into(), "5".into()], Duration::from_millis(200)).is_err());
        assert!(t0.elapsed() < Duration::from_secs(3));
        assert!(SystemRunner.run(&["/no/such/program".into()], Duration::from_secs(1)).is_err());
    }
}
