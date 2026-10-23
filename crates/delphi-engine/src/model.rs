//! The model-call abstraction. The agent loop talks to a [`ModelClient`]; production uses
//! [`HttpModelClient`] (credentials → platform envelope → streaming HTTP), tests use [`ScriptedModel`].

use delphi_adapter::blocks::ContentBlock;
use delphi_adapter::claude::{Completed, Compiled, StreamError, StreamEvent, Usage};
use delphi_adapter::config::{Endpoint, SharedConfig};
use delphi_adapter::credential::{CredentialError, CredentialStore};
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
            ModelError::NeedsReauth { endpoint } => write!(f, "re-authenticate {}", endpoint),
        }
    }
}

impl std::error::Error for ModelError {}

pub trait ModelClient: Send + Sync {
    fn call<'a>(&'a self, endpoint: &'a Endpoint, model_id: &'a str, compiled: &'a Compiled, on_event: OnEvent<'a>) -> CallFuture<'a>;
}

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
                Err(CallError::RateLimited { message, .. }) => Err(ModelError::Throttled(message)),
                Err(CallError::Stream(StreamError::Incomplete)) => Err(ModelError::Interrupted("the stream ended before the message completed".into())),
                Err(CallError::Transport(m)) => Err(ModelError::Interrupted(m)),
                Err(CallError::Stream(StreamError::Provider { kind, message })) if kind == "overloaded_error" => Err(ModelError::Throttled(message)),
                Err(other) => Err(ModelError::Failed(other.to_string())),
            }
        })
    }
}

/// What a [`ScriptedModel`] does on one call.
pub enum Step {
    /// Stream these blocks, then complete.
    Reply { blocks: Vec<ContentBlock>, usage: Usage, stop_reason: &'static str },
    /// Show some deltas, then break the stream.
    Interrupt,
    Throttle,
    NeedsReauth,
    Fail(&'static str),
}

impl Step {
    pub fn text(text: &str) -> Step {
        Step::Reply { blocks: vec![ContentBlock::Text { text: text.into() }], usage: Usage { input_tokens: 1000, cache_read_tokens: 0, cache_write_tokens: 0, output_tokens: 200 }, stop_reason: "end_turn" }
    }

    pub fn tool(id: &str, name: &str, args: Value) -> Step {
        Step::Reply { blocks: vec![ContentBlock::ToolCall { id: id.into(), name: name.into(), arguments: args }], usage: Usage { input_tokens: 1000, cache_read_tokens: 0, cache_write_tokens: 0, output_tokens: 100 }, stop_reason: "tool_use" }
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

/// A deterministic stand-in for the provider: replies from a script and records every request.
pub struct ScriptedModel {
    script: Mutex<VecDeque<Step>>,
    pub seen: Mutex<Vec<Seen>>,
}

impl ScriptedModel {
    pub fn new(steps: Vec<Step>) -> Arc<ScriptedModel> {
        Arc::new(ScriptedModel { script: Mutex::new(steps.into()), seen: Mutex::new(vec![]) })
    }

    pub fn remaining(&self) -> usize {
        self.script.lock().unwrap().len()
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl ModelClient for ScriptedModel {
    fn call<'a>(&'a self, endpoint: &'a Endpoint, model_id: &'a str, compiled: &'a Compiled, on_event: OnEvent<'a>) -> CallFuture<'a> {
        Box::pin(async move {
            self.seen.lock().unwrap().push(Seen { endpoint_id: endpoint.id.clone(), model_id: model_id.into(), body: compiled.body.clone(), betas: compiled.betas.clone(), thinking_disabled_reason: compiled.thinking_disabled_reason.clone() });
            let step = self.script.lock().unwrap().pop_front().unwrap_or(Step::Fail("the script ran out of steps"));
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
                Step::NeedsReauth => Err(ModelError::NeedsReauth { endpoint: endpoint.id.clone() }),
                Step::Fail(m) => Err(ModelError::Failed(m.into())),
            }
        })
    }
}
