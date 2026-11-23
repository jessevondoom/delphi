//! The merge flow against a fake forge (spec 094 AC-1, AC-2, AC-3 and the preconditions). No network, no `gh`.

use super::approve::FakeTerminal;
use super::check::{evaluate, QUEUE_LABEL, REVIEW_LABEL};
use super::errors::Errors;
use super::fake::{FakeForge, APPROVED_SPEC, DRAFT_SPEC, SPEC_PATH};

const OTHER_SPEC: &str = "specs/095-other.md";

fn trailers(specs: &[&str]) -> Vec<String> {
    specs.iter().enumerate().map(|(i, s)| format!("Slice {i}\n\nImplements-Spec: {s}\n\nCo-Authored-By: x")).collect()
}
use super::forge::{Check, Forge};
use super::gates::Source;
use super::record::{Attestation, Ledger, SpecState};
use super::{merge_many, merge_one, queue_one, status_lines, Ctx, Outcome};
use std::time::Duration;

struct World {
    forge: FakeForge,
    home: tempfile::TempDir,
}

fn world() -> World {
    World { forge: FakeForge::default(), home: tempfile::tempdir().unwrap() }
}

fn with_ctx<R>(w: &World, term: &mut FakeTerminal, quote: Option<&str>, dry_run: bool, f: impl FnOnce(&mut Ctx) -> R) -> R {
    let ledger = Ledger::at(w.home.path());
    let errors = Errors::at(w.home.path());
    let mut ctx = Ctx { forge: &w.forge, ledger: &ledger, errors: &errors, term, quote: quote.map(String::from), dry_run, session: None, git: None, settle: Duration::ZERO, now: || "2026-11-07T00:00:00Z".to_string(), refresh: None, claims: None, rollup: None };
    f(&mut ctx)
}

fn kinds(w: &World) -> Vec<String> {
    Ledger::at(w.home.path()).read().iter().map(|r| r["kind"].as_str().unwrap().to_string()).collect()
}

fn merged(w: &World) -> Vec<(u64, String)> {
    w.forge.st.borrow().merged.clone()
}

#[test]
fn a_ready_pull_request_merges_on_a_chat_quote_and_leaves_evidence() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["crates/delphi/src/foo.rs", "docs/x.md"]);
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("please merge #7"), false, |c| merge_one(c, 7, None));
    assert_eq!(out, Outcome::Merged { pr: 7, head: "head7".into() });
    // AC-2: the head that GitHub is told to merge is the one that was derived.
    assert_eq!(merged(&w), vec![(7, "head7".to_string())]);
    assert_eq!(kinds(&w), vec!["approved", "merged"]);
    let row = &Ledger::at(w.home.path()).read()[0];
    assert_eq!((row["head_sha"].as_str(), row["paths_count"].as_u64(), row["self_attested"].as_bool(), row["quote"].as_str()), (Some("head7"), Some(2), Some(true), Some("please merge #7")));
    assert_eq!(row["spec"]["kind"], "approved-on-base");
    // The decision is visible in plain GitHub.
    let comments = w.forge.st.borrow().comments.clone();
    assert!(comments.len() == 1 && comments[0].1.contains("self-attested") && comments[0].1.contains("head7"), "{comments:?}");
}

#[test]
fn a_terminal_approval_is_recorded_as_tty_and_is_not_self_attested() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["a.rs"]);
    let out = with_ctx(&w, &mut FakeTerminal::new(true, &["7"]), None, false, |c| merge_one(c, 7, None));
    assert!(matches!(out, Outcome::Merged { .. }), "{out:?}");
    let row = &Ledger::at(w.home.path()).read()[0];
    assert_eq!((row["attestation"].as_str(), row["self_attested"].as_bool()), (Some("tty"), Some(false)));
}

#[test]
fn every_precondition_is_refused_with_its_reason_and_nothing_is_written_or_merged() {
    type Setup = fn(&FakeForge);
    let cases: Vec<(&str, &str, Setup)> = vec![
        ("a draft", "draft", |f| f.edit(7, |p| p.is_draft = true)),
        ("a closed pull request", "not open", |f| f.edit(7, |p| p.state = "CLOSED".into())),
        ("an unqueued pull request", "not queued", |f| f.edit(7, |p| p.labels.clear())),
        ("a conflict", "conflicts with `main`", |f| f.edit(7, |p| p.mergeable = "CONFLICTING".into())),
        ("no checks at all", "no checks have reported on head `head7` yet", |f| {
            f.st.borrow_mut().checks.insert(7, vec![]);
        }),
        ("a failed check", "checks failed: ui", |f| {
            f.st.borrow_mut().checks.insert(7, vec![Check { name: "ui".into(), bucket: "fail".into() }]);
        }),
        ("a running check", "still running: test", |f| {
            f.st.borrow_mut().checks.insert(7, vec![Check { name: "test".into(), bucket: "pending".into() }]);
        }),
        ("a draft spec on the base", "is not approved on main", |f| {
            f.st.borrow_mut().files.insert(("main".into(), SPEC_PATH.into()), DRAFT_SPEC.into());
        }),
        ("no trailer and a big change to code", "bounded exemption", |f| f.edit(7, |p| p.commit_messages = vec!["no trailer".into()])),
    ];
    for (name, want, setup) in cases {
        let w = world();
        w.forge.with_ready_pr(7, "head7", &["a.rs"]);
        setup(&w.forge);
        let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
        match &out {
            Outcome::Refused { reasons, .. } => assert!(reasons.iter().any(|r| r.contains(want)), "{name}: {reasons:?}"),
            other => panic!("{name}: {other:?}"),
        }
        assert!(merged(&w).is_empty() && kinds(&w).is_empty(), "{name}: nothing merged, nothing recorded");
    }
}

#[test]
fn a_push_after_the_record_was_derived_cannot_merge() {
    // AC-2: GitHub is told to merge only the derived head and refuses once the branch moved.
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["a.rs"]);
    w.forge.st.borrow_mut().push_after_read = Some("head7-moved".into());
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(&out, Outcome::Failed { detail, .. } if detail.contains("modified")), "{out:?}");
    assert!(merged(&w).is_empty());
    assert_eq!(kinds(&w), vec!["approved", "failed"], "the attempt and its failure are both on record");
}

#[test]
fn a_quote_that_does_not_name_the_pull_request_is_declined_and_so_is_having_no_approval() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["a.rs"]);
    let wrong = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #8"), false, |c| merge_one(c, 7, None));
    assert!(matches!(&wrong, Outcome::Declined { why, .. } if why.contains("does not name")), "{wrong:?}");
    let none = with_ctx(&w, &mut FakeTerminal::new(false, &[]), None, false, |c| merge_one(c, 7, None));
    assert!(matches!(&none, Outcome::Declined { why, .. } if why.contains("no terminal")), "{none:?}");
    assert!(merged(&w).is_empty() && kinds(&w).is_empty());
}

#[test]
fn a_gate_changing_pull_request_needs_a_person_and_is_shown_its_diff() {
    // AC-3. It is not queued (a gate-changing pull request never is) and it still merges when a human confirms.
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["AGENTS.md", "docs/x.md"]);
    w.forge.edit(7, |p| p.labels.clear());
    w.forge.st.borrow_mut().diff = "DIFF OF AGENTS.md".into();
    // A chat quote is refused.
    let q = with_ctx(&w, &mut FakeTerminal::new(true, &["7"]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(&q, Outcome::Declined { why, .. } if why.contains("not accepted")), "{q:?}");
    // The wrong word at a terminal is refused.
    let wrong = with_ctx(&w, &mut FakeTerminal::new(true, &["yes"]), None, false, |c| merge_one(c, 7, None));
    assert!(matches!(wrong, Outcome::Declined { .. }));
    assert!(merged(&w).is_empty());
    // A person at a terminal is shown the files and the whole diff, types the number, and it merges.
    let mut term = FakeTerminal::new(true, &["7"]);
    let ok = with_ctx(&w, &mut term, None, false, |c| merge_one(c, 7, None));
    assert!(matches!(ok, Outcome::Merged { .. }), "{ok:?}");
    let shown = term.shown.join("\n");
    assert!(shown.contains("AGENTS.md") && shown.contains("DIFF OF AGENTS.md"), "{shown}");
    assert_eq!(Ledger::at(w.home.path()).read()[0]["gate_changing"], true);
    // Without a terminal it cannot be merged at all.
    let w2 = world();
    w2.forge.with_ready_pr(7, "head7", &["AGENTS.md"]);
    let no_tty = with_ctx(&w2, &mut FakeTerminal::new(false, &[]), None, false, |c| merge_one(c, 7, None));
    assert!(matches!(&no_tty, Outcome::Declined { why, .. } if why.contains("needs a person")), "{no_tty:?}");
}

#[test]
fn a_batch_skips_a_gate_changing_pull_request_and_re_derives_each_one_just_before_merging_it() {
    let w = world();
    w.forge.with_ready_pr(1, "h1", &["a.rs"]);
    w.forge.with_ready_pr(2, "h2", &[".github/workflows/ci.yml"]);
    w.forge.with_ready_pr(3, "h3", &["c.rs"]);
    let att = Attestation::ChatQuote("merge all of them".into());
    let order = w.forge.queued(QUEUE_LABEL).unwrap();
    assert_eq!(order, vec![1, 2, 3], "oldest first");
    // AC-3: --all never merges the one that changes a gate; the others go through.
    let outs = with_ctx(&w, &mut FakeTerminal::new(false, &[]), None, false, |c| merge_many(c, &order, Some(&att)));
    assert!(matches!(outs[0], Outcome::Merged { .. }) && matches!(&outs[1], Outcome::NeedsHuman { paths, .. } if paths == &vec![".github/workflows/ci.yml".to_string()]) && matches!(outs[2], Outcome::Merged { .. }), "{outs:?}");
    assert!(w.forge.st.borrow().labels.contains(&(2, REVIEW_LABEL.to_string(), true)), "the gate-changing one is labelled for a human");
    assert_eq!(merged(&w).iter().map(|(n, _)| *n).collect::<Vec<_>>(), vec![1, 3]);
}

#[test]
fn a_pull_request_that_conflicts_after_an_earlier_merge_is_refused_not_merged_on_stale_facts() {
    let w = world();
    w.forge.with_ready_pr(1, "h1", &["a.rs"]);
    w.forge.with_ready_pr(2, "h2", &["b.rs"]);
    w.forge.st.borrow_mut().conflict_others_on_merge = true;
    let att = Attestation::ChatQuote("merge all".into());
    let outs = with_ctx(&w, &mut FakeTerminal::new(false, &[]), None, false, |c| merge_many(c, &[1, 2], Some(&att)));
    assert!(matches!(outs[0], Outcome::Merged { .. }), "{outs:?}");
    assert!(matches!(&outs[1], Outcome::Refused { reasons, .. } if reasons.iter().any(|r| r.contains("conflicts"))), "{outs:?}");
    assert_eq!(merged(&w).len(), 1);
}

#[test]
fn github_still_working_out_mergeability_is_waited_for_not_treated_as_a_conflict() {
    let w = world();
    w.forge.with_ready_pr(7, "h7", &["a.rs"]);
    w.forge.st.borrow_mut().unknown_reads.insert(7, 2);
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(out, Outcome::Merged { .. }), "{out:?}");
    // But it does not wait forever.
    let w2 = world();
    w2.forge.with_ready_pr(7, "h7", &["a.rs"]);
    w2.forge.st.borrow_mut().unknown_reads.insert(7, 99);
    let out = with_ctx(&w2, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(&out, Outcome::Refused { reasons, .. } if reasons[0].contains("has not finished")), "{out:?}");
    assert!(merged(&w2).is_empty());
}

#[test]
fn a_dry_run_asks_for_no_approval_and_writes_and_merges_nothing() {
    let w = world();
    w.forge.with_ready_pr(7, "h7", &["a.rs"]);
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), None, true, |c| merge_one(c, 7, None));
    assert_eq!(out, Outcome::DryRun { pr: 7, head: "h7".into(), gate_changing: false });
    assert!(merged(&w).is_empty() && kinds(&w).is_empty() && w.forge.st.borrow().comments.is_empty());
}

#[test]
fn no_merge_happens_when_the_record_cannot_be_written() {
    let w = world();
    w.forge.with_ready_pr(7, "h7", &["a.rs"]);
    // `merge` where the ledger directory should be is a file, so the ledger cannot be created.
    std::fs::write(w.home.path().join("merge"), "in the way").unwrap();
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(&out, Outcome::Failed { detail, .. } if detail.contains("without a record")), "{out:?}");
    assert!(merged(&w).is_empty());
}

#[test]
fn the_gate_list_is_read_from_the_base_branch_and_never_from_the_pull_request() {
    let w = world();
    w.forge.with_ready_pr(7, "h7", &["AGENTS.md", "merge-gates.toml", "docs/x.md"]);
    // The base's list guards AGENTS.md; the pull request's own copy of the file would empty the list.
    w.forge.st.borrow_mut().files.insert(("main".into(), "merge-gates.toml".into()), "paths = [\"AGENTS.md\", \"merge-gates.toml\"]".into());
    w.forge.st.borrow_mut().files.insert(("h7".into(), "merge-gates.toml".into()), "paths = [\"docs/**\"]".into());
    let v = evaluate(&w.forge, 7).unwrap();
    assert_eq!((v.gates_source, v.gate_hits), (Source::BaseBranch, vec!["AGENTS.md".to_string(), "merge-gates.toml".to_string()]));
    // No list on the base yet: the built-in one is used, and the verdict says so.
    let w2 = world();
    w2.forge.with_ready_pr(7, "h7", &["AGENTS.md"]);
    let v2 = evaluate(&w2.forge, 7).unwrap();
    assert_eq!((v2.gates_source, v2.gate_hits), (Source::BuiltIn, vec!["AGENTS.md".to_string()]));
    // A damaged list on the base refuses everything instead of waving it through.
    let w3 = world();
    w3.forge.with_ready_pr(7, "h7", &["a.rs"]);
    w3.forge.st.borrow_mut().files.insert(("main".into(), "merge-gates.toml".into()), "paths = []".into());
    let out = with_ctx(&w3, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(&out, Outcome::Refused { reasons, .. } if reasons.iter().any(|r| r.contains("gate list"))), "{out:?}");
}

#[test]
fn the_linked_spec_rules() {
    let spec_state = |setup: &dyn Fn(&FakeForge)| {
        let w = world();
        w.forge.with_ready_pr(7, "h7", &["a.rs"]);
        setup(&w.forge);
        evaluate(&w.forge, 7).unwrap().spec
    };
    // Approved on the base: the record carries the hash of the file at the head.
    assert!(matches!(spec_state(&|_| {}), SpecState::Approved { ref path, .. } if path == SPEC_PATH));
    // A draft on the base is not enough.
    let draft = spec_state(&|f| {
        f.st.borrow_mut().files.insert(("main".into(), SPEC_PATH.into()), DRAFT_SPEC.into());
    });
    assert!(matches!(draft, SpecState::Missing(_)), "{draft:?}");
    // A Stage 1 spec pull request brings the approval itself, with the operator named.
    let spec_pr = spec_state(&|f| {
        f.st.borrow_mut().files.remove(&("main".to_string(), SPEC_PATH.to_string()));
        f.st.borrow_mut().paths.insert(7, vec![SPEC_PATH.to_string()]);
    });
    assert!(matches!(spec_pr, SpecState::SpecPr { .. }), "{spec_pr:?}");
    // ... but not when the head copy is still a draft, or approved without a name.
    let draft_pr = spec_state(&|f| {
        f.st.borrow_mut().files.remove(&("main".to_string(), SPEC_PATH.to_string()));
        f.st.borrow_mut().files.insert(("h7".into(), SPEC_PATH.into()), DRAFT_SPEC.into());
        f.st.borrow_mut().paths.insert(7, vec![SPEC_PATH.to_string()]);
    });
    assert!(matches!(draft_pr, SpecState::Missing(_)), "{draft_pr:?}");
    let nameless = spec_state(&|f| {
        f.st.borrow_mut().files.remove(&("main".to_string(), SPEC_PATH.to_string()));
        f.st.borrow_mut().files.insert(("h7".into(), SPEC_PATH.into()), "---\nstatus: approved\n---\n".into());
        f.st.borrow_mut().paths.insert(7, vec![SPEC_PATH.to_string()]);
    });
    assert!(matches!(nameless, SpecState::Missing(_)), "{nameless:?}");
    // An amendment may not leave an approved spec un-approved.
    let unapproving = spec_state(&|f| {
        f.st.borrow_mut().files.insert(("h7".into(), SPEC_PATH.into()), DRAFT_SPEC.into());
        f.st.borrow_mut().paths.insert(7, vec![SPEC_PATH.to_string()]);
    });
    assert!(matches!(unapproving, SpecState::Missing(_)), "{unapproving:?}");
    // A link to a spec that exists nowhere.
    let ghost = spec_state(&|f| f.edit(7, |p| p.commit_messages = vec!["x\n\nImplements-Spec: specs/999-none.md".into()]));
    assert!(matches!(ghost, SpecState::Missing(_)), "{ghost:?}");
    // No trailer: the bounded exemption is 2 files and 100 lines, and never a manifest, lockfile or governed path.
    let small = |paths: &[&str], add: u64, del: u64| {
        let w = world();
        w.forge.with_ready_pr(7, "h7", paths);
        w.forge.edit(7, |p| {
            p.commit_messages = vec!["no trailer".into()];
            p.additions = add;
            p.deletions = del;
        });
        evaluate(&w.forge, 7).unwrap().spec
    };
    assert_eq!(small(&["src/a.rs"], 20, 5), SpecState::Exempt);
    assert!(matches!(small(&["a.rs", "b.rs", "c.rs"], 5, 5), SpecState::Missing(_)), "three files");
    assert!(matches!(small(&["a.rs"], 90, 20), SpecState::Missing(_)), "110 lines");
    assert!(matches!(small(&["Cargo.lock"], 1, 1), SpecState::Missing(_)), "a lockfile");
    assert!(matches!(small(&["crates/x/Cargo.toml"], 1, 1), SpecState::Missing(_)), "a manifest");
    assert!(matches!(small(&["skills/x/SKILL.md"], 1, 1), SpecState::Missing(_)), "a skill needs a spec");
    assert!(matches!(small(&["agents/builder/AGENT.md"], 1, 1), SpecState::Missing(_)), "an agent manifest needs a spec");
    // Documents and Stage 1 spec pull requests are Markdown, not software: no trailer is needed, however large.
    assert_eq!(small(&["docs/guide.md", "README.md", "missions/x/notes.md"], 900, 40), SpecState::MarkdownOnly);
    assert_eq!(small(&["specs/095-new.md", "GOVERNANCE.md", "specs/003-collaboration-and-council.md"], 300, 10), SpecState::MarkdownOnly, "a spec pull request");
    // ... unless something that is not Markdown rides along.
    assert!(matches!(small(&["docs/guide.md", "crates/x/src/lib.rs"], 900, 40), SpecState::Missing(_)), "code rides along");
    assert!(matches!(small(&["specs/095-new.md", "dev/run.py"], 900, 40), SpecState::Missing(_)), "a script rides along");
    assert!(matches!(small(&["docs/guide.md", "skills/x/SKILL.md"], 900, 40), SpecState::Missing(_)), "a governed directory rides along");
}

#[test]
fn several_slices_of_the_same_spec_in_one_pull_request_pass_and_record_the_spec_once() {
    let w = world();
    w.forge.with_ready_pr(7, "h7", &["a.rs", "b.rs"]);
    w.forge.edit(7, |p| p.commit_messages = trailers(&[SPEC_PATH, SPEC_PATH, SPEC_PATH]));
    let v = evaluate(&w.forge, 7).unwrap();
    assert!(v.ready(), "{:?}", v.refusals);
    assert_eq!(v.specs.len(), 1);
    assert!(matches!(&v.specs[0], SpecState::Approved { path, .. } if path == SPEC_PATH));
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(out, Outcome::Merged { .. }), "{out:?}");
    let row = &Ledger::at(w.home.path()).read()[0];
    assert_eq!(row["spec"]["kind"], "approved-on-base");
    assert_eq!(row["specs"].as_array().unwrap().len(), 1);
    assert_eq!(row["specs"][0]["path"], SPEC_PATH);
}

#[test]
fn a_pull_request_naming_different_specs_is_refused_and_every_spec_is_named() {
    // Bundles are for slices of one spec only; this is enforced here, not just in the rule text.
    for order in [[SPEC_PATH, OTHER_SPEC], [OTHER_SPEC, SPEC_PATH]] {
        let w = world();
        w.forge.with_ready_pr(7, "h7", &["a.rs"]);
        w.forge.edit(7, |p| p.commit_messages = trailers(&order));
        // Both are fully ratified: the refusal is about the mix, not about approval.
        for r in ["main", "h7"] {
            w.forge.st.borrow_mut().files.insert((r.into(), OTHER_SPEC.into()), APPROVED_SPEC.into());
        }
        let v = evaluate(&w.forge, 7).unwrap();
        assert!(!v.ready() && matches!(v.spec, SpecState::Missing(_)), "{:?}", v.refusals);
        assert!(v.refusals.iter().any(|r| r.contains(SPEC_PATH) && r.contains(OTHER_SPEC) && r.contains("one spec only")), "{:?}", v.refusals);
        let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
        assert!(matches!(out, Outcome::Refused { .. }), "{out:?}");
        assert!(merged(&w).is_empty() && kinds(&w).is_empty());
    }
}

#[test]
fn the_same_spec_in_a_commit_and_in_the_body_is_not_a_conflict() {
    let w = world();
    w.forge.with_ready_pr(7, "h7", &["a.rs"]);
    w.forge.edit(7, |p| {
        p.commit_messages = trailers(&[SPEC_PATH]);
        p.body = format!("Implements-Spec: {SPEC_PATH}");
    });
    let v = evaluate(&w.forge, 7).unwrap();
    assert!(v.ready(), "{:?}", v.refusals);
    assert_eq!(v.specs.len(), 1);
    // A different spec only in the body is still a mix.
    w.forge.edit(7, |p| p.body = format!("Implements-Spec: {OTHER_SPEC}"));
    assert!(!evaluate(&w.forge, 7).unwrap().ready());
}

#[test]
fn an_unratified_spec_still_blocks_a_single_spec_pull_request_and_a_mixed_one_names_both() {
    let w = world();
    w.forge.with_ready_pr(7, "h7", &["a.rs"]);
    w.forge.edit(7, |p| p.commit_messages = trailers(&[OTHER_SPEC, OTHER_SPEC]));
    w.forge.st.borrow_mut().files.insert(("main".into(), OTHER_SPEC.into()), DRAFT_SPEC.into());
    let v = evaluate(&w.forge, 7).unwrap();
    assert!(!v.ready() && v.refusals.iter().any(|r| r.contains(OTHER_SPEC) && r.contains("not approved")), "{:?}", v.refusals);
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(out, Outcome::Refused { .. }), "{out:?}");
    assert!(merged(&w).is_empty() && kinds(&w).is_empty());
    // Mixed and one unratified: refused, both named.
    w.forge.edit(7, |p| p.commit_messages = trailers(&[SPEC_PATH, OTHER_SPEC]));
    let v = evaluate(&w.forge, 7).unwrap();
    assert!(!v.ready() && v.refusals.iter().any(|r| r.contains(SPEC_PATH) && r.contains(OTHER_SPEC)), "{:?}", v.refusals);
}

#[test]
fn a_single_spec_row_keeps_its_old_field_and_specs_is_additive() {
    let w = world();
    w.forge.with_ready_pr(7, "h7", &["a.rs"]);
    with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    let row = &Ledger::at(w.home.path()).read()[0];
    assert_eq!(row["spec"], row["specs"][0]);
}

#[test]
fn queueing_checks_readiness_and_sends_a_gate_changing_pull_request_to_a_human() {
    let w = world();
    w.forge.with_ready_pr(1, "h1", &["a.rs"]);
    w.forge.edit(1, |p| p.labels.clear());
    assert!(queue_one(&w.forge, 1).unwrap().contains("queued"));
    assert!(w.forge.st.borrow().labels.contains(&(1, QUEUE_LABEL.to_string(), true)));
    // A draft cannot be queued.
    w.forge.with_ready_pr(2, "h2", &["a.rs"]);
    w.forge.edit(2, |p| p.is_draft = true);
    assert!(queue_one(&w.forge, 2).unwrap_err().contains("draft"));
    // A gate-changing one is not queueable; it is labelled for a human.
    w.forge.with_ready_pr(3, "h3", &["deny.toml"]);
    w.forge.edit(3, |p| p.labels.clear());
    let e = queue_one(&w.forge, 3).unwrap_err();
    assert!(e.contains("not queueable") && e.contains("delphi merge 3"), "{e}");
    assert!(w.forge.st.borrow().labels.contains(&(3, REVIEW_LABEL.to_string(), true)));
    assert!(!w.forge.st.borrow().labels.contains(&(3, QUEUE_LABEL.to_string(), true)));
}

#[test]
fn status_says_what_each_queued_pull_request_would_do() {
    let w = world();
    assert!(status_lines(&w.forge).unwrap()[0].contains("nothing is queued"));
    w.forge.with_ready_pr(1, "h1", &["a.rs"]);
    w.forge.with_ready_pr(2, "h2", &["AGENTS.md"]);
    w.forge.with_ready_pr(3, "h3", &["c.rs"]);
    w.forge.edit(3, |p| p.is_draft = true);
    let lines = status_lines(&w.forge).unwrap();
    assert!(lines[0].starts_with("#1: ready") && lines[1].contains("changes a gate") && lines[2].contains("not ready: it is a draft"), "{lines:?}");
}

// ---- the error log through the real flow (spec 094 AC-6) -----------------------------------------------------------------

fn error_classes(w: &World) -> Vec<(u64, String, String, u32)> {
    Errors::at(w.home.path()).entries().into_iter().map(|e| (e.pr, e.class, e.state, e.attempts)).collect()
}

#[test]
fn a_refusal_that_is_a_problem_is_logged_with_its_class_and_one_that_is_just_waiting_is_not() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["a.rs"]);
    w.forge.edit(7, |p| p.mergeable = "CONFLICTING".into());
    w.forge.st.borrow_mut().checks.insert(7, vec![Check { name: "ui".into(), bucket: "fail".into() }]);
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(out, Outcome::Refused { .. }));
    assert_eq!(error_classes(&w), vec![(7, "conflict".to_string(), "open".to_string(), 1), (7, "ci-red".to_string(), "open".to_string(), 1)]);
    // Waiting is not a problem: a draft, an unqueued pull request and running checks log nothing.
    let w2 = world();
    w2.forge.with_ready_pr(8, "h8", &["a.rs"]);
    w2.forge.edit(8, |p| {
        p.is_draft = true;
        p.labels.clear();
    });
    w2.forge.st.borrow_mut().checks.insert(8, vec![Check { name: "t".into(), bucket: "pending".into() }]);
    with_ctx(&w2, &mut FakeTerminal::new(false, &[]), Some("merge #8"), false, |c| merge_one(c, 8, None));
    assert!(error_classes(&w2).is_empty(), "{:?}", error_classes(&w2));
}

#[test]
fn the_same_problem_on_the_same_head_is_counted_not_duplicated_and_a_merge_resolves_it() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["a.rs"]);
    w.forge.edit(7, |p| p.mergeable = "CONFLICTING".into());
    for _ in 0..3 {
        with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    }
    assert_eq!(error_classes(&w), vec![(7, "conflict".to_string(), "open".to_string(), 3)]);
    // The builder fixes it (the head moves, the conflict is gone), and the merge closes the entry.
    w.forge.edit(7, |p| {
        p.mergeable = "MERGEABLE".into();
        p.head_sha = "head7-fixed".into();
    });
    w.forge.st.borrow_mut().files.insert(("head7-fixed".into(), SPEC_PATH.into()), APPROVED_SPEC.into());
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(out, Outcome::Merged { .. }), "{out:?}");
    assert_eq!(error_classes(&w), vec![(7, "conflict".to_string(), "resolved".to_string(), 3)]);
}

#[test]
fn a_failed_merge_is_logged_as_what_it_was_and_a_dry_run_logs_nothing() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["a.rs"]);
    w.forge.st.borrow_mut().push_after_read = Some("head7-moved".into());
    with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert_eq!(error_classes(&w), vec![(7, "stale-record".to_string(), "open".to_string(), 1)]);
    let w2 = world();
    w2.forge.with_ready_pr(7, "head7", &["a.rs"]);
    w2.forge.edit(7, |p| p.mergeable = "CONFLICTING".into());
    with_ctx(&w2, &mut FakeTerminal::new(false, &[]), None, true, |c| merge_one(c, 7, None));
    assert!(error_classes(&w2).is_empty(), "a dry run changes nothing");
}

#[test]
fn the_log_view_filters_by_open_and_by_pull_request() {
    use super::errors::Class;
    use super::log_view;
    let w = world();
    let log = Errors::at(w.home.path());
    log.record(7, "h", Class::Conflict, "c", "t1").unwrap();
    log.record(8, "h", Class::CiRed, "r", "t2").unwrap();
    log.resolve_pr(8, "t3");
    let (all, _) = log_view(w.home.path(), false, None);
    let (open, _) = log_view(w.home.path(), true, None);
    let (seven, _) = log_view(w.home.path(), false, Some(7));
    assert_eq!((all.len(), open.len(), seven.len()), (2, 1, 1));
    assert_eq!(open[0].pr, 7);
}

// ---- freshness (spec 094 §11.3.1, AC-9) ----------------------------------------------------------------------------------

#[test]
fn a_pull_request_behind_the_base_is_refused_with_class_behind_and_its_count_and_nothing_merges() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["a.rs"]);
    w.forge.st.borrow_mut().behind.insert("head7".into(), 2);
    let v = evaluate(&w.forge, 7).unwrap();
    assert_eq!(v.behind_by, 2);
    assert!(!v.ready() && v.ready_but_maybe_behind(), "{:?}", v.refusals);
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    match &out {
        Outcome::Refused { reasons, .. } => {
            let r = reasons.iter().find(|r| r.contains("behind")).expect("a freshness reason");
            assert!(r.contains("2 commit(s) behind `main`") && r.contains("delphi merge refresh 7") && r.contains("wait for CI"), "{r}");
        }
        other => panic!("{other:?}"),
    }
    assert!(merged(&w).is_empty() && kinds(&w).is_empty(), "nothing merged, nothing recorded");
    assert_eq!(error_classes(&w), vec![(7, "behind".to_string(), "open".to_string(), 1)]);
    // Current again (the branch was refreshed and is green on its new head): it merges.
    w.forge.st.borrow_mut().behind.clear();
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(out, Outcome::Merged { .. }), "{out:?}");
}

#[test]
fn a_failed_compare_is_a_forge_failure_never_a_pass() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["a.rs"]);
    w.forge.st.borrow_mut().fail_compare = Some("HTTP 502".into());
    assert!(evaluate(&w.forge, 7).is_err());
    let out = with_ctx(&w, &mut FakeTerminal::new(false, &[]), Some("merge #7"), false, |c| merge_one(c, 7, None));
    assert!(matches!(&out, Outcome::Failed { detail, .. } if detail.contains("compare")), "{out:?}");
    assert!(merged(&w).is_empty());
}

#[test]
fn freshness_is_required_of_a_gate_changing_pull_request_too_and_a_dry_run_shows_it() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["AGENTS.md"]);
    w.forge.st.borrow_mut().behind.insert("head7".into(), 1);
    let out = with_ctx(&w, &mut FakeTerminal::new(true, &["7"]), None, false, |c| merge_one(c, 7, None));
    assert!(matches!(&out, Outcome::Refused { reasons, .. } if reasons.iter().any(|r| r.contains("1 commit(s) behind"))), "{out:?}");
    assert!(merged(&w).is_empty());
    let w2 = world();
    w2.forge.with_ready_pr(8, "head8", &["a.rs"]);
    w2.forge.st.borrow_mut().behind.insert("head8".into(), 3);
    let out = with_ctx(&w2, &mut FakeTerminal::new(false, &[]), None, true, |c| merge_one(c, 8, None));
    assert!(matches!(&out, Outcome::Refused { reasons, .. } if reasons.iter().any(|r| r.contains("3 commit(s) behind"))), "{out:?}");
    assert!(out.line().contains("refresh"), "{}", out.line());
    assert!(error_classes(&w2).is_empty(), "a dry run logs nothing");
}

#[test]
fn after_one_merge_of_a_batch_the_next_pull_request_is_behind_and_is_refused_not_merged_on_stale_checks() {
    // The failure of 2026-11-20: two green pull requests, each tested against the base before the other merged.
    let w = world();
    w.forge.with_ready_pr(1, "h1", &["a.rs"]);
    w.forge.with_ready_pr(2, "h2", &["b.rs"]);
    w.forge.st.borrow_mut().behind_others_on_merge = true;
    let att = Attestation::ChatQuote("merge all".into());
    let outs = with_ctx(&w, &mut FakeTerminal::new(false, &[]), None, false, |c| merge_many(c, &[1, 2], Some(&att)));
    assert!(matches!(outs[0], Outcome::Merged { .. }), "{outs:?}");
    assert!(matches!(&outs[1], Outcome::Refused { reasons, .. } if reasons.iter().any(|r| r.contains("behind `main`"))), "{outs:?}");
    assert_eq!(merged(&w), vec![(1, "h1".to_string())]);
}

#[test]
fn a_behind_pull_request_can_still_be_queued_and_status_names_it_as_behind() {
    let w = world();
    w.forge.with_ready_pr(1, "h1", &["a.rs"]);
    w.forge.edit(1, |p| p.labels.clear());
    w.forge.st.borrow_mut().behind.insert("h1".into(), 4);
    let msg = queue_one(&w.forge, 1).unwrap();
    assert!(msg.contains("queued") && msg.contains("4 commit(s) behind") && msg.contains("delphi merge refresh 1"), "{msg}");
    let lines = status_lines(&w.forge).unwrap();
    assert!(lines[0].contains("will be refreshed at merge") && lines[0].contains("4 commit(s) behind `main`"), "{lines:?}");
    // Behind and red: queueing still refuses on the red check.
    w.forge.with_ready_pr(2, "h2", &["a.rs"]);
    w.forge.edit(2, |p| p.labels.clear());
    w.forge.st.borrow_mut().behind.insert("h2".into(), 1);
    w.forge.st.borrow_mut().checks.insert(2, vec![Check { name: "ui".into(), bucket: "fail".into() }]);
    assert!(queue_one(&w.forge, 2).unwrap_err().contains("checks failed"));
}

// ---- spec 135 F0b: a merge releases the merged branch's slice claims; another holder's claim is a note, never a refusal ----

const REPO_ID: &str = "/repo/.git";
const NOW: i64 = 1_795_000_000;
const STEM: &str = "094-core-repo-merge-flow";

fn claim_row(spec: &str, slice: &str, branch: &str, repo: &str, expires: i64) -> serde_json::Value {
    let t = |s: i64| delphi_core::time::format_unix_millis(s, 0);
    serde_json::json!({"repo": repo, "spec": spec, "slice": slice, "by": branch, "branch": branch, "at": t(NOW - 60), "expires": t(expires)})
}

fn write_claims(w: &World, claims: &[serde_json::Value]) {
    let dir = w.home.path().join("forge");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("claims.json"), serde_json::to_vec(&serde_json::json!({"version": 1, "claims": claims})).unwrap()).unwrap();
}

fn claim_slices(w: &World) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(w.home.path().join("forge/claims.json")).unwrap()).unwrap();
    v["claims"].as_array().unwrap().iter().map(|c| format!("{} {}", c["slice"].as_str().unwrap(), c["branch"].as_str().unwrap())).collect()
}

fn merge_with_claims(w: &World, term: &mut FakeTerminal, dry_run: bool) -> Outcome {
    let ledger = Ledger::at(w.home.path());
    let errors = Errors::at(w.home.path());
    let claims = super::Claims { home: w.home.path(), repo: REPO_ID, now: || NOW };
    let mut ctx = Ctx { forge: &w.forge, ledger: &ledger, errors: &errors, term, quote: Some("merge #7".into()), dry_run, session: None, git: None, settle: Duration::ZERO, now: || "2026-11-19T00:00:00Z".to_string(), refresh: None, claims: Some(claims), rollup: None };
    merge_one(&mut ctx, 7, None)
}

#[test]
fn a_merge_releases_the_claims_of_its_branch_in_this_repository_only() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["a.rs"]);
    write_claims(
        &w,
        &[
            claim_row(STEM, "S1", "branch-7", REPO_ID, NOW + 3600),
            claim_row(STEM, "S1b", "branch-7", REPO_ID, NOW - 10),
            claim_row(STEM, "S1", "branch-7", "/elsewhere/.git", NOW + 3600),
            claim_row(STEM, "S2", "branch-8", REPO_ID, NOW + 3600),
        ],
    );
    let mut term = FakeTerminal::new(false, &[]);
    let out = merge_with_claims(&w, &mut term, false);
    assert_eq!(out, Outcome::Merged { pr: 7, head: "head7".into() });
    assert_eq!(claim_slices(&w), vec!["S1 branch-7", "S2 branch-8"], "only this repository's branch-7 claims (live or expired) are gone");
    let released: Vec<&String> = term.shown.iter().filter(|l| l.contains("released the claim")).collect();
    assert_eq!(released.len(), 2, "one line per released claim: {:?}", term.shown);
    assert!(released.iter().all(|l| l.starts_with("#7: ") && l.contains("branch-7")), "{released:?}");
}

#[test]
fn another_holders_claim_on_the_same_spec_is_noted_and_the_merge_still_proceeds() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["a.rs"]);
    write_claims(&w, &[claim_row(STEM, "S2", "branch-8", REPO_ID, NOW + 3600), claim_row(STEM, "S3", "branch-9", REPO_ID, NOW - 10), claim_row("095-other", "S1", "branch-9", REPO_ID, NOW + 3600)]);
    let notes = |t: &FakeTerminal| t.shown.iter().filter(|l| l.contains("note: another session")).cloned().collect::<Vec<_>>();
    // The dry run says so too, and changes nothing.
    let mut dry = FakeTerminal::new(false, &[]);
    assert!(matches!(merge_with_claims(&w, &mut dry, true), Outcome::DryRun { .. }));
    assert_eq!(notes(&dry).len(), 1, "{:?}", dry.shown);
    assert!(notes(&dry)[0].contains("S2") && notes(&dry)[0].contains("branch-8"), "{:?}", notes(&dry));
    let mut term = FakeTerminal::new(false, &[]);
    let out = merge_with_claims(&w, &mut term, false);
    assert_eq!(out, Outcome::Merged { pr: 7, head: "head7".into() }, "a claim never refuses a merge");
    assert_eq!(notes(&term).len(), 1, "expired claims and other specs are not noted: {:?}", term.shown);
    assert_eq!(claim_slices(&w).len(), 3, "another holder's claims are not released");
}

#[test]
fn a_broken_claims_file_does_not_block_the_merge_and_is_logged_as_cleanup() {
    let w = world();
    w.forge.with_ready_pr(7, "head7", &["a.rs"]);
    std::fs::create_dir_all(w.home.path().join("forge")).unwrap();
    std::fs::write(w.home.path().join("forge/claims.json"), b"{not json").unwrap();
    let mut term = FakeTerminal::new(false, &[]);
    let out = merge_with_claims(&w, &mut term, false);
    assert_eq!(out, Outcome::Merged { pr: 7, head: "head7".into() });
    assert_eq!(merged(&w), vec![(7, "head7".to_string())]);
    assert!(term.shown.iter().any(|l| l.contains("could not be read")), "{:?}", term.shown);
    let open: Vec<_> = Errors::at(w.home.path()).entries().into_iter().filter(|e| e.open()).collect();
    assert!(open.iter().any(|e| e.detail.contains("slice claims") && e.class == "cleanup"), "{open:?}");
    assert_eq!(std::fs::read(w.home.path().join("forge/claims.json")).unwrap(), b"{not json", "a malformed file is never overwritten");
}
