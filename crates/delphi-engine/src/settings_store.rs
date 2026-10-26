//! Settings the operator changes while Delphi runs (`specs/035` D-17..D-19): which tier a role starts on, and the mission's budget ceiling.
//! The shipped defaults are tuned in the core (`agents/*/AGENT.md`); this file is the operator's word over them, in the workspace, never in
//! the release. It is read afresh whenever a span starts, so a change takes effect on the next task without a restart.
//!
//! Pure functions and one small file; `settings_run.rs` applies a change and logs it.

use delphi_core::event::EventEnvelope;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the overrides live, relative to the workspace.
pub const FILE: &str = "local/settings.toml";
pub const TIERS: [&str; 3] = ["fast", "workhorse", "frontier"];
/// What the tool and the command say can be set, so a refusal tells the operator what to do instead.
pub const SETTABLE: &str = "role_tier.<role> (fast, workhorse, frontier, or default to go back to the shipped one) and mission_budget_usd (a number up to 10000). Permissions, endpoints, access, hooks and the relay are changed with their own commands, never by a prompt";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overrides {
    pub role_tier: BTreeMap<String, String>,
}

pub fn path(ws_dir: &Path) -> PathBuf {
    ws_dir.join(FILE)
}

/// The overrides on disk. A missing or unreadable file means none; a word that is not a tier is ignored, so a hand edit cannot break a span.
pub fn load(ws_dir: &Path) -> Overrides {
    let mut out = Overrides::default();
    let Ok(text) = std::fs::read_to_string(path(ws_dir)) else { return out };
    let Ok(v) = toml::from_str::<toml::Value>(&text) else { return out };
    if let Some(roles) = v.get("roles").and_then(|r| r.as_table()) {
        for (role, t) in roles {
            if let Some(tier) = t.get("default_tier").and_then(|x| x.as_str()).filter(|x| TIERS.contains(x)) {
                out.role_tier.insert(role.clone(), tier.to_string());
            }
        }
    }
    out
}

/// Write the overrides (atomically: a temporary file, then a rename).
pub fn save(ws_dir: &Path, o: &Overrides) -> std::io::Result<()> {
    let p = path(ws_dir);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut roles = toml::value::Table::new();
    for (role, tier) in &o.role_tier {
        let mut t = toml::value::Table::new();
        t.insert("default_tier".into(), toml::Value::String(tier.clone()));
        roles.insert(role.clone(), toml::Value::Table(t));
    }
    let mut root = toml::value::Table::new();
    if !roles.is_empty() {
        root.insert("roles".into(), toml::Value::Table(roles));
    }
    let body = toml::to_string(&toml::Value::Table(root)).map_err(std::io::Error::other)?;
    let tmp = p.with_extension("toml.tmp");
    std::fs::write(&tmp, format!("# Written by Delphi when you change a setting (`/settings`, or by asking). You may edit it by hand.\n{}", body))?;
    std::fs::rename(&tmp, &p)
}

/// One thing the operator can set.
#[derive(Debug, Clone, PartialEq)]
pub enum Setting {
    /// A role's starting tier; `None` goes back to the shipped one.
    RoleTier { role: String, tier: Option<String> },
    MissionBudget { usd: f64 },
}

impl Setting {
    pub fn key(&self) -> String {
        match self {
            Setting::RoleTier { role, .. } => format!("role_tier.{}", role),
            Setting::MissionBudget { .. } => "mission_budget_usd".into(),
        }
    }

    /// The value as the operator would say it.
    pub fn value_text(&self) -> String {
        match self {
            Setting::RoleTier { tier, .. } => tier.clone().unwrap_or_else(|| "default".into()),
            Setting::MissionBudget { usd } => format!("{}", usd),
        }
    }
}

/// A key and a value, as the tool, the command and the RPC give them, checked: or what is wrong and what can be set.
pub fn parse(key: &str, value: &Value, enabled: &[String]) -> Result<Setting, String> {
    let text = match value {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    if let Some(role) = key.strip_prefix("role_tier.") {
        if !enabled.iter().any(|r| r == role) {
            return Err(format!("`{}` is not an enabled role (enabled: {})", role, enabled.join(", ")));
        }
        return match text.as_str() {
            "default" | "reset" => Ok(Setting::RoleTier { role: role.into(), tier: None }),
            t if TIERS.contains(&t) => Ok(Setting::RoleTier { role: role.into(), tier: Some(t.into()) }),
            other => Err(format!("`{}` is not a tier: use fast, workhorse, frontier, or default", other)),
        };
    }
    if key == "mission_budget_usd" {
        return match text.parse::<f64>() {
            Ok(usd) if usd.is_finite() && usd > 0.0 && usd <= 10_000.0 => Ok(Setting::MissionBudget { usd }),
            _ => Err("mission_budget_usd must be a number above 0 and at most 10000".into()),
        };
    }
    Err(format!("`{}` cannot be changed this way. Settable: {}", key, SETTABLE))
}

/// A change somebody proposed that the operator has not answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub id: String,
    pub key: String,
    pub value: String,
    pub reason: String,
    /// `usage` (Delphi noticed a pattern), `model` (the orchestrator, without being asked) or `prompt` (asked for, but it could not be applied).
    pub by: String,
}

/// The suggestions in a log that are still open: suggested, and neither accepted (a `setting.changed` naming it) nor dismissed.
pub fn pending(events: &[EventEnvelope]) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    for e in events {
        match e.kind.as_str() {
            "setting.suggested" => out.push(Suggestion {
                id: e.payload["id"].as_str().unwrap_or("").into(),
                key: e.payload["key"].as_str().unwrap_or("").into(),
                value: e.payload["value"].as_str().unwrap_or("").into(),
                reason: e.payload["reason"].as_str().unwrap_or("").into(),
                by: e.payload["by"].as_str().unwrap_or("").into(),
            }),
            "setting.dismissed" => {
                let id = e.payload["id"].as_str().unwrap_or("");
                out.retain(|s| s.id != id);
            }
            "setting.changed" => {
                if let Some(id) = e.payload["suggestion"].as_str() {
                    out.retain(|s| s.id != id);
                }
                // A change to the same key supersedes an older suggestion for it.
                let key = e.payload["key"].as_str().unwrap_or("");
                out.retain(|s| s.key != key);
            }
            _ => {}
        }
    }
    out
}

/// The next suggestion id.
pub fn next_id(events: &[EventEnvelope]) -> String {
    format!("sg_{}", events.iter().filter(|e| e.kind == "setting.suggested").count() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enabled() -> Vec<String> {
        ["orchestrator", "architect", "builder"].iter().map(|s| s.to_string()).collect()
    }
    fn ev(kind: &str, payload: Value) -> EventEnvelope {
        EventEnvelope::new("m", kind, payload)
    }

    #[test]
    fn overrides_round_trip_and_a_hand_edit_with_a_bad_word_is_ignored() {
        let dir = std::env::temp_dir().join(format!("delphi-settings-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&dir), Overrides::default(), "no file, no overrides");
        let mut o = Overrides::default();
        o.role_tier.insert("architect".into(), "frontier".into());
        o.role_tier.insert("builder".into(), "fast".into());
        save(&dir, &o).unwrap();
        assert_eq!(load(&dir), o);
        std::fs::write(path(&dir), "[roles.architect]\ndefault_tier = \"enormous\"\n[roles.builder]\ndefault_tier = \"fast\"\n").unwrap();
        assert_eq!(load(&dir).role_tier.get("architect"), None);
        assert_eq!(load(&dir).role_tier.get("builder").map(String::as_str), Some("fast"));
        std::fs::write(path(&dir), "this is [not toml").unwrap();
        assert_eq!(load(&dir), Overrides::default(), "a broken file never stops a span");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keys_and_values_are_checked_and_what_cannot_be_set_this_way_says_what_can() {
        assert_eq!(parse("role_tier.architect", &json!("frontier"), &enabled()).unwrap(), Setting::RoleTier { role: "architect".into(), tier: Some("frontier".into()) });
        assert_eq!(parse("role_tier.architect", &json!("default"), &enabled()).unwrap(), Setting::RoleTier { role: "architect".into(), tier: None });
        assert!(parse("role_tier.architect", &json!("huge"), &enabled()).unwrap_err().contains("not a tier"));
        assert!(parse("role_tier.scribe", &json!("fast"), &enabled()).unwrap_err().contains("not an enabled role"));
        assert_eq!(parse("mission_budget_usd", &json!("12.5"), &enabled()).unwrap(), Setting::MissionBudget { usd: 12.5 });
        assert_eq!(parse("mission_budget_usd", &json!(20), &enabled()).unwrap(), Setting::MissionBudget { usd: 20.0 });
        assert!(parse("mission_budget_usd", &json!("-1"), &enabled()).is_err() && parse("mission_budget_usd", &json!("20000"), &enabled()).is_err());
        for k in ["permissions", "access.allowed_endpoints", "hooks.deny", "relay", "exec.sandbox"] {
            let e = parse(k, &json!("anything"), &enabled()).unwrap_err();
            assert!(e.contains("cannot be changed this way") && e.contains("never by a prompt"), "{}: {}", k, e);
        }
    }

    #[test]
    fn a_suggestion_stays_open_until_it_is_accepted_dismissed_or_overtaken() {
        let log = vec![
            ev("setting.suggested", json!({"id": "sg_1", "key": "role_tier.architect", "value": "frontier", "reason": "raised 3 times", "by": "usage"})),
            ev("setting.suggested", json!({"id": "sg_2", "key": "role_tier.builder", "value": "frontier", "reason": "x", "by": "model"})),
            ev("setting.suggested", json!({"id": "sg_3", "key": "mission_budget_usd", "value": "20", "reason": "y", "by": "model"})),
        ];
        assert_eq!(pending(&log).len(), 3);
        assert_eq!(next_id(&log), "sg_4");
        let mut l = log.clone();
        l.push(ev("setting.dismissed", json!({"id": "sg_2"})));
        l.push(ev("setting.changed", json!({"key": "role_tier.architect", "old": "workhorse", "new": "frontier", "by": "operator", "suggestion": "sg_1"})));
        let open = pending(&l);
        assert_eq!(open.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), vec!["sg_3"]);
        l.push(ev("setting.changed", json!({"key": "mission_budget_usd", "old": "5", "new": "30", "by": "operator"})));
        assert!(pending(&l).is_empty(), "a change to the same key makes an older suggestion for it moot");
    }
}
