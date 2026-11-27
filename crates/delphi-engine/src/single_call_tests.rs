//! `specs/143` AC-13: every caller whose route request is marked `single_call` makes one request, with no tool definitions.
//!
//! One test for each file of [`crate::route::SINGLE_CALLERS`], named `<file>_makes_one_request_with_no_tools`. The last test reads
//! the sources: the files that set `single_call`, the list, and the tests here must be the same set, so a caller added later (the
//! synthetic-question call of `specs/140` slice C, the source summary of `specs/142` slice G) fails until it is listed and tested.

use crate::model::Step;
use crate::testkit::{setup, Fx};
use std::collections::BTreeSet;

/// The scripted model saw exactly one request, and it defined no tool.
fn one_request_with_no_tools(f: &Fx) {
    let seen = f.model.requests();
    assert_eq!(seen.len(), 1, "one call: {:?}", seen.iter().map(|r| r.model_id.clone()).collect::<Vec<_>>());
    let tools = &seen[0].body["tools"];
    assert!(tools.is_null() || tools.as_array().is_some_and(|t| t.is_empty()), "no tool definitions: {tools}");
}

#[tokio::test(flavor = "multi_thread")]
async fn summary_makes_one_request_with_no_tools() {
    let f = setup(vec![Step::text("They chose Postgres.")], 5.0, |_| {});
    let w = f.engine.summarize_session(crate::summary::SummaryMode::Fast, "Operator: pick a queue\nTeam: Postgres.", Some(delphi_core::provenance::Tier::L3)).await;
    assert!(w.is_some());
    one_request_with_no_tools(&f);
}

#[tokio::test(flavor = "multi_thread")]
async fn decision_run_makes_one_request_with_no_tools() {
    let f = setup(vec![Step::text("It only edits one file.")], 5.0, |_| {});
    let card = serde_json::json!({"id": "card:a1", "title": "write deck.rs", "tier": "amber", "kind": "action", "payload_blake3": "h1", "choices": [{"id": "allow", "label": "Approve once"}]});
    let _ = f.engine.make_reply(&card, &[], "what does it change?").await;
    one_request_with_no_tools(&f);
}

#[tokio::test(flavor = "multi_thread")]
async fn page_run_makes_one_request_with_no_tools() {
    use crate::library::page_validate::PageWriter;
    let f = setup(vec![Step::text("# Tide tables\n\nA page.")], 5.0, |_| {});
    let key = f.engine.writer_key().expect("a writer");
    let writer = crate::page_run::EnginePageWriter { engine: f.engine.clone(), key, max_tokens: 500 };
    let _ = writer.write("Write a page on tide tables from the material below.").await;
    one_request_with_no_tools(&f);
}

#[tokio::test(flavor = "multi_thread")]
async fn library_page_validate_makes_one_request_with_no_tools() {
    use crate::library::page_validate::{EnginePageValidator, PageQuery, PageValidator};
    let f = setup(vec![Step::text("[]")], 5.0, |_| {});
    let validator = EnginePageValidator { engine: f.engine.clone(), writer_family: None, writer_model: None };
    let _ = validator.validate(&PageQuery { body: "Tides follow the moon.".into(), material: vec![], omitted: vec![] }).await;
    one_request_with_no_tools(&f);
}

#[tokio::test(flavor = "multi_thread")]
async fn fact_judge_makes_one_request_with_no_tools() {
    use crate::fact_judge::{EngineJudge, FactJudge, JudgeQuery};
    let f = setup(vec![Step::text("entails")], 5.0, |_| {});
    let judge = EngineJudge { engine: f.engine.clone(), author_family: None, author_model: None };
    let _ = judge.judge(&JudgeQuery { claim: "Tides follow the moon.".into(), passage: "The moon raises the tides.".into() }).await;
    one_request_with_no_tools(&f);
}

#[tokio::test(flavor = "multi_thread")]
async fn research_makes_one_request_with_no_tools() {
    use crate::research::{EngineJudge, Judge, JudgeInput};
    let f = setup(vec![Step::text("{}")], 5.0, |_| {});
    let text = "The moon raises the tides twice a day.".to_string();
    let input = JudgeInput { topic: "tides".into(), url: "https://example.org/tides".into(), publisher: "Example".into(), title: "Tides".into(), text: text.clone(), full_text: text };
    let _ = EngineJudge(f.engine.clone()).judge(input).await;
    one_request_with_no_tools(&f);
}

#[tokio::test(flavor = "multi_thread")]
async fn library_acquire_run_makes_one_request_with_no_tools() {
    use crate::library::acquire::Scout;
    let f = setup(vec![Step::text("tide tables\nharbour lights")], 5.0, |_| {});
    let scout = crate::library_acquire_run::EngineScout { engine: f.engine.clone() };
    let seed = vec!["lighthouses".to_string()];
    let _ = scout.widen(&seed, 100_000).await;
    one_request_with_no_tools(&f);
}

#[tokio::test(flavor = "multi_thread")]
async fn source_summary_makes_one_request_with_no_tools() {
    let f = setup(vec![Step::text("A page on the tides: they come twice a day.")], 5.0, |_| {});
    let lib = crate::library::Library::in_home(&f.engine.ws.home);
    let url = "https://tides.example/a";
    let snap = crate::fact_fetch::test_snapshot(url, "Tides", &["The tide comes in twice a day."]);
    let id = lib.add_fetched(url, &snap, &Default::default(), &Default::default()).unwrap().id;
    let key = f.engine.source_summary_key().expect("a summariser");
    let _ = f.engine.summarize_source(&lib, &id, &key).await;
    one_request_with_no_tools(&f);
}

fn source(file: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    // A file's test module is not a caller (`#[cfg(test)]` on its own line, then `mod`).
    text.split("\n#[cfg(test)]\nmod ").next().unwrap_or("").split("\n#[cfg(test)]\npub(crate) mod ").next().unwrap_or("").to_string()
}

/// The marked callers, the list and the tested callers are one set (AC-13).
#[test]
fn the_marked_callers_are_the_listed_and_the_tested_callers() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".rs") && !n.ends_with("_tests.rs")).collect();
    // A route request that is marked (a launch profile of the bridge has a field of the same name and is not one).
    let marked: BTreeSet<String> = files.iter().filter(|n| source(n).contains("turn_by_turn: true, single_call: true")).cloned().collect();
    let listed: BTreeSet<String> = crate::route::SINGLE_CALLERS.iter().map(|s| s.to_string()).collect();
    assert_eq!(marked, listed, "a file sets `single_call` and is not in route::SINGLE_CALLERS, or the reverse");
    let here = std::fs::read_to_string(dir.join("single_call_tests.rs")).unwrap();
    let tested: BTreeSet<String> = here.lines().filter_map(|l| l.trim().strip_prefix("async fn ")).filter_map(|l| l.split("_makes_one_request_with_no_tools").next().filter(|stem| l.len() > stem.len())).map(|stem| format!("{stem}.rs")).collect();
    assert_eq!(tested, listed, "every marked caller has its test here, and nothing else does");
}
