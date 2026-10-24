//! `delphi-adapter`: how Delphi reaches models (`specs/009`, `specs/002`).

pub mod blocks;
pub mod claude;
pub mod config;
pub mod credential;
pub mod eventstream;
pub mod gemini;
pub mod http;
pub mod ipr;
pub mod mcp;
pub mod oauth2;
pub mod openai;
pub mod platform;
pub mod sigv4;
pub use reqwest;

use crate::blocks::Turn;
use crate::claude::{CompileError, CompileOptions, Compiled, Target};
use crate::config::{AccessPath, Endpoint};
use crate::ipr::Ipr;

/// Which wire dialect an endpoint speaks: a compat gateway always chat-completions, otherwise its family's own.
fn dialect_of(ep: &Endpoint) -> platform::Dialect {
    if ep.path == AccessPath::GatewayCompat {
        platform::Dialect::OpenAi
    } else if ep.family == "gemini" {
        platform::Dialect::Gemini
    } else {
        platform::Dialect::Claude
    }
}

/// Provider name recorded on turns and usage for calls to this endpoint.
pub fn provider_for(ep: &Endpoint) -> &'static str {
    match dialect_of(ep) {
        platform::Dialect::OpenAi => openai::PROVIDER,
        platform::Dialect::Gemini => gemini::PROVIDER,
        platform::Dialect::Claude => claude::PROVIDER,
    }
}

/// The request core for an endpoint's dialect.
pub fn compile_for(ep: &Endpoint, ipr: &Ipr, history: &[Turn], target: &Target, opts: &CompileOptions) -> Result<Compiled, CompileError> {
    match dialect_of(ep) {
        platform::Dialect::OpenAi => openai::compile(ipr, history, opts),
        platform::Dialect::Gemini => gemini::compile(ipr, history, target, opts, &gemini::GeminiOptions::default()),
        platform::Dialect::Claude => claude::compile(ipr, history, target, opts),
    }
}

/// The system prompt part of a compiled body, for sizing a cost reservation.
pub fn system_of<'a>(ep: &Endpoint, body: &'a serde_json::Value) -> &'a serde_json::Value {
    match dialect_of(ep) {
        platform::Dialect::OpenAi => &body["messages"][0],
        platform::Dialect::Gemini => &body["systemInstruction"],
        platform::Dialect::Claude => &body["system"],
    }
}
