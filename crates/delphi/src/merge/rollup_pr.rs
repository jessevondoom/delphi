//! The rollup as a pull request (spec 094 §13.2 steps 2 to 5, §13.3; slice S19): the operator's approval of a batch, opening the
//! candidate with a plain push of `delphi-rollup/<id>` and a body that carries exactly one manifest, and recognising a rollup again
//! at merge time. Recognition takes three things together: the branch prefix, exactly one manifest, and a rebuild from the
//! manifest (its base, which must be the base tip, and each constituent's approved head) whose tree equals the rollup head's tree
//! byte for byte. Only then is the cross-spec content accepted; anything else is an ordinary pull request again, refused as a
//! cross-spec bundle as before. The body is trusted for nothing: every fact in it is derived again.
//!
//! Nothing here merges. This module is on the gate list.

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
    let mut verdict = v.clone();
    let spec_whys: Vec<&String> = v.specs.iter().filter_map(|s| if let SpecState::Missing(why) = s { Some(why) } else { None }).collect();
    verdict.refusals.retain(|r| !spec_whys.contains(&r));
    Ok(Recognised { verdict, manifest: m, parts })
}

/// `delphi merge --batch` without `--dry-run`: choose, show the batch, take the operator's approval of it, build, open. Returns the
/// rollup's number and the approval (bound to the heads shown).
pub fn batch(ctx: &mut Ctx, numbers: &[u64], max: usize, from_all: bool, id: &str) -> Result<(u64, Approval), String> {
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
    Ok((pr, approval))
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
    match recognise(ctx.forge, env, &v) {
        Err(why) => refuse(why, &v),
        Ok(r) => {
            let list: Vec<String> = r.parts.iter().map(|p| format!("#{}", p.pr.number)).collect();
            let mut reasons = vec![format!("recognised as a rollup of {} (manifest base {}); merging a rollup is slice S20 of spec 094 §13, not in this build", list.join(", "), short(&r.manifest.base_sha))];
            reasons.extend(r.verdict.refusals);
            Outcome::Refused { pr: n, reasons, head }
        }
    }
}
