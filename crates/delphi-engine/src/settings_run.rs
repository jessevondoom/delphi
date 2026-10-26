//! Changing settings while Delphi runs (`specs/035` D-17..D-19): the tier a role starts on and the mission's budget ceiling. The operator
//! can ask for a change in their own words; the change is made and the reply says exactly what changed. A change anyone else proposes (the
//! orchestrator unasked, or Delphi noticing a pattern) waits as a suggestion for the operator to accept.

use crate::engine::{tool_result_error, Engine, SpanCtx};
use crate::mission::MissionState;
use crate::settings_store::{self, Setting};
use crate::tools::ToolResult;
use crate::mission::EngineError;
use delphi_core::event::EventEnvelope;
use delphi_core::provenance::Tier;
use serde_json::{json, Value};
use std::sync::Arc;

/// How many times the orchestrator raises one role to one tier, in a mission, before Delphi suggests making it the role's default.
pub const RAISES_BEFORE_A_SUGGESTION: usize = 3;

/// What a change did, in the words the operator is told.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub key: String,
    pub old: String,
    pub new: String,
    pub text: String,
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Is `quote` something the operator wrote in `prompt`? At least eight characters, so "yes" cannot vouch for anything.
pub fn quoted_from(prompt: &str, quote: &str) -> bool {
    let q = normalize(quote);
    q.chars().count() >= 8 && normalize(prompt).contains(&q)
}

impl Engine {
    /// The tier a role starts on in this workspace: the operator's override, else the shipped manifest's.
    pub fn role_default_tier(&self, role: &str) -> String {
        if let Some(t) = settings_store::load(&self.ws.dir).role_tier.get(role) {
            return t.clone();
        }
        self.ws.load_manifest(role).map(|m| m.tier().to_string()).unwrap_or_else(|_| "workhorse".into())
    }

    /// As [`Engine::role_default_tier`] for a manifest already in hand.
    pub(crate) fn default_tier_of(&self, role: &str, manifest: &crate::workspace::AgentManifest) -> String {
        settings_store::load(&self.ws.dir).role_tier.get(role).cloned().unwrap_or_else(|| manifest.tier().to_string())
    }

    /// Make one change and log it. `by` is `operator` (their command), `prompt` (asked for in their words, verified) or `suggestion` (they
    /// accepted one); `suggestion` names the suggestion that was accepted.
    pub async fn apply_setting(&self, m: &Arc<MissionState>, s: &Setting, by: &str, quote: Option<&str>, reason: &str, suggestion: Option<&str>) -> Result<Applied, String> {
        let applied = match s {
            Setting::RoleTier { role, tier } => {
                let shipped = self.ws.load_manifest(role).map(|mf| mf.tier().to_string()).map_err(|e| format!("cannot read the `{}` manifest: {:?}", role, e))?;
                let mut o = settings_store::load(&self.ws.dir);
                let old = o.role_tier.get(role).cloned().unwrap_or_else(|| shipped.clone());
                match tier {
                    Some(t) => {
                        o.role_tier.insert(role.clone(), t.clone());
                    }
                    None => {
                        o.role_tier.remove(role);
                    }
                }
                settings_store::save(&self.ws.dir, &o).map_err(|e| format!("could not save the setting: {}", e))?;
                let new = tier.clone().unwrap_or_else(|| shipped.clone());
                let text = if old == new {
                    format!("{} already starts on {}; nothing changed.", role, new)
                } else if tier.is_none() {
                    format!("{} is back to its shipped default, {} (it started on {}). This applies to tasks that start from now on.", role, new, old)
                } else {
                    format!("{} now starts on {} (it started on {}; shipped: {}). This applies to tasks that start from now on. Undo: /settings reset {}.", role, new, old, shipped, role)
                };
                Applied { key: s.key(), old, new, text }
            }
            Setting::MissionBudget { usd } => {
                let old = m.budget.lock().unwrap().cap_micro() as f64 / 1e6;
                m.append(EventEnvelope::new(&m.id, "budget.cap", json!({"cap": delphi_core::budget::usd_to_micro(*usd), "usd": usd, "by": by}))).await.map_err(|e| e.to_string())?;
                Applied { key: s.key(), old: format!("{}", old), new: format!("{}", usd), text: format!("The mission's budget ceiling is now ${:.2} (it was ${:.2}).", usd, old) }
            }
        };
        let _ = m.append(EventEnvelope::new(&m.id, "setting.changed", json!({"key": applied.key, "old": applied.old, "new": applied.new, "by": by, "quote": quote, "reason": reason, "suggestion": suggestion, "text": applied.text}))).await;
        Ok(applied)
    }

    /// Record a suggestion, unless the same one is already open.
    pub async fn suggest_setting(&self, m: &Arc<MissionState>, s: &Setting, reason: &str, by: &str) -> Option<String> {
        let events = m.replay().ok()?;
        if settings_store::pending(&events).iter().any(|p| p.key == s.key() && p.value == s.value_text()) {
            return None;
        }
        let id = settings_store::next_id(&events);
        let why = crate::observe::sanitize(reason).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect::<String>();
        m.append(EventEnvelope::new(&m.id, "setting.suggested", json!({"id": id, "key": s.key(), "value": s.value_text(), "reason": why, "by": by}))).await.ok()?;
        Some(id)
    }

    /// The `change_setting` tool. Applied only when the operator really asked: this is the root task of a console message, nothing untrusted has
    /// been read in it, and `quote` is the operator's own words from that message. Anything else becomes a suggestion, and the reply says so.
    pub(crate) async fn exec_settings(self: &Arc<Self>, sc: &SpanCtx, args: &Value) -> Result<ToolResult, EngineError> {
        let (Some(key), Some(value)) = (args["key"].as_str(), args["value"].as_str().map(String::from).or_else(|| args["value"].as_f64().map(|n| n.to_string()))) else {
            return Ok(tool_result_error(format!("change_setting needs `key` and `value`. Settable: {}", settings_store::SETTABLE)));
        };
        let setting = match settings_store::parse(key, &json!(value), &self.settings.enabled_agents) {
            Ok(s) => s,
            Err(e) => return Ok(tool_result_error(e)),
        };
        let quote = args["quote"].as_str().unwrap_or("").trim();
        let reason = args["reason"].as_str().unwrap_or("");
        let m = &sc.m;
        let events = m.replay().unwrap_or_default();
        let prompt = events.iter().find(|e| e.kind == "task.created" && e.task_id.as_deref() == Some(sc.task_id.as_str()) && e.payload["source"] == "operator").and_then(|e| e.payload["prompt"].as_str()).unwrap_or("");
        let asked = sc.depth == 0 && sc.span.ceiling() == Tier::L5 && !quote.is_empty() && quoted_from(prompt, quote);
        if asked {
            return Ok(match self.apply_setting(m, &setting, "prompt", Some(quote), reason, None).await {
                Ok(a) => ToolResult { content: format!("{} (Tell the operator exactly this.)", a.text), is_error: false },
                Err(e) => tool_result_error(e),
            });
        }
        let why = if quote.is_empty() { "you did not quote the operator's words, so it is only a suggestion" } else if sc.depth != 0 { "only the operator's own task can change a setting; this one was dispatched" } else if sc.span.ceiling() != Tier::L5 { "something untrusted has been read in this task, so a setting cannot be changed from it" } else { "the quote is not in the operator's message" };
        let by = if quote.is_empty() { "model" } else { "prompt" };
        let id = self.suggest_setting(m, &setting, if reason.is_empty() { "the orchestrator proposed it" } else { reason }, by).await;
        Ok(ToolResult {
            content: match id {
                Some(id) => format!("Not applied: {}. It is recorded as suggestion {} for the operator, who accepts it with `/settings accept {}`. Tell them it is waiting.", why, id, id.trim_start_matches("sg_")),
                None => format!("Not applied: {}. The same suggestion is already waiting for the operator (`/settings`).", why),
            },
            is_error: false,
        })
    }

    /// After the orchestrator raises a role above its default for a task: if it keeps doing that, suggest making it the default (Delphi
    /// learning from use). Once per role and tier: a dismissed suggestion is not made again.
    pub(crate) async fn note_raise(&self, m: &Arc<MissionState>, role: &str, tier: &str) {
        let Ok(events) = m.replay() else { return };
        let key = format!("role_tier.{}", role);
        if events.iter().any(|e| e.kind == "setting.suggested" && e.payload["key"] == key.as_str() && e.payload["value"] == tier) {
            return;
        }
        let raised = events.iter().filter(|e| e.kind == "dag.node.added" && e.payload["role"] == role && e.payload["tier"] == tier && e.payload["default_tier"] != tier).count() + 1;
        if raised >= RAISES_BEFORE_A_SUGGESTION {
            let setting = Setting::RoleTier { role: role.into(), tier: Some(tier.into()) };
            let _ = self.suggest_setting(m, &setting, &format!("the orchestrator has raised {} to {} for {} tasks in this mission", role, tier, raised), "usage").await;
        }
    }
}

/// The tool the orchestrator is offered.
pub fn def() -> delphi_adapter::claude::ToolDef {
    delphi_adapter::claude::ToolDef {
        name: TOOL.into(),
        description: format!("Change one Delphi setting the operator asked you to change, and tell them exactly what changed. Put the operator's own words that ask for it in `quote`, copied exactly from their message; without a verifiable quote nothing is changed and the change waits as a suggestion for them to accept. Settable: {}. Do not use it to change anything the operator did not ask for.", settings_store::SETTABLE),
        input_schema: json!({"type": "object", "required": ["key", "value"], "properties": {
            "key": {"type": "string"}, "value": {"type": "string"},
            "quote": {"type": "string"}, "reason": {"type": "string"}}}),
    }
}

pub const TOOL: &str = "change_setting";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_words_the_operator_wrote_vouch_for_a_change() {
        let prompt = "Please make the Architect start on   Opus from now on, and show me the cost.";
        assert!(quoted_from(prompt, "make the architect start on opus"));
        assert!(quoted_from(prompt, "MAKE THE ARCHITECT START ON OPUS FROM NOW ON"));
        assert!(!quoted_from(prompt, "make the architect start on gemini"));
        assert!(!quoted_from(prompt, "yes"), "too short to vouch for anything");
        assert!(!quoted_from(prompt, ""));
    }
}
