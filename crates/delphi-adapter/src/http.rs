//! HTTP transport for model calls (`specs/009` §5, `specs/002` §3.1 rule 4).
//!
//! * **No cross-origin redirects:** a redirect to a different scheme, host or port is refused, never
//!   followed with credentials attached.
//! * Status mapping: 401/403 → [`CallError::Auth`]; 429/503/529 → [`CallError::RateLimited`] (with
//!   `retry-after`); other 4xx/5xx → [`CallError::Http`]. Error bodies pass through the caller's
//!   `redact` function, so a token echoed by a provider never reaches an error message or log.
//! * Backoff: 429/529 are retried on the same endpoint with exponential backoff and jitter, honoring
//!   `retry-after`, up to a wall-clock limit, and only before any event has been delivered. Failing
//!   over to the next endpoint is the caller's decision, within the ratified order.
//! * A stream that ends early yields [`CallError::Stream`] with `Incomplete`; the caller discards the
//!   whole turn (`specs/005` §2.5).

use crate::claude::{Assembler, Completed, SseDecoder, SseEvent, StreamError, StreamEvent};
use crate::eventstream;
use crate::platform::{HttpRequest, ResponseKind};
use futures_util::StreamExt;
use std::time::{Duration, Instant};

const MAX_ERROR_BODY: usize = 64 * 1024;

#[derive(Debug)]
pub enum CallError {
    Auth { status: u16, message: String },
    RateLimited { retry_after: Option<Duration>, message: String },
    Http { status: u16, message: String },
    /// A redirect to another origin was refused.
    CrossOriginRedirect { to: String },
    Transport(String),
    Stream(StreamError),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Auth { status, message } => write!(f, "authentication failed ({}): {}", status, message),
            CallError::RateLimited { message, .. } => write!(f, "rate limited or overloaded: {}", message),
            CallError::Http { status, message } => write!(f, "HTTP {}: {}", status, message),
            CallError::CrossOriginRedirect { to } => write!(f, "refused a redirect to another origin ({})", to),
            CallError::Transport(m) => write!(f, "transport error: {}", m),
            CallError::Stream(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for CallError {}

fn origin(u: &reqwest::Url) -> (String, Option<String>, Option<u16>) {
    (u.scheme().to_string(), u.host_str().map(String::from), u.port_or_known_default())
}

/// A client that never follows a redirect off the original origin.
pub fn build_client() -> reqwest::Client {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        let first = attempt.previous().first().cloned();
        match first {
            Some(f) if origin(&f) == origin(attempt.url()) && attempt.previous().len() < 3 => attempt.follow(),
            _ => attempt.error("cross-origin redirect refused"),
        }
    });
    reqwest::Client::builder().redirect(policy).connect_timeout(Duration::from_secs(10)).build().expect("HTTP client builds")
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers.get("retry-after")?.to_str().ok()?.trim().parse::<f64>().ok().filter(|s| *s >= 0.0).map(Duration::from_secs_f64)
}

fn classify(status: u16, retry: Option<Duration>, message: String) -> CallError {
    match status {
        401 | 403 => CallError::Auth { status, message },
        429 | 503 | 529 => CallError::RateLimited { retry_after: retry, message },
        _ => CallError::Http { status, message },
    }
}

fn error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| v["error"]["message"].as_str().or(v["message"].as_str()).map(String::from)).unwrap_or_else(|| body.chars().take(500).collect())
}

/// One attempt. `on_event` receives live deltas and completed blocks as they arrive.
pub async fn call(client: &reqwest::Client, req: &HttpRequest, idle_timeout: Duration, redact: &dyn Fn(&str) -> String, on_event: &mut dyn FnMut(StreamEvent)) -> Result<Completed, CallError> {
    let mut builder = client.post(&req.url).body(req.body.clone());
    for (k, v) in &req.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let resp = builder.send().await.map_err(|e| {
        if e.is_redirect() {
            CallError::CrossOriginRedirect { to: e.url().map(|u| format!("{}://{}", u.scheme(), u.authority())).unwrap_or_default() }
        } else {
            CallError::Transport(redact(&e.without_url().to_string()))
        }
    })?;
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        let retry = retry_after(resp.headers());
        let mut body = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(Ok(chunk)) = stream.next().await {
            body.extend_from_slice(&chunk);
            if body.len() > MAX_ERROR_BODY {
                break;
            }
        }
        return Err(classify(status, retry, redact(&error_message(&String::from_utf8_lossy(&body)))));
    }

    let mut sse = SseDecoder::default();
    let mut aws = eventstream::Decoder::default();
    let mut asm = Assembler::new();
    let mut stream = resp.bytes_stream();
    loop {
        let next = tokio::time::timeout(idle_timeout, stream.next()).await.map_err(|_| CallError::Transport("the stream went idle".into()))?;
        let Some(chunk) = next else { break };
        let chunk = chunk.map_err(|e| CallError::Transport(redact(&e.without_url().to_string())))?;
        let events: Vec<SseEvent> = match req.response {
            ResponseKind::Sse => sse.push(&chunk),
            ResponseKind::AwsEventStream => {
                let mut out = Vec::new();
                for m in aws.push(&chunk).map_err(|e| CallError::Stream(StreamError::Malformed(e)))? {
                    if let Some(ev) = eventstream::to_sse(&m).map_err(|e| match e {
                        StreamError::Provider { kind, message } => CallError::Stream(StreamError::Provider { kind, message: redact(&message) }),
                        other => CallError::Stream(other),
                    })? {
                        out.push(ev);
                    }
                }
                out
            }
        };
        for ev in events {
            for se in asm.handle(&ev).map_err(|e| match e {
                StreamError::Provider { kind, message } => CallError::Stream(StreamError::Provider { kind, message: redact(&message) }),
                other => CallError::Stream(other),
            })? {
                on_event(se);
            }
        }
    }
    asm.finish().map_err(CallError::Stream)
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Total wall-clock budget for retries on one endpoint.
    pub max_elapsed: Duration,
    pub base: Duration,
    pub cap: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy { max_elapsed: Duration::from_secs(60), base: Duration::from_millis(500), cap: Duration::from_secs(30) }
    }
}

/// Delay before retry number `attempt` (0-based): full jitter over an exponential ceiling, never
/// shorter than the server's `retry-after`. `jitter` is in `[0, 1)`.
pub fn backoff_delay(attempt: u32, retry_after: Option<Duration>, p: &RetryPolicy, jitter: f64) -> Duration {
    let ceiling = p.base.saturating_mul(1u32 << attempt.min(16)).min(p.cap);
    ceiling.mul_f64(jitter.clamp(0.0, 1.0)).max(retry_after.unwrap_or_default())
}

fn jitter() -> f64 {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0) as u64;
    let mut x = n.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    x ^= x >> 33;
    (x % 10_000) as f64 / 10_000.0
}

/// [`call`] with rate-limit retries. Retries only while nothing has been delivered to `on_event`.
pub async fn call_with_retry(client: &reqwest::Client, req: &HttpRequest, idle_timeout: Duration, policy: &RetryPolicy, redact: &dyn Fn(&str) -> String, on_event: &mut dyn FnMut(StreamEvent)) -> Result<Completed, CallError> {
    let started = Instant::now();
    let mut attempt = 0u32;
    loop {
        let mut delivered = false;
        let result = call(client, req, idle_timeout, redact, &mut |e| {
            delivered = true;
            on_event(e)
        })
        .await;
        match result {
            Err(CallError::RateLimited { retry_after, message }) if !delivered => {
                let delay = backoff_delay(attempt, retry_after, policy, jitter());
                if started.elapsed() + delay > policy.max_elapsed {
                    return Err(CallError::RateLimited { retry_after, message });
                }
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::tests::{fixture, ipr, loop_history, opts, parse, sse as make_sse, target};
    use crate::claude::compile;
    use crate::config::{tests::SAMPLE, Config};
    use crate::credential::{AwsCredentials, Resolved, Secret};
    use crate::eventstream;
    use crate::platform::{build, BuildInput};
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[derive(Debug, Clone)]
    struct Captured {
        path: String,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    }

    #[derive(Clone)]
    struct Reply {
        status: u16,
        headers: Vec<(String, String)>,
        pieces: Vec<Vec<u8>>,
    }

    impl Reply {
        fn sse(body: &str) -> Reply {
            // Small pieces with pauses, so the client sees a genuine stream.
            let pieces = body.as_bytes().chunks(97).map(|c| c.to_vec()).collect();
            Reply { status: 200, headers: vec![("content-type".into(), "text/event-stream".into())], pieces }
        }
        fn status(status: u16, body: &str) -> Reply {
            Reply { status, headers: vec![("content-type".into(), "application/json".into())], pieces: vec![body.as_bytes().to_vec()] }
        }
    }

    /// A tiny HTTP/1.1 server: records requests, replies from a script (the last reply repeats).
    async fn mock(replies: Vec<Reply>) -> (SocketAddr, Arc<Mutex<Vec<Captured>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let (l2, script) = (log.clone(), Arc::new(Mutex::new(replies)));
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let (log, script) = (l2.clone(), script.clone());
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
                    let mut lines = head.lines();
                    let path = lines.next().unwrap_or("").split_whitespace().nth(1).unwrap_or("").to_string();
                    let headers: BTreeMap<String, String> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
                    let want: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
                    while buf.len() < head_end + want {
                        let n = s.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    log.lock().unwrap().push(Captured { path, headers, body: buf[head_end..].to_vec() });
                    let reply = {
                        let mut sc = script.lock().unwrap();
                        if sc.len() > 1 { sc.remove(0) } else { sc[0].clone() }
                    };
                    let mut out = format!("HTTP/1.1 {} X\r\nconnection: close\r\n", reply.status);
                    for (k, v) in &reply.headers {
                        out.push_str(&format!("{}: {}\r\n", k, v.replace("{ADDR}", &addr.to_string())));
                    }
                    out.push_str("\r\n");
                    let _ = s.write_all(out.as_bytes()).await;
                    for p in &reply.pieces {
                        if s.write_all(p).await.is_err() {
                            return;
                        }
                        let _ = s.flush().await;
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                    let _ = s.shutdown().await;
                });
            }
        });
        (addr, log)
    }

    fn cfg_for(addr: SocketAddr, extra_endpoints: &str) -> Config {
        let text = format!(
            "{}\n[[endpoints]]\nid = \"local-direct\"\nfamily = \"claude\"\npath = \"api\"\nbase_url = \"http://{}\"\ncredential = \"env:K\"\n[[endpoints]]\nid = \"local-gw\"\nfamily = \"claude\"\npath = \"gateway-native\"\nbase_url = \"http://{}/anthropic\"\ncredential = \"env:K\"\nheaders = {{ \"X-Cost-Center\" = \"eng-9\" }}\n[[endpoints]]\nid = \"local-bedrock\"\nfamily = \"claude\"\npath = \"cloud\"\nplatform = \"bedrock\"\nregion = \"us-east-1\"\nbase_url = \"http://{}\"\ncredential = \"aws:p\"\n[[endpoints]]\nid = \"local-vertex\"\nfamily = \"claude\"\npath = \"cloud\"\nplatform = \"vertex\"\nregion = \"us-east5\"\nproject = \"proj-1\"\nbase_url = \"http://{}\"\ncredential = \"gcloud-adc\"\n{}",
            SAMPLE.split("[access]").next().unwrap(),
            addr, addr, addr, addr, extra_endpoints
        );
        Config::parse(&text).unwrap()
    }

    fn fast_policy() -> RetryPolicy {
        RetryPolicy { max_elapsed: Duration::from_secs(5), base: Duration::from_millis(5), cap: Duration::from_millis(20) }
    }

    async fn run(cfg: &Config, id: &str, model: &str, creds: &Resolved, policy: &RetryPolicy) -> (Result<Completed, CallError>, Vec<StreamEvent>) {
        let compiled = compile(&ipr(), &loop_history("direct", "m1"), &target("direct", "m1"), &opts(Some(2048))).unwrap();
        let req = build(&BuildInput { cfg, endpoint: cfg.endpoint(id).unwrap(), model_id: model, compiled: &compiled, creds, now_unix: 1_790_000_000, register: &|_| {} }).unwrap();
        let mut events = Vec::new();
        let secret = "sk-live-SECRET-1234567890";
        let r = call_with_retry(&build_client(), &req, Duration::from_secs(5), policy, &|s| s.replace(secret, "[REDACTED]"), &mut |e| events.push(e)).await;
        (r, events)
    }

    fn tok(v: &str) -> Resolved {
        Resolved::Token(Secret::new(v.into(), None))
    }

    fn want_blocks() -> Vec<crate::blocks::ContentBlock> {
        parse(&fixture(), 1 << 20).unwrap().blocks
    }

    fn hdr(c: &Captured, k: &str) -> String {
        c.headers.get(k).cloned().unwrap_or_default()
    }

    #[tokio::test]
    async fn direct_api_streams_a_turn_and_replays_the_signature_in_the_request() {
        let (addr, log) = mock(vec![Reply::sse(&fixture())]).await;
        let cfg = cfg_for(addr, "");
        let (res, events) = run(&cfg, "local-direct", "sonnet-x", &tok("sk-key-1"), &RetryPolicy::default()).await;
        let done = res.unwrap();
        assert_eq!(done.blocks, want_blocks());
        assert_eq!(done.usage.output_tokens, 321);
        assert!(events.iter().any(|e| matches!(e, StreamEvent::ThinkingDelta { .. })) && events.iter().any(|e| matches!(e, StreamEvent::TextDelta { .. })), "live deltas arrived");
        let req = log.lock().unwrap()[0].clone();
        assert_eq!(req.path, "/v1/messages");
        assert_eq!((hdr(&req, "x-api-key").as_str(), hdr(&req, "anthropic-version").as_str()), ("sk-key-1", "2023-06-01"));
        let body: Value = serde_json::from_slice(&req.body).unwrap();
        assert_eq!((body["model"].as_str(), body["stream"].as_bool()), (Some("sonnet-x"), Some(true)));
        // Same endpoint and model as the history: the signed block was replayed exactly.
        assert_eq!(body["messages"][1]["content"][0]["signature"], "EqQBCgIYAhIM+/==");
    }

    #[tokio::test]
    async fn gateway_native_sends_bearer_and_configured_headers() {
        let (addr, log) = mock(vec![Reply::sse(&fixture())]).await;
        let cfg = cfg_for(addr, "");
        let (res, _) = run(&cfg, "local-gw", "m", &tok("gw-token"), &RetryPolicy::default()).await;
        assert_eq!(res.unwrap().blocks, want_blocks());
        let req = log.lock().unwrap()[0].clone();
        assert_eq!(req.path, "/anthropic/v1/messages");
        assert_eq!((hdr(&req, "authorization").as_str(), hdr(&req, "x-cost-center").as_str()), ("Bearer gw-token", "eng-9"));
        assert!(!req.headers.contains_key("x-api-key"));
    }

    #[tokio::test]
    async fn bedrock_streams_through_the_aws_event_stream_framing_and_is_sigv4_signed() {
        // Re-frame the fixture's events as Bedrock chunks.
        let mut dec = SseDecoder::default();
        let wire: Vec<u8> = dec.push(fixture().as_bytes()).iter().flat_map(|ev| eventstream::tests::chunk(&ev.data)).collect();
        let reply = Reply { status: 200, headers: vec![("content-type".into(), "application/vnd.amazon.eventstream".into())], pieces: wire.chunks(61).map(|c| c.to_vec()).collect() };
        let (addr, log) = mock(vec![reply]).await;
        let cfg = cfg_for(addr, "");
        let creds = Resolved::Aws(AwsCredentials::new("AKIDEXAMPLE".into(), "aws-secret".into(), Some("sess".into()), None));
        let (res, _) = run(&cfg, "local-bedrock", "anthropic.claude-x:0", &creds, &RetryPolicy::default()).await;
        assert_eq!(res.unwrap().blocks, want_blocks());
        let req = log.lock().unwrap()[0].clone();
        assert_eq!(req.path, "/model/anthropic.claude-x%3A0/invoke-with-response-stream");
        assert!(hdr(&req, "authorization").starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"));
        assert_eq!(hdr(&req, "x-amz-security-token"), "sess");
        let body: Value = serde_json::from_slice(&req.body).unwrap();
        assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");
        assert!(body.get("model").is_none() && body.get("stream").is_none());
        assert!(!String::from_utf8_lossy(&req.body).contains("aws-secret"));
    }

    #[tokio::test]
    async fn vertex_posts_to_stream_raw_predict() {
        let (addr, log) = mock(vec![Reply::sse(&fixture())]).await;
        let cfg = cfg_for(addr, "");
        let (res, _) = run(&cfg, "local-vertex", "claude-x@1", &tok("ya29.x"), &RetryPolicy::default()).await;
        assert_eq!(res.unwrap().blocks, want_blocks());
        let req = log.lock().unwrap()[0].clone();
        assert_eq!(req.path, "/v1/projects/proj-1/locations/us-east5/publishers/anthropic/models/claude-x@1:streamRawPredict");
        assert_eq!(hdr(&req, "authorization"), "Bearer ya29.x");
        assert_eq!(serde_json::from_slice::<Value>(&req.body).unwrap()["anthropic_version"], "vertex-2023-10-16");
    }

    #[tokio::test]
    async fn auth_failures_are_classified_and_the_error_body_is_redacted() {
        let (addr, _) = mock(vec![Reply::status(401, r#"{"error":{"type":"authentication_error","message":"invalid key sk-live-SECRET-1234567890"}}"#)]).await;
        let (res, _) = run(&cfg_for(addr, ""), "local-direct", "m", &tok("k"), &fast_policy()).await;
        match res.unwrap_err() {
            CallError::Auth { status: 401, message } => assert!(message.contains("[REDACTED]") && !message.contains("SECRET"), "{}", message),
            other => panic!("{:?}", other),
        }
    }

    #[tokio::test]
    async fn rate_limits_are_retried_with_backoff_then_succeed() {
        let limited = Reply { status: 429, headers: vec![("retry-after".into(), "0".into()), ("content-type".into(), "application/json".into())], pieces: vec![br#"{"error":{"message":"slow down"}}"#.to_vec()] };
        let overloaded = Reply::status(529, r#"{"error":{"type":"overloaded_error","message":"Overloaded"}}"#);
        let (addr, log) = mock(vec![limited, overloaded, Reply::sse(&fixture())]).await;
        let (res, _) = run(&cfg_for(addr, ""), "local-direct", "m", &tok("k"), &fast_policy()).await;
        assert_eq!(res.unwrap().blocks, want_blocks());
        assert_eq!(log.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn persistent_rate_limiting_gives_up_within_the_wall_clock_limit() {
        let (addr, log) = mock(vec![Reply::status(429, r#"{"error":{"message":"no"}}"#)]).await;
        let policy = RetryPolicy { max_elapsed: Duration::from_millis(150), base: Duration::from_millis(10), cap: Duration::from_millis(40) };
        let t0 = Instant::now();
        let (res, _) = run(&cfg_for(addr, ""), "local-direct", "m", &tok("k"), &policy).await;
        assert!(matches!(res, Err(CallError::RateLimited { .. })));
        assert!(t0.elapsed() < Duration::from_secs(2));
        assert!(log.lock().unwrap().len() >= 2, "it did retry before giving up");
    }

    #[tokio::test]
    async fn a_stream_cut_mid_turn_is_not_retried_and_yields_no_turn() {
        let full = fixture();
        let cut = &full[..full.find("event: message_delta").unwrap()];
        let (addr, log) = mock(vec![Reply::sse(cut)]).await;
        let (res, events) = run(&cfg_for(addr, ""), "local-direct", "m", &tok("k"), &fast_policy()).await;
        assert!(matches!(res, Err(CallError::Stream(StreamError::Incomplete))), "{:?}", res.err());
        assert!(!events.is_empty(), "deltas had already been shown");
        assert_eq!(log.lock().unwrap().len(), 1, "no silent retry of a partially delivered turn");
    }

    #[tokio::test]
    async fn a_mid_stream_provider_error_surfaces_redacted() {
        let body = make_sse(&[("error", json!({"type": "error", "error": {"type": "overloaded_error", "message": "busy sk-live-SECRET-1234567890"}}))]);
        let (addr, _) = mock(vec![Reply::sse(&body)]).await;
        let (res, _) = run(&cfg_for(addr, ""), "local-direct", "m", &tok("k"), &fast_policy()).await;
        match res.unwrap_err() {
            CallError::Stream(StreamError::Provider { kind, message }) => assert!(kind == "overloaded_error" && !message.contains("SECRET"), "{}", message),
            other => panic!("{:?}", other),
        }
    }

    #[tokio::test]
    async fn cross_origin_redirects_are_refused_and_nothing_follows_the_credentials() {
        let (victim, victim_log) = mock(vec![Reply::sse(&fixture())]).await;
        let redirect = Reply { status: 302, headers: vec![("location".into(), format!("http://{}/steal", victim))], pieces: vec![] };
        let (addr, _) = mock(vec![redirect]).await;
        let (res, _) = run(&cfg_for(addr, ""), "local-direct", "m", &tok("k"), &fast_policy()).await;
        assert!(matches!(res, Err(CallError::CrossOriginRedirect { .. })), "{:?}", res.err());
        assert!(victim_log.lock().unwrap().is_empty(), "the other origin must never be contacted");
    }

    #[tokio::test]
    async fn same_origin_redirects_are_followed() {
        let redirect = Reply { status: 307, headers: vec![("location".into(), "http://{ADDR}/v1/messages-moved".into())], pieces: vec![] };
        let (addr, log) = mock(vec![redirect, Reply::sse(&fixture())]).await;
        let (res, _) = run(&cfg_for(addr, ""), "local-direct", "m", &tok("k"), &fast_policy()).await;
        assert_eq!(res.unwrap().blocks, want_blocks());
        let paths: Vec<String> = log.lock().unwrap().iter().map(|c| c.path.clone()).collect();
        assert_eq!(paths, vec!["/v1/messages".to_string(), "/v1/messages-moved".to_string()]);
    }

    #[test]
    fn backoff_is_exponential_jittered_capped_and_honors_retry_after() {
        let p = RetryPolicy { max_elapsed: Duration::from_secs(60), base: Duration::from_millis(100), cap: Duration::from_secs(2) };
        assert_eq!(backoff_delay(0, None, &p, 1.0), Duration::from_millis(100));
        assert_eq!(backoff_delay(3, None, &p, 1.0), Duration::from_millis(800));
        assert_eq!(backoff_delay(10, None, &p, 1.0), Duration::from_secs(2), "capped");
        assert_eq!(backoff_delay(3, None, &p, 0.5), Duration::from_millis(400), "jittered");
        assert_eq!(backoff_delay(0, Some(Duration::from_secs(7)), &p, 0.1), Duration::from_secs(7), "never shorter than retry-after");
    }
}
