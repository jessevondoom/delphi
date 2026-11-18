//! Spec 124 AC-5 to AC-9: each skill the spec adds for the architect and for the strategist's breadth validates against
//! `schemas/`, carries "Done when" and "Pitfalls", is on its role's list, and is retrieved from the task it was written for; and
//! the skill the spec retires is gone from the manifests, the catalogs and the cases.
//!
//! The retrieval here is deterministic and lexical: a role sees only the names and descriptions of its own skills (the discovery
//! tier of `specs/001` §3), so each task in a role's case file is scored against every skill on that role's list by the words
//! they share, and the expected skill must score strictly highest. This is the offline half of the
//! `dev/skill_trigger_eval.py` method. The other half asks a model the same question with the same cases
//! (`python3 -I dev/skill_trigger_eval.py trigger <scratch dir> --role architect`); it spends model calls and no test runs it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One role's new skills and the file that holds its cases.
struct Set {
    role: &'static str,
    skills: &'static [&'static str],
    cases: &'static str,
}

/// The skills spec 124 adds, by role: the architect's (S6, S7, S9) and the strategist's breadth skills (S8). The strategist's
/// `strategy-kernel` and `strategy-cascade` (S5) are held by `crates/delphi-engine/src/strategy_trace.rs`.
const SETS: &[Set] = &[
    Set { role: "architect", skills: &["divergent-options", "quality-scenarios", "adr-authoring", "fitness-functions", "spec-premortem", "boundary-design"], cases: "dev/skill-evals/spec124-architect.json" },
    Set { role: "strategist", skills: &["positioning-and-product-strategy", "advantage-analysis", "scenario-and-premortem"], cases: "dev/skill-evals/spec124-strategist.json" },
];
/// The skill spec 124 section 9 retires; `advantage-analysis` replaces it (the operator, 2026-11-17).
const RETIRED: &str = "competitive-swot-analysis";
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

fn listed_by(role: &str) -> Vec<String> {
    strings(&frontmatter(&root().join("agents").join(role).join("AGENT.md")), "authorized_skills")
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
fn each_new_skill_is_valid_listed_and_complete() {
    let schema: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root().join("schemas/skill-manifest.schema.json")).unwrap()).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("the skill schema compiles");
    for set in SETS {
        let role = set.role;
        let listed = listed_by(role);
        for name in set.skills {
            let path = root().join("skills").join(name).join("SKILL.md");
            let fm = frontmatter(&path);
            let errors: Vec<String> = validator.iter_errors(&fm).map(|e| e.to_string()).collect();
            assert!(errors.is_empty(), "{name} does not satisfy schemas/skill-manifest.schema.json: {errors:?}");
            assert_eq!(fm["name"].as_str(), Some(*name));
            assert_eq!(strings(&fm, "authorized_agents"), [role], "{name} authorizes the {role} and no other role");
            assert!(listed.iter().any(|s| s == name), "{name} is not in the {role}'s authorized_skills");
            let description = fm["description"].as_str().unwrap();
            assert!(description.len() <= 200 && description.contains("Use "), "{name}: a description is at most 200 characters and says when to use the skill");
            let body = std::fs::read_to_string(&path).unwrap();
            for heading in ["## When to use", "## Steps", "## Done when", "## Pitfalls"] {
                assert!(body.lines().any(|l| l == heading), "{name} has no `{heading}` section");
            }
            assert!(body.contains("secondary summaries") && body.contains("OD-3"), "{name} says its outside methods are from secondary summaries until the operator ratifies primary sources (spec 124 OD-3)");
        }
    }
}

#[test]
fn each_new_skill_is_retrieved_from_the_task_it_was_written_for() {
    for set in SETS {
        let (role, file) = (set.role, set.cases);
        let cases: Vec<(String, String, Option<String>)> = serde_json::from_str(&std::fs::read_to_string(root().join(file)).unwrap()).unwrap();
        let listed = listed_by(role);
        let catalog: Vec<(String, BTreeSet<String>)> = listed
            .iter()
            .map(|name| {
                let fm = frontmatter(&root().join("skills").join(name).join("SKILL.md"));
                (name.clone(), words(&format!("{} {}", name, fm["description"].as_str().unwrap())))
            })
            .collect();
        let mut covered = BTreeSet::new();
        for (case_role, task, want) in &cases {
            assert_eq!(case_role, role, "{file} holds the {role}'s cases only");
            // A task no skill covers has no lexical answer; those cases are for the model half of the method.
            let Some(want) = want else { continue };
            assert!(listed.contains(want), "{want} is not on the {role}'s list");
            let task_words = words(task);
            let mut scores: Vec<(usize, &str)> = catalog.iter().map(|(name, w)| (w.intersection(&task_words).count(), name.as_str())).collect();
            scores.sort_by(|a, b| b.cmp(a));
            assert!(scores[0].1 == want && scores[0].0 > scores[1].0, "task {task:?} should retrieve {want}; the scores were {:?}", &scores[..3]);
            covered.insert(want.as_str());
        }
        for name in set.skills {
            assert!(covered.contains(name), "{name} has no case in {file}");
        }
    }
}

// Spec 124 section 9 and AC-9: what the strategist's three skills must say, held so an edit cannot quietly drop it.
#[test]
fn the_strategist_breadth_skills_keep_what_section_9_asks_of_them() {
    let body = |name: &str| std::fs::read_to_string(root().join("skills").join(name).join("SKILL.md")).unwrap();
    let positioning = body("positioning-and-product-strategy");
    for must in ["Competitive alternatives", "Unique attributes", "Value", "Best-fit customer", "Brand architecture", "a roadmap that is a list of features", "Messaging:** only now"] {
        assert!(positioning.contains(must), "positioning-and-product-strategy lacks {must:?}");
    }
    let at = |text: &str, what: &str| text.find(what).unwrap_or_else(|| panic!("{what:?} is missing"));
    assert!(at(&positioning, "**Competitive alternatives:**") < at(&positioning, "**Messaging:**"), "positioning comes before messaging");
    let advantage = body("advantage-analysis");
    for must in ["**Benefit:**", "**Barrier:**", "**Evidence:**", "or it is struck", "Map the components", "**eliminate**", "**reduce**", "**raise**", "**create**"] {
        assert!(advantage.contains(must), "advantage-analysis lacks {must:?}");
    }
    let scenario = body("scenario-and-premortem");
    for must in ["at least three scenarios", "a date by which it will be checked", "Premortem on the chosen strategy"] {
        assert!(scenario.contains(must), "scenario-and-premortem lacks {must:?}");
    }
    for name in ["positioning-and-product-strategy", "advantage-analysis"] {
        assert!(body(name).contains("from model memory, not sourced"), "{name}: a claim about the outside carries a source or that label (the strategist's charter)");
    }
}

// AC-9: the retired skill is in no manifest, catalog or case, and has no package.
#[test]
fn the_retired_swot_skill_is_in_no_manifest_catalog_or_case() {
    assert!(!root().join("skills").join(RETIRED).exists(), "skills/{RETIRED} still exists");
    for entry in std::fs::read_dir(root().join("agents")).unwrap().flatten() {
        let manifest = entry.path().join("AGENT.md");
        if manifest.is_file() {
            assert!(!std::fs::read_to_string(&manifest).unwrap().contains(RETIRED), "{} still names {RETIRED}", manifest.display());
        }
    }
    let mut files = vec![root().join("skills/CATALOG.md"), root().join("docs/reference/catalog.md"), root().join("crates/delphi-engine/tests/fixtures/route/labeled.json"), root().join("crates/delphi-engine/tests/fixtures/route/named.json")];
    files.extend(std::fs::read_dir(root().join("dev/skill-evals")).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")));
    for file in files {
        assert!(!std::fs::read_to_string(&file).unwrap().contains(RETIRED), "{} still names {RETIRED}", file.display());
    }
    assert!(listed_by("strategist").iter().any(|s| s == "advantage-analysis"), "advantage-analysis replaces it on the strategist's list");
}
