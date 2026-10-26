//! The model-call abstraction. The agent loop talks to a [`ModelClient`]; production uses
//! [`HttpModelClient`] (credentials → platform envelope → streaming HTTP), tests use [`ScriptedModel`].

use delphi_adapter::blocks::ContentBlock;
use delphi_adapter::claude::{Completed, Compiled, StreamError, StreamEvent, Usage};
use delphi_adapter::config::{Endpoint, SharedConfig};
use delphi_adapter::credential::{CredentialError, CredentialStore};
use delphi_adapter::exhaust::Exhaustion;
use delphi_adapter::http::{call_with_retry, CallError, RetryPolicy};
use delphi_adapter::platform::{build, BuildError, BuildInput};
use serde_json::Value;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type OnEvent<'a> = &'a mut (dyn FnMut(StreamEvent) + Send);
pub type CallFuture<'a> = Pin<Box<dyn Future<Output = Result<Completed, ModelError>> + Send + 'a>>;

#[derive(Debug)]
pub enum ModelError {
    /// Refused or throttled before any content: try the next endpoint in the ratified order.
    Throttled(String),
    /// The model's quota, spend limit or subscription window is used up (`specs/002` §4.1): waiting
    /// here will not help, so go to the next endpoint without retrying this one.
    Exhausted(Exhaustion),
    /// The credential expired and cannot be renewed: pause, never fail over.
    NeedsReauth { endpoint: String },
    /// The endpoint is outside the pin, or the request can't be built.
    Refused(String),
    /// The stream broke mid-turn: the whole turn is discarded.
    Interrupted(String),
    /// Anything else the provider rejected (bad request, auth, server error).
    Failed(String),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelError::Throttled(m) | ModelError::Refused(m) | ModelError::Interrupted(m) | ModelError::Failed(m) => write!(f, "{}", m),
            ModelError::Exhausted(e) => write!(f, "quota exhausted: {}", e),
            ModelError::NeedsReauth { endpoint } => write!(f, "re-authenticate {}", endpoint),
        }
    }
}

impl std::error::Error for ModelError {}

pub trait ModelClient: Send + Sync {
    fn call<'a>(&'a self, endpoint: &'a Endpoint, model_id: &'a str, compiled: &'a Compiled, on_event: OnEvent<'a>) -> CallFuture<'a>;

    /// The API key a harness endpoint authenticates its CLI with, when its `credential` is a key source (`keychain:`,
    /// `env:`, `command:`) rather than `harness-login`. `None` leaves the CLI on its own login (`specs/021` §4).
    fn harness_key<'a>(&'a self, _endpoint: &'a Endpoint) -> KeyFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

pub type KeyFuture<'a> = Pin<Box<dyn Future<Output = Result<Option<delphi_adapter::credential::Secret>, ModelError>> + Send + 'a>>;

/// The real client.
pub struct HttpModelClient {
    pub cfg: SharedConfig,
    pub creds: Arc<CredentialStore>,
    pub http: delphi_adapter::reqwest::Client,
    pub retry: RetryPolicy,
    pub idle_timeout: Duration,
    /// Masks secrets in error text (the event redactor, plus registered credential literals).
    pub redact: Arc<dyn Fn(&str) -> String + Send + Sync>,
    /// Receives secret header values for redaction.
    pub register: Arc<dyn Fn(&str) + Send + Sync>,
}

impl ModelClient for HttpModelClient {
    fn harness_key<'a>(&'a self, endpoint: &'a Endpoint) -> KeyFuture<'a> {
        Box::pin(async move {
            if endpoint.credential == "harness-login" {
                return Ok(None);
            }
            // Credential commands are blocking subprocesses.
            let (creds, id, cred) = (self.creds.clone(), endpoint.id.clone(), endpoint.credential.clone());
            let resolved = tokio::task::spawn_blocking(move || creds.get(&id, &cred)).await.map_err(|e| ModelError::Failed(e.to_string()))?;
            match resolved {
                Ok(delphi_adapter::credential::Resolved::Token(t)) => Ok(Some(t)),
                Ok(_) => Err(ModelError::Failed(format!("endpoint `{}`: a harness API key must come from `keychain:`, `env:` or `command:`", endpoint.id))),
                Err(CredentialError::NeedsReauth { endpoint }) => Err(ModelError::NeedsReauth { endpoint }),
                Err(other) => Err(ModelError::Failed((self.redact)(&other.to_string()))),
            }
        })
    }

    fn call<'a>(&'a self, endpoint: &'a Endpoint, model_id: &'a str, compiled: &'a Compiled, on_event: OnEvent<'a>) -> CallFuture<'a> {
        Box::pin(async move {
            // Credential commands are blocking subprocesses.
            let (creds, id, cred) = (self.creds.clone(), endpoint.id.clone(), endpoint.credential.clone());
            let resolved = tokio::task::spawn_blocking(move || creds.get(&id, &cred)).await.map_err(|e| ModelError::Failed(e.to_string()))?;
            let resolved = resolved.map_err(|e| match e {
                CredentialError::NeedsReauth { endpoint } => ModelError::NeedsReauth { endpoint },
                other => ModelError::Failed((self.redact)(&other.to_string())),
            })?;
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            let req = build(&BuildInput { cfg: &self.cfg.get(), endpoint, model_id, compiled, creds: &resolved, now_unix: now, register: &*self.register }).map_err(|e| match e {
                BuildError::NotAllowed(id) => ModelError::Refused(format!("endpoint `{}` is not in allowed_endpoints", id)),
                other => ModelError::Refused(other.to_string()),
            })?;
            let redact = self.redact.clone();
            match call_with_retry(&self.http, &req, self.idle_timeout, &self.retry, &*redact, on_event).await {
                Ok(done) => Ok(done),
                Err(e) => Err(map_call_error(e, now)),
            }
        })
    }
}

/// How a failed call is told to the agent loop. A quota error that arrives inside a stream (an HTTP 200 followed by an
/// error event) is classified by its text like a CLI's, so it is exhaustion and not a throttle that backs off in vain.
pub fn map_call_error(e: CallError, now: i64) -> ModelError {
    match e {
        CallError::RateLimited { message, .. } => ModelError::Throttled(message),
        CallError::Exhausted(e) => ModelError::Exhausted(e),
        CallError::Stream(StreamError::Incomplete) => ModelError::Interrupted("the stream ended before the message completed".into()),
        CallError::Unreachable(m) => ModelError::Throttled(m), // nothing was sent: no charge, next endpoint
        CallError::Transport(m) => ModelError::Interrupted(m),
        CallError::Stream(StreamError::Provider { kind, message }) => match delphi_adapter::exhaust::classify_cli_error(&message, now) {
            Some(x) => ModelError::Exhausted(x),
            None if matches!(kind.as_str(), "overloaded_error" | "rate_limit_error" | "throttlingException" | "ThrottlingException" | "serviceUnavailableException" | "modelNotReadyException" | "RESOURCE_EXHAUSTED" | "UNAVAILABLE" | "rate_limit_exceeded") => ModelError::Throttled(message),
            None => ModelError::Failed(CallError::Stream(StreamError::Provider { kind, message }).to_string()),
        },
        other => ModelError::Failed(other.to_string()),
    }
}

/// What a [`ScriptedModel`] does on one call.
pub enum Step {
    /// Stream these blocks, then complete.
    Reply { blocks: Vec<ContentBlock>, usage: Usage, stop_reason: &'static str },
    /// Show some deltas, then break the stream.
    Interrupt,
    Throttle,
    /// The quota is used up; resets 5 days from the call (a script cannot know the real clock).
    Exhaust,
    /// The account is out of credit: every model on the endpoint is out (`Exhaustion::is_account_wide`).
    ExhaustBilling,
    NeedsReauth,
    Fail(&'static str),
    /// The call never returns (the daemon is killed while waiting for the provider).
    Hang,
}

impl Step {
    pub fn text(text: &str) -> Step {
        Step::Reply { blocks: vec![ContentBlock::Text { text: text.into() }], usage: Usage { input_tokens: 1000, cache_read_tokens: 0, cache_write_tokens: 0, output_tokens: 200, thinking_tokens: 0 }, stop_reason: "end_turn" }
    }

    pub fn tool(id: &str, name: &str, args: Value) -> Step {
        Step::Reply { blocks: vec![ContentBlock::ToolCall { id: id.into(), name: name.into(), arguments: args }], usage: Usage { input_tokens: 1000, cache_read_tokens: 0, cache_write_tokens: 0, output_tokens: 100, thinking_tokens: 0 }, stop_reason: "tool_use" }
    }
}

/// One recorded request.
#[derive(Debug, Clone)]
pub struct Seen {
    pub endpoint_id: String,
    pub model_id: String,
    pub body: Value,
    pub betas: Vec<String>,
    pub thinking_disabled_reason: Option<String>,
}

type Responder = Box<dyn Fn(&Value) -> Step + Send + Sync>;

/// A deterministic stand-in for the provider: replies from a script and records every request.
pub struct ScriptedModel {
    script: Mutex<VecDeque<Step>>,
    /// When set, each call is answered by this function of the request body instead of the next scripted step
    /// (for tests where several spans run at once and the order of calls is not fixed).
    responder: Mutex<Option<Responder>>,
    pub seen: Mutex<Vec<Seen>>,
    /// The API key a harness endpoint resolves to in tests.
    pub harness_key: Mutex<Option<String>>,
}

impl ScriptedModel {
    pub fn new(steps: Vec<Step>) -> Arc<ScriptedModel> {
        Arc::new(ScriptedModel { script: Mutex::new(steps.into()), responder: Mutex::new(None), seen: Mutex::new(vec![]), harness_key: Mutex::new(None) })
    }

    pub fn respond_with(&self, f: impl Fn(&Value) -> Step + Send + Sync + 'static) {
        *self.responder.lock().unwrap() = Some(Box::new(f));
    }

    pub fn remaining(&self) -> usize {
        self.script.lock().unwrap().len()
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl ModelClient for ScriptedModel {
    fn harness_key<'a>(&'a self, endpoint: &'a Endpoint) -> KeyFuture<'a> {
        Box::pin(async move { Ok(if endpoint.credential == "harness-login" { None } else { self.harness_key.lock().unwrap().clone().map(|k| delphi_adapter::credential::Secret::new(k, None)) }) })
    }

    fn call<'a>(&'a self, endpoint: &'a Endpoint, model_id: &'a str, compiled: &'a Compiled, on_event: OnEvent<'a>) -> CallFuture<'a> {
        Box::pin(async move {
            self.seen.lock().unwrap().push(Seen { endpoint_id: endpoint.id.clone(), model_id: model_id.into(), body: compiled.body.clone(), betas: compiled.betas.clone(), thinking_disabled_reason: compiled.thinking_disabled_reason.clone() });
            let answered = self.responder.lock().unwrap().as_ref().map(|r| r(&compiled.body));
            let step = answered.unwrap_or_else(|| self.script.lock().unwrap().pop_front().unwrap_or(Step::Fail("the script ran out of steps")));
            match step {
                Step::Reply { blocks, usage, stop_reason } => {
                    for (i, b) in blocks.iter().enumerate() {
                        if let ContentBlock::Text { text } = b {
                            on_event(StreamEvent::TextDelta { index: i, text: text.clone() });
                        }
                    }
                    Ok(Completed { blocks, usage, stop_reason: Some(stop_reason.into()) })
                }
                Step::Interrupt => {
                    on_event(StreamEvent::TextDelta { index: 0, text: "partial ".into() });
                    Err(ModelError::Interrupted("the stream ended before the message completed".into()))
                }
                Step::Throttle => Err(ModelError::Throttled("slow down".into())),
                Step::Exhaust => {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
                    Err(ModelError::Exhausted(Exhaustion { reason: "quota reached".into(), reset_at: Some(now + 5 * 86_400), raw: "Individual quota reached. Resets in 120h0m0s.".into() }))
                }
                Step::ExhaustBilling => Err(ModelError::Exhausted(Exhaustion { reason: delphi_adapter::exhaust::BILLING.into(), reset_at: None, raw: "Your credit balance is too low.".into() })),
                Step::NeedsReauth => Err(ModelError::NeedsReauth { endpoint: endpoint.id.clone() }),
                Step::Fail(m) => Err(ModelError::Failed(m.into())),
                Step::Hang => std::future::pending().await,
            }
        })
    }
}

#[cfg(test)]
mod map_tests {
    use super::*;
    use delphi_adapter::claude::StreamError;

    const NOW: i64 = 1_790_000_000;

    fn stream(kind: &str, message: &str) -> CallError {
        CallError::Stream(StreamError::Provider { kind: kind.into(), message: message.into() })
    }

    // Independent review (Gemini, 2026-10-25): a quota error delivered inside an HTTP 200 stream was always a throttle.
    #[test]
    fn a_quota_error_inside_the_stream_is_exhaustion_and_an_ordinary_overload_is_still_a_throttle() {
        match map_call_error(stream("RESOURCE_EXHAUSTED", "Individual quota reached. Please upgrade. Resets in 10h0m0s."), NOW) {
            ModelError::Exhausted(x) => assert_eq!(x.reset_at, Some(NOW + 36_000)),
            other => panic!("{:?}", other),
        }
        assert!(matches!(map_call_error(stream("RESOURCE_EXHAUSTED", "too many requests, slow down"), NOW), ModelError::Throttled(_)), "no quota wording and no reset: unknown, so a throttle");
        assert!(matches!(map_call_error(stream("overloaded_error", "Overloaded"), NOW), ModelError::Throttled(_)));
        assert!(matches!(map_call_error(stream("invalid_request_error", "bad"), NOW), ModelError::Failed(_)));
        assert!(matches!(map_call_error(CallError::Stream(StreamError::Incomplete), NOW), ModelError::Interrupted(_)));
        assert!(matches!(map_call_error(CallError::Unreachable("dns".into()), NOW), ModelError::Throttled(_)));
    }
}
