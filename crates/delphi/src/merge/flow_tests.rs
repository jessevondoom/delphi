//! The merge flow against a fake forge (spec 094 AC-1, AC-2, AC-3 and the preconditions). No network, no `gh`.

use super::approve::FakeTerminal;
use super::check::{evaluate, QUEUE_LABEL, REVIEW_LABEL};
use super::fake::{FakeForge, DRAFT_SPEC, SPEC_PATH};
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
    let mut ctx = Ctx { forge: &w.forge, ledger: &ledger, term, quote: quote.map(String::from), dry_run, session: None, git: None, settle: Duration::ZERO, now: || "2026-11-07T00:00:00Z".to_string() };
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
        ("no checks at all", "no checks are reported", |f| {
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
