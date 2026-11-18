//! Spec 124 S12 (G-6, AC-3, AC-4, AC-6 to AC-8): the evidence for the new skills that needs no model.
//!
//! Two things are held here, both from data under `dev/`, so that adding a skill or a case is a change to data:
//!
//! 1. **Trigger tests.** Every skill in `dev/train/spec124/skills.toml` is retrieved from the task it was written for and is not
//!    retrieved from a task that only resembles it. Retrieval is the lexical method of `skill_triggers.rs`: a role sees the names
//!    and descriptions of its own skills, a task is scored against each by the words they share, and a skill is retrieved when it
//!    scores strictly highest with at least two shared words. This says a description carries the words of its task. It does not
//!    say a model picks the skill: that is the other half (`dev/skill_trigger_eval.py trigger`), which spends model calls.
//! 2. **The `strategy_trace` cases.** For each `trace-*` case of `dev/train/suites/spec124`, the tool's own function is run on the
//!    case's fixture and must report exactly the findings in the case's `expect.toml`. Then the checks a model run would be graded
//!    by (`case.toml`) are run on the text the tool prints: they pass on their own case and fail on every other, so a seat that
//!    quoted the tool faithfully would pass and one that quoted a different document's findings would not.
//!
//! The capture cases' no-model arm is `spec_124_s12_the_capture_cases_…` in `delphi-engine`; the "Options considered" check and
//! its samples are in `src/train_options.rs`. What none of this shows is in `dev/train/spec124/EVIDENCE.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const STOP: &[&str] = &["with", "that", "this", "from", "when", "into", "uses", "used", "using", "before", "after", "their", "them", "each", "your", "what", "which", "have", "will", "would", "should", "does"];
/// A skill is retrieved only when it shares at least this many words with the task: one shared word is a coincidence.
const MIN_SHARED: usize = 2;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn frontmatter(rel: &str) -> serde_json::Value {
    let text = read(rel);
    let rest = text.strip_prefix("---\n").unwrap_or_else(|| panic!("{rel} has no frontmatter"));
    let end = rest.find("\n---\n").unwrap_or_else(|| panic!("{rel} has no closing ---"));
    serde_yaml::from_str(&rest[..end]).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn strings(v: &serde_json::Value, key: &str) -> Vec<String> {
    v[key].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// Words of four letters or more, lower case, cut to five letters, as in `skill_triggers.rs`.
fn words(text: &str) -> BTreeSet<String> {
    text.to_lowercase().split(|c: char| !c.is_ascii_alphabetic()).filter(|w| w.len() >= 4 && !STOP.contains(w)).map(|w| w.chars().take(5).collect()).collect()
}

/// The score of every skill on a role's list for a task, by name.
fn scores(catalog: &[(String, BTreeSet<String>)], task: &str) -> BTreeMap<String, usize> {
    let t = words(task);
    catalog.iter().map(|(name, w)| (name.clone(), w.intersection(&t).count())).collect()
}

/// The skill a task retrieves: the one that scores strictly highest, with at least `MIN_SHARED` words. Else none.
fn retrieved(scores: &BTreeMap<String, usize>) -> Option<&str> {
    let mut by: Vec<(usize, &str)> = scores.iter().map(|(n, s)| (*s, n.as_str())).collect();
    by.sort_by(|a, b| b.cmp(a));
    (by[0].0 >= MIN_SHARED && by[0].0 > by.get(1).map_or(0, |s| s.0)).then_some(by[0].1)
}

#[test]
fn every_new_skill_is_retrieved_from_its_task_and_not_from_one_that_resembles_it() {
    let manifest: toml::Table = read("dev/train/spec124/skills.toml").parse().unwrap();
    let skills: Vec<(String, String)> = manifest["skill"].as_array().unwrap().iter().map(|s| (s["name"].as_str().unwrap().to_string(), s["role"].as_str().unwrap().to_string())).collect();
    assert!(skills.len() >= 9, "the nine skills of S2, S5, S6, S7 and S9 at least");
    let schema: serde_json::Value = serde_json::from_str(&read("schemas/skill-manifest.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("the skill schema compiles");
    let roles: BTreeSet<&str> = skills.iter().map(|(_, r)| r.as_str()).collect();
    for role in roles {
        let listed = strings(&frontmatter(&format!("agents/{role}/AGENT.md")), "authorized_skills");
        let catalog: Vec<(String, BTreeSet<String>)> = listed.iter().map(|name| (name.clone(), words(&format!("{} {}", name, frontmatter(&format!("skills/{name}/SKILL.md"))["description"].as_str().unwrap())))).collect();
        let file = format!("dev/skill-evals/spec124-{role}.json");
        let cases: Vec<(String, String, Option<String>)> = serde_json::from_str(&read(&file)).unwrap();
        // Every case of the file holds, whichever skill it names: a named skill is retrieved, and a task for no skill retrieves none.
        for (r, task, want) in &cases {
            assert_eq!(r, role, "{file} holds the {role}'s cases only");
            let s = scores(&catalog, task);
            if let Some(w) = want {
                assert!(listed.contains(w), "{w} is not on the {role}'s list");
            }
            assert_eq!(retrieved(&s), want.as_deref(), "{role}: task {task:?}; scores {s:?}");
        }
        for (name, _) in skills.iter().filter(|(_, r)| r == role) {
            let path = format!("skills/{name}/SKILL.md");
            let fm = frontmatter(&path);
            let errors: Vec<String> = validator.iter_errors(&fm).map(|e| e.to_string()).collect();
            assert!(errors.is_empty(), "{name} does not satisfy schemas/skill-manifest.schema.json: {errors:?}");
            assert!(listed.contains(name), "{name} is not on the {role}'s list");
            assert!(strings(&fm, "authorized_agents").iter().any(|a| a == role), "{name} does not authorize the {role}");
            let body = read(&path);
            for heading in ["## Done when", "## Pitfalls"] {
                assert!(body.lines().any(|l| l == heading), "{name} has no `{heading}` section");
            }
            let positives = cases.iter().filter(|(_, _, want)| want.as_deref() == Some(name.as_str())).count();
            assert!(positives >= 2, "{name}: {file} needs two tasks written for it, has {positives}");
            // A negative case that means something: the task shares a word with the skill, and still does not retrieve it.
            let near = cases.iter().filter(|(_, task, want)| want.as_deref() != Some(name.as_str()) && scores(&catalog, task)[name] >= 1).count();
            assert!(near >= 1, "{name}: {file} has no task that resembles the skill and must not retrieve it");
        }
    }
}

/// AC-10: the report exists, says what was and was not shown, and carries one verdict line.
#[test]
fn the_report_says_what_was_and_was_not_shown() {
    let report = read("dev/train/spec124/EVIDENCE.md");
    for heading in ["## Method", "## Arms", "## Results", "## What was not shown"] {
        assert!(report.lines().any(|l| l == heading), "the report has no `{heading}` section");
    }
    assert_eq!(report.lines().filter(|l| l.starts_with("**Verdict:**")).count(), 1, "one verdict line");
}

/// The findings of a strategy document as `<code> <item>`, in the order the tool reports them.
fn found(text: &str) -> Vec<String> {
    delphi_engine::strategy_trace::trace(text).0.iter().map(|f| format!("{} {}", f.code, f.id)).collect()
}

/// Do a case's `text_matches` checks pass on this text? As `delphi train run` evaluates them: case-insensitive.
fn text_checks_pass(case: &toml::Table, text: &str) -> bool {
    let checks: Vec<&toml::Table> = case["check"].as_array().unwrap().iter().filter_map(|c| c.as_table()).filter(|c| c["kind"].as_str() == Some("text_matches")).collect();
    assert!(!checks.is_empty());
    checks.iter().all(|c| {
        let re = regex::RegexBuilder::new(c["pattern"].as_str().unwrap()).case_insensitive(true).build().unwrap();
        re.is_match(text) == c.get("present").and_then(|p| p.as_bool()).unwrap_or(true)
    })
}

#[test]
fn the_strategy_trace_cases_report_exactly_their_planted_findings_with_no_model() {
    let dir = root().join("dev/train/suites/spec124/cases");
    let mut ids: Vec<String> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("trace-")).collect();
    ids.sort();
    // (id, the case, what the tool prints for its fixture, the findings)
    let mut all: Vec<(String, toml::Table, String, Vec<String>)> = Vec::new();
    for id in &ids {
        let text = std::fs::read_to_string(dir.join(id).join("fixture/strategy.md")).unwrap();
        let expect: toml::Table = std::fs::read_to_string(dir.join(id).join("expect.toml")).unwrap().parse().unwrap();
        let want: Vec<String> = expect["findings"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect();
        assert_eq!(found(&text), want, "{id}: the findings of fixture/strategy.md");
        let case: toml::Table = std::fs::read_to_string(dir.join(id).join("case.toml")).unwrap().parse().unwrap();
        assert_eq!(case["role"].as_str(), Some("strategist"), "{id}: the tool is offered to the strategist");
        let (printed, _) = delphi_engine::strategy_trace::report("strategy.md", &text);
        all.push((id.clone(), case, printed, want));
    }
    // What the cases cover between them (AC-3 and AC-4): each planted fault alone, all together, and a clean trace.
    let alone = |code: &str| all.iter().any(|(_, _, _, f)| !f.is_empty() && f.iter().all(|x| x.starts_with(code)));
    for code in ["TRACE-ACT-01", "TRACE-POL-01", "TRACE-DIA-01", "TRACE-FOR-01"] {
        assert!(alone(code), "no case plants {code} alone");
        assert!(all.iter().any(|(_, _, _, f)| f.len() >= 4 && f.iter().any(|x| x.starts_with(code))), "no case plants {code} with the others");
    }
    assert_eq!(all.iter().filter(|(_, _, _, f)| f.is_empty()).count(), 1, "one clean trace");
    // The checks of a model run pass on what the tool prints for their own case, and on no other case's.
    for (id, case, _, _) in &all {
        for (other, _, printed, _) in &all {
            assert_eq!(text_checks_pass(case, printed), id == other, "the checks of {id} on what the tool prints for {other}:\n{printed}");
        }
    }
}
