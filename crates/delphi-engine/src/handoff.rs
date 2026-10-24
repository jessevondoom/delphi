//! `HandoffEnvelope` (`specs/001` §4): the typed contract between caller and target. Every task, even
//! a direct one from the console, has one (`caller_agent: "operator"` for a root handoff).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPointer {
    /// Workspace-relative logical path, optionally `#Lstart-Lend`.
    pub uri: String,
    #[serde(default)]
    pub heading: String,
    #[serde(default)]
    pub blake3: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpectedOutput {
    pub artifact_type: String,
    #[serde(default)]
    pub required_fields: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoutingDirective {
    /// `fast` | `workhorse` | `frontier`. Tiers, never model IDs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_tier: Option<String>,
    /// `claude` (the default) or `gemini`. Failover never crosses families.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget_tokens: Option<u32>,
    #[serde(default)]
    pub attempt_number: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffEnvelope {
    pub handoff_id: String,
    pub mission_id: String,
    pub parent_span_id: String,
    pub caller_agent: String,
    pub target_agent: String,
    pub objective: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implements_spec: Option<String>,
    #[serde(default)]
    pub context_pointers: Vec<ContextPointer>,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub expected_output: ExpectedOutput,
    #[serde(default)]
    pub routing_directive: RoutingDirective,
}

/// Pointers + objective may not exceed 2,000 tokens, counted as bytes / 4.
pub const MAX_ENVELOPE_BYTES: usize = 8000;

impl HandoffEnvelope {
    /// The root handoff for a console prompt.
    pub fn root(mission_id: &str, task_id: &str, target: &str, prompt: &str, pointers: Vec<ContextPointer>) -> HandoffEnvelope {
        HandoffEnvelope {
            handoff_id: format!("ho_{}", task_id),
            mission_id: mission_id.into(),
            parent_span_id: "operator".into(),
            caller_agent: "operator".into(),
            target_agent: target.into(),
            objective: prompt.into(),
            implements_spec: None,
            context_pointers: pointers,
            constraints: vec!["Read-only work: you cannot run code or change the project; write only the notes and drafts your tools allow.".into()],
            expected_output: ExpectedOutput { artifact_type: "answer".into(), required_fields: vec![] },
            routing_directive: RoutingDirective { model_tier: None, model_family: None, thinking_budget_tokens: None, attempt_number: 1 },
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, v) in [("handoff_id", &self.handoff_id), ("mission_id", &self.mission_id), ("caller_agent", &self.caller_agent), ("target_agent", &self.target_agent), ("objective", &self.objective)] {
            if v.trim().is_empty() {
                return Err(format!("`{}` is required", name));
            }
        }
        if serde_json::to_vec(self).map(|b| b.len()).unwrap_or(usize::MAX) > MAX_ENVELOPE_BYTES {
            return Err(format!("the envelope exceeds {} bytes (2,000 tokens): pass pointers, not content", MAX_ENVELOPE_BYTES));
        }
        for p in &self.context_pointers {
            if p.uri.trim().is_empty() || p.reason.trim().is_empty() {
                return Err("every context pointer needs a uri and a reason".into());
            }
        }
        Ok(())
    }

    /// Block 7: the contract as the target sees it (plain text; the IPR fences and escapes it).
    pub fn contract_text(&self) -> String {
        let mut s = format!("Objective: {}\n", self.objective);
        if !self.constraints.is_empty() {
            s.push_str("Constraints:\n");
            for c in &self.constraints {
                s.push_str(&format!("- {}\n", c));
            }
        }
        s.push_str(&format!("Expected output: {}", self.expected_output.artifact_type));
        if !self.expected_output.required_fields.is_empty() {
            s.push_str(&format!(" (fields: {})", self.expected_output.required_fields.join(", ")));
        }
        s.push_str(&format!("\nFrom: {} → {}\n", self.caller_agent, self.target_agent));
        s
    }

    pub fn reminder_text(&self) -> String {
        format!("Stay within the objective and constraints of this task: {}", self.objective.chars().take(200).collect::<String>())
    }
}

/// `@path` references in a console prompt become pointers (`@project/app/src/lib.rs`, `@specs/009.md`).
pub fn pointers_from_prompt(prompt: &str) -> Vec<ContextPointer> {
    let mut out: Vec<ContextPointer> = Vec::new();
    for word in prompt.split_whitespace() {
        let Some(path) = word.strip_prefix('@') else { continue };
        let path = path.trim_end_matches([',', '.', ';', ':', ')', '?']);
        let looks_like_path = path.contains('/') && !path.starts_with('/') && !path.contains("..");
        // `@project/…` and `@worktree/…` are real logical roots; any other `@` is just a mention marker.
        let uri = if path.starts_with("project/") || path.starts_with("worktree/") { format!("@{}", path) } else { path.to_string() };
        if looks_like_path && !out.iter().any(|p| p.uri == uri) {
            out.push(ContextPointer { uri, heading: String::new(), blake3: String::new(), reason: "referenced by the operator".into() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_handoff_is_valid_and_names_the_operator_as_caller() {
        let h = HandoffEnvelope::root("m1", "t_0001", "orchestrator", "Summarize the retry ladder", vec![]);
        assert!(h.validate().is_ok());
        assert_eq!((h.caller_agent.as_str(), h.parent_span_id.as_str(), h.handoff_id.as_str()), ("operator", "operator", "ho_t_0001"));
        assert!(h.contract_text().contains("Objective: Summarize the retry ladder"));
        assert!(!serde_json::to_string(&h).unwrap().contains("\"model_tier\""), "tiers are omitted unless set");
    }

    #[test]
    fn oversized_or_incomplete_envelopes_are_rejected() {
        let mut h = HandoffEnvelope::root("m1", "t1", "architect", &"x".repeat(MAX_ENVELOPE_BYTES), vec![]);
        assert!(h.validate().unwrap_err().contains("pointers, not content"));
        h = HandoffEnvelope::root("m1", "t1", "architect", "ok", vec![ContextPointer { uri: "specs/a.md".into(), heading: String::new(), blake3: String::new(), reason: String::new() }]);
        assert!(h.validate().is_err());
        h.context_pointers.clear();
        h.objective = "  ".into();
        assert!(h.validate().is_err());
    }

    #[test]
    fn at_references_become_pointers_only_when_they_look_like_logical_paths() {
        let p = pointers_from_prompt("Review @project/app/src/lib.rs, and @specs/009.md. Ping @architect about @/etc/passwd and @../x/y @project/app/src/lib.rs");
        assert_eq!(p.iter().map(|x| x.uri.as_str()).collect::<Vec<_>>(), vec!["@project/app/src/lib.rs", "specs/009.md"]);
    }
}
