//! The `agent.usage` payload (`specs/005` §2.2, §2.4): everything the ledger is rebuilt from.

use serde::{Deserialize, Serialize};

pub const USAGE_SOURCES: [&str; 2] = ["reported", "estimated"];
pub const COST_MODELS: [&str; 2] = ["metered", "subscription"];
pub const OUTCOMES: [&str; 5] = ["ok", "failed_verification", "rate_limited", "error", "cancelled"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsagePayload {
    pub provider: String,
    /// Uncached input tokens.
    pub input_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub thinking_tokens: u64,
    pub cost_usd: f64,
    pub reserved_usd: f64,
    pub usage_source: String,
    pub cost_model: String,
    pub outcome: String,
    /// The budget reservation this call settles, when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reservation_id: Option<String>,
}

impl UsagePayload {
    pub fn from_value(v: &serde_json::Value) -> Result<UsagePayload, String> {
        let u: UsagePayload = serde_json::from_value(v.clone()).map_err(|e| format!("agent.usage payload: {}", e))?;
        u.validate()?;
        Ok(u)
    }

    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("UsagePayload serializes")
    }

    pub fn validate(&self) -> Result<(), String> {
        if !USAGE_SOURCES.contains(&self.usage_source.as_str()) {
            return Err(format!("usage_source `{}` must be one of {:?}", self.usage_source, USAGE_SOURCES));
        }
        if !COST_MODELS.contains(&self.cost_model.as_str()) {
            return Err(format!("cost_model `{}` must be one of {:?}", self.cost_model, COST_MODELS));
        }
        if !OUTCOMES.contains(&self.outcome.as_str()) {
            return Err(format!("outcome `{}` must be one of {:?}", self.outcome, OUTCOMES));
        }
        for (name, v) in [("cost_usd", self.cost_usd), ("reserved_usd", self.reserved_usd)] {
            if !v.is_finite() || v < 0.0 {
                return Err(format!("{} must be a finite non-negative number", name));
            }
        }
        if self.provider.trim().is_empty() {
            return Err("provider is required".into());
        }
        Ok(())
    }

    /// All tokens a subscription budget counts.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.cache_read_tokens + self.cache_write_tokens + self.output_tokens + self.thinking_tokens
    }
}
