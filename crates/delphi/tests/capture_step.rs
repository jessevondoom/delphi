//! Spec 124 §5 (AC-3), the text half: the strategist's and the architect's charters end with a capture step that names a staging
//! tool each role really holds, and the skills that make forecasts or record decisions carry the rule and still validate against
//! `schemas/`.
//!
//! What this does not show: that a model follows the step. The scripted half (a correction stages a note for each role, and nothing
//! ratified or in a system prompt changes) is `spec_124_a_scripted_correction_…` in `delphi-engine`; the forecast check is
//! `TRACE-FOR-01` in `strategy_trace.rs`.

use std::path::{Path, PathBuf};

/// The skills this slice changed, with the role each is written for.
const CHANGED: &[(&str, &str)] = &[("strategy-kernel", "strategist"), ("strategy-cascade", "strategist"), ("roadmap-sequencing", "strategist"), ("risk-roi-analysis", "strategist"), ("adr-authoring", "architect")];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn frontmatter(text: &str, what: &str) -> serde_json::Value {
    let rest = text.strip_prefix("---\n").unwrap_or_else(|| panic!("{what} has no frontmatter"));
    let end = rest.find("\n---\n").unwrap_or_else(|| panic!("{what} has no closing ---"));
    serde_yaml::from_str(&rest[..end]).unwrap_or_else(|e| panic!("{what}: {e}"))
}

fn strings(v: &serde_json::Value, key: &str) -> Vec<String> {
    v[key].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

#[test]
fn both_charters_end_with_a_capture_step_that_only_stages() {
    for role in ["strategist", "architect"] {
        let charter = read(&format!("agents/{role}/AGENT.md"));
        let at = charter.find("Capture (`specs/124` §5)").unwrap_or_else(|| panic!("{role}: the charter has no capture step"));
        let step = &charter[at..];
        assert!(!step.contains("\n## "), "{role}: the capture step is the charter's last section");
        // The tool is the one a seat inside Delphi holds, within its real limits (`tools.rs`: 280 characters, a fixed list of kinds,
        // three notes a task).
        for must in ["When the operator corrects you or a Council overrules you, stage one note that says what was wrong", "the `note` tool", "at most 280 characters", "`kind: pitfall`", "three a task are kept", "Note to stage:"] {
            assert!(step.contains(must), "{role}: the capture step lacks {must:?}");
        }
        // Staging only, and what staging does and does not do (`specs/124` §5 item 3, `specs/116` §2).
        for must in ["You hold no path to `lessons.md`, `context/heuristics.md` or any ratified file", "never an instruction and never part of a system prompt", "nothing ratifies a note automatically", "only as a passage of data labelled unreviewed"] {
            assert!(step.contains(must), "{role}: the capture step lacks {must:?}");
        }
        // The host tools that stage are not on these roles' worker lists (`specs/113` §4), so the charter must not promise them.
        for tool in ["propose_note", "suggest_rule", "record_finding", "record_session_summary"] {
            assert!(!charter.contains(&format!("`{tool}`")), "{role}: names `{tool}`, which the role does not hold");
        }
        // No write scope reaches a ratified file or a role's memory.
        let scopes = &frontmatter(&charter, role)["default_access_scope"]["write"];
        for scope in scopes.as_array().unwrap() {
            let s = scope.as_str().unwrap();
            assert!(!s.starts_with("memory") && !s.starts_with("context") && !s.starts_with("local") && s != "*" && s != "**", "{role}: write scope {s}");
        }
    }
    assert!(read("agents/strategist/AGENT.md").contains("carries the label `forecast`, a date (`YYYY-MM-DD`) and a tripwire: an observable that would show it wrong"));
    assert!(read("agents/architect/AGENT.md").contains("carries a reversal trigger and, where a check guards the decision, the name of that check"));
}

#[test]
fn the_changed_skills_still_validate_and_carry_the_rule() {
    let schema: serde_json::Value = serde_json::from_str(&read("schemas/skill-manifest.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("the skill schema compiles");
    for (name, role) in CHANGED {
        let body = read(&format!("skills/{name}/SKILL.md"));
        let fm = frontmatter(&body, name);
        let errors: Vec<String> = validator.iter_errors(&fm).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "{name} does not satisfy schemas/skill-manifest.schema.json: {errors:?}");
        assert!(strings(&fm, "authorized_agents").iter().any(|a| a == role), "{name} authorizes the {role}");
        assert!(strings(&frontmatter(&read(&format!("agents/{role}/AGENT.md")), role), "authorized_skills").iter().any(|s| s == name), "{name} is on the {role}'s list");
        for heading in ["## When to use", "## Steps", "## Done when", "## Pitfalls"] {
            assert!(body.lines().any(|l| l == heading), "{name} has no `{heading}` section");
        }
        if *role == "strategist" {
            assert!(body.contains("Forecast (") && body.contains("Tripwire:"), "{name} says how a forecast is written");
        }
    }
    let adr = read("skills/adr-authoring/SKILL.md");
    for must in ["**Reversal trigger.**", "**Guard.** Where a check exists that would fail if the decision eroded, name it", "- **Guard:**", "stage one note saying what was wrong, as the capture step of your charter says", "it changes no lesson, no rule and no record"] {
        assert!(adr.contains(must), "adr-authoring lacks {must:?}");
    }
}
