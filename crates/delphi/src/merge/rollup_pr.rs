//! The rollup as a pull request (spec 094 §13.2 steps 2 to 5, §13.3; slice S19): the operator's approval of a batch, opening the
//! candidate with a plain push of `delphi-rollup/<id>` and a body that carries exactly one manifest, and recognising a rollup again
//! at merge time. Recognition takes three things together: the branch prefix, exactly one manifest, and a rebuild from the
//! manifest (its base, which must be the base tip, and each constituent's approved head) whose tree equals the rollup head's tree
//! byte for byte. Only then is the cross-spec content accepted; anything else is an ordinary pull request again, refused as a
//! cross-spec bundle as before. The body is trusted for nothing: every fact in it is derived again.
//!
//! `merge` (S20) is the only path here to `Forge::merge`: inside an invocation the operator started and approved, on a green rollup
//! whose constituents are all still at their approved heads. A red rollup is never merged: `bisect` (S21) closes it and splits it.
//! This module is on the gate list.

use super::check::{evaluate, Verdict, BEHIND_MARK};
use super::errors::{Class, Errors};
use super::forge::Forge;
use super::record::{Attestation, Ledger, SpecState};
use super::refresh::Runner;
use super::rollup::{parse_manifest, rebuild, spec_of, Candidate, Choice, Constituent, Manifest, BRANCH_PREFIX};
use super::approve::attest_rollup;
use super::{short, wait, Ctx, Outcome};
use serde_json::json;
use std::path::Path;

/// Every refusal of a rollup starts with one of these (`errors::classify_refusal` reads them, §13.8).
pub const REFUSED: &str = "rollup refused: ";
pub const STALE: &str = "rollup stale: ";
/// An entry for a constituent bisection found red on its own, or red together and green apart (§13.5, S21).
pub const RED: &str = "rollup red: ";

/// Where a rollup is built and rebuilt: the repository the command runs in and the Delphi home (for the temporary worktree).
#[derive(Clone, Copy)]
pub struct Env<'a> {
    pub git: &'a Runner,
    pub home: &'a Path,
}

/// The operator's approval of a batch, bound to the `(pr, head)` pairs shown, not to the rollup (§13.4 invariant 4).
#[derive(Debug, Clone, PartialEq)]
pub struct Approval {
    pub att: Attestation,
    pub pairs: Vec<(u64, String)>,
}

impl Approval {
    pub fn covers(&self, pr: u64, head: &str) -> bool {
        self.pairs.iter().any(|(n, h)| *n == pr && h == head)
    }
}

/// What the operator is shown before approving a batch: each constituent's number, title, head and spec.
pub fn shown(choice: &Choice) -> Vec<String> {
    let mut out = vec![format!("a rollup of {} pull request(s) into `{}`, merged as one pull request once its CI is green:", choice.chosen.len(), choice.base_ref)];
    out.extend(choice.chosen.iter().map(|c| format!("  #{} {} (head {}, spec {})", c.pr, c.title, short(&c.head_sha), c.spec.as_deref().unwrap_or("none"))));
    out
}

/// The body a rollup pull request is opened with: a plain list for people, then the one manifest block.
pub fn body(c: &Candidate) -> String {
    let list: Vec<String> = c.merged.iter().map(|(k, m)| format!("- #{} (head `{}`, spec {}; merge commit `{}`)", k.pr, k.head_sha, k.spec.as_deref().unwrap_or("none"), short(m))).collect();
    format!(
        "A rollup made by `delphi merge --batch` (spec 094 §13): {} pull request(s), each approved by the operator on its own head, merged into `{}` at `{}` in this order with one merge commit each.\n\n{}\n\nOnly the command that made it merges it, and only after a rebuild from the manifest below reproduces this branch's tree. Do not push to this branch. Without Delphi: check the manifest by hand and merge with a merge commit, or close it and merge the pull requests one by one.\n\n{}",
        c.merged.len(),
        c.base_ref,
        c.base_sha,
        list.join("\n"),
        c.manifest()
    )
}

/// Open the candidate as a rollup pull request: a plain push of a new branch (never a force; one that exists rejects it), then the
/// pull request. Constituents the build left out are written to the error log, each with its class. Returns the number.
pub fn open(forge: &dyn Forge, env: Env, ledger: &Ledger, errors: &Errors, c: &Candidate, approval: &Approval, now: &str) -> Result<u64, String> {
    for (n, why) in &c.dropped {
        let class = if why.contains("its branch is at") { Class::RollupStale } else { Class::RollupRefused };
        let head = approval.pairs.iter().find(|p| p.0 == *n).map(|p| p.1.clone()).unwrap_or_default();
        let _ = errors.record(*n, &head, class, &format!("left out of rollup `{}`: {why}", c.branch), now);
    }
    if let Some((k, _)) = c.merged.iter().find(|(k, _)| !approval.covers(k.pr, &k.head_sha)) {
        return Err(format!("{STALE}#{} at `{}` is not a head the operator approved", k.pr, short(&k.head_sha)));
    }
    if !c.branch.starts_with(BRANCH_PREFIX) {
        return Err(format!("{REFUSED}`{}` is not a rollup branch", c.branch));
    }
    env.git.git(&env.git.repo, &["push", "-q", "origin", &format!("{}:refs/heads/{}", c.head, c.branch)]).map_err(|e| format!("the push of `{}` was rejected (does the branch exist already?): {e}", c.branch))?;
    let numbers: Vec<String> = c.merged.iter().map(|(k, _)| format!("#{}", k.pr)).collect();
    let title = format!("Rollup of {} pull requests: {}", c.merged.len(), numbers.join(", "));
    let pr = forge.open_pr(&c.base_ref, &c.branch, &title, &body(c)).map_err(|e| format!("pushed `{}`, but the pull request could not be opened ({e}); close the branch by hand", c.branch))?;
    let parts: Vec<_> = c.merged.iter().map(|(k, m)| json!({"pr": k.pr, "head_sha": k.head_sha, "spec": k.spec, "merge_commit": m})).collect();
    let row = json!({"kind": "rollup_opened", "pr": pr, "branch": c.branch, "head_sha": c.head, "base_ref": c.base_ref, "base_sha": c.base_sha, "constituents": parts, "attestation": approval.att.label(), "quote": approval.att.quote(), "delphi_version": env!("CARGO_PKG_VERSION"), "time": now});
    if let Err(e) = ledger.append(&row) {
        eprintln!("delphi merge: rollup #{pr} was opened, but the ledger row could not be written: {e}");
    }
    Ok(pr)
}

/// A rollup that was recognised: its own verdict with the cross-spec refusal lifted, its manifest, and each constituent's verdict.
#[derive(Debug, Clone)]
pub struct Recognised {
    pub verdict: Verdict,
    pub manifest: Manifest,
    pub parts: Vec<Verdict>,
}

/// Recognise pull request `v` as a rollup (§13.3), or say why it is not one (a reason that starts with `REFUSED` or `STALE`).
pub fn recognise(forge: &dyn Forge, env: Env, v: &Verdict) -> Result<Recognised, String> {
    let refused = |why: String| format!("{REFUSED}{why}; as an ordinary pull request it is refused as before");
    if !v.pr.head_ref.starts_with(BRANCH_PREFIX) {
        return Err(refused(format!("its branch `{}` does not start with `{BRANCH_PREFIX}`", v.pr.head_ref)));
    }
    let m = parse_manifest(&v.pr.body).map_err(refused)?;
    if m.base_ref != v.pr.base_ref {
        return Err(refused(format!("the manifest's base is `{}`, the pull request's `{}`", m.base_ref, v.pr.base_ref)));
    }
    if v.gate_changing() {
        return Err(refused(format!("it changes a gate ({}); a rollup never does", v.gate_hits.join(", "))));
    }
    let (mut parts, mut chosen) = (Vec::new(), Vec::new());
    for (k, head, spec) in &m.prs {
        let cv = evaluate(forge, *k).map_err(|e| refused(format!("#{k} cannot be read: {e}")))?;
        if cv.pr.head_sha != *head {
            return Err(format!("{STALE}#{k} is at `{}`, not the head `{}` the manifest names; it needs the operator's approval again, or the batch is built again", short(&cv.pr.head_sha), short(head)));
        }
        let blocking: Vec<String> = wait::reasons(&cv).into_iter().filter(|r| !r.contains(BEHIND_MARK)).collect();
        if cv.gate_changing() || !blocking.is_empty() || cv.pr.base_ref != m.base_ref {
            let why = if cv.gate_changing() { format!("it changes a gate ({})", cv.gate_hits.join(", ")) } else if blocking.is_empty() { format!("its base is `{}`", cv.pr.base_ref) } else { blocking.join("; ") };
            return Err(refused(format!("#{k} may not be in a batch: {why}")));
        }
        if spec_of(&cv) != *spec {
            return Err(refused(format!("#{k} names spec {}, the manifest {}", spec_of(&cv).unwrap_or("-".into()), spec.as_deref().unwrap_or("-"))));
        }
        chosen.push(Constituent { pr: *k, head_sha: head.clone(), head_ref: cv.pr.head_ref.clone(), title: cv.pr.title.clone(), spec: spec.clone() });
        parts.push(cv);
    }
    let choice = Choice { base_ref: m.base_ref.clone(), chosen, skipped: Vec::new() };
    let c = rebuild(forge, env.git, env.home, &choice, &format!("check-{}", v.pr.number), &m.base_sha).map_err(|e| refused(format!("the rebuild from the manifest failed: {e}")))?;
    if let Some((k, why)) = c.dropped.first() {
        return Err(if why.contains("its branch is at") { format!("{STALE}#{k}: {why}") } else { refused(format!("the rebuild left #{k} out: {why}")) });
    }
    let g = env.git;
    let tip = |r: &str| g.git(&g.repo, &["rev-parse", "--verify", "-q", r]).map(|s| s.trim().to_string());
    // The rebuild fetched the base; the manifest's base must be its tip, or the rollup was built on a base that has moved since.
    if tip(&format!("refs/remotes/origin/{}", m.base_ref)).unwrap_or_default() != c.base_sha {
        return Err(format!("{STALE}the manifest's base `{}` is no longer the tip of `{}`; build the batch again", short(&m.base_sha), m.base_ref));
    }
    g.git(&g.repo, &["fetch", "-q", "origin", &v.pr.head_ref]).map_err(|e| refused(format!("could not fetch the rollup branch: {e}")))?;
    if tip(&format!("refs/remotes/origin/{}", v.pr.head_ref)).unwrap_or_default() != v.pr.head_sha {
        return Err(refused(format!("the branch `{}` is not at the head GitHub reports (`{}`)", v.pr.head_ref, short(&v.pr.head_sha))));
    }
    let tree = tip(&format!("{}^{{tree}}", v.pr.head_sha)).map_err(|e| refused(format!("could not read the rollup's tree: {e}")))?;
    if tree != c.tree {
        return Err(refused(format!("a rebuild from the manifest does not reproduce its tree (the rollup's is `{}`, the rebuild's `{}`): it carries a change no approved constituent makes", short(&tree), short(&c.tree))));
    }
    // Recognised: the cross-spec refusal (and any other spec refusal of the whole) is lifted; each constituent's own spec was
    // judged above. Invariant 3 held twice: the rollup's own paths against the base's list, and the rebuild's.
    Ok(Recognised { verdict: lifted(v), manifest: m, parts })
}

/// A recognised rollup's own verdict, its spec refusals lifted.
fn lifted(v: &Verdict) -> Verdict {
    let mut out = v.clone();
    let whys: Vec<&String> = v.specs.iter().filter_map(|s| if let SpecState::Missing(why) = s { Some(why) } else { None }).collect();
    out.refusals.retain(|r| !whys.contains(&r));
    out
}

/// `delphi merge --batch` without `--dry-run`: choose, show the batch, take the operator's approval of it, build, open, then wait
/// for the rollup's checks and merge it (§13.2). The approval is bound to the heads shown.
pub fn batch(ctx: &mut Ctx, numbers: &[u64], max: usize, from_all: bool, id: &str) -> Result<Outcome, String> {
    let env = ctx.rollup.ok_or("a rollup is built only in a repository")?;
    let choice = super::rollup::choose(ctx.forge, numbers, max);
    for (n, why) in &choice.skipped {
        ctx.term.show(&format!("#{n}: not in the batch, it stays on the serial path: {why}"));
    }
    if choice.chosen.is_empty() {
        return Err("no pull request can be batched".into());
    }
    shown(&choice).iter().for_each(|l| ctx.term.show(l));
    let prs: Vec<u64> = choice.chosen.iter().map(|c| c.pr).collect();
    let att = attest_rollup(ctx.term, ctx.quote.as_deref(), &prs, from_all)?;
    let approval = Approval { att, pairs: choice.chosen.iter().map(|c| (c.pr, c.head_sha.clone())).collect() };
    let c = super::rollup::build(ctx.forge, env.git, env.home, &choice, id)?;
    c.lines().iter().for_each(|l| ctx.term.show(l));
    let pr = open(ctx.forge, env, ctx.ledger, ctx.errors, &c, &approval, &(ctx.now)())?;
    ctx.term.show(&format!("opened rollup #{pr} from `{}`", c.branch));
    let v = evaluate(ctx.forge, pr).map_err(|e| format!("rollup #{pr} was opened, but it cannot be read ({e}); run `delphi merge {pr}` later"))?;
    let out = merge(ctx, env, v, Some(&approval));
    super::note(ctx, &out);
    Ok(out)
}

/// `delphi merge <PR>` on a pull request whose branch is a rollup's: recognised, or refused with the reasons of an ordinary one.
pub(super) fn decide(ctx: &mut Ctx, v: Verdict, queued: bool) -> Outcome {
    let (n, head) = (v.pr.number, v.pr.head_sha.clone());
    let refuse = |first: String, v: &Verdict| Outcome::Refused { pr: n, reasons: std::iter::once(first).chain(v.refusals.iter().cloned()).collect(), head: head.clone() };
    if queued {
        return refuse(format!("{REFUSED}a rollup is never merged from the queue; only `delphi merge {n}` with the operator's approval of its pull requests merges it"), &v);
    }
    let Some(env) = ctx.rollup else {
        return refuse(format!("{REFUSED}a rollup is recognised only by a rebuild in the repository"), &v);
    };
    merge(ctx, env, v, None)
}

/// Recognise the rollup `v`, take the approval of its constituents (this invocation's, or the operator's now), wait for its checks,
/// and merge it only when every check passed (`ci-gate` among them) and every constituent is still at its approved head (§13.2
/// steps 6 and 7). Never merges a red or pending rollup.
fn merge(ctx: &mut Ctx, env: Env, v: Verdict, approval: Option<&Approval>) -> Outcome {
    let (n, head) = (v.pr.number, v.pr.head_sha.clone());
    let refuse = |reasons: Vec<String>| Outcome::Refused { pr: n, reasons, head: head.clone() };
    let r = match recognise(ctx.forge, env, &v) {
        Ok(r) => r,
        Err(why) => return refuse(std::iter::once(why).chain(v.refusals.iter().cloned()).collect()),
    };
    let approval = match approval {
        Some(a) => a.clone(),
        None if ctx.dry_run => Approval { att: Attestation::Tty, pairs: r.manifest.prs.iter().map(|p| (p.0, p.1.clone())).collect() },
        None => {
            ctx.term.show(&format!("rollup #{n} carries, each at its approved head:"));
            r.parts.iter().for_each(|p| ctx.term.show(&format!("  #{} {} (head {}, spec {})", p.pr.number, p.pr.title, short(&p.pr.head_sha), spec_of(p).unwrap_or("none".into()))));
            let prs: Vec<u64> = r.parts.iter().map(|p| p.pr.number).collect();
            match attest_rollup(ctx.term, ctx.quote.as_deref(), &prs, false) {
                Ok(att) => Approval { att, pairs: r.parts.iter().map(|p| (p.pr.number, p.pr.head_sha.clone())).collect() },
                Err(why) => return Outcome::Declined { pr: n, why },
            }
        }
    };
    match judge(ctx, &r, n, &head, &approval) {
        Judged::Green if ctx.dry_run => Outcome::DryRun { pr: n, head: head.clone(), gate_changing: false },
        Judged::Green => record_and_merge(ctx, env, &r, n, &head, &approval),
        // A red rollup is bisected (§13.5, S21), never merged; a dry run only says so.
        Judged::Red(_) if !ctx.dry_run => super::bisect::bisect(ctx, env, &r, n, &approval),
        Judged::Red(reasons) | Judged::Refused(reasons) => refuse(reasons),
    }
}

/// What the rollup's checks and constituents say, once its checks finished (or the wait ran out).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Judged {
    Green,
    /// Its checks finished and `ci-gate` failed; nothing else refuses it. Bisected (S21).
    Red(Vec<String>),
    /// Anything else: still running, no `ci-gate`, a constituent that moved or closed, an approval that does not cover it.
    Refused(Vec<String>),
}

/// Judge the recognised rollup `n` at `head` under `approval`: wait for its checks, then require a passing `ci-gate` and every
/// constituent still open at the head the manifest names (invariant 2, at the last moment).
pub(super) fn judge(ctx: &Ctx, r: &Recognised, n: u64, head: &str, approval: &Approval) -> Judged {
    if let Some((k, h, _)) = r.manifest.prs.iter().find(|(k, h, _)| !approval.covers(*k, h)) {
        return Judged::Refused(vec![format!("{STALE}#{k} at `{}` is not a head the operator approved in this run", short(h))]);
    }
    let v = match wait_green(ctx, n, head) {
        Ok(v) => lifted(&v),
        Err(why) => return Judged::Refused(why),
    };
    let mut reasons = v.refusals.clone();
    let gate = v.checks.iter().find(|c| c.name == REQUIRED_CHECK).map(|c| c.bucket.clone());
    if gate.as_deref() != Some("pass") {
        reasons.push(format!("checks failed: `{REQUIRED_CHECK}` has not passed on the rollup"));
    }
    for p in &r.parts {
        match ctx.forge.pr(p.pr.number) {
            Ok(now) if now.head_sha == p.pr.head_sha && now.state == "OPEN" => {}
            Ok(now) => reasons.push(format!("{STALE}#{} is {} at `{}` now, not open at the approved head", p.pr.number, now.state.to_lowercase(), short(&now.head_sha))),
            Err(e) => reasons.push(e.to_string()),
        }
    }
    if reasons.is_empty() {
        Judged::Green
    } else if gate.as_deref() == Some("fail") && reasons.iter().all(|r| r.starts_with("checks failed")) {
        Judged::Red(reasons)
    } else {
        Judged::Refused(reasons)
    }
}

/// The check every rollup must have passed (§12.2.1: the one the ruleset requires).
pub const REQUIRED_CHECK: &str = "ci-gate";

/// Wait for the rollup's checks within the invocation's wait; its head must not move meanwhile.
fn wait_green(ctx: &Ctx, n: u64, head: &str) -> Result<Verdict, Vec<String>> {
    let start = ctx.refresh.map(|r| r.clock.now_secs()).unwrap_or(0);
    loop {
        let v = evaluate(ctx.forge, n).map_err(|e| vec![e.to_string()])?;
        if v.pr.head_sha != head {
            return Err(vec![format!("{REFUSED}the rollup's head moved to `{}`, a commit this run did not make; do not push to a rollup", short(&v.pr.head_sha))]);
        }
        if !v.refusals.iter().any(|r| wait::not_yet(r)) {
            return Ok(v);
        }
        match ctx.refresh {
            Some(r) if r.wait.is_some_and(|w| r.clock.now_secs().saturating_sub(start) < w.as_secs()) => r.clock.sleep(r.poll),
            _ => return Err(vec![format!("the rollup's checks have not finished; run `delphi merge {n}` once they are green (nothing was merged)")]),
        }
    }
}

/// One `approved` row per constituent (its own approval, spec and head, `carried_into` the rollup), the merge of exactly the rollup
/// head, the `merged` rows, the check of each constituent's state (OD-17) and cleanup.
pub(super) fn record_and_merge(ctx: &mut Ctx, env: Env, r: &Recognised, n: u64, head: &str, approval: &Approval) -> Outcome {
    let now = (ctx.now)();
    let row = |p: &Verdict, kind: &str| {
        let mut row = p.record(approval.att.clone(), ctx.session.clone(), now.clone()).to_json(kind);
        row["rollup"] = json!(n);
        row["carried_into"] = json!(n);
        row
    };
    for p in &r.parts {
        if let Err(e) = ctx.ledger.append(&row(p, "approved")) {
            return Outcome::Failed { pr: n, detail: format!("cannot write the ledger ({e}); not merging without a record"), head: head.to_string() };
        }
    }
    if let Err(e) = ctx.forge.merge(n, head) {
        for p in &r.parts {
            let _ = ctx.ledger.append(&row(p, "failed"));
        }
        return Outcome::Failed { pr: n, detail: e.to_string(), head: head.to_string() };
    }
    for p in &r.parts {
        let _ = ctx.ledger.append(&row(p, "merged"));
    }
    let list: Vec<String> = r.parts.iter().map(|p| format!("#{}", p.pr.number)).collect();
    let _ = ctx.ledger.append(&json!({"kind": "rollup_merged", "pr": n, "head_sha": head, "constituents": list, "time": now}));
    let comment = format!("Merged by `delphi merge` (spec 094 §13) at head `{head}`: a rollup of {}, each approved on its own head ({}). Each is its own merge commit inside it.", list.join(", "), approval.att.label());
    let _ = ctx.forge.comment(n, &comment);
    ctx.errors.resolve_pr(n, &now);
    let g = env.git;
    let _ = g.git(&g.repo, &["fetch", "-q", "origin", &r.manifest.base_ref]);
    for p in &r.parts {
        let k = p.pr.number;
        ctx.errors.resolve_pr(k, &now);
        let state = ctx.forge.pr(k).map(|x| x.state).unwrap_or_default();
        let landed = g.git(&g.repo, &["merge-base", "--is-ancestor", &p.pr.head_sha, &format!("refs/remotes/origin/{}", r.manifest.base_ref)]).is_ok();
        let line = match state.as_str() {
            "MERGED" => format!("#{k}: marked merged through rollup #{n}"),
            "OPEN" if landed => match ctx.forge.close(k, &format!("Merged through rollup #{n} (head `{head}`): this pull request's approved head `{}` is in `{}`, but GitHub did not mark it merged, so `delphi merge` closed it (spec 094 §13.2 step 7).", p.pr.head_sha, r.manifest.base_ref)) {
                Ok(()) => format!("#{k}: GitHub did not mark it merged; its head is in `{}`, so it was closed with a comment linking #{n}", r.manifest.base_ref),
                Err(e) => format!("#{k}: GitHub did not mark it merged, and closing it failed: {e}"),
            },
            other => format!("#{k}: left as it is ({}): GitHub did not mark it merged and its head is not in `{}`; look at it", other.to_lowercase(), r.manifest.base_ref),
        };
        ctx.term.show(&line);
    }
    for p in r.parts.iter().chain(std::iter::once(&r.verdict)) {
        for problem in super::after_merge(ctx, p) {
            super::put_error(ctx.errors, &now, p.pr.number, &p.pr.head_sha, Class::Cleanup, &problem);
        }
    }
    Outcome::Merged { pr: n, head: head.to_string() }
}
