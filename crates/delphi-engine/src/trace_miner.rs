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
/// How long a harvested episode is retrieved (`specs/116` §6 item 5).
pub const EPISODE_DAYS: i64 = 180;

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
        // Every free-text value is a JSON string (a valid YAML double-quoted scalar): tool output with a backslash, a
        // bracket or a comma cannot break the frontmatter the index and the schema read.
        let q = |s: &str| serde_json::to_string(s).expect("a string serializes");
        let list = |v: &[String]| serde_json::to_string(v).expect("strings serialize");
        out.push_str(&format!("title: {}\n", q(&self.title)));
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
        out.push_str(&format!("mission: {}\n", q(&self.mission)));
        out.push_str(&format!("session_id: {}\n", q(&self.session_id)));
        out.push_str(&format!("task_id: {}\n", q(&self.task_id)));

        out.push_str(&format!("skills_used: {}\n", list(&self.skills_used)));
        out.push_str(&format!("tools_invoked: {}\n", list(&self.tools_invoked)));
        out.push_str(&format!("outcome: {}\n", self.outcome.as_str()));
        out.push_str(&format!("error_fingerprints: {}\n", list(&self.error_fingerprints)));
        out.push_str(&format!("target_files: {}\n", list(&self.target_files)));
        out.push_str(&format!("supersedes: {}\n", list(&self.supersedes)));
        out.push_str(&format!("tags: {}\n", list(&self.tags)));
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
    /// At least one event of the task belongs to a span (a host mission's staging task has none).
    has_span: bool,
}

/// Cleanly extract an error fingerprint from a tool result string or payload.
fn extract_error_fingerprint(text: &str) -> String {
    let line = text.lines().find(|l| {
        let lower = l.to_lowercase();
        lower.contains("error") || lower.contains("failed") || lower.contains("denied") || lower.contains("fatal") || lower.contains("panic")
    }).unwrap_or_else(|| text.lines().next().unwrap_or("unknown error"));
    
    // The fingerprint is shown inside an inline code span in the body (065 §4.1 item 4): no backtick may close it.
    let cleaned = line.trim().replace('`', "'");
    let trimmed = cleaned.as_str();
    if trimmed.chars().count() > 100 {
        format!("{}...", trimmed.chars().take(97).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

/// The tier an episode is stamped with (`specs/108` A1): the lowest `span.taint` ceiling among the task's spans, never
/// above `internal_generated` (an episode is the daemon's summary of what models did, not a measurement or a ratification).
/// A task with no span, or whose spans recorded no taint event, is `external_untrusted`: no record of a clean read is not
/// evidence of one. The task's own ceiling (the `SpanOutcome` a Council or a build finishes with) is never consulted.
fn episode_tier(has_span: bool, lowest_span_taint: Option<Tier>) -> Tier {
    match lowest_span_taint {
        Some(t) if has_span => t.min(Tier::L3),
        _ => Tier::L0,
    }
}

/// A well-formed RFC 3339 date-time (or date) whose first ten characters are the day.
fn valid_time(ts: &str) -> bool {
    ts.is_char_boundary(10) && ts.len() >= 10 && crate::frontmatter::entry_date_ok(&ts[..10])
}

/// Deterministically harvest episodes from recorded event stream (`specs/065` §4.1). An episode is dated by its task's
/// first well-formed event time; `now` (RFC 3339) dates one whose events carry none, so no episode carries a made-up date.
pub fn harvest_episodes(mission_id: &str, events: &[EventEnvelope], now: &str) -> Vec<Episode> {
    let mut tasks: BTreeMap<String, TaskAccumulator> = BTreeMap::new();

    for ev in events {
        let task_id = match &ev.task_id {
            Some(t) if !t.is_empty() => t.clone(),
            _ => continue,
        };

        let acc = tasks.entry(task_id).or_default();

        if acc.timestamp.is_none() && valid_time(&ev.timestamp) {
            acc.timestamp = Some(ev.timestamp.clone());
        }
        acc.has_span |= ev.span_id.as_deref().is_some_and(|s| !s.is_empty());

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

                if let Some(file) = ev.payload["path"].as_str().map(|f| f.replace('`', "'")) {
                    if !file.is_empty() && !acc.files.contains(&file) {
                        acc.files.push(file);
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
        if !acc.has_span {
            continue; // nothing ran in a span (a host staging task): no episode (`specs/108` §5)
        }
        let Some(timestamp) = acc.timestamp.or_else(|| valid_time(now).then(|| now.to_string())) else { continue };
        let source_type = episode_tier(acc.has_span, acc.lowest_tier).name().to_string();
        let date_part = &timestamp[0..10];

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
            // `specs/116` §6 item 5: an episode is retrieved for 180 days.
            valid_until: Some(crate::provisional_notes::date_plus(now, EPISODE_DAYS)),
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

/// Where a task's episode is written, relative to the mission directory: `traces/<role>-<task>.md`, the name the
/// index reads a vector's role from and `schemas/episode.schema.json` states.
pub fn episode_file(role: &str, task_id: &str) -> String {
    format!("traces/{}-{}.md", role, task_id)
}

/// A plain identifier (role or task id) that is safe as a file-name part.
fn plain_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') && s.starts_with(|c: char| c.is_ascii_lowercase())
}

/// The active episodes `ep` supersedes: those of the same mission and of its sibling sessions whose first error fingerprint is
/// `ep`'s and whose outcome was not a resolution, as `(file stem, path)`. Nothing is followed through a symlink.
fn superseded_by(mission_dir: &std::path::Path, ep: &Episode) -> Vec<(String, std::path::PathBuf)> {
    let Some(err) = ep.error_fingerprints.first() else { return Vec::new() };
    let mut out = Vec::new();
    let Some(root) = mission_dir.parent() else { return out };
    for m in std::fs::read_dir(root).into_iter().flatten().flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
        let traces = m.path().join("traces");
        if std::fs::symlink_metadata(&traces).is_ok_and(|t| !t.is_dir()) {
            continue;
        }
        for e in std::fs::read_dir(&traces).into_iter().flatten().flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())) {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".md").filter(|s| !s.starts_with('.')) else { continue };
            let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
            let Some(fm) = crate::workspace::split_frontmatter(&text).0 else { continue };
            let first = fm.get("error_fingerprints").and_then(|v| v.as_sequence()).and_then(|s| s.first()).and_then(|v| v.as_str());
            let s = |k: &str| fm.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            if first == Some(err.as_str()) && s("status") == "active" && s("outcome") != EpisodeOutcome::ResolvedAfterFailure.as_str() {
                out.push((stem.to_string(), e.path()));
            }
        }
    }
    out
}

/// The dedup key of an episode already on disk, read back from its frontmatter.
fn dedup_key_of(text: &str) -> Option<String> {
    let (fm, _) = crate::workspace::split_frontmatter(text);
    let fm = fm?;
    let first = |k: &str| fm.get(k).and_then(|v| v.as_sequence()).and_then(|s| s.first()).and_then(|v| v.as_str()).unwrap_or("none").to_string();
    let role = fm.get("role")?.as_str()?.to_string();
    Some(blake3::hash(format!("{}:{}:{}", role, first("tools_invoked"), first("error_fingerprints")).as_bytes()).to_hex().to_string())
}

/// Harvest one finished task of a mission and write its episode (`specs/108` M4, `specs/065` §4.1 and §6): at most one per
/// task, at most [`MAX_EPISODES_PER_MISSION`] per mission, none that repeats an episode already written for the mission
/// (same role, tool and error), and none for a task with no span events. `redact` is the daemon's secret filter, applied
/// to the whole file. Returns the path written, or `None` when nothing was due. The `traces` directory is never followed
/// through a symlink.
pub fn write_task_episode(mission_dir: &std::path::Path, mission_id: &str, task_id: &str, events: &[EventEnvelope], now: &str, redact: impl Fn(&str) -> String) -> Result<Option<std::path::PathBuf>, String> {
    if !plain_id(task_id) {
        return Err(format!("task id `{}` is not a plain id", task_id));
    }
    let mine: Vec<EventEnvelope> = events.iter().filter(|e| e.task_id.as_deref() == Some(task_id)).cloned().collect();
    let Some(ep) = harvest_episodes(mission_id, &mine, now).into_iter().next() else { return Ok(None) };
    if !plain_id(&ep.role) {
        return Err(format!("role `{}` is not a plain id", ep.role));
    }
    let dir = mission_dir.join("traces");
    match std::fs::symlink_metadata(&dir) {
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => return Err(format!("{} is not a plain directory", dir.display())),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {}", dir.display(), e)),
    }
    let mut existing = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".md") && !name.starts_with('.') && e.file_type().is_ok_and(|t| t.is_file()) {
                existing.push((name, std::fs::read_to_string(e.path()).unwrap_or_default()));
            }
        }
    }
    let suffix = format!("-{}.md", task_id);
    let key = ep.dedup_key();
    if existing.len() >= MAX_EPISODES_PER_MISSION || existing.iter().any(|(n, text)| n.ends_with(&suffix) || dedup_key_of(text).as_deref() == Some(key.as_str())) {
        return Ok(None);
    }
    let mut ep = ep;
    let file = episode_file(&ep.role, task_id);
    // `specs/116` §6 item 5: an episode that resolved an error supersedes the earlier unresolved episodes with the same error
    // fingerprint, in this mission and its sibling sessions. Their files stay, marked superseded, and the index leaves them out.
    let stem = file.trim_start_matches("traces/").trim_end_matches(".md").to_string();
    let earlier = if ep.outcome == EpisodeOutcome::ResolvedAfterFailure { superseded_by(mission_dir, &ep) } else { Vec::new() };
    ep.supersedes.extend(earlier.iter().map(|(s, _)| s.clone()));
    let path = mission_dir.join(&file);
    delphi_core::lock::write_atomic(&path, redact(&ep.to_markdown()).as_bytes()).map_err(|e| format!("{}: {}", path.display(), e))?;
    for (_, old) in earlier {
        if let Ok(text) = std::fs::read_to_string(&old) {
            let _ = delphi_core::lock::write_atomic(&old, text.replacen("\nstatus: active\n", &format!("\nstatus: superseded\nsuperseded_by: {}\n", serde_json::to_string(&stem).unwrap_or_default()), 1).as_bytes());
        }
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-11-13T00:00:00Z";

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
            pool_slot: None,
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

        let episodes = harvest_episodes("m_test", &events, NOW);
        assert_eq!(episodes.len(), 1);
        let ep = &episodes[0];
        assert_eq!(ep.role, "builder");
        assert_eq!(ep.outcome, EpisodeOutcome::ResolvedAfterFailure);
        assert_eq!(ep.source_type, "external_untrusted", "no span.taint event: nothing shows the reads were clean");
        assert!(ep.error_fingerprints[0].contains("Blocking waiting for file lock"));
        assert!(ep.to_markdown().contains("## Resolution & Pattern"));
    }

    fn resolved(task: &str, taint: &[&str]) -> Vec<EventEnvelope> {
        let mut v: Vec<EventEnvelope> = taint.iter().enumerate().map(|(i, c)| mock_envelope(i as u64, Some(task), Some("builder"), "span.taint", json!({"ceiling": c}))).collect();
        v.push(mock_envelope(10, Some(task), Some("builder"), "tool.result", json!({"name": "run_command", "is_error": true, "content": "error: x"})));
        v.push(mock_envelope(11, Some(task), Some("builder"), "task.completed", json!({})));
        v
    }

    #[test]
    fn the_tier_is_the_lowest_span_taint_capped_at_internal_generated_and_never_defaults_upward() {
        let tier = |taint: &[&str]| harvest_episodes("m", &resolved("t_1", taint), NOW)[0].source_type.clone();
        assert_eq!(tier(&[]), "external_untrusted", "no taint event");
        assert_eq!(tier(&["L5"]), "internal_generated", "a span at L5 is capped: an episode is never ratified or empirical");
        assert_eq!(tier(&["L4"]), "internal_generated");
        assert_eq!(tier(&["L5", "L2", "L4"]), "federated_verified", "the lowest of the spans");
        assert_eq!(tier(&["L3", "L0"]), "external_untrusted");
        // A task whose events carry no span (a host staging task) yields no episode at all.
        let mut no_span = resolved("t_2", &["L5"]);
        for e in &mut no_span {
            e.span_id = None;
        }
        assert!(harvest_episodes("m", &no_span, NOW).is_empty());
    }

    #[test]
    fn an_episode_is_dated_by_its_events_or_by_now_never_by_a_fixed_default() {
        let ep = &harvest_episodes("m", &resolved("t_1", &["L3"]), NOW)[0];
        assert_eq!(ep.timestamp, "2026-11-01T14:30:00Z", "the task's first event time");
        let mut undated = resolved("t_1", &["L3"]);
        for e in &mut undated {
            e.timestamp = String::new();
        }
        let ep = &harvest_episodes("m", &undated, "2027-02-03T04:05:06Z")[0];
        assert_eq!(ep.timestamp, "2027-02-03T04:05:06Z");
        assert!(ep.id.starts_with("ep-2027-02-03-builder-"), "{}", ep.id);
        for e in &mut undated {
            e.timestamp = "garbage".into();
        }
        assert_eq!(harvest_episodes("m", &undated, "2027-02-03T04:05:06Z")[0].timestamp, "2027-02-03T04:05:06Z", "a malformed time is not used");
        assert!(harvest_episodes("m", &undated, "").is_empty(), "no usable time at all: no episode rather than an invented date");
    }

    #[test]
    fn frontmatter_stays_valid_yaml_whatever_the_tool_output_says() {
        let mut ev = resolved("t_1", &["L3"]);
        ev[1].payload = json!({"name": "run_command", "is_error": true, "content": "error: C:\\path [a, b] \"q\" `x`", "path": "src/a\\b.rs"});
        let ep = &harvest_episodes("m", &ev, NOW)[0];
        let md = ep.to_markdown();
        let (fm, _) = crate::workspace::split_frontmatter(&md);
        let fm = fm.expect("frontmatter parses");
        assert_eq!(fm["error_fingerprints"][0].as_str(), Some("error: C:\\path [a, b] \"q\" 'x'"));
        assert_eq!(fm["source_type"].as_str(), Some("internal_generated"));
        assert_eq!(fm["kind"].as_str(), Some("episode"));
    }

    #[test]
    fn a_task_episode_is_written_once_within_the_mission_quota_and_never_through_a_symlink() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("m1");
        std::fs::create_dir_all(&dir).unwrap();
        let id = |s: &str| s.to_string();
        let p = write_task_episode(&dir, "m1", "t_1", &resolved("t_1", &["L3"]), NOW, id).unwrap().expect("written");
        assert_eq!(p, dir.join("traces/builder-t_1.md"));
        assert!(write_task_episode(&dir, "m1", "t_1", &resolved("t_1", &["L3"]), NOW, id).unwrap().is_none(), "one per task");
        // Same role, tool and error in another task: a repeat, not a new episode.
        assert!(write_task_episode(&dir, "m1", "t_2", &resolved("t_2", &["L3"]), NOW, id).unwrap().is_none(), "deduplicated across the mission");
        let distinct = |task: &str, err: &str| {
            let mut v = resolved(task, &["L3"]);
            v[1].payload["content"] = json!(err);
            v
        };
        assert!(write_task_episode(&dir, "m1", "t_3", &distinct("t_3", "error: y"), NOW, id).unwrap().is_some());
        assert!(write_task_episode(&dir, "m1", "t_4", &distinct("t_4", "error: z"), NOW, id).unwrap().is_some());
        assert!(write_task_episode(&dir, "m1", "t_5", &distinct("t_5", "error: w"), NOW, id).unwrap().is_none(), "at most three per mission");
        assert_eq!(std::fs::read_dir(dir.join("traces")).unwrap().count(), MAX_EPISODES_PER_MISSION);
        // The redactor sees the whole file.
        let other = t.path().join("m2");
        std::fs::create_dir_all(&other).unwrap();
        let p = write_task_episode(&other, "m2", "t_1", &distinct("t_1", "error: sk-SECRET"), NOW, |s| s.replace("sk-SECRET", "[REDACTED]")).unwrap().unwrap();
        assert!(!std::fs::read_to_string(p).unwrap().contains("sk-SECRET"));
        #[cfg(unix)]
        {
            let m3 = t.path().join("m3");
            std::fs::create_dir_all(&m3).unwrap();
            std::os::unix::fs::symlink(t.path(), m3.join("traces")).unwrap();
            assert!(write_task_episode(&m3, "m3", "t_1", &resolved("t_1", &["L3"]), NOW, id).is_err());
        }
    }

    /// `specs/116` §6 item 5: an episode is retrieved for 180 days, and one that resolved an error supersedes the earlier unresolved
    /// episodes with the same error fingerprint, in sibling sessions too; their files stay, marked, and the index leaves them out.
    #[test]
    fn an_episode_expires_after_180_days_and_a_resolution_supersedes_earlier_episodes_of_the_same_error() {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path();
        let (old_dir, new_dir) = (delphi_core::sessions::dir(ws, "m0"), delphi_core::sessions::dir(ws, "m1"));
        std::fs::create_dir_all(old_dir.join("traces")).unwrap();
        std::fs::create_dir_all(&new_dir).unwrap();
        let fresh = &harvest_episodes("m1", &resolved("t_1", &["L3"]), NOW)[0];
        assert_eq!(fresh.valid_until.as_deref(), Some("2027-05-12"));
        let mut failed = fresh.clone();
        (failed.outcome, failed.mission, failed.task_id) = (EpisodeOutcome::Failure, "m0".into(), "t_9".into());
        std::fs::write(old_dir.join("traces/builder-t_9.md"), failed.to_markdown()).unwrap();
        let mut other = failed.clone();
        other.error_fingerprints = vec!["error: something else".into()];
        std::fs::write(old_dir.join("traces/builder-t_8.md"), other.to_markdown()).unwrap();
        let p = write_task_episode(&new_dir, "m1", "t_1", &resolved("t_1", &["L3"]), NOW, |s| s.to_string()).unwrap().unwrap();
        let new = std::fs::read_to_string(&p).unwrap();
        assert!(new.contains("supersedes: [\"builder-t_9\"]") && new.contains("valid_until: 2027-05-12"), "{new}");
        let old = std::fs::read_to_string(old_dir.join("traces/builder-t_9.md")).unwrap();
        assert!(old.contains("\nstatus: superseded\nsuperseded_by: \"builder-t_1\"\n"), "{old}");
        assert!(std::fs::read_to_string(old_dir.join("traces/builder-t_8.md")).unwrap().contains("\nstatus: active\n"), "another error is untouched");
        // The index leaves the superseded one out of an episodes search.
        let mut kb = crate::kb::Kb::open_in_memory().unwrap();
        kb.sync_lexical(&crate::kb::sources(ws), "t").unwrap();
        let hits = kb.search("builder", &crate::kb::SearchOpts { include_episodes: true, role: Some("builder".into()), legs: crate::kb::Legs::LexicalOnly, today: Some("2026-11-20".into()), ..Default::default() }).unwrap();
        assert!(hits.iter().all(|h| !h.rel_path.ends_with("builder-t_9.md")), "{:?}", hits.iter().map(|h| &h.rel_path).collect::<Vec<_>>());
        assert!(kb.search("builder", &crate::kb::SearchOpts { include_episodes: true, role: Some("builder".into()), legs: crate::kb::Legs::LexicalOnly, today: Some("2027-05-13".into()), ..Default::default() }).unwrap().iter().all(|h| h.zone != "episodes"), "none after 180 days");
    }

    #[test]
    fn a_written_episode_is_indexed_in_the_episodes_zone_under_its_role() {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path();
        let dir = delphi_core::sessions::dir(ws, "m1");
        std::fs::create_dir_all(&dir).unwrap();
        write_task_episode(&dir, "m1", "t_1", &resolved("t_1", &["L3"]), NOW, |s| s.to_string()).unwrap().unwrap();
        let files = crate::kb::sources(ws);
        let f = files.iter().find(|f| f.rel_path == "missions/m1/traces/builder-t_1.md").expect("listed");
        assert_eq!(f.zone, "episodes");
    }

    #[test]
    fn test_taint_inheritance_to_external_untrusted() {
        let events = vec![
            mock_envelope(1, Some("t_2"), Some("investigator"), "span.taint", json!({"ceiling": "L0"})),
            mock_envelope(2, Some("t_2"), Some("investigator"), "tool.result", json!({"name": "read_file", "is_error": true, "content": "fatal: corrupted git tree"})),
            mock_envelope(3, Some("t_2"), Some("investigator"), "task.completed", json!({})),
        ];

        let episodes = harvest_episodes("m_test", &events, NOW);
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

        let episodes = harvest_episodes("m_test", &events, NOW);
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

        let episodes = harvest_episodes("m_test", &events, NOW);
        assert_eq!(episodes.len(), 1, "flapping identical errors should deduplicate to 1 episode");
    }
}
