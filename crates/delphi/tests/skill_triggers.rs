//! Spec 124 AC-5, AC-6 and AC-8: each skill the spec adds for the architect validates against `schemas/`, carries "Done when" and
//! "Pitfalls", is on the architect's list, and is retrieved from the task it was written for.
//!
//! The retrieval here is deterministic and lexical: a role sees only the names and descriptions of its own skills (the discovery
//! tier of `specs/001` §3), so each task in `dev/skill-evals/spec124-architect.json` is scored against every skill on the
//! architect's list by the words they share, and the expected skill must score strictly highest. This is the offline half of the
//! `dev/skill_trigger_eval.py` method. The other half asks a model the same question with the same cases
//! (`python3 -I dev/skill_trigger_eval.py trigger <scratch dir> --role architect`); it spends model calls and no test runs it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The skills spec 124 adds for the architect, by slice.
const NEW_SKILLS: &[&str] = &["divergent-options", "quality-scenarios", "adr-authoring", "fitness-functions", "spec-premortem", "boundary-design"];
const CASES: &str = "dev/skill-evals/spec124-architect.json";
const STOP: &[&str] = &["with", "that", "this", "from", "when", "into", "uses", "used", "using", "before", "after", "their", "them", "each", "your", "what", "which", "have", "will", "would", "should", "does"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn frontmatter(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let rest = text.strip_prefix("---\n").unwrap_or_else(|| panic!("{} has no frontmatter", path.display()));
    let end = rest.find("\n---\n").unwrap_or_else(|| panic!("{} has no closing ---", path.display()));
    serde_yaml::from_str(&rest[..end]).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn strings(v: &serde_json::Value, key: &str) -> Vec<String> {
    v[key].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// Words of four letters or more, lower case, cut to five letters so "deadlocked" meets "deadlocks" and "options" meets "option".
fn words(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| w.len() >= 4 && !STOP.contains(w))
        .map(|w| w.chars().take(5).collect())
        .collect()
}

#[test]
fn each_new_architect_skill_is_valid_listed_and_complete() {
    let schema: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root().join("schemas/skill-manifest.schema.json")).unwrap()).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("the skill schema compiles");
    let listed = strings(&frontmatter(&root().join("agents/architect/AGENT.md")), "authorized_skills");
    for name in NEW_SKILLS {
        let path = root().join("skills").join(name).join("SKILL.md");
        let fm = frontmatter(&path);
        let errors: Vec<String> = validator.iter_errors(&fm).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "{name} does not satisfy schemas/skill-manifest.schema.json: {errors:?}");
        assert_eq!(fm["name"].as_str(), Some(*name));
        assert_eq!(strings(&fm, "authorized_agents"), ["architect"], "{name} authorizes the architect and no other role");
        assert!(listed.iter().any(|s| s == name), "{name} is not in the architect's authorized_skills");
        let description = fm["description"].as_str().unwrap();
        assert!(description.len() <= 200 && description.contains("Use "), "{name}: a description is at most 200 characters and says when to use the skill");
        let body = std::fs::read_to_string(&path).unwrap();
        for heading in ["## When to use", "## Steps", "## Done when", "## Pitfalls"] {
            assert!(body.lines().any(|l| l == heading), "{name} has no `{heading}` section");
        }
        assert!(body.contains("secondary summaries") && body.contains("OD-3"), "{name} says its outside methods are from secondary summaries until the operator ratifies primary sources (spec 124 OD-3)");
    }
}

#[test]
fn each_new_architect_skill_is_retrieved_from_the_task_it_was_written_for() {
    let cases: Vec<(String, String, Option<String>)> = serde_json::from_str(&std::fs::read_to_string(root().join(CASES)).unwrap()).unwrap();
    let listed = strings(&frontmatter(&root().join("agents/architect/AGENT.md")), "authorized_skills");
    let catalog: Vec<(String, BTreeSet<String>)> = listed
        .iter()
        .map(|name| {
            let fm = frontmatter(&root().join("skills").join(name).join("SKILL.md"));
            (name.clone(), words(&format!("{} {}", name, fm["description"].as_str().unwrap())))
        })
        .collect();
    let mut covered = BTreeSet::new();
    for (role, task, want) in &cases {
        assert_eq!(role, "architect", "this file holds the architect's cases only");
        // A task no skill covers has no lexical answer; those cases are for the model half of the method.
        let Some(want) = want else { continue };
        assert!(listed.contains(want), "{want} is not on the architect's list");
        let task_words = words(task);
        let mut scores: Vec<(usize, &str)> = catalog.iter().map(|(name, w)| (w.intersection(&task_words).count(), name.as_str())).collect();
        scores.sort_by(|a, b| b.cmp(a));
        assert!(scores[0].1 == want && scores[0].0 > scores[1].0, "task {task:?} should retrieve {want}; the scores were {:?}", &scores[..3]);
        covered.insert(want.as_str());
    }
    for name in NEW_SKILLS {
        assert!(covered.contains(name), "{name} has no case in {CASES}");
    }
}
