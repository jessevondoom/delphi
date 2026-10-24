//! Claude platform envelopes (`specs/002` §2.1.1, `specs/009` §3): turn a compiled request core into a
//! concrete HTTP request for the direct API, a native gateway, AWS Bedrock or Google Vertex AI.
//!
//! * Direct and gateway-native: the Messages API (`model`, `stream`). Direct authenticates with
//!   `x-api-key`; gateways with `Authorization: Bearer`.
//! * Bedrock: `InvokeModelWithResponseStream` with an Anthropic Messages body — **never Converse**,
//!   which drops thinking signatures and cache control. The body carries `anthropic_version` and no
//!   `model`/`stream`; the request is SigV4-signed.
//! * Vertex: `streamRawPredict` with an Anthropic Messages body (`anthropic_version`, `stream: true`,
//!   no `model`); Bearer token.
//!
//! The compliance pin applies first: a request is built only for an endpoint in `allowed_endpoints`.

use crate::claude::Compiled;
use crate::config::{AccessPath, Config, Endpoint};
use crate::credential::{CredentialError, Resolved};
use crate::sigv4;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt;

pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const BEDROCK_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";
pub const VERTEX_ANTHROPIC_VERSION: &str = "vertex-2023-10-16";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseKind {
    Sse,
    AwsEventStream,
}

/// Which wire dialect the response speaks; it picks the stream assembler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Claude,
    Gemini,
    /// OpenAI chat-completions, for compat gateways.
    OpenAi,
}

pub struct HttpRequest {
    pub dialect: Dialect,
    pub url: String,
    /// Lowercase names.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub response: ResponseKind,
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(k, _)| k.as_str()).collect();
        write!(f, "HttpRequest {{ url: {:?}, headers: {:?} (values hidden), body: {} bytes }}", self.url, names, self.body.len())
    }
}

#[derive(Debug)]
pub enum BuildError {
    /// The endpoint is outside `allowed_endpoints`.
    NotAllowed(String),
    Unsupported(String),
    Credential(CredentialError),
    /// A configured header would break the request (CR/LF, NUL).
    BadHeader(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::NotAllowed(id) => write!(f, "endpoint `{}` is not in allowed_endpoints; refusing to send", id),
            BuildError::Unsupported(m) => write!(f, "{}", m),
            BuildError::Credential(e) => write!(f, "{}", e),
            BuildError::BadHeader(n) => write!(f, "header `{}` contains a control character", n),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<CredentialError> for BuildError {
    fn from(e: CredentialError) -> Self {
        BuildError::Credential(e)
    }
}

pub struct BuildInput<'a> {
    pub cfg: &'a Config,
    pub endpoint: &'a Endpoint,
    pub model_id: &'a str,
    pub compiled: &'a Compiled,
    pub creds: &'a Resolved,
    pub now_unix: i64,
    /// Receives every secret header value (configured headers) for redaction.
    pub register: &'a dyn Fn(&str),
}

fn check_header(name: &str, value: &str) -> Result<(), BuildError> {
    if value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) || name.bytes().any(|b| b == b'\r' || b == b'\n' || b == b':' || b == 0) {
        return Err(BuildError::BadHeader(name.to_string()));
    }
    Ok(())
}

/// A value interpolated into a URL host or path: no character that could change which host or path is
/// meant (`/`, `?`, `#`, `@`, `\\`, spaces, `..`).
fn safe_segment(what: &str, v: &str) -> Result<(), BuildError> {
    // Model ids may carry `@` (Vertex Claude versions) and `:` (Bedrock-style); hosts and projects may not carry `@`.
    let extra: &[char] = if what == "model id" { &['@', ':'] } else { &[] };
    if !v.is_empty() && !v.chars().all(|c| c == '.') && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') || extra.contains(&c)) {
        Ok(())
    } else {
        Err(BuildError::Unsupported(format!("{} `{}` has characters that cannot appear in a URL path or host", what, v)))
    }
}

/// `https://host:port/prefix` → (`https://host:port`, `host:port`, `/prefix`).
fn split_url(url: &str) -> Option<(String, String, String)> {
    let (scheme, rest) = url.split_once("://")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    Some((format!("{}://{}", scheme, authority), authority.to_string(), path.trim_end_matches('/').to_string()))
}

fn token<'a>(creds: &'a Resolved, what: &str) -> Result<&'a str, BuildError> {
    match creds {
        Resolved::Token(t) => Ok(t.expose()),
        _ => Err(BuildError::Unsupported(format!("{} needs a token credential (env:, keychain:, command:, gcloud-adc)", what))),
    }
}

/// Gemini over the Developer API (`x-goog-api-key`), a native gateway (Bearer) or Vertex AI (Bearer). The
/// key travels in a header, never the URL, so it cannot reach a log through a request line.
fn build_gemini(i: &BuildInput) -> Result<HttpRequest, BuildError> {
    let e = i.endpoint;
    let mut headers: Vec<(String, String)> = vec![("content-type".into(), "application/json".into()), ("accept".into(), "text/event-stream".into())];
    for (k, v) in &e.headers {
        check_header(k, v)?;
        (i.register)(v);
        headers.push((k.to_lowercase(), v.clone()));
    }
    let model = i.model_id.strip_prefix("models/").unwrap_or(i.model_id);
    safe_segment("model id", model)?;
    let url = match (e.path, e.platform.as_deref()) {
        (AccessPath::Api, _) | (AccessPath::GatewayNative, _) => {
            let base = e.base_url.clone().unwrap_or_else(|| "https://generativelanguage.googleapis.com".into());
            let key = token(i.creds, "the Gemini API")?;
            // An API key goes in `x-goog-api-key`; anything that is an OAuth or ADC access token is a Bearer
            // token, and a user token needs the quota project named (the operator's own Google Cloud project).
            let bearer = e.path == AccessPath::GatewayNative || e.credential.starts_with("oauth2:") || e.credential == "gcloud-adc" || e.credential.starts_with("command:");
            if bearer {
                headers.push(("authorization".into(), format!("Bearer {}", key)));
                if let Some(p) = e.project.as_deref().filter(|p| !p.is_empty()) {
                    safe_segment("project", p)?;
                    headers.push(("x-goog-user-project".into(), p.to_string()));
                }
            } else {
                headers.push(("x-goog-api-key".into(), key.to_string()));
            }
            format!("{}/v1beta/models/{}:streamGenerateContent?alt=sse", base.trim_end_matches('/'), model)
        }
        (AccessPath::Cloud, Some("vertex")) => {
            let region = e.region.clone().unwrap_or_default();
            let project = e.project.clone().unwrap_or_default();
            safe_segment("region", &region)?;
            safe_segment("project", &project)?;
            let base = e.base_url.clone().unwrap_or_else(|| if region == "global" { "https://aiplatform.googleapis.com".into() } else { format!("https://{}-aiplatform.googleapis.com", region) });
            headers.push(("authorization".into(), format!("Bearer {}", token(i.creds, "Vertex AI")?)));
            format!("{}/v1/projects/{}/locations/{}/publishers/google/models/{}:streamGenerateContent?alt=sse", base.trim_end_matches('/'), project, region, model)
        }
        _ => return Err(BuildError::Unsupported(format!("endpoint `{}` ({}) is not a Gemini generateContent path in this step", e.id, e.path.as_str()))),
    };
    let body = serde_json::to_vec(&i.compiled.body).expect("body serializes");
    Ok(HttpRequest { dialect: Dialect::Gemini, url, headers, body, response: ResponseKind::Sse })
}

/// A compat gateway: `POST {base_url}/v1/chat/completions` with a Bearer token.
fn build_openai(i: &BuildInput) -> Result<HttpRequest, BuildError> {
    let e = i.endpoint;
    let base = e.base_url.clone().ok_or_else(|| BuildError::Unsupported(format!("compat endpoint `{}` needs a base_url", e.id)))?;
    let mut headers: Vec<(String, String)> = vec![("content-type".into(), "application/json".into()), ("accept".into(), "text/event-stream".into())];
    for (k, v) in &e.headers {
        check_header(k, v)?;
        (i.register)(v);
        headers.push((k.to_lowercase(), v.clone()));
    }
    headers.push(("authorization".into(), format!("Bearer {}", token(i.creds, "a compat gateway")?)));
    let mut core = i.compiled.body.as_object().cloned().unwrap_or_default();
    core.insert("model".into(), json!(i.model_id));
    core.insert("stream".into(), json!(true));
    core.insert("stream_options".into(), json!({"include_usage": true}));
    let body = serde_json::to_vec(&Value::Object(core)).expect("body serializes");
    Ok(HttpRequest { dialect: Dialect::OpenAi, url: format!("{}/v1/chat/completions", base.trim_end_matches('/')), headers, body, response: ResponseKind::Sse })
}

pub fn build(i: &BuildInput) -> Result<HttpRequest, BuildError> {
    if !i.cfg.is_allowed(&i.endpoint.id) {
        return Err(BuildError::NotAllowed(i.endpoint.id.clone()));
    }
    let e = i.endpoint;
    if e.path == AccessPath::GatewayCompat {
        return build_openai(i);
    }
    if e.family == "gemini" {
        return build_gemini(i);
    }
    let mut core = i.compiled.body.as_object().cloned().unwrap_or_default();
    let mut headers: Vec<(String, String)> = vec![("content-type".into(), "application/json".into())];
    for (k, v) in &e.headers {
        check_header(k, v)?;
        (i.register)(v);
        headers.push((k.to_lowercase(), v.clone()));
    }
    let url;
    match (e.path, e.platform.as_deref()) {
        (AccessPath::Api, _) | (AccessPath::GatewayNative, _) => {
            let base = e.base_url.clone().unwrap_or_else(|| "https://api.anthropic.com".into());
            url = format!("{}/v1/messages", base.trim_end_matches('/'));
            core.insert("model".into(), json!(i.model_id));
            core.insert("stream".into(), json!(true));
            let key = token(i.creds, "the Messages API")?;
            if e.path == AccessPath::Api {
                headers.push(("x-api-key".into(), key.to_string()));
            } else {
                headers.push(("authorization".into(), format!("Bearer {}", key)));
            }
            headers.push(("anthropic-version".into(), ANTHROPIC_VERSION.into()));
            if !i.compiled.betas.is_empty() {
                headers.push(("anthropic-beta".into(), i.compiled.betas.join(",")));
            }
            headers.push(("accept".into(), "text/event-stream".into()));
        }
        (AccessPath::Cloud, Some("bedrock")) => {
            let region = e.region.clone().unwrap_or_default();
            safe_segment("region", &region)?;
            let base = e.base_url.clone().unwrap_or_else(|| format!("https://bedrock-runtime.{}.amazonaws.com", region));
            let (origin, authority, prefix) = split_url(&base).ok_or_else(|| BuildError::Unsupported(format!("bad base_url `{}`", base)))?;
            let wire_path = format!("{}/model/{}/invoke-with-response-stream", prefix, sigv4::uri_encode(i.model_id));
            url = format!("{}{}", origin, wire_path);
            core.insert("anthropic_version".into(), json!(BEDROCK_ANTHROPIC_VERSION));
            if !i.compiled.betas.is_empty() {
                core.insert("anthropic_beta".into(), json!(i.compiled.betas));
            }
            let body = serde_json::to_vec(&Value::Object(core.clone())).expect("body serializes");
            let aws = match i.creds {
                Resolved::Aws(a) => a,
                _ => return Err(BuildError::Unsupported("Bedrock needs an `aws:<profile>` credential".into())),
            };
            let date = sigv4::amz_date(i.now_unix);
            let mut signed: BTreeMap<String, String> = BTreeMap::new();
            signed.insert("host".into(), authority);
            signed.insert("x-amz-date".into(), date.clone());
            signed.insert("content-type".into(), "application/json".into());
            let auth = sigv4::sign("POST", &wire_path, "", &signed, &body, aws, &region, "bedrock", &date);
            headers.push(("x-amz-date".into(), date));
            headers.push(("accept".into(), "application/vnd.amazon.eventstream".into()));
            headers.extend(auth);
            return Ok(HttpRequest { dialect: Dialect::Claude, url, headers, body, response: ResponseKind::AwsEventStream });
        }
        (AccessPath::Cloud, Some("vertex")) => {
            let region = e.region.clone().unwrap_or_default();
            let project = e.project.clone().unwrap_or_default();
            safe_segment("region", &region)?;
            safe_segment("project", &project)?;
            safe_segment("model id", i.model_id)?;
            let base = e.base_url.clone().unwrap_or_else(|| if region == "global" { "https://aiplatform.googleapis.com".into() } else { format!("https://{}-aiplatform.googleapis.com", region) });
            url = format!("{}/v1/projects/{}/locations/{}/publishers/anthropic/models/{}:streamRawPredict", base.trim_end_matches('/'), project, region, i.model_id);
            core.insert("anthropic_version".into(), json!(VERTEX_ANTHROPIC_VERSION));
            core.insert("stream".into(), json!(true));
            if !i.compiled.betas.is_empty() {
                core.insert("anthropic_beta".into(), json!(i.compiled.betas));
            }
            headers.push(("authorization".into(), format!("Bearer {}", token(i.creds, "Vertex AI")?)));
            headers.push(("accept".into(), "text/event-stream".into()));
        }
        (AccessPath::GatewayCompat, _) | (AccessPath::Harness, _) | (AccessPath::Cloud, _) => {
            return Err(BuildError::Unsupported(format!("endpoint `{}` ({}) is not a Claude Messages-API path in this step", e.id, e.path.as_str())));
        }
    }
    let body = serde_json::to_vec(&Value::Object(core)).expect("body serializes");
    Ok(HttpRequest { dialect: Dialect::Claude, url, headers, body, response: ResponseKind::Sse })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::tests::{ipr, loop_history, opts, target};
    use crate::claude::{compile, INTERLEAVED_THINKING_BETA};
    use crate::config::tests::SAMPLE;
    use crate::credential::{AwsCredentials, Secret};

    fn compiled() -> Compiled {
        compile(&ipr(), &loop_history("direct", "m1"), &target("direct", "m1"), &opts(Some(2048))).unwrap()
    }

    /// SAMPLE with `direct` added to the pin, plus `extra` endpoints (which may add `vx`).
    fn cfg_with(extra: &str) -> Config {
        let allowed = if extra.contains("id = \"vx\"") { "[\"corp-gateway\", \"bedrock-us\", \"direct\", \"vx\"]" } else { "[\"corp-gateway\", \"bedrock-us\", \"direct\"]" };
        Config::parse(&format!("{}\n{}", SAMPLE.replace("allowed_endpoints = [\"corp-gateway\", \"bedrock-us\"]", &format!("allowed_endpoints = {}", allowed)), extra)).unwrap()
    }

    const VERTEX: &str = "[[endpoints]]\nid = \"vx\"\nfamily = \"claude\"\npath = \"cloud\"\nplatform = \"vertex\"\nregion = \"us-east5\"\nproject = \"proj-1\"\ncredential = \"gcloud-adc\"\n";

    fn token(v: &str) -> Resolved {
        Resolved::Token(Secret::new(v.into(), None))
    }

    fn aws() -> Resolved {
        Resolved::Aws(AwsCredentials::new("AKIDEXAMPLE".into(), "secret".into(), Some("sess".into()), None))
    }

    fn header<'a>(r: &'a HttpRequest, n: &str) -> Option<&'a str> {
        r.headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str())
    }

    fn body(r: &HttpRequest) -> Value {
        serde_json::from_slice(&r.body).unwrap()
    }

    fn build_for(cfg: &Config, id: &str, model: &str, creds: &Resolved) -> Result<HttpRequest, BuildError> {
        let c = compiled();
        build(&BuildInput { cfg, endpoint: cfg.endpoint(id).unwrap(), model_id: model, compiled: &c, creds, now_unix: 1_790_000_000, register: &|_| {} })
    }

    #[test]
    fn direct_api_uses_the_messages_api_with_an_api_key() {
        let cfg = cfg_with("");
        let r = build_for(&cfg, "direct", "sonnet-x", &token("sk-key")).unwrap();
        assert_eq!(r.url, "https://api.anthropic.com/v1/messages");
        assert_eq!((header(&r, "x-api-key"), header(&r, "anthropic-version"), header(&r, "anthropic-beta")), (Some("sk-key"), Some("2023-06-01"), Some(INTERLEAVED_THINKING_BETA)));
        let b = body(&r);
        assert_eq!((b["model"].as_str(), b["stream"].as_bool()), (Some("sonnet-x"), Some(true)));
        assert!(b["messages"].is_array() && b["system"].is_array());
        assert_eq!(r.response, ResponseKind::Sse);
    }

    #[test]
    fn a_native_gateway_uses_bearer_auth_and_configured_headers_that_are_registered_as_secrets() {
        let cfg = cfg_with("");
        let c = compiled();
        let seen = std::sync::Mutex::new(Vec::new());
        let reg = |v: &str| seen.lock().unwrap().push(v.to_string());
        let r = build(&BuildInput { cfg: &cfg, endpoint: cfg.endpoint("corp-gateway").unwrap(), model_id: "m", compiled: &c, creds: &token("tok"), now_unix: 0, register: &reg }).unwrap();
        assert_eq!(r.url, "https://llm-gateway.example.internal/anthropic/v1/messages");
        assert_eq!((header(&r, "authorization"), header(&r, "x-cost-center"), header(&r, "x-api-key")), (Some("Bearer tok"), Some("eng-123"), None));
        assert_eq!(seen.lock().unwrap().as_slice(), ["eng-123"]);
    }

    #[test]
    fn gemini_endpoints_use_generate_content_with_the_right_auth() {
        use crate::claude::tests::opts;
        let extra = "[[endpoints]]\nid = \"gd\"\nfamily = \"gemini\"\npath = \"api\"\ncredential = \"env:K\"\n[[endpoints]]\nid = \"gv\"\nfamily = \"gemini\"\npath = \"cloud\"\nplatform = \"vertex\"\nregion = \"global\"\nproject = \"p1\"\ncredential = \"gcloud-adc\"\n[[endpoints]]\nid = \"gb\"\nfamily = \"gemini\"\npath = \"cloud\"\nplatform = \"bedrock\"\nregion = \"us-east-1\"\ncredential = \"aws:p\"\n";
        let cfg = Config::parse(&format!("{}\n{}", SAMPLE.replace("allowed_endpoints = [\"corp-gateway\", \"bedrock-us\"]", "allowed_endpoints = [\"corp-gateway\", \"bedrock-us\", \"gd\", \"gv\", \"gb\"]"), extra)).unwrap();
        let g = crate::claude::Target { provider: "google".into(), endpoint_id: "gd".into(), model_id: "m".into() };
        let c = crate::gemini::compile(&ipr(), &[], &g, &opts(None), &crate::gemini::GeminiOptions::default()).unwrap();
        let go = |id: &str, creds: &Resolved| build(&BuildInput { cfg: &cfg, endpoint: cfg.endpoint(id).unwrap(), model_id: "gemini-x", compiled: &c, creds, now_unix: 0, register: &|_| {} });
        let r = go("gd", &token("AIza-k")).unwrap();
        assert_eq!(r.url, "https://generativelanguage.googleapis.com/v1beta/models/gemini-x:streamGenerateContent?alt=sse");
        assert_eq!((header(&r, "x-goog-api-key"), header(&r, "authorization")), (Some("AIza-k"), None));
        assert!(!r.url.contains("AIza"));
        let v = go("gv", &token("ya29.t")).unwrap();
        assert_eq!(v.url, "https://aiplatform.googleapis.com/v1/projects/p1/locations/global/publishers/google/models/gemini-x:streamGenerateContent?alt=sse");
        assert_eq!(header(&v, "authorization"), Some("Bearer ya29.t"));
        assert_eq!(body(&v), c.body, "no model or stream field in the body");
        assert!(matches!(go("gb", &aws()), Err(BuildError::Unsupported(_))), "Bedrock does not serve Gemini");
        assert!(matches!(go("gd", &aws()), Err(BuildError::Unsupported(_))), "an AWS credential cannot authenticate the Gemini API");
    }

    #[test]
    fn a_gemini_endpoint_with_an_oauth_credential_sends_a_bearer_token_and_the_quota_project() {
        use crate::claude::tests::opts;
        let extra = "[auth.google]\nflow = \"loopback_pkce\"\nissuer = \"https://accounts.google.com\"\nclient_id = \"cid.apps.googleusercontent.com\"\nscopes = [\"https://www.googleapis.com/auth/generative-language\"]\n[[endpoints]]\nid = \"go\"\nfamily = \"gemini\"\npath = \"api\"\nproject = \"my-gcp-project\"\ncredential = \"oauth2:google\"\n";
        let cfg = Config::parse(&format!("{}\n{}", SAMPLE.replace("allowed_endpoints = [\"corp-gateway\", \"bedrock-us\"]", "allowed_endpoints = [\"corp-gateway\", \"bedrock-us\", \"go\"]"), extra)).unwrap();
        let g = crate::claude::Target { provider: "google".into(), endpoint_id: "go".into(), model_id: "m".into() };
        let c = crate::gemini::compile(&ipr(), &[], &g, &opts(None), &crate::gemini::GeminiOptions::default()).unwrap();
        let r = build(&BuildInput { cfg: &cfg, endpoint: cfg.endpoint("go").unwrap(), model_id: "gemini-x", compiled: &c, creds: &token("ya29.user-token"), now_unix: 0, register: &|_| {} }).unwrap();
        assert_eq!((header(&r, "authorization"), header(&r, "x-goog-api-key"), header(&r, "x-goog-user-project")), (Some("Bearer ya29.user-token"), None, Some("my-gcp-project")));
        assert!(r.url.starts_with("https://generativelanguage.googleapis.com/v1beta/models/gemini-x:streamGenerateContent"));
    }

    #[test]
    fn values_that_could_redirect_a_vertex_url_are_refused() {
        use crate::claude::tests::opts;
        let g = crate::claude::Target { provider: "google".into(), endpoint_id: "gv".into(), model_id: "m".into() };
        let c = crate::gemini::compile(&ipr(), &[], &g, &opts(None), &crate::gemini::GeminiOptions::default()).unwrap();
        for (region, project, model) in [("evil.com/x#", "p", "m"), ("us-east5", "p/../q", "m"), ("us-east5", "p", "m?x=1"), ("us-east5", "p", "../../x"), ("us-east5", "", "m")] {
            let extra = format!("[[endpoints]]\nid = \"gv\"\nfamily = \"gemini\"\npath = \"cloud\"\nplatform = \"vertex\"\nregion = \"{}\"\nproject = \"{}\"\ncredential = \"gcloud-adc\"\n", region, project);
            let Ok(cfg) = Config::parse(&format!("{}\n{}", SAMPLE.replace("allowed_endpoints = [\"corp-gateway\", \"bedrock-us\"]", "allowed_endpoints = [\"corp-gateway\", \"bedrock-us\", \"gv\"]"), extra)) else { continue };
            let r = build(&BuildInput { cfg: &cfg, endpoint: cfg.endpoint("gv").unwrap(), model_id: model, compiled: &c, creds: &token("t"), now_unix: 0, register: &|_| {} });
            assert!(matches!(r, Err(BuildError::Unsupported(_))), "{:?}", (region, project, model));
        }
    }

    #[test]
    fn compat_gateways_get_chat_completions_for_any_family() {
        let extra = "[[endpoints]]\nid = \"cc\"\nfamily = \"claude\"\npath = \"gateway-compat\"\nbase_url = \"https://gw.example.internal/openai/\"\ncredential = \"env:K\"\nheaders = { \"X-Team\" = \"t1\" }\n[[endpoints]]\nid = \"cg\"\nfamily = \"gemini\"\npath = \"gateway-compat\"\nbase_url = \"https://gw.example.internal\"\ncredential = \"env:K\"\n";
        let cfg = Config::parse(&format!("{}\n{}", SAMPLE.replace("allowed_endpoints = [\"corp-gateway\", \"bedrock-us\"]", "allowed_endpoints = [\"corp-gateway\", \"bedrock-us\", \"cc\", \"cg\"]"), extra)).unwrap();
        let c = crate::openai::compile(&ipr(), &[], &crate::claude::tests::opts(None)).unwrap();
        for id in ["cc", "cg"] {
            let r = build(&BuildInput { cfg: &cfg, endpoint: cfg.endpoint(id).unwrap(), model_id: "some-model", compiled: &c, creds: &token("tok"), now_unix: 0, register: &|_| {} }).unwrap();
            assert_eq!(r.dialect, Dialect::OpenAi);
            assert!(r.url.ends_with("/v1/chat/completions") && !r.url.contains("//v1"), "{}", r.url);
            assert_eq!(header(&r, "authorization"), Some("Bearer tok"));
            let b = body(&r);
            assert_eq!((b["model"].as_str(), b["stream"].as_bool(), b["stream_options"]["include_usage"].as_bool()), (Some("some-model"), Some(true), Some(true)));
        }
        let r = build(&BuildInput { cfg: &cfg, endpoint: cfg.endpoint("cc").unwrap(), model_id: "m", compiled: &c, creds: &token("tok"), now_unix: 0, register: &|_| {} }).unwrap();
        assert_eq!(header(&r, "x-team"), Some("t1"));
    }

    #[test]
    fn bedrock_uses_invoke_with_response_stream_never_converse() {
        let cfg = cfg_with("");
        let r = build_for(&cfg, "bedrock-us", "anthropic.claude-x:0", &aws()).unwrap();
        assert_eq!(r.url, "https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.claude-x%3A0/invoke-with-response-stream");
        assert!(!r.url.contains("converse"));
        let b = body(&r);
        assert_eq!(b["anthropic_version"], BEDROCK_ANTHROPIC_VERSION);
        assert!(b.get("model").is_none() && b.get("stream").is_none(), "the model is in the URL; streaming is the operation");
        assert_eq!(b["anthropic_beta"], json!([INTERLEAVED_THINKING_BETA]));
        let auth = header(&r, "authorization").unwrap();
        assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260921/us-east-1/bedrock/aws4_request") && auth.contains("SignedHeaders=content-type;host;x-amz-date;x-amz-security-token"), "{}", auth);
        assert_eq!(header(&r, "x-amz-security-token"), Some("sess"));
        assert_eq!(r.response, ResponseKind::AwsEventStream);
        assert!(r.headers.iter().all(|(_, v)| !v.contains("secret")), "the secret key itself is never sent");
    }

    #[test]
    fn vertex_uses_stream_raw_predict_with_a_bearer_token() {
        let cfg = cfg_with(VERTEX);
        let r = build_for(&cfg, "vx", "claude-x@20250101", &token("ya29.t")).unwrap();
        assert_eq!(r.url, "https://us-east5-aiplatform.googleapis.com/v1/projects/proj-1/locations/us-east5/publishers/anthropic/models/claude-x@20250101:streamRawPredict");
        let b = body(&r);
        assert_eq!((b["anthropic_version"].as_str(), b["stream"].as_bool()), (Some(VERTEX_ANTHROPIC_VERSION), Some(true)));
        assert!(b.get("model").is_none());
        assert_eq!(header(&r, "authorization"), Some("Bearer ya29.t"));
        let global = VERTEX.replace("us-east5", "global");
        let r = build_for(&cfg_with(&global), "vx", "m", &token("t")).unwrap();
        assert!(r.url.starts_with("https://aiplatform.googleapis.com/v1/projects/proj-1/locations/global/"), "{}", r.url);
    }

    #[test]
    fn the_pin_stops_a_request_before_anything_is_built() {
        let cfg = Config::parse(SAMPLE).unwrap(); // `direct` is outside allowed_endpoints
        assert!(matches!(build_for(&cfg, "direct", "m", &token("k")), Err(BuildError::NotAllowed(id)) if id == "direct"));
    }

    #[test]
    fn credential_kinds_must_fit_the_platform() {
        let cfg = cfg_with(VERTEX);
        assert!(build_for(&cfg, "bedrock-us", "m", &token("t")).is_err());
        assert!(build_for(&cfg, "vx", "m", &aws()).is_err());
        assert!(build_for(&cfg, "direct", "m", &Resolved::None).is_err());
    }

    #[test]
    fn header_injection_is_refused_and_debug_hides_values() {
        let cfg = Config::parse(&SAMPLE.replace("eng-123", "x\\r\\nX-Evil: 1")).unwrap();
        let c = compiled();
        let err = build(&BuildInput { cfg: &cfg, endpoint: cfg.endpoint("corp-gateway").unwrap(), model_id: "m", compiled: &c, creds: &token("t"), now_unix: 0, register: &|_| {} });
        assert!(matches!(err, Err(BuildError::BadHeader(_))));
        let ok = build_for(&cfg_with(""), "direct", "m", &token("sk-super-secret")).unwrap();
        assert!(!format!("{:?}", ok).contains("sk-super-secret"));
    }

    #[test]
    fn cloud_endpoints_may_use_a_private_base_url() {
        let text = SAMPLE.replace("credential = \"aws:work-sso\"", "credential = \"aws:work-sso\"\nbase_url = \"https://vpce-1.bedrock-runtime.us-east-1.vpce.amazonaws.com\"");
        let cfg = Config::parse(&text).unwrap();
        let r = build_for(&cfg, "bedrock-us", "m", &aws()).unwrap();
        assert!(r.url.starts_with("https://vpce-1.bedrock-runtime.us-east-1.vpce.amazonaws.com/model/m/"), "{}", r.url);
    }
}
