//! Opening and recognising a rollup pull request (spec 094 §13, S19, AC-19) against real git repositories with a local bare
//! remote and a fake forge. No network.

use super::approve::FakeTerminal;
use super::errors::Errors;
use super::fake::{FakeForge, SPEC_PATH};
use super::forge::Check;
use super::record::Ledger;
use super::refresh::Runner;
use super::refresh_tests::{commit, fixture, git, Fixture};
use super::rollup::{manifest, parse_manifest};
use super::rollup_pr::{batch, Env, REFUSED, STALE};
use super::rollup_tests::{branch, ready, OTHER_SPEC};
use super::{merge_one, Ctx, Outcome};
use std::time::Duration;

pub(super) const ID: &str = "20261122000000";
pub(super) const ROLLUP: u64 = 900;

/// Two ready pull requests of two specs (#1 changes README.md, #2 adds OTHER.md), and a forge that sees the origin.
pub(super) fn world() -> (Fixture, FakeForge, String, String) {
    let f = fixture();
    let h1 = branch(&f, "b1", |d| std::fs::write(d.join("README.md"), "readme\nmore\n").unwrap());
    let h2 = branch(&f, "b2", |d| std::fs::write(d.join("OTHER.md"), "other\n").unwrap());
    let forge = FakeForge::default();
    ready(&forge, 1, "b1", &h1, &["README.md"], Some(SPEC_PATH));
    ready(&forge, 2, "b2", &h2, &["OTHER.md"], Some(OTHER_SPEC));
    {
        let mut s = forge.st.borrow_mut();
        s.origin = Some(f.origin.clone());
        s.checks_on_open = vec![Check { name: "test".into(), bucket: "pass".into() }, Check { name: "ci-gate".into(), bucket: "pass".into() }];
    }
    (f, forge, h1, h2)
}

/// Run `body` with a context over `f` and `forge`, the operator's words `quote`, and a rollup environment.
pub(super) fn with_ctx<R>(f: &Fixture, forge: &FakeForge, quote: Option<&str>, body: impl FnOnce(&mut Ctx) -> R) -> R {
    let (ledger, errors, runner) = (Ledger::at(f.home.path()), Errors::at(f.home.path()), Runner::new(&f.work));
    let mut term = FakeTerminal::new(false, &[]);
    let mut ctx = Ctx { forge, ledger: &ledger, errors: &errors, term: &mut term, quote: quote.map(String::from), dry_run: false, git: None, session: None, settle: Duration::ZERO, now: || "2026-11-22T00:00:00Z".to_string(), refresh: None, claims: None, rollup: Some(Env { git: &runner, home: f.home.path() }) };
    body(&mut ctx)
}

/// Open the rollup of #1 and #2 on the operator's words, and give it the commits GitHub would list (two specs: cross-spec).
pub(super) fn opened(f: &Fixture, forge: &FakeForge) {
    let (pr, approval) = with_ctx(f, forge, Some("batch #1 and #2"), |ctx| batch(ctx, &[1, 2], 4, false, ID)).unwrap();
    assert_eq!((pr, approval.pairs.len()), (ROLLUP, 2));
    forge.edit(ROLLUP, |p| p.commit_messages = vec![format!("one\n\nImplements-Spec: {SPEC_PATH}\n"), format!("two\n\nImplements-Spec: {OTHER_SPEC}\n")]);
}

fn refusal(f: &Fixture, forge: &FakeForge) -> Vec<String> {
    match with_ctx(f, forge, None, |ctx| merge_one(ctx, ROLLUP, None)) {
        Outcome::Refused { reasons, .. } => reasons,
        other => panic!("{other:?}"),
    }
}

/// Push one more commit to the rollup branch (a change of `path`) and tell the forge.
fn tamper(f: &Fixture, forge: &FakeForge, path: &str) {
    let b = format!("delphi-rollup/{ID}");
    git(&f.work, &["fetch", "-q", "origin", &b]);
    git(&f.work, &["switch", "-q", "--detach", &format!("origin/{b}")]);
    std::fs::write(f.work.join(path), "smuggled\n").unwrap();
    commit(&f.work, "one more change");
    git(&f.work, &["push", "-q", "origin", &format!("HEAD:refs/heads/{b}")]);
    let head = git(&f.work, &["rev-parse", "HEAD"]);
    git(&f.work, &["switch", "-q", "main"]);
    forge.edit(ROLLUP, |p| p.head_sha = head);
}

#[test]
fn an_approved_batch_opens_one_pull_request_whose_body_carries_exactly_the_manifest() {
    let (f, forge, h1, h2) = world();
    opened(&f, &forge);
    let s = forge.st.borrow();
    assert_eq!(s.opened.len(), 1);
    let (base, head, title, body) = &s.opened[0];
    assert_eq!((base.as_str(), head.as_str()), ("main", format!("delphi-rollup/{ID}").as_str()));
    assert!(title.contains("#1") && title.contains("#2"), "{title}");
    let m = parse_manifest(body).unwrap();
    assert_eq!(m.prs, vec![(1, h1, Some(SPEC_PATH.to_string())), (2, h2, Some(OTHER_SPEC.to_string()))]);
    assert_eq!(m.base_sha, git(&f.origin, &["rev-parse", "refs/heads/main"]));
    // Pushed plainly to a new branch on the origin; the forge's pull request is at that head. Nothing merged or labelled.
    assert_eq!(s.prs[&ROLLUP].head_sha, git(&f.origin, &["rev-parse", &format!("refs/heads/delphi-rollup/{ID}")]));
    assert!(s.merged.is_empty() && s.labels.is_empty() && s.comments.is_empty());
    let rows = Ledger::at(f.home.path()).read();
    assert!(rows.iter().any(|r| r["kind"] == "rollup_opened" && r["pr"] == ROLLUP && r["attestation"] == "chat-quote"), "{rows:?}");
}

#[test]
fn a_quote_that_does_not_name_every_pull_request_opens_nothing_and_a_gate_changing_one_is_never_batched() {
    let (f, forge, _, _) = world();
    let e = with_ctx(&f, &forge, Some("batch #1"), |ctx| batch(ctx, &[1, 2], 4, false, ID)).unwrap_err();
    assert!(e.contains("#2"), "{e}");
    forge.st.borrow_mut().paths.insert(2, vec!["AGENTS.md".into()]);
    forge.st.borrow_mut().paths.insert(1, vec!["crates/delphi/src/merge/mod.rs".into()]);
    let e = with_ctx(&f, &forge, Some("batch #1 and #2"), |ctx| batch(ctx, &[1, 2], 4, false, ID)).unwrap_err();
    assert!(e.contains("no pull request can be batched"), "{e}");
    assert_eq!(git(&f.origin, &["branch", "--list", "delphi-rollup/*"]), "");
    assert!(forge.st.borrow().opened.is_empty());
}

#[test]
fn a_rollup_is_recognised_by_its_rebuild_and_its_cross_spec_content_accepted() {
    let (f, forge, _, _) = world();
    opened(&f, &forge);
    let r = with_ctx(&f, &forge, None, |ctx| super::rollup_pr::recognise(ctx.forge, ctx.rollup.unwrap(), &super::check::evaluate(ctx.forge, ROLLUP).unwrap()));
    let r = r.unwrap();
    assert_eq!(r.parts.len(), 2);
    assert!(r.verdict.refusals.is_empty(), "the cross-spec refusal is lifted: {:?}", r.verdict.refusals);
}

#[test]
fn a_tampered_rollup_is_refused_as_a_cross_spec_pull_request() {
    // An extra commit that changes a file: the rebuild's tree differs.
    let (f, forge, _, _) = world();
    opened(&f, &forge);
    tamper(&f, &forge, "evil.sh");
    let why = refusal(&f, &forge);
    assert!(why[0].starts_with(REFUSED) && why[0].contains("does not reproduce its tree"), "{why:?}");
    assert!(why.iter().any(|r| r.contains("different specs")), "refused as before: {why:?}");
    let errors = Errors::at(f.home.path()).entries();
    assert!(errors.iter().any(|e| e.pr == ROLLUP && e.class == "rollup-refused"), "{errors:?}");
    assert!(forge.st.borrow().merged.is_empty());
}

#[test]
fn a_second_manifest_or_a_branch_without_the_prefix_is_an_ordinary_pull_request() {
    let (f, forge, h1, _) = world();
    opened(&f, &forge);
    let body = forge.st.borrow().prs[&ROLLUP].body.clone();
    let extra = manifest("main", &"c".repeat(40), &[super::rollup::Constituent { pr: 1, head_sha: h1, head_ref: "b1".into(), title: "t".into(), spec: None }]);
    forge.edit(ROLLUP, |p| p.body = format!("{body}\n{extra}"));
    let why = refusal(&f, &forge);
    assert!(why[0].starts_with(REFUSED) && why[0].contains("exactly one"), "{why:?}");
    forge.edit(ROLLUP, |p| {
        p.body = body;
        p.head_ref = format!("rollup/{ID}");
    });
    let why = refusal(&f, &forge);
    assert!(!why.iter().any(|r| r.starts_with(REFUSED)) && why.iter().any(|r| r.contains("different specs")), "{why:?}");
}

#[test]
fn a_constituent_whose_head_moved_since_the_approval_makes_the_rollup_stale() {
    let (f, forge, _, _) = world();
    opened(&f, &forge);
    forge.edit(2, |p| p.head_sha = "d".repeat(40));
    let why = refusal(&f, &forge);
    assert!(why[0].starts_with(STALE) && why[0].contains("#2"), "{why:?}");
    assert!(Errors::at(f.home.path()).entries().iter().any(|e| e.class == "rollup-stale"));
}
