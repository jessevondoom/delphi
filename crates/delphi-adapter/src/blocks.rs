//! The canonical `ContentBlock` turn store (`specs/002` §3): every turn of an agent's trace as typed
//! blocks that keep provider signatures byte-exact. The persisted JSON shape is what
//! `delphi-core::redact` expects (`provider_meta.signature` / `.data`, `replayable`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Provider-owned opaque data. Never edited, only echoed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Claude `thinking.signature`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Claude `redacted_thinking.data`, echoed byte-for-byte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Thinking {
        thought: String,
        #[serde(default = "yes")]
        replayable: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature_dropped_reason: Option<String>,
        #[serde(default)]
        provider_meta: ProviderMeta,
    },
    /// Provider-redacted reasoning: opaque, no plaintext, echoed byte-for-byte.
    RedactedThinking {
        #[serde(default = "yes")]
        replayable: bool,
        #[serde(default)]
        provider_meta: ProviderMeta,
    },
    Text {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    ToolResult {
        tool_call_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

impl ContentBlock {
    /// A reasoning block that can be sent back with its signature: replayable and actually carrying
    /// the provider's signature (or opaque data).
    pub fn is_signed_reasoning(&self) -> bool {
        match self {
            ContentBlock::Thinking { replayable, provider_meta, .. } => *replayable && provider_meta.signature.is_some(),
            ContentBlock::RedactedThinking { replayable, provider_meta } => *replayable && provider_meta.data.is_some(),
            _ => false,
        }
    }

    pub fn is_reasoning(&self) -> bool {
        matches!(self, ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// One turn. Assistant turns record where they were produced so the compiler can decide whether
/// their signed reasoning may be echoed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub turn_index: u32,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub blocks: Vec<ContentBlock>,
}

impl Turn {
    pub fn user(turn_index: u32, blocks: Vec<ContentBlock>) -> Turn {
        Turn { turn_index, role: Role::User, provider: None, endpoint_id: None, model_id: None, blocks }
    }

    pub fn assistant(turn_index: u32, provider: &str, endpoint_id: &str, model_id: &str, blocks: Vec<ContentBlock>) -> Turn {
        Turn { turn_index, role: Role::Assistant, provider: Some(provider.into()), endpoint_id: Some(endpoint_id.into()), model_id: Some(model_id.into()), blocks }
    }

    /// A user turn that carries tool results (a continuation of the current tool loop), as opposed to
    /// fresh operator or handoff text.
    pub fn is_tool_result_turn(&self) -> bool {
        self.role == Role::User && self.blocks.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serialized_shape_matches_what_the_redactor_and_log_expect() {
        let b = ContentBlock::Thinking { thought: "t".into(), replayable: true, signature_dropped_reason: None, provider_meta: ProviderMeta { provider: Some("anthropic".into()), signature: Some("SIG".into()), data: None } };
        assert_eq!(serde_json::to_value(&b).unwrap(), json!({"type": "thinking", "thought": "t", "replayable": true, "provider_meta": {"provider": "anthropic", "signature": "SIG"}}));
        let r = ContentBlock::RedactedThinking { replayable: true, provider_meta: ProviderMeta { provider: Some("anthropic".into()), signature: None, data: Some("OPAQUE==".into()) } };
        assert_eq!(serde_json::to_value(&r).unwrap()["provider_meta"]["data"], "OPAQUE==");
        // And it round-trips through the log's redaction step untouched when nothing is secret.
        let redactor = delphi_core::redact::Redactor::new();
        let mut v = serde_json::to_value(&b).unwrap();
        assert!(!redactor.redact_value(&mut v));
        assert_eq!(serde_json::from_value::<ContentBlock>(v).unwrap(), b);
    }

    #[test]
    fn a_redacted_and_dropped_block_deserializes_as_not_signed() {
        let v = json!({"type": "thinking", "thought": "x", "replayable": false, "signature_dropped_reason": "redacted", "provider_meta": {"provider": "anthropic"}});
        let b: ContentBlock = serde_json::from_value(v).unwrap();
        assert!(!b.is_signed_reasoning() && b.is_reasoning());
    }

    #[test]
    fn signed_reasoning_needs_the_signature_itself() {
        let no_sig = ContentBlock::Thinking { thought: "t".into(), replayable: true, signature_dropped_reason: None, provider_meta: ProviderMeta::default() };
        assert!(!no_sig.is_signed_reasoning());
    }
}
