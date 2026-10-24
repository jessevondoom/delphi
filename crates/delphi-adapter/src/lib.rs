//! `delphi-adapter`: how Delphi reaches models (`specs/009`, `specs/002`).

pub mod blocks;
pub mod claude;
pub mod config;
pub mod credential;
pub mod eventstream;
pub mod gemini;
pub mod http;
pub mod ipr;
pub mod oauth2;
pub mod platform;
pub mod sigv4;
pub use reqwest;

use crate::blocks::Turn;
use crate::claude::{CompileError, CompileOptions, Compiled, Target};
use crate::ipr::Ipr;

/// Provider name recorded on turns and usage for a model family.
pub fn provider_for(family: &str) -> &'static str {
    if family == "gemini" { gemini::PROVIDER } else { claude::PROVIDER }
}

/// The request core for a family's dialect.
pub fn compile_for(family: &str, ipr: &Ipr, history: &[Turn], target: &Target, opts: &CompileOptions) -> Result<Compiled, CompileError> {
    if family == "gemini" {
        gemini::compile(ipr, history, target, opts, &gemini::GeminiOptions::default())
    } else {
        claude::compile(ipr, history, target, opts)
    }
}

/// The system prompt part of a compiled body, for sizing a cost reservation.
pub fn system_of<'a>(family: &str, body: &'a serde_json::Value) -> &'a serde_json::Value {
    if family == "gemini" { &body["systemInstruction"] } else { &body["system"] }
}
