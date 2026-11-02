//! Deterministic trace harvesting and episodic memory distillation (`specs/065` §4).
//!
//! Converts execution logs (`events.jsonl`) into bounded, structured Markdown episode records
//! (`missions/<id>/traces/<role>-<task>.md`) with daemon-enforced YAML frontmatter at zero model token cost.

use delphi_core::event::EventEnvelope;
use delphi_core::provenance::Tier;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

/// Maximum episodes generated per mission to prevent database bloat (`specs/065` §4.1).
pub const MAX_EPISODES_PER_MISSION: usize = 3;

/// Outcome classification for an episodic record (`specs/065` §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeOutcome {
    Success,
    Failure,
    ResolvedAfterFailure,
    RejectedGate,
}

impl EpisodeOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            EpisodeOutcome::Success => "success",
            EpisodeOutcome::Failure => "failure",
            EpisodeOutcome::ResolvedAfterFailure => "resolved_after_failure",
            EpisodeOutcome::RejectedGate => "rejected_gate",
        }
    }
}

/// A structured episode extracted from execution telemetry (`specs/065` §3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub title: String,
    pub author: String,
    pub role: String,
    pub timestamp: String,
    pub kind: String,
    pub source_type: String,
    pub confidence: String,
    pub status: String,
    pub valid_until: Option<String>,
    pub mission: String,
    pub session_id: String,
    pub task_id: String,
    pub skills_used: Vec<String>,
    pub tools_invoked: Vec<String>,
    pub outcome: EpisodeOutcome,
    pub error_fingerprints: Vec<String>,
    pub target_files: Vec<String>,
    pub supersedes: Vec<String>,
    pub tags: Vec<String>,
    pub context_obstacle: String,
    pub tool_signature: String,
    pub resolution_pattern: String,
}

impl Episode {
    /// Frontmatter serialized as JSON for schema validation.
    pub fn frontmatter_json(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.title,
            "author": self.author,
            "role": self.role,
            "timestamp": self.timestamp,
            "kind": self.kind,
            "source_type": self.source_type,
            "confidence": self.confidence,
            "status": self.status,
            "valid_until": self.valid_until,
            "mission": self.mission,
            "session_id": self.session_id,
            "task_id": self.task_id,
            "skills_used": self.skills_used,
            "tools_invoked": self.tools_invoked,
            "outcome": self.outcome.as_str(),
            "error_fingerprints": self.error_fingerprints,
            "target_files": self.target_files,
            "supersedes": self.supersedes,
            "tags": self.tags,
        })
    }

    /// Deterministic deduplication key based on role, primary tool, and error fingerprint (`specs/065` §4.1).
    pub fn dedup_key(&self) -> String {
        let tool = self.tools_invoked.first().map(|s| s.as_str()).unwrap_or("none");
        let err = self.error_fingerprints.first().map(|s| s.as_str()).unwrap_or("none");
        let raw = format!("{}:{}:{}", self.role, tool, err);
        blake3::hash(raw.as_bytes()).to_hex().to_string()
    }

    /// Render episode as standard 3-section Markdown document (`specs/065` §3.2).
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("---\n");
        out.push_str(&format!("id: {}\n", self.id));
        out.push_str(&format!("title: \"{}\"\n", self.title.replace('"', "\\\"")));
        out.push_str(&format!("author: {}\n", self.author));
        out.push_str(&format!("role: {}\n", self.role));
        out.push_str(&format!("timestamp: {}\n", self.timestamp));
        out.push_str("kind: episode\n");
        out.push_str(&format!("source_type: {}\n", self.source_type));
        out.push_str(&format!("confidence: {}\n", self.confidence));
        out.push_str(&format!("status: {}\n", self.status));
        match &self.valid_until {
            Some(v) => out.push_str(&format!("valid_until: {}\n", v)),
            None => out.push_str("valid_until: null\n"),
        }
        out.push_str(&format!("mission: {}\n", self.mission));
        out.push_str(&format!("session_id: {}\n", self.session_id));
        out.push_str(&format!("task_id: {}\n", self.task_id));

        out.push_str(&format!("skills_used: [{}]\n", self.skills_used.join(", ")));
        out.push_str(&format!("tools_invoked: [{}]\n", self.tools_invoked.join(", ")));
        out.push_str(&format!("outcome: {}\n", self.outcome.as_str()));
        
        let escaped_errs: Vec<String> = self.error_fingerprints.iter().map(|e| format!("\"{}\"", e.replace('"', "\\\""))).collect();
        out.push_str(&format!("error_fingerprints: [{}]\n", escaped_errs.join(", ")));

        let escaped_files: Vec<String> = self.target_files.iter().map(|f| format!("\"{}\"", f.replace('"', "\\\""))).collect();
        out.push_str(&format!("target_files: [{}]\n", escaped_files.join(", ")));

        out.push_str(&format!("supersedes: [{}]\n", self.supersedes.join(", ")));
        out.push_str(&format!("tags: [{}]\n", self.tags.join(", ")));
        out.push_str("---\n\n");

        out.push_str(&format!("# Episode: {}\n\n", self.title));
        out.push_str("## Context & Obstacle\n");
        out.push_str(&self.context_obstacle);
        out.push_str("\n\n");

        out.push_str("## Tool & Execution Signature\n");
        out.push_str(&self.tool_signature);
        out.push_str("\n\n");

        out.push_str("## Resolution & Pattern\n");
        out.push_str(&self.resolution_pattern);
        out.push('\n');

        out
    }
}

/// Accumulator for task telemetry events during mining.
#[derive(Default)]
struct TaskAccumulator {
    role: Option<String>,
    session_id: Option<String>,
    timestamp: Option<String>,
    lowest_tier: Option<Tier>,
    tools: Vec<String>,
    skills: Vec<String>,
    errors: Vec<String>,
    files: Vec<String>,
    gate_failed: bool,
    gate_code: Option<String>,
    had_error: bool,
    completed: bool,
    tool_call_count: usize,
}

/// Cleanly extract an error fingerprint from a tool result string or payload.
fn extract_error_fingerprint(text: &str) -> String {
    let line = text.lines().find(|l| {
        let lower = l.to_lowercase();
        lower.contains("error") || lower.contains("failed") || lower.contains("denied") || lower.contains("fatal") || lower.contains("panic")
    }).unwrap_or_else(|| text.lines().next().unwrap_or("unknown error"));
    
    let trimmed = line.trim();
    if trimmed.chars().count() > 100 {
        format!("{}...", trimmed.chars().take(97).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

/// Deterministically harvest episodes from recorded event stream (`specs/065` §4.1).
pub fn harvest_episodes(mission_id: &str, events: &[EventEnvelope]) -> Vec<Episode> {
    let mut tasks: BTreeMap<String, TaskAccumulator> = BTreeMap::new();

    for ev in events {
        let task_id = match &ev.task_id {
            Some(t) if !t.is_empty() => t.clone(),
            _ => continue,
        };

        let acc = tasks.entry(task_id).or_default();

        if acc.timestamp.is_none() && !ev.timestamp.is_empty() {
            acc.timestamp = Some(ev.timestamp.clone());
        }

        if let Some(agent) = &ev.agent {
            if acc.role.is_none() && !agent.is_empty() {
                acc.role = Some(agent.clone());
            }
        }

        if acc.session_id.is_none() {
            if let Some(s) = ev.payload["session_id"].as_str() {
                acc.session_id = Some(s.to_string());
            }
        }

        match ev.kind.as_str() {
            "span.taint" => {
                if let Some(c_str) = ev.payload["ceiling"].as_str() {
                    if let Some(t) = Tier::parse(c_str) {
                        let cur = acc.lowest_tier.unwrap_or(Tier::L5);
                        acc.lowest_tier = Some(cur.min(t));
                    }
                }
            }
            "tool.result" => {
                acc.tool_call_count += 1;
                let name = ev.payload["name"].as_str().unwrap_or_default();
                if !name.is_empty() && !acc.tools.iter().any(|t| t == name) {
                    acc.tools.push(name.to_string());
                }

                let is_err = ev.payload["is_error"].as_bool().unwrap_or(false);
                let content = ev.payload["content"].as_str().unwrap_or_default();

                if is_err {
                    acc.had_error = true;
                    let fp = extract_error_fingerprint(content);
                    if !acc.errors.iter().any(|e| e == &fp) {
                        acc.errors.push(fp);
                    }
                }

                if let Some(file) = ev.payload["path"].as_str() {
                    if !file.is_empty() && !acc.files.iter().any(|f| f == file) {
                        acc.files.push(file.to_string());
                    }
                }
            }
            "harness.bridge_call" => {
                acc.tool_call_count += 1;
                let name = ev.payload["tool"].as_str().unwrap_or_default();
                if !name.is_empty() && !acc.tools.iter().any(|t| t == name) {
                    acc.tools.push(name.to_string());
                }
                if let Some(skill) = ev.payload["skill"].as_str() {
                    if !skill.is_empty() && !acc.skills.iter().any(|s| s == skill) {
                        acc.skills.push(skill.to_string());
                    }
                }
            }
            "gate.eval" => {
                let passed = ev.payload["passed"].as_bool().unwrap_or(true);
                if !passed {
                    acc.gate_failed = true;
                    acc.gate_code = ev.payload["gate"].as_str().map(|s| s.to_string());
                }
            }
            "task.completed" | "task.finish" => {
                acc.completed = true;
            }
            _ => {}
        }
    }

    let mut candidate_episodes: Vec<Episode> = Vec::new();
    let mut seen_dedup_keys: HashSet<String> = HashSet::new();

    for (task_id, acc) in tasks {
        // Significance Filter:
        // Must have:
        // 1. Encountered a gate rejection, OR
        // 2. Encountered a tool error and resolved it, OR
        // 3. Outright failed with error, OR
        // 4. Completed a multi-tool execution (>3 tool calls)
        let outcome = if acc.gate_failed {
            EpisodeOutcome::RejectedGate
        } else if acc.had_error && acc.completed {
            EpisodeOutcome::ResolvedAfterFailure
        } else if acc.had_error && !acc.completed {
            EpisodeOutcome::Failure
        } else if acc.completed && acc.tool_call_count > 3 {
            EpisodeOutcome::Success
        } else {
            continue; // Skip routine/trivial tasks
        };

        let role = acc.role.unwrap_or_else(|| "orchestrator".to_string());
        let session_id = acc.session_id.unwrap_or_else(|| "s_0000".to_string());
        let timestamp = acc.timestamp.unwrap_or_else(|| "2026-11-01T00:00:00Z".to_string());
        
        // Taint calculation: inherits lowest span taint, defaults to internal_empirical
        let tier = acc.lowest_tier.unwrap_or(Tier::L4);
        let source_type = tier.name().to_string();

        let date_part = if timestamp.len() >= 10 {
            &timestamp[0..10]
        } else {
            "2026-11-01"
        };

        let id = format!("ep-{}-{}-{}", date_part, role, task_id.replace('_', "-"));

        let primary_tool = acc.tools.first().cloned().unwrap_or_else(|| "task".to_string());
        let primary_err = acc.errors.first().cloned();

        let title = match (&outcome, &primary_err) {
            (EpisodeOutcome::RejectedGate, _) => format!("{}: Verification gate rejection in task {}", role, task_id),
            (EpisodeOutcome::ResolvedAfterFailure, Some(err)) => format!("{}: Resolving '{}' in {}", role, err, primary_tool),
            (EpisodeOutcome::Failure, Some(err)) => format!("{}: Unresolved failure '{}' in {}", role, err, primary_tool),
            (EpisodeOutcome::Success, _) => format!("{}: Successful multi-tool execution in task {}", role, task_id),
            _ => format!("{}: Execution trace for task {}", role, task_id),
        };

        let context_obstacle = match &primary_err {
            Some(err) => format!("During task `{}`, execution encountered obstacle: `{}`.", task_id, err),
            None => format!("Execution of multi-step task `{}` utilizing {} tool invocations.", task_id, acc.tool_call_count),
        };

        let tool_signature = format!(
            "- Role: `{}`\n- Tools Invoked: {}\n- Files Accessed: {}",
            role,
            if acc.tools.is_empty() { "none".to_string() } else { acc.tools.iter().map(|t| format!("`{}`", t)).collect::<Vec<_>>().join(", ") },
            if acc.files.is_empty() { "none".to_string() } else { acc.files.iter().map(|f| format!("`{}`", f)).collect::<Vec<_>>().join(", ") }
        );

        let resolution_pattern = match outcome {
            EpisodeOutcome::ResolvedAfterFailure => {
                format!("Task successfully completed after resolving `{}` via subsequent retry steps.", primary_err.as_deref().unwrap_or("error"))
            }
            EpisodeOutcome::RejectedGate => {
                format!("Gate {} rejected task output. Requires amendment and spec alignment before merge.", acc.gate_code.as_deref().unwrap_or("verification"))
            }
            EpisodeOutcome::Failure => {
                "Execution halted on unhandled failure; logged for retrospective analysis.".to_string()
            }
            EpisodeOutcome::Success => {
                "Completed multi-step execution cleanly without errors.".to_string()
            }
        };

        let mut tags = vec!["episode".to_string(), role.clone()];
        tags.extend(acc.tools.clone());

        let episode = Episode {
            id,
            title,
            author: role.clone(),
            role,
            timestamp,
            kind: "episode".to_string(),
            source_type,
            confidence: "high".to_string(),
            status: "active".to_string(),
            valid_until: None,
            mission: mission_id.to_string(),
            session_id,
            task_id,
            skills_used: acc.skills,
            tools_invoked: acc.tools,
            outcome,
            error_fingerprints: acc.errors,
            target_files: acc.files,
            supersedes: Vec::new(),
            tags,
            context_obstacle,
            tool_signature,
            resolution_pattern,
        };

        // Deduplication check
        let dedup = episode.dedup_key();
        if seen_dedup_keys.insert(dedup) {
            candidate_episodes.push(episode);
        }
    }

    // Sort candidates: prioritize resolutions and failures over clean successes
    candidate_episodes.sort_by(|a, b| {
        let score = |o: EpisodeOutcome| match o {
            EpisodeOutcome::ResolvedAfterFailure => 0,
            EpisodeOutcome::RejectedGate => 1,
            EpisodeOutcome::Failure => 2,
            EpisodeOutcome::Success => 3,
        };
        score(a.outcome).cmp(&score(b.outcome))
    });

    // Enforce quota limit: at most MAX_EPISODES_PER_MISSION
    candidate_episodes.truncate(MAX_EPISODES_PER_MISSION);
    candidate_episodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_envelope(seq: u64, task_id: Option<&str>, agent: Option<&str>, kind: &str, payload: Value) -> EventEnvelope {
        EventEnvelope {
            v: 1,
            seq,
            timestamp: "2026-11-01T14:30:00Z".to_string(),
            mission_id: "m_test".to_string(),
            task_id: task_id.map(|s| s.to_string()),
            trace_id: None,
            span_id: task_id.map(|s| format!("span_{}", s)),
            parent_span_id: None,
            agent: agent.map(|s| s.to_string()),
            endpoint_id: None,
            access_path: None,
            model_tier: None,
            model_id: None,
            attempt: None,
            mode: None,
            round: None,
            kind: kind.to_string(),
            payload,
            prev: None,
            h: None,
        }
    }

    #[test]
    fn test_harvest_resolved_after_failure() {
        let events = vec![
            mock_envelope(1, Some("t_1"), Some("builder"), "tool.result", json!({"name": "shell-worktree-scoped", "is_error": true, "content": "error: Blocking waiting for file lock on package cache"})),
            mock_envelope(2, Some("t_1"), Some("builder"), "tool.result", json!({"name": "test-runner", "is_error": false, "content": "test passed"})),
            mock_envelope(3, Some("t_1"), Some("builder"), "task.completed", json!({})),
        ];

        let episodes = harvest_episodes("m_test", &events);
        assert_eq!(episodes.len(), 1);
        let ep = &episodes[0];
        assert_eq!(ep.role, "builder");
        assert_eq!(ep.outcome, EpisodeOutcome::ResolvedAfterFailure);
        assert_eq!(ep.source_type, "internal_empirical");
        assert!(ep.error_fingerprints[0].contains("Blocking waiting for file lock"));
        assert!(ep.to_markdown().contains("## Resolution & Pattern"));
    }

    #[test]
    fn test_taint_inheritance_to_external_untrusted() {
        let events = vec![
            mock_envelope(1, Some("t_2"), Some("investigator"), "span.taint", json!({"ceiling": "L0"})),
            mock_envelope(2, Some("t_2"), Some("investigator"), "tool.result", json!({"name": "read_file", "is_error": true, "content": "fatal: corrupted git tree"})),
            mock_envelope(3, Some("t_2"), Some("investigator"), "task.completed", json!({})),
        ];

        let episodes = harvest_episodes("m_test", &events);
        assert_eq!(episodes.len(), 1);
        let ep = &episodes[0];
        assert_eq!(ep.source_type, "external_untrusted");
    }

    #[test]
    fn test_quota_bounds_at_most_three_episodes() {
        let mut events = Vec::new();
        for i in 1..=6 {
            let task = format!("t_{}", i);
            events.push(mock_envelope(i * 2, Some(&task), Some("builder"), "tool.result", json!({"name": format!("tool_{}", i), "is_error": true, "content": format!("error: code {}", i)})));
            events.push(mock_envelope(i * 2 + 1, Some(&task), Some("builder"), "task.completed", json!({})));
        }

        let episodes = harvest_episodes("m_test", &events);
        assert_eq!(episodes.len(), MAX_EPISODES_PER_MISSION);
    }

    #[test]
    fn test_deduplication_of_identical_flapping_errors() {
        let events = vec![
            mock_envelope(1, Some("t_a"), Some("builder"), "tool.result", json!({"name": "cargo", "is_error": true, "content": "error: timeout"})),
            mock_envelope(2, Some("t_a"), Some("builder"), "task.completed", json!({})),
            mock_envelope(3, Some("t_b"), Some("builder"), "tool.result", json!({"name": "cargo", "is_error": true, "content": "error: timeout"})),
            mock_envelope(4, Some("t_b"), Some("builder"), "task.completed", json!({})),
        ];

        let episodes = harvest_episodes("m_test", &events);
        assert_eq!(episodes.len(), 1, "flapping identical errors should deduplicate to 1 episode");
    }
}
