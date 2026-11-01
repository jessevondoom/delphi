//! The five merge gates (`specs/003` §3), evaluated against **one exact commit**:
//!
//! 1. tests green in the sandboxed worktree;
//! 2. deterministic checks pass: the project's check command, every changed file carries a provenance
//!    stamp for its exact content, and no protected governance file changed;
//! 3. `Implements-Spec` names a spec whose *current* hash is ratified, or the change qualifies for the
//!    bounded exemption and the operator approved it;
//! 4. the base-branch `REVIEW.md` audit recorded for this SHA has no open `P0` or `P1`;
//! 5. the operator approves the merge (a Red card; see `finalize.rs`).
//!
//! These functions are pure: callers gather facts (run the commands, read the lock and the log) and the
//! verdicts here never depend on an agent's say-so. A gate that cannot be evaluated is Red, never Green.

use crate::worktree::DiffSummary;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum State {
    Green,
    Red,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gate {
    pub n: u8,
    pub name: &'static str,
    pub state: State,
    pub detail: String,
}

impl Gate {
    fn new(n: u8, name: &'static str, ok: bool, detail: impl Into<String>) -> Gate {
        Gate { n, name, state: if ok { State::Green } else { State::Red }, detail: detail.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub sha: String,
    pub gates: Vec<Gate>,
}

impl Report {
    pub fn all_green(&self) -> bool {
        self.gates.iter().all(|g| g.state == State::Green)
    }

    pub fn red(&self) -> Vec<&Gate> {
        self.gates.iter().filter(|g| g.state == State::Red).collect()
    }
}

/// What running a configured command came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub ok: bool,
    pub detail: String,
    /// The command actually ran (false when it was refused or the operator declined it).
    pub ran: bool,
}

/// Gate 1. A project with no `test_command` configured cannot pass: "no tests" is not "tests green".
pub fn tests(run: Option<RunSummary>) -> Gate {
    match run {
        None => Gate::new(1, "tests", false, "no `test_command` is configured in [sandbox], so there is nothing to run"),
        Some(r) => Gate::new(1, "tests", r.ok, r.detail),
    }
}

/// Paths a build may not change on its own: governance and agent definitions change only through ratified
/// proposals (`specs/001` §5.1 gate, GOVERNANCE §5.7).
pub fn is_protected_path(path: &str) -> bool {
    let l = path.to_lowercase();
    l == "governance.md" || l == "review.md" || l.starts_with("agents/") || l.starts_with("skills/") || l == "context/heuristics.md" || l.starts_with("memory/agents/") || l == "delphi.toml" || l.starts_with(".github/") || l.starts_with(".delphi/")
}

/// Gate 2. `check` is `None` when no `check_command` is configured (the other checks still apply).
pub fn checks(check: Option<RunSummary>, unstamped: &[String], diff: &DiffSummary) -> Gate {
    let mut problems = Vec::new();
    if let Some(c) = &check {
        if !c.ok {
            problems.push(format!("check command failed: {}", c.detail));
        }
    }
    if !unstamped.is_empty() {
        problems.push(format!("{} changed file(s) have no provenance stamp for their exact content (changed outside the tools): {}", unstamped.len(), unstamped.iter().take(5).cloned().collect::<Vec<_>>().join(", ")));
    }
    let protected: Vec<&str> = diff.files.iter().filter(|f| is_protected_path(&f.path)).map(|f| f.path.as_str()).collect();
    if !protected.is_empty() {
        problems.push(format!("protected governance files changed (they change only through ratified proposals): {}", protected.join(", ")));
    }
    if problems.is_empty() {
        Gate::new(2, "checks", true, if check.is_some() { "check command, provenance and protected-path checks passed" } else { "provenance and protected-path checks passed (no check_command configured)" })
    } else {
        Gate::new(2, "checks", false, problems.join("; "))
    }
}

const DEPENDENCY_FILES: [&str; 28] = [
    "makefile", "dockerfile", "rust-toolchain.toml", "setup.py", "setup.cfg", "uv.lock", ".npmrc", "config.toml", "build.rs", "justfile", "tox.ini",
    "cargo.toml", "cargo.lock", "package.json", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "go.mod", "go.sum", "pyproject.toml", "pipfile", "pipfile.lock", "poetry.lock", "gemfile", "gemfile.lock", "composer.json", "composer.lock", "requirements.txt",
];

pub(crate) fn is_dependency_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    DEPENDENCY_FILES.contains(&name.as_str()) || (name.starts_with("requirements") && name.ends_with(".txt"))
}

/// Why a change does not qualify for the bounded exemption (`specs/003` §3.1.2). Empty means it does.
pub fn exemption_violations(diff: &DiffSummary) -> Vec<String> {
    let mut v = Vec::new();
    if diff.files.len() > 2 {
        v.push(format!("{} files changed (at most 2)", diff.files.len()));
    }
    let lines = diff.added() + diff.removed();
    if lines > 100 {
        v.push(format!("{} lines changed (at most 100)", lines));
    }
    for f in &diff.files {
        if f.host_exec {
            v.push(format!("{} runs on the operator's machine", f.path));
        }
        if is_dependency_file(&f.path) {
            v.push(format!("{} is a dependency manifest or lockfile", f.path));
        }
        if is_protected_path(&f.path) {
            v.push(format!("{} is protected governance", f.path));
        }
    }
    v
}

/// The linked spec, as far as the ratification lock can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecLink {
    /// No `Implements-Spec`.
    None,
    /// Named, and its current content hash is the ratified one.
    Ratified(String),
    /// Named, but missing, edited since ratification, or never ratified.
    NotRatified { spec: String, why: String },
}

/// Directories whose Markdown a build, a role or a ratification reads: a change there is never plain notes.
const GOVERNED_DIRS: [&str; 9] = [".github/", "specs/", "agents/", "skills/", "themes/", "memory/", "about/", "context/facts/", "connectors/"];

/// A diff of ordinary Markdown files and nothing else: added or modified, regular mode, outside the governed
/// directories, not run on the host, and inside the same size limits as the bounded exemption.
pub fn notes_only(diff: &DiffSummary) -> bool {
    !diff.files.is_empty()
        && diff.files.len() <= 2
        && diff.added() + diff.removed() <= 100
        && diff.files.iter().all(|f| {
            let p = f.path.to_lowercase();
            p.ends_with(".md")
                && matches!(f.status, 'A' | 'M')
                && matches!(f.mode.as_str(), "" | "100644")
                && f.special.is_none()
                && !f.host_exec
                && !is_protected_path(&f.path)
                && !GOVERNED_DIRS.iter().any(|d| p.starts_with(d) || p.contains(&format!("/{}", d)))
                && !matches!(p.as_str(), "governance.md" | "agents.md" | "review.md")
        })
}

/// Gate 3.
pub fn spec(link: &SpecLink, diff: &DiffSummary, exemption_approved: bool) -> Gate {
    match link {
        SpecLink::Ratified(s) => Gate::new(3, "spec", true, format!("implements {} (its current hash is ratified)", s)),
        SpecLink::NotRatified { spec, why } => Gate::new(3, "spec", false, format!("{} is not a ratified spec: {}", spec, why)),
        SpecLink::None if notes_only(diff) => Gate::new(3, "spec", true, "Markdown notes only: nothing a build reads, so no spec is needed (specs/064 §6)"),
        SpecLink::None => {
            let v = exemption_violations(diff);
            if !v.is_empty() {
                Gate::new(3, "spec", false, format!("no ratified spec is linked and the change does not qualify for the bounded exemption: {}", v.join("; ")))
            } else if !exemption_approved {
                Gate::new(3, "spec", false, "no ratified spec is linked; the change qualifies for the bounded exemption, but the operator has not approved it")
            } else {
                Gate::new(3, "spec", true, "bounded exemption (at most 2 files and 100 lines, nothing sensitive), approved by the operator")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: String,
    pub title: String,
}

/// A recorded audit of one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audit {
    pub sha: String,
    pub role: String,
    pub findings: Vec<Finding>,
}

/// Gate 4: the audit must name *this* SHA; an audit of an earlier commit proves nothing about this one. For one
/// commit, the latest audit **of each role** counts (a re-audit after fixes supersedes that role's earlier one),
/// and every role's latest must be clean: another role's clean audit cannot hide an open finding. A severity that
/// is not clearly minor (`P2`, `P3`) blocks, so a malformed one fails closed.
pub fn audit(sha: &str, audits: &[Audit]) -> Gate {
    let mut latest: Vec<&Audit> = Vec::new();
    for a in audits.iter().filter(|a| a.sha == sha) {
        match latest.iter_mut().find(|l| l.role == a.role) {
            Some(slot) => *slot = a,
            None => latest.push(a),
        }
    }
    if latest.is_empty() {
        return Gate::new(4, "audit", false, if audits.is_empty() { "no REVIEW.md audit has been recorded for this task".to_string() } else { "the recorded audit is for an earlier commit; audit this one".to_string() });
    }
    let blocking = |f: &Finding| !matches!(f.severity.trim().to_uppercase().as_str(), "P2" | "P3");
    let open: Vec<(&str, &Finding)> = latest.iter().flat_map(|a| a.findings.iter().filter(|f| blocking(f)).map(move |f| (a.role.as_str(), f))).collect();
    if open.is_empty() {
        Gate::new(4, "audit", true, format!("{} audited this commit: no open P0 or P1", latest.iter().map(|a| a.role.as_str()).collect::<Vec<_>>().join(", ")))
    } else {
        Gate::new(4, "audit", false, format!("{} open P0/P1 finding(s): {}", open.len(), open.iter().take(3).map(|(r, f)| format!("[{} {}] {}", r, f.severity, f.title)).collect::<Vec<_>>().join("; ")))
    }
}

/// Gate 5: the operator's approval, recorded for this SHA.
pub fn approval(sha: &str, approved_shas: &[String]) -> Gate {
    Gate::new(5, "operator", approved_shas.iter().any(|s| s == sha), if approved_shas.iter().any(|s| s == sha) { "the operator approved this commit" } else { "waiting for the operator's approval of this commit" })
}

/// What the ledger of provisional decisions (`specs/015`) says about a task, for gate 6.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecisionCounts {
    /// Provisional decisions that allowed something: only these enabled work, so only these need review.
    pub allowed: usize,
    /// Ids of allowed decisions the operator has not yet reviewed.
    pub pending: Vec<String>,
    /// Ids of allowed decisions the operator rejected.
    pub rejected: Vec<String>,
}

/// Gate 6: every provisional decision that allowed something has been reviewed and ratified by the operator.
/// A task that made none is green. A rejection is red and says so: the work built on it is suspect.
pub fn decisions(c: &DecisionCounts) -> Gate {
    if !c.rejected.is_empty() {
        return Gate::new(6, "decisions", false, format!("{} provisional decision(s) were rejected by the operator: {}", c.rejected.len(), c.rejected.iter().take(5).cloned().collect::<Vec<_>>().join(", ")));
    }
    if !c.pending.is_empty() {
        return Gate::new(6, "decisions", false, format!("{} provisional decision(s) await the operator's review: {}", c.pending.len(), c.pending.iter().take(5).cloned().collect::<Vec<_>>().join(", ")));
    }
    if c.allowed == 0 {
        Gate::new(6, "decisions", true, "no provisional decisions were made")
    } else {
        Gate::new(6, "decisions", true, format!("{} provisional decision(s), all reviewed and ratified by the operator", c.allowed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::FileChange;

    #[test]
    fn gate_six_is_green_with_no_decisions_or_all_ratified_and_red_for_pending_or_rejected() {
        assert_eq!(decisions(&DecisionCounts::default()).state, State::Green);
        let all = decisions(&DecisionCounts { allowed: 3, ..Default::default() });
        assert!(all.state == State::Green && all.detail.contains("3 provisional") && all.n == 6);
        let pending = decisions(&DecisionCounts { allowed: 3, pending: vec!["dec_a".into()], rejected: vec![] });
        assert!(pending.state == State::Red && pending.detail.contains("await") && pending.detail.contains("dec_a"));
        let rejected = decisions(&DecisionCounts { allowed: 3, pending: vec!["dec_a".into()], rejected: vec!["dec_b".into()] });
        assert!(rejected.state == State::Red && rejected.detail.contains("rejected") && rejected.detail.contains("dec_b"), "a rejection outranks a pending review");
    }

    fn fc(path: &str, added: u64, removed: u64, host_exec: bool) -> FileChange {
        FileChange { path: path.into(), status: 'M', added, removed, host_exec, special: None, mode: String::new() }
    }

    fn diff(files: Vec<FileChange>) -> DiffSummary {
        DiffSummary { head_sha: "abc".into(), files }
    }

    fn ok() -> RunSummary {
        RunSummary { ok: true, detail: "exit 0".into(), ran: true }
    }

    #[test]
    fn no_configured_tests_is_not_green() {
        assert_eq!(tests(None).state, State::Red);
        assert_eq!(tests(Some(ok())).state, State::Green);
        let fail = tests(Some(RunSummary { ok: false, detail: "exit 101".into(), ran: true }));
        assert_eq!((fail.state, fail.detail.as_str()), (State::Red, "exit 101"));
    }

    #[test]
    fn checks_need_provenance_for_every_changed_file_and_leave_governance_alone() {
        let d = diff(vec![fc("src/a.rs", 3, 0, false)]);
        assert_eq!(checks(Some(ok()), &[], &d).state, State::Green);
        assert_eq!(checks(None, &[], &d).state, State::Green);
        let g = checks(Some(ok()), &["src/a.rs".into()], &d);
        assert!(g.state == State::Red && g.detail.contains("no provenance stamp"), "{}", g.detail);
        let g = checks(Some(RunSummary { ok: false, detail: "clippy".into(), ran: true }), &[], &d);
        assert!(g.state == State::Red && g.detail.contains("check command failed"));
        for p in ["GOVERNANCE.md", "agents/builder/AGENT.md", "skills/x/SKILL.md", "REVIEW.md", "memory/agents/builder/lessons.md"] {
            let g = checks(None, &[], &diff(vec![fc(p, 1, 0, false)]));
            assert!(g.state == State::Red && g.detail.contains("protected"), "{}: {}", p, g.detail);
        }
    }

    #[test]
    fn markdown_only_diffs_need_no_spec_but_governed_or_special_files_still_do() {
        let ok = |paths: Vec<FileChange>| spec(&SpecLink::None, &diff(paths), false);
        assert_eq!(ok(vec![fc("docs/notes.md", 20, 0, false)]).state, State::Green);
        assert_eq!(ok(vec![fc("research/a.md", 20, 0, false), fc("research/b.md", 20, 0, false)]).state, State::Green);
        for p in ["specs/001-x.md", "agents/builder/AGENT.md", "memory/a.md", "about/x.md", ".github/x.md", "GOVERNANCE.md", "docs/a.rs", "docs/a.md.sh"] {
            assert_eq!(ok(vec![fc(p, 1, 0, false)]).state, State::Red, "{}", p);
        }
        let mut exec = fc("docs/run.md", 1, 0, false);
        exec.mode = "100755".into();
        assert_eq!(ok(vec![exec]).state, State::Red);
        let mut gone = fc("docs/old.md", 0, 5, false);
        gone.status = 'D';
        assert_eq!(ok(vec![gone]).state, State::Red);
        assert_eq!(ok(vec![fc("docs/a.md", 150, 0, false)]).state, State::Red);
        assert_eq!(ok(vec![fc("docs/a.md", 1, 0, false), fc("docs/b.md", 1, 0, false), fc("docs/c.md", 1, 0, false)]).state, State::Red);
    }

    #[test]
    fn the_bounded_exemption_has_hard_numeric_and_path_limits() {
        assert!(exemption_violations(&diff(vec![fc("src/a.rs", 60, 40, false), fc("src/b.rs", 0, 0, false)])).is_empty(), "exactly 2 files and 100 lines qualify");
        assert_eq!(exemption_violations(&diff(vec![fc("a", 1, 0, false), fc("b", 1, 0, false), fc("c", 1, 0, false)])).len(), 1);
        assert_eq!(exemption_violations(&diff(vec![fc("a", 101, 0, false)])).len(), 1);
        for (path, host) in [("build.rs", true), ("Cargo.lock", false), ("sub/package.json", false), ("requirements-dev.txt", false), ("agents/x/AGENT.md", false)] {
            assert!(!exemption_violations(&diff(vec![fc(path, 1, 0, host)])).is_empty(), "{}", path);
        }
    }

    #[test]
    fn the_spec_gate_needs_a_ratified_spec_or_an_approved_small_change() {
        let small = diff(vec![fc("src/a.rs", 5, 1, false)]);
        assert_eq!(spec(&SpecLink::Ratified("specs/011-x.md".into()), &small, false).state, State::Green);
        let stale = spec(&SpecLink::NotRatified { spec: "specs/011-x.md".into(), why: "edited since it was ratified".into() }, &small, true);
        assert!(stale.state == State::Red && stale.detail.contains("edited since"), "an approved exemption does not rescue a named but unratified spec");
        assert_eq!(spec(&SpecLink::None, &small, false).state, State::Red);
        assert_eq!(spec(&SpecLink::None, &small, true).state, State::Green);
        let big = diff(vec![fc("a", 90, 20, false)]);
        let g = spec(&SpecLink::None, &big, true);
        assert!(g.state == State::Red && g.detail.contains("does not qualify"));
    }

    #[test]
    fn an_audit_counts_only_for_the_commit_it_names_and_only_p0_p1_block() {
        let f = |s: &str| Finding { severity: s.into(), title: format!("{} issue", s) };
        let audits = vec![Audit { sha: "aaa".into(), role: "red-team".into(), findings: vec![f("P2"), f("P3")] }];
        assert_eq!(audit("aaa", &audits).state, State::Green);
        assert!(audit("bbb", &audits).detail.contains("earlier commit"), "a new commit re-runs the gate");
        assert!(audit("aaa", &[]).detail.contains("no REVIEW.md audit"));
        let bad = vec![Audit { sha: "aaa".into(), role: "red-team".into(), findings: vec![f("p1"), f("P2")] }];
        let g = audit("aaa", &bad);
        assert!(g.state == State::Red && g.detail.contains("p1"), "{}", g.detail);
        // The latest audit of the same commit wins (a re-audit after fixes).
        let both = vec![bad[0].clone(), Audit { sha: "aaa".into(), role: "red-team".into(), findings: vec![] }];
        assert_eq!(audit("aaa", &both).state, State::Green);
    }

    #[test]
    fn another_roles_clean_audit_cannot_hide_an_open_finding_and_odd_severities_block() {
        let f = |s: &str| Finding { severity: s.into(), title: "t".into() };
        let audits = vec![
            Audit { sha: "aaa".into(), role: "red-team".into(), findings: vec![f("P0")] },
            Audit { sha: "aaa".into(), role: "strategist".into(), findings: vec![] },
        ];
        let g = audit("aaa", &audits);
        assert!(g.state == State::Red && g.detail.contains("red-team"), "{}", g.detail);
        // The same role re-auditing after fixes does supersede its own earlier audit.
        let mut fixed = audits.clone();
        fixed.push(Audit { sha: "aaa".into(), role: "red-team".into(), findings: vec![f("P2")] });
        assert_eq!(audit("aaa", &fixed).state, State::Green);
        for odd in [" P0", "P-0", "critical", "", "blocker"] {
            let a = vec![Audit { sha: "aaa".into(), role: "red-team".into(), findings: vec![f(odd)] }];
            assert_eq!(audit("aaa", &a).state, State::Red, "{:?} must block", odd);
        }
        assert_eq!(audit("aaa", &[Audit { sha: "aaa".into(), role: "r".into(), findings: vec![f(" p3 ")] }]).state, State::Green);
    }

    #[test]
    fn the_operators_approval_is_bound_to_the_sha() {
        assert_eq!(approval("aaa", &["aaa".into()]).state, State::Green);
        assert_eq!(approval("bbb", &["aaa".into()]).state, State::Red);
        let r = Report { sha: "aaa".into(), gates: vec![tests(Some(ok())), approval("aaa", &[])] };
        assert!(!r.all_green() && r.red().len() == 1 && r.red()[0].n == 5);
    }
}
