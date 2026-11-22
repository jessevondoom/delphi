//! `delphi forge finish --gates` (spec 135 §14.4, F1b): the three gates of `AGENTS.md`, run in order in the checkout as the
//! operator's own process (not sandboxed: the finisher is a terminal command), stopping at the first that fails. A failing
//! `cargo test` is rerun once for its failing tests only, by name with `--exact`; a test that passes on the rerun is `flaky`
//! and the gate passes with that note, as `delphi-engine` `finalize.rs` does for a Builder. When `cargo nextest` is
//! installed (spec 125-A1 rule 5) the tests run as CI runs them instead: nextest's `ci` profile, whose own single retry
//! replaces the rerun here (its FLAKY tests are named the same way), then the doc tests by `cargo test --doc`. The process
//! start is behind [`Runner`], so the tests use a fake.

use std::path::{Path, PathBuf};

/// One command to run: a program, its arguments and the variables added to the operator's environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateCmd {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(&'static str, &'static str)>,
}

/// What a run gave: whether the program started at all, whether it exited 0, and its standard output and error together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// False when the program could not be started (`cargo` not found, say): the gate fails, never passes.
    pub started: bool,
    pub ok: bool,
    pub output: String,
}

/// Starts a gate's process in the checkout. [`SystemRunner`] is the real one.
pub trait Runner {
    fn run(&mut self, repo: &Path, cmd: &GateCmd) -> Ran;
    /// Whether `cargo nextest` can be run here. When it can, the tests run as CI runs them (spec 125-A1): nextest's `ci`
    /// profile, which retries a failing test once itself. A runner that does not say so has none (the tests' fakes).
    fn has_nextest(&mut self, _repo: &Path) -> bool {
        false
    }
}

/// The operator's own process, in the checkout, its output read after it ends (and its start printed to standard error,
/// since `cargo test --workspace` takes minutes).
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&mut self, repo: &Path, cmd: &GateCmd) -> Ran {
        eprintln!("running: {}", shown(cmd));
        // Standard output and error go to one file, so the last lines keep their order.
        let log = std::env::temp_dir().join(format!("delphi-forge-gate-{}-{}.log", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
        let file = match std::fs::File::create(&log) {
            Ok(f) => f,
            Err(e) => return Ran { started: false, ok: false, output: format!("cannot create the gate log {}: {}", log.display(), e) },
        };
        let err_file = match file.try_clone() {
            Ok(f) => f,
            Err(e) => return Ran { started: false, ok: false, output: format!("cannot open the gate log {}: {}", log.display(), e) },
        };
        let status = std::process::Command::new(&cmd.program).args(&cmd.args).envs(cmd.env.iter().copied()).current_dir(repo).stdin(std::process::Stdio::null()).stdout(file).stderr(err_file).status();
        let output = String::from_utf8_lossy(&std::fs::read(&log).unwrap_or_default()).into_owned();
        let _ = std::fs::remove_file(&log);
        match status {
            Ok(s) => Ran { started: true, ok: s.success(), output },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ran { started: false, ok: false, output: format!("{}`{}` was not found (is it on PATH?), so this gate did not run: it FAILED, it did not pass", output, cmd.program.display()) },
            Err(e) => Ran { started: false, ok: false, output: format!("{}cannot start `{}`: {}; this gate did not run: it FAILED, it did not pass", output, cmd.program.display(), e) },
        }
    }

    /// Asked through [`Runner::run`], the one place this file starts a process.
    fn has_nextest(&mut self, repo: &Path) -> bool {
        let r = self.run(repo, &GateCmd { program: "cargo".into(), args: strs(&["nextest", "--version"]), env: Vec::new() });
        r.started && r.ok
    }
}

/// A gate as the operator would type it.
pub fn shown(cmd: &GateCmd) -> String {
    let mut s = String::new();
    for (k, v) in &cmd.env {
        s.push_str(&format!("{}={} ", k, v));
    }
    s.push_str(&cmd.program.display().to_string());
    for a in &cmd.args {
        s.push(' ');
        s.push_str(a);
    }
    s
}

const LEAN: [(&str, &str); 2] = [("CARGO_INCREMENTAL", "0"), ("CARGO_PROFILE_DEV_DEBUG", "0")];

fn strs(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// The three gates of `AGENTS.md`. `cargo test` takes `--no-fail-fast`, so a failing test binary does not hide the tests of
/// the next one (the rerun would otherwise pass with those never run). The lint runs this executable, not `cargo run`.
pub fn commands(exe: &Path, repo: &Path) -> [GateCmd; 3] {
    [
        GateCmd { program: "cargo".into(), args: strs(&["test", "--workspace", "--locked", "--no-fail-fast"]), env: LEAN.to_vec() },
        GateCmd { program: "cargo".into(), args: strs(&["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"]), env: Vec::new() },
        GateCmd { program: exe.to_path_buf(), args: vec!["lint".into(), "--repo".into(), repo.display().to_string()], env: Vec::new() },
    ]
}

/// The gates when nextest is installed (spec 125-A1 rule 5): the same tests as `cargo test --workspace`, split as CI splits
/// them. nextest's `ci` profile (`.config/nextest.toml`) runs every test but the doc tests and retries a failing one once
/// itself; the doc tests, which nextest does not run, follow with `cargo test --doc` and are not retried. Then Clippy and
/// the lint as in [`commands`].
pub fn nextest_commands(exe: &Path, repo: &Path) -> [GateCmd; 4] {
    let [_, clippy, lint] = commands(exe, repo);
    [
        GateCmd { program: "cargo".into(), args: strs(&["nextest", "run", "--workspace", "--locked", "--profile", "ci"]), env: LEAN.to_vec() },
        GateCmd { program: "cargo".into(), args: strs(&["test", "--workspace", "--locked", "--doc"]), env: LEAN.to_vec() },
        clippy,
        lint,
    ]
}

/// The tests nextest reports as flaky: its `FLAKY <try>/<tries> [ <time>] <binary> <test>` lines, as `<binary> <test>`.
pub fn nextest_flaky(output: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for l in output.lines() {
        let Some(rest) = l.trim_start().strip_prefix("FLAKY ") else { continue };
        let Some((_, name)) = rest.split_once("] ") else { continue };
        let name = name.trim();
        if !name.is_empty() && !names.iter().any(|x| x == name) {
            names.push(name.to_string());
        }
    }
    names
}

/// The rerun of `cargo test` for the named tests only. The names go to every test binary of the workspace; a binary that
/// has none of them runs nothing, so this builds once more and runs only those tests.
fn rerun(names: &[String]) -> GateCmd {
    let mut args = strs(&["test", "--workspace", "--locked", "--no-fail-fast", "--"]);
    args.extend(names.iter().cloned());
    args.push("--exact".into());
    GateCmd { program: "cargo".into(), args, env: LEAN.to_vec() }
}

/// The names of the tests libtest reported failed: the `test <name> ... FAILED` lines and each `failures:` list (the one of
/// bare indented names, not the one of `---- <name> stdout ----` sections), over every test binary of the output.
pub fn failed_tests(output: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut add = |n: &str| {
        let n = n.trim();
        if !n.is_empty() && !names.iter().any(|x| x == n) {
            names.push(n.to_string());
        }
    };
    let lines: Vec<&str> = output.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if let Some(n) = l.strip_prefix("test ").and_then(|r| r.strip_suffix(" ... FAILED")) {
            add(n);
        }
        if l.trim_end() == "failures:" && lines.get(i + 1).is_some_and(|n| n.starts_with("    ") && !n.trim().is_empty()) {
            i += 1;
            while i < lines.len() && lines[i].starts_with("    ") && !lines[i].trim().is_empty() {
                add(lines[i]);
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    names
}

/// The tests the output says passed (`test <name> ... ok`).
fn passed_tests(output: &str) -> Vec<String> {
    output.lines().filter_map(|l| l.strip_prefix("test ").and_then(|r| r.strip_suffix(" ... ok"))).map(|n| n.trim().to_string()).collect()
}

/// How a gate ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Exited 0, or every failing test passed on its rerun (those are named in `flaky`).
    Passed,
    /// Failed; `tail` is the last 40 lines of the failing run.
    Failed { tail: String },
    /// Not run, because an earlier gate failed.
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateResult {
    pub shown: String,
    pub state: State,
    pub flaky: Vec<String>,
}

const TAIL_LINES: usize = 40;

fn tail(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}

/// Runs the gates in order and stops at the first that fails; the rest are `NotRun`. With nextest installed the tests run
/// by [`nextest_commands`] and nextest does the one retry; without, by [`commands`] and a failing test is rerun here.
pub fn run(repo: &Path, exe: &Path, runner: &mut dyn Runner) -> Vec<GateResult> {
    let nextest = runner.has_nextest(repo);
    let cmds: Vec<GateCmd> = if nextest { nextest_commands(exe, repo).to_vec() } else { commands(exe, repo).to_vec() };
    let mut out = Vec::new();
    let mut stopped = false;
    for (i, cmd) in cmds.into_iter().enumerate() {
        let shown = shown(&cmd);
        if stopped {
            out.push(GateResult { shown, state: State::NotRun, flaky: Vec::new() });
            continue;
        }
        let first = runner.run(repo, &cmd);
        let (state, flaky) = if nextest && i == 0 {
            // nextest has already retried each failing test once: a pass is a pass (its flaky tests named), a failure is final.
            if first.started && first.ok { (State::Passed, nextest_flaky(&first.output)) } else { (State::Failed { tail: tail(&first.output) }, Vec::new()) }
        } else if first.started && first.ok {
            (State::Passed, Vec::new())
        } else if i == 0 && first.started {
            test_rerun(repo, runner, &first)
        } else {
            (State::Failed { tail: tail(&first.output) }, Vec::new())
        };
        stopped = matches!(state, State::Failed { .. });
        out.push(GateResult { shown, state, flaky });
    }
    out
}

/// A failing `cargo test`: rerun once for the failing tests; the gate passes only when each of them is seen passing.
fn test_rerun(repo: &Path, runner: &mut dyn Runner, first: &Ran) -> (State, Vec<String>) {
    let names = failed_tests(&first.output);
    if names.is_empty() {
        // A build error, a crashed binary: nothing to rerun by name.
        return (State::Failed { tail: tail(&first.output) }, Vec::new());
    }
    let second = runner.run(repo, &rerun(&names));
    if !second.started {
        return (State::Failed { tail: tail(&second.output) }, Vec::new());
    }
    let passed = passed_tests(&second.output);
    let again = failed_tests(&second.output);
    if second.ok && again.is_empty() && names.iter().all(|n| passed.contains(n)) {
        (State::Passed, names)
    } else {
        let mut t = format!("did not pass on the rerun: {}\n", names.iter().filter(|n| !passed.contains(n)).cloned().collect::<Vec<_>>().join(", "));
        t.push_str(&tail(&second.output));
        (State::Failed { tail: t }, Vec::new())
    }
}

/// The finisher's printed report of the gates.
pub fn report(results: &[GateResult]) -> String {
    let mut s = String::from("gates:\n");
    for (i, r) in results.iter().enumerate() {
        match &r.state {
            State::Passed if r.flaky.is_empty() => s.push_str(&format!("  {}/{} passed: {}\n", i + 1, results.len(), r.shown)),
            State::Passed => s.push_str(&format!("  {}/{} passed, flaky (failed, then passed on a rerun): {}: {}\n", i + 1, results.len(), r.flaky.join(", "), r.shown)),
            State::Failed { tail } => {
                s.push_str(&format!("  {}/{} FAILED: {}\n  last lines:\n", i + 1, results.len(), r.shown));
                for l in tail.lines() {
                    s.push_str(&format!("    {}\n", l));
                }
            }
            State::NotRun => s.push_str(&format!("  {}/{} not run (an earlier gate failed): {}\n", i + 1, results.len(), r.shown)),
        }
    }
    s
}

/// The `Checked` lines of the pull request body: ticked when passed, with the flaky tests named.
pub fn checked_lines(results: &[GateResult]) -> String {
    let mut s = String::new();
    for r in results {
        let line = match &r.state {
            State::Passed if r.flaky.is_empty() => format!("- [x] `{}`", r.shown),
            State::Passed => format!("- [x] `{}` (flaky, passed on a rerun: {})", r.shown, r.flaky.iter().map(|n| format!("`{}`", n)).collect::<Vec<_>>().join(", ")),
            State::Failed { .. } => format!("- [ ] `{}` (FAILED)", r.shown),
            State::NotRun => format!("- [ ] `{}` (not run: an earlier gate failed)", r.shown),
        };
        s.push_str(&line);
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Answers each run with the next scripted result and records what was run.
    struct Fake {
        answers: VecDeque<Ran>,
        ran: Vec<GateCmd>,
    }

    impl Runner for Fake {
        fn run(&mut self, _repo: &Path, cmd: &GateCmd) -> Ran {
            self.ran.push(cmd.clone());
            self.answers.pop_front().expect("an unexpected run")
        }
    }

    fn fake(a: Vec<(bool, &str)>) -> Fake {
        Fake { answers: a.into_iter().map(|(ok, o)| Ran { started: true, ok, output: o.to_string() }).collect(), ran: Vec::new() }
    }

    const TWO_BINARIES: &str = "     Running unittests src/lib.rs (target/debug/deps/a-1)\n\nrunning 2 tests\ntest m::one ... ok\ntest m::two ... FAILED\n\nfailures:\n\n---- m::two stdout ----\n    indented output that is not a name\n\nfailures:\n    m::two\n\ntest result: FAILED. 1 passed; 1 failed\n\n     Running tests/x.rs (target/debug/deps/x-2)\n\nrunning 1 test\ntest three ... FAILED\n\nfailures:\n    three\n\ntest result: FAILED. 0 passed; 1 failed\nerror: 2 targets failed\n";

    #[test]
    fn failing_tests_are_read_from_every_binary() {
        assert_eq!(failed_tests(TWO_BINARIES), vec!["m::two".to_string(), "three".to_string()]);
        assert!(failed_tests("error[E0308]: mismatched types\nerror: could not compile").is_empty());
    }

    #[test]
    fn ac15_a_test_that_fails_then_passes_is_flaky_and_the_gate_passes() {
        let mut f = fake(vec![(false, TWO_BINARIES), (true, "test m::two ... ok\ntest three ... ok\n"), (true, ""), (true, "")]);
        let r = run(Path::new("/r"), Path::new("/bin/delphi"), &mut f);
        assert_eq!(r[0].state, State::Passed);
        assert_eq!(r[0].flaky, vec!["m::two".to_string(), "three".to_string()]);
        assert!(r.iter().all(|g| g.state == State::Passed));
        assert_eq!(f.ran.len(), 4);
        assert_eq!(f.ran[1].args[f.ran[1].args.len() - 4..], strs(&["--", "m::two", "three", "--exact"]));
        assert_eq!(f.ran[1].env, LEAN.to_vec());
        assert_eq!(f.ran[3].program, PathBuf::from("/bin/delphi"));
        assert_eq!(f.ran[3].args, strs(&["lint", "--repo", "/r"]));
        assert!(report(&r).contains("flaky (failed, then passed on a rerun): m::two, three"));
        assert!(checked_lines(&r).starts_with("- [x] `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked --no-fail-fast` (flaky, passed on a rerun: `m::two`, `three`)"));
    }

    #[test]
    fn ac15_a_test_that_fails_twice_fails_the_gate_and_stops_the_rest() {
        let mut f = fake(vec![(false, TWO_BINARIES), (false, "test m::two ... ok\ntest three ... FAILED\nfailures:\n    three\n")]);
        let r = run(Path::new("/r"), Path::new("/bin/delphi"), &mut f);
        assert_eq!(f.ran.len(), 2, "clippy and the lint must not run");
        let State::Failed { tail } = &r[0].state else { panic!("{:?}", r[0]) };
        assert!(tail.starts_with("did not pass on the rerun: three\n"), "{}", tail);
        assert!(r[0].flaky.is_empty());
        assert_eq!((r[1].state.clone(), r[2].state.clone()), (State::NotRun, State::NotRun));
        assert!(checked_lines(&r).contains("- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings` (not run: an earlier gate failed)"));
    }

    #[test]
    fn ac15_the_first_failing_gate_stops_the_rest_and_shows_its_last_40_lines() {
        let long: String = (1..=100).map(|i| format!("line {}\n", i)).collect();
        let mut f = fake(vec![(true, ""), (false, &long)]);
        let r = run(Path::new("/r"), Path::new("/bin/delphi"), &mut f);
        assert_eq!(f.ran.len(), 2);
        let State::Failed { tail } = &r[1].state else { panic!("{:?}", r[1]) };
        assert!(tail.starts_with("line 61\n") && tail.ends_with("line 100"), "{}", tail);
        assert_eq!(r[2].state, State::NotRun);
        let p = report(&r);
        assert!(p.contains("2/3 FAILED: cargo clippy") && p.contains("    line 100\n") && !p.contains("line 60\n") && p.contains("3/3 not run"), "{}", p);
    }

    #[test]
    fn a_test_failure_with_no_names_is_not_rerun_and_a_rerun_that_runs_nothing_fails() {
        let mut f = fake(vec![(false, "error: could not compile `x`")]);
        let r = run(Path::new("/r"), Path::new("/bin/delphi"), &mut f);
        assert_eq!(f.ran.len(), 1);
        assert!(matches!(r[0].state, State::Failed { .. }));
        // The rerun exits 0 but never shows the test passing (no binary had it): not flaky, failed.
        let mut f = fake(vec![(false, TWO_BINARIES), (true, "test m::two ... ok\nrunning 0 tests\n")]);
        let r = run(Path::new("/r"), Path::new("/bin/delphi"), &mut f);
        let State::Failed { tail } = &r[0].state else { panic!("{:?}", r[0]) };
        assert!(tail.starts_with("did not pass on the rerun: three\n"), "{}", tail);
    }

    /// A [`Fake`] on a machine where nextest is installed.
    struct WithNextest(Fake);

    impl Runner for WithNextest {
        fn run(&mut self, repo: &Path, cmd: &GateCmd) -> Ran {
            self.0.run(repo, cmd)
        }
        fn has_nextest(&mut self, _repo: &Path) -> bool {
            true
        }
    }

    const NEXTEST_FLAKY: &str = "    Starting 3 tests across 2 binaries\n        PASS [   0.010s] delphi-engine kb::tests::a\n   TRY 1 FAIL [   0.200s] delphi-engine service::tests::cancelling\n     TRY 2 PASS [   0.190s] delphi-engine service::tests::cancelling\n       FLAKY 2/2 [   0.190s] delphi-engine service::tests::cancelling\n        PASS [   0.020s] delphi::e2e three\n     Summary [   0.400s] 3 tests run: 3 passed (1 flaky), 0 skipped\n   FLAKY 2/2 [   0.190s] delphi-engine service::tests::cancelling\n";

    #[test]
    fn spec_125_a1_with_nextest_the_tests_run_as_ci_runs_them_and_a_flaky_pass_is_named() {
        let mut f = WithNextest(fake(vec![(true, NEXTEST_FLAKY), (true, ""), (true, ""), (true, "")]));
        let r = run(Path::new("/r"), Path::new("/bin/delphi"), &mut f);
        assert_eq!(r.len(), 4);
        assert!(r.iter().all(|g| g.state == State::Passed), "{:?}", r);
        assert_eq!(f.0.ran[0].args, strs(&["nextest", "run", "--workspace", "--locked", "--profile", "ci"]));
        assert_eq!(f.0.ran[0].env, LEAN.to_vec());
        assert_eq!(f.0.ran[1].args, strs(&["test", "--workspace", "--locked", "--doc"]));
        assert_eq!(f.0.ran[3].args, strs(&["lint", "--repo", "/r"]));
        assert_eq!(r[0].flaky, vec!["delphi-engine service::tests::cancelling".to_string()], "named once, though nextest prints it twice");
        assert!(r[1].flaky.is_empty());
        assert!(checked_lines(&r).starts_with("- [x] `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo nextest run --workspace --locked --profile ci` (flaky, passed on a rerun: `delphi-engine service::tests::cancelling`)"), "{}", checked_lines(&r));
    }

    #[test]
    fn spec_125_a1_with_nextest_a_failure_is_final_and_is_not_rerun_again() {
        // nextest has retried already: no rerun by name here, and nothing after the failing gate runs.
        let mut f = WithNextest(fake(vec![(false, "   TRY 1 FAIL [ 0.2s] delphi-engine x::y\n   TRY 2 FAIL [ 0.2s] delphi-engine x::y\nerror: test run failed\n")]));
        let r = run(Path::new("/r"), Path::new("/bin/delphi"), &mut f);
        assert_eq!(f.0.ran.len(), 1);
        let State::Failed { tail } = &r[0].state else { panic!("{:?}", r[0]) };
        assert!(tail.contains("TRY 2 FAIL"), "{}", tail);
        assert!(r[1..].iter().all(|g| g.state == State::NotRun));
        // A run with no FLAKY line names none.
        assert!(nextest_flaky("        PASS [   0.010s] delphi-engine kb::tests::a\n").is_empty());
    }

    #[test]
    fn ac15_cargo_not_found_fails_the_gate_and_is_said_plainly() {
        // The fake says the program could not be started, even with ok set: never a pass, no rerun, nothing after it.
        let mut f = Fake { answers: VecDeque::from(vec![Ran { started: false, ok: true, output: "`cargo` was not found (is it on PATH?), so this gate did not run: it FAILED, it did not pass".into() }]), ran: Vec::new() };
        let r = run(Path::new("/r"), Path::new("/bin/delphi"), &mut f);
        assert_eq!(f.ran.len(), 1, "no rerun, no clippy, no lint");
        let State::Failed { tail } = &r[0].state else { panic!("{:?}", r[0]) };
        assert!(tail.contains("`cargo` was not found"), "{}", tail);
        assert_eq!((r[1].state.clone(), r[2].state.clone()), (State::NotRun, State::NotRun));
        assert!(report(&r).contains("1/3 FAILED"), "{}", report(&r));
        // The real runner reports a missing program as not started.
        let t = tempfile::tempdir().unwrap();
        let missing = GateCmd { program: t.path().join("no-such-cargo"), args: Vec::new(), env: Vec::new() };
        let ran = SystemRunner.run(t.path(), &missing);
        assert!(!ran.started && !ran.ok, "{:?}", ran);
        assert!(ran.output.contains("was not found") && ran.output.contains("FAILED"), "{}", ran.output);
    }
}
