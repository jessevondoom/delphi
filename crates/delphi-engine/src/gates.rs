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
    l == "governance.md" || l == "review.md" || l.starts_with("agents/") || l.starts_with("skills/") || l == "context/heuristics.md" || l.starts_with("memory/agents/")
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

const DEPENDENCY_FILES: [&str; 17] = [
    "cargo.toml", "cargo.lock", "package.json", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "go.mod", "go.sum", "pyproject.toml", "pipfile", "pipfile.lock", "poetry.lock", "gemfile", "gemfile.lock", "composer.json", "composer.lock", "requirements.txt",
];

fn is_dependency_file(path: &str) -> bool {
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

/// Gate 3.
pub fn spec(link: &SpecLink, diff: &DiffSummary, exemption_approved: bool) -> Gate {
    match link {
        SpecLink::Ratified(s) => Gate::new(3, "spec", true, format!("implements {} (its current hash is ratified)", s)),
        SpecLink::NotRatified { spec, why } => Gate::new(3, "spec", false, format!("{} is not a ratified spec: {}", spec, why)),
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

/// Gate 4: the audit must name *this* SHA; an audit of an earlier commit proves nothing about this one.
pub fn audit(sha: &str, audits: &[Audit]) -> Gate {
    let Some(a) = audits.iter().rev().find(|a| a.sha == sha) else {
        return Gate::new(4, "audit", false, if audits.is_empty() { "no REVIEW.md audit has been recorded for this task".to_string() } else { "the recorded audit is for an earlier commit; audit this one".to_string() });
    };
    let open: Vec<&Finding> = a.findings.iter().filter(|f| matches!(f.severity.to_uppercase().as_str(), "P0" | "P1")).collect();
    if open.is_empty() {
        Gate::new(4, "audit", true, format!("{} audited this commit: no open P0 or P1", a.role))
    } else {
        Gate::new(4, "audit", false, format!("{} open P0/P1 finding(s): {}", open.len(), open.iter().take(3).map(|f| format!("[{}] {}", f.severity, f.title)).collect::<Vec<_>>().join("; ")))
    }
}

/// Gate 5: the operator's approval, recorded for this SHA.
pub fn approval(sha: &str, approved_shas: &[String]) -> Gate {
    Gate::new(5, "operator", approved_shas.iter().any(|s| s == sha), if approved_shas.iter().any(|s| s == sha) { "the operator approved this commit" } else { "waiting for the operator's approval of this commit" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::FileChange;

    fn fc(path: &str, added: u64, removed: u64, host_exec: bool) -> FileChange {
        FileChange { path: path.into(), status: 'M', added, removed, host_exec, special: None }
    }

    fn diff(files: Vec<FileChange>) -> DiffSummary {
        DiffSummary { head_sha: "abc".into(), files }
    }

    fn ok() -> RunSummary {
        RunSummary { ok: true, detail: "exit 0".into() }
    }

    #[test]
    fn no_configured_tests_is_not_green() {
        assert_eq!(tests(None).state, State::Red);
        assert_eq!(tests(Some(ok())).state, State::Green);
        let fail = tests(Some(RunSummary { ok: false, detail: "exit 101".into() }));
        assert_eq!((fail.state, fail.detail.as_str()), (State::Red, "exit 101"));
    }

    #[test]
    fn checks_need_provenance_for_every_changed_file_and_leave_governance_alone() {
        let d = diff(vec![fc("src/a.rs", 3, 0, false)]);
        assert_eq!(checks(Some(ok()), &[], &d).state, State::Green);
        assert_eq!(checks(None, &[], &d).state, State::Green);
        let g = checks(Some(ok()), &["src/a.rs".into()], &d);
        assert!(g.state == State::Red && g.detail.contains("no provenance stamp"), "{}", g.detail);
        let g = checks(Some(RunSummary { ok: false, detail: "clippy".into() }), &[], &d);
        assert!(g.state == State::Red && g.detail.contains("check command failed"));
        for p in ["GOVERNANCE.md", "agents/builder/AGENT.md", "skills/x/SKILL.md", "REVIEW.md", "memory/agents/builder/lessons.md"] {
            let g = checks(None, &[], &diff(vec![fc(p, 1, 0, false)]));
            assert!(g.state == State::Red && g.detail.contains("protected"), "{}: {}", p, g.detail);
        }
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
        assert!(g.state == State::Red && g.detail.contains("[p1]"), "{}", g.detail);
        // The latest audit of the same commit wins (a re-audit after fixes).
        let both = vec![bad[0].clone(), Audit { sha: "aaa".into(), role: "red-team".into(), findings: vec![] }];
        assert_eq!(audit("aaa", &both).state, State::Green);
    }

    #[test]
    fn the_operators_approval_is_bound_to_the_sha() {
        assert_eq!(approval("aaa", &["aaa".into()]).state, State::Green);
        assert_eq!(approval("bbb", &["aaa".into()]).state, State::Red);
        let r = Report { sha: "aaa".into(), gates: vec![tests(Some(ok())), approval("aaa", &[])] };
        assert!(!r.all_green() && r.red().len() == 1 && r.red()[0].n == 5);
    }
}
