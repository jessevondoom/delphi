//! The canonical `EventEnvelope` (`specs/005` §2.2).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Schema major this build writes and understands. Readers refuse any other major.
pub const SCHEMA_MAJOR: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub v: u32,
    pub seq: u64,
    pub timestamp: String,
    pub mission_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    /// Canonical role ID, never a theme name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<u32>,
    #[serde(rename = "type")]
    pub kind: String,
    pub payload: Value,
}

impl EventEnvelope {
    /// A new event for the log to stamp: `v`, `seq` and `timestamp` are assigned on append.
    pub fn new(mission_id: &str, kind: &str, payload: Value) -> EventEnvelope {
        EventEnvelope {
            v: SCHEMA_MAJOR,
            seq: 0,
            timestamp: String::new(),
            mission_id: mission_id.into(),
            task_id: None,
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            agent: None,
            endpoint_id: None,
            access_path: None,
            model_tier: None,
            model_id: None,
            attempt: None,
            mode: None,
            round: None,
            kind: kind.into(),
            payload,
        }
    }

    /// Required fields (`specs/005` §2.2): everywhere `mission_id`, `type`, an object `payload`; on
    /// `agent.*` events also the routing and attribution fields the ledger is rebuilt from.
    pub fn validate(&self) -> Result<(), String> {
        if self.v != SCHEMA_MAJOR {
            return Err(format!("unsupported schema major {} (this build understands {})", self.v, SCHEMA_MAJOR));
        }
        if self.mission_id.trim().is_empty() {
            return Err("mission_id is required".into());
        }
        if self.kind.trim().is_empty() {
            return Err("type is required".into());
        }
        if !self.payload.is_object() {
            return Err("payload must be a JSON object".into());
        }
        if self.kind.starts_with("agent.") {
            let missing: Vec<&str> = [
                ("agent", self.agent.is_some()),
                ("span_id", self.span_id.is_some()),
                ("task_id", self.task_id.is_some()),
                ("endpoint_id", self.endpoint_id.is_some()),
                ("access_path", self.access_path.is_some()),
                ("model_tier", self.model_tier.is_some()),
                ("model_id", self.model_id.is_some()),
                ("attempt", self.attempt.is_some()),
            ]
            .iter()
            .filter(|(_, present)| !present)
            .map(|(name, _)| *name)
            .collect();
            if !missing.is_empty() {
                return Err(format!("{} event is missing required field(s): {}", self.kind, missing.join(", ")));
            }
        }
        if self.kind == "agent.usage" {
            crate::usage::UsagePayload::from_value(&self.payload)?;
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Not valid JSON, or not an envelope.
    Malformed(String),
    /// A schema major this build doesn't understand (including a missing `v`).
    UnknownMajor(Option<u64>),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Malformed(m) => write!(f, "malformed event: {}", m),
            ParseError::UnknownMajor(Some(v)) => write!(f, "unknown schema major {} (this build understands {})", v, SCHEMA_MAJOR),
            ParseError::UnknownMajor(None) => write!(f, "event has no schema version `v`"),
        }
    }
}

/// Parse one log line. The `v` check comes first so an event from a newer schema is refused rather
/// than half-understood; unknown fields in a known major are ignored.
pub fn parse_line(line: &str) -> Result<EventEnvelope, ParseError> {
    let value: Value = serde_json::from_str(line).map_err(|e| ParseError::Malformed(e.to_string()))?;
    match value.get("v").and_then(|v| v.as_u64()) {
        Some(v) if v == SCHEMA_MAJOR as u64 => {}
        other => return Err(ParseError::UnknownMajor(other)),
    }
    serde_json::from_value(value).map_err(|e| ParseError::Malformed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn agent_event() -> EventEnvelope {
        let mut e = EventEnvelope::new("m1", "agent.block.complete", json!({}));
        e.agent = Some("builder".into());
        e.span_id = Some("sp1".into());
        e.task_id = Some("t1".into());
        e.endpoint_id = Some("direct".into());
        e.access_path = Some("api".into());
        e.model_tier = Some("workhorse".into());
        e.model_id = Some("m".into());
        e.attempt = Some(1);
        e
    }

    #[test]
    fn agent_events_need_attribution() {
        assert!(agent_event().validate().is_ok());
        let mut e = agent_event();
        e.endpoint_id = None;
        e.attempt = None;
        let err = e.validate().unwrap_err();
        assert!(err.contains("endpoint_id") && err.contains("attempt"), "{}", err);
        assert!(EventEnvelope::new("m1", "mission.created", json!({})).validate().is_ok());
        // agent.usage payloads are checked too: they are what the ledger is rebuilt from.
        let mut u = agent_event();
        u.kind = "agent.usage".into();
        assert!(u.validate().unwrap_err().contains("agent.usage payload"));
        assert!(EventEnvelope::new("", "mission.created", json!({})).validate().is_err());
        assert!(EventEnvelope::new("m1", "x", json!([1])).validate().is_err());
    }

    #[test]
    fn unknown_major_is_refused_and_unknown_fields_ignored() {
        let ok = r#"{"v":1,"seq":1,"timestamp":"t","mission_id":"m","type":"x","payload":{},"future_field":7}"#;
        assert_eq!(parse_line(ok).unwrap().seq, 1);
        let newer = ok.replace("\"v\":1", "\"v\":2");
        assert_eq!(parse_line(&newer).unwrap_err(), ParseError::UnknownMajor(Some(2)));
        let none = r#"{"seq":1,"mission_id":"m","type":"x","payload":{}}"#;
        assert_eq!(parse_line(none).unwrap_err(), ParseError::UnknownMajor(None));
        assert!(matches!(parse_line("{not json"), Err(ParseError::Malformed(_))));
    }
}
