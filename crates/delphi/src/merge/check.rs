//! The preconditions (spec 094 §3.2): what a pull request must be before `delphi merge` will touch it, decided from facts read
//! at this moment, never from a label or a caller's say-so. `evaluate` never merges and never writes; it returns a verdict that
//! lists every reason the PR is not ready, so a refusal is specific.

use super::forge::{Check, Forge, ForgeError, PrInfo};
use super::gates::{self, Source, GATES_FILE};
use super::record::{blake3_hex, paths_hash, spec_approval, spec_links, Attestation, Record, SpecState};

pub const QUEUE_LABEL: &str = "merge-queue";
pub const REVIEW_LABEL: &str = "needs-human-review";

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub pr: PrInfo,
    pub paths: Vec<String>,
    /// The changed paths that are in the gate class; empty for an ordinary pull request.
    pub gate_hits: Vec<String>,
    pub gates_source: Source,
    pub checks: Vec<Check>,
    /// The first spec state (kept for callers and ledger readers that expect one).
    pub spec: SpecState,
    /// Every distinct spec the trailers name, each judged on its own; one entry (`Exempt`, `MarkdownOnly` or `Missing`) when none is named.
    pub specs: Vec<SpecState>,
    /// Every reason this pull request cannot merge now; empty when it can.
    pub refusals: Vec<String>,
}

impl Verdict {
    pub fn ready(&self) -> bool {
        self.refusals.is_empty()
    }
    pub fn gate_changing(&self) -> bool {
        !self.gate_hits.is_empty()
    }
    /// The record for this verdict, stamped with how it was approved.
    pub fn record(&self, attestation: Attestation, session: Option<String>, time: String) -> Record {
        Record {
            pr: self.pr.number,
            head_sha: self.pr.head_sha.clone(),
            base_ref: self.pr.base_ref.clone(),
            base_sha: self.pr.base_sha.clone(),
            paths_count: self.paths.len(),
            paths_hash: paths_hash(&self.paths),
            spec: self.spec.clone(),
            specs: self.specs.clone(),
            checks: self.checks.iter().map(|c| (c.name.clone(), c.bucket.clone())).collect(),
            gate_changing: self.gate_changing(),
            attestation,
            session,
            time,
        }
    }
}

/// Is the change inside the bounded exemption of `specs/003` §3.1.2: at most 2 files and 100 changed lines, no host-executed file,
/// dependency manifest or lockfile, nothing under the charter, the rubric, `agents/` or `skills/`, and nothing in the gate class?
fn within_exemption(paths: &[String], additions: u64, deletions: u64, gate_hits: &[String]) -> bool {
    const BLOCKED: &[&str] = &[
        "**/Cargo.toml",
        "**/Cargo.lock",
        "**/package.json",
        "**/package-lock.json",
        "**/build.rs",
        ".cargo/**",
        ".envrc",
        ".gitattributes",
        ".gitmodules",
        ".github/**",
        ".vscode/**",
        "GOVERNANCE.md",
        "REVIEW.md",
        "agents/**",
        "skills/**",
    ];
    paths.len() <= 2 && additions + deletions <= 100 && gate_hits.is_empty() && !paths.iter().any(|p| BLOCKED.iter().any(|b| gates::matches(b, p)))
}

/// Documents and Stage 1 spec pull requests: every changed file is Markdown, and none is under a governed directory that needs a
/// spec of its own (`agents/`, `skills/`, `themes/`, `memory/`). `specs/` is allowed: a spec pull request has no spec to link.
fn markdown_only(paths: &[String]) -> bool {
    const NEEDS_A_SPEC: &[&str] = &["agents/**", "skills/**", "themes/**", "memory/**"];
    !paths.is_empty() && paths.iter().all(|p| p.to_lowercase().ends_with(".md") && !NEEDS_A_SPEC.iter().any(|g| gates::matches(g, p)))
}

/// Judge every distinct spec the pull request's trailers name (any commit, or the body). The bundle is valid only if all of them
/// are ratified; each one that is not is returned as `Missing`, so the refusal lists them all. Naming different specs is accepted
/// today (each is still checked); that is recorded in full rather than changed here.
fn spec_states(forge: &dyn Forge, pr: &PrInfo, paths: &[String], gate_hits: &[String]) -> Result<Vec<SpecState>, ForgeError> {
    let links = spec_links(&pr.commit_messages, &pr.body);
    if links.is_empty() {
        return Ok(vec![if markdown_only(paths) {
            SpecState::MarkdownOnly
        } else if within_exemption(paths, pr.additions, pr.deletions, gate_hits) {
            SpecState::Exempt
        } else {
            SpecState::Missing("no `Implements-Spec:` trailer, and the change is neither only Markdown nor inside the bounded exemption (more than 2 files or 100 lines, or it touches code, a manifest, a lockfile, or `agents/`, `skills/`, `themes/` or `memory/`)".into())
        }]);
    }
    let mut states: Vec<SpecState> = Vec::new();
    for link in links {
        let touched = paths.iter().any(|p| p == &link);
        let head_text = forge.file_at(&pr.head_sha, &link)?;
        let state = match forge.file_at(&pr.base_ref, &link)? {
            Some(base) if spec_approval(&base).0 => match &head_text {
                // An amendment must leave the spec approved.
                Some(h) if !touched || spec_approval(h).0 => SpecState::Approved { path: link.clone(), blake3: blake3_hex(h) },
                Some(_) => SpecState::Missing(format!("{link} is approved on {} but this pull request changes it to a state that is not approved", pr.base_ref)),
                None => SpecState::Missing(format!("{link} does not exist on the pull request's head")),
            },
            _ => match head_text {
                // A Stage 1 spec pull request: it brings the approval itself, with the operator's name on it.
                Some(h) if touched && spec_approval(&h).0 && spec_approval(&h).1 => SpecState::SpecPr { path: link.clone(), blake3: blake3_hex(&h) },
                _ => SpecState::Missing(format!("{link} is not approved on {} (it needs `status: approved` and `approved_by:` in its frontmatter)", pr.base_ref)),
            },
        };
        states.push(state);
    }
    Ok(states)
}

/// Decide whether pull request `n` could merge now, from facts only. Whether it is *queued* is the caller's to judge: an ordinary
/// pull request must carry the label, while a gate-changing one is never queueable and a human merges it directly.
pub fn evaluate(forge: &dyn Forge, n: u64) -> Result<Verdict, ForgeError> {
    let pr = forge.pr(n)?;
    let mut refusals: Vec<String> = Vec::new();

    if pr.state != "OPEN" {
        refusals.push(format!("it is {}, not open", pr.state.to_lowercase()));
    }
    if pr.is_draft {
        refusals.push("it is a draft".into());
    }
    match pr.mergeable.as_str() {
        "MERGEABLE" => {}
        "CONFLICTING" => refusals.push(format!("it conflicts with `{}`: merge the base into the branch and resolve (the builder's git skill covers this)", pr.base_ref)),
        other => refusals.push(format!("GitHub has not finished working out whether it merges (`{}`): try again in a moment", if other.is_empty() { "unknown" } else { other })),
    }

    let checks = forge.checks(n)?;
    if checks.is_empty() {
        refusals.push("no checks are reported on the head commit (a pull request that conflicts with its base gets none until the base is merged in)".into());
    } else {
        let names = |wanted: &dyn Fn(&str) -> bool| checks.iter().filter(|c| wanted(&c.bucket)).map(|c| c.name.clone()).collect::<Vec<_>>().join(", ");
        let failed = names(&|b| b == "fail" || b == "cancel");
        let pending = names(&|b| !matches!(b, "pass" | "fail" | "cancel" | "skipping"));
        if !failed.is_empty() {
            refusals.push(format!("checks failed: {failed}"));
        }
        if !pending.is_empty() {
            refusals.push(format!("checks are still running: {pending}"));
        }
    }

    let paths = forge.changed_paths(n)?;
    // The gate list is read from the base branch, never from the pull request.
    let (patterns, gates_source) = match forge.file_at(&pr.base_ref, GATES_FILE)? {
        Some(text) => match gates::parse(&text) {
            Ok(p) => (p, Source::BaseBranch),
            Err(e) => {
                refusals.push(format!("the gate list on `{}` cannot be used, so nothing can be judged: {e}", pr.base_ref));
                (gates::DEFAULT_GATES.iter().map(|s| s.to_string()).collect(), Source::BuiltIn)
            }
        },
        None => (gates::DEFAULT_GATES.iter().map(|s| s.to_string()).collect(), Source::BuiltIn),
    };
    let gate_hits = gates::hits(&patterns, &paths);

    let specs = spec_states(forge, &pr, &paths, &gate_hits)?;
    for s in &specs {
        if let SpecState::Missing(why) = s {
            refusals.push(why.clone());
        }
    }
    // The first Missing if any, else the first state: a single-spec reader still sees a blocked bundle as blocked.
    let spec = specs.iter().find(|s| matches!(s, SpecState::Missing(_))).or(specs.first()).cloned().expect("at least one spec state");

    Ok(Verdict { pr, paths, gate_hits, gates_source, checks, spec, specs, refusals })
}
