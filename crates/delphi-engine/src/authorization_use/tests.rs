//! Spec 092 S2d2b tests: the daemon check that approves a Red `run_command` under an operator authorization (AC-1, AC-3, AC-4), through
//! the engine and a stand-in `claude` Builder seat that calls Delphi's bridge `run_command`, exactly as a real bridge-only seat does.
//! They run only on a real Mac (and the GitHub macOS runner): the authorized profile is `sandbox-exec`, and Linux refuses it, so
//! elsewhere each prints `skip:` and returns.

use super::*;
use crate::authorization as a;
use crate::authorization_anchor::launch_granted_for;
use crate::authorization_grant::tests::claude_seat::{fx_with, script_n, PROMPT};
use crate::authorization_grant::{dry_count, ledger_count, Grant, NO_MATCH, REFUSED_USE, WOULD_USE};
use crate::authorization_run::{RunPlaces, USED, RAN};
use crate::exec::SandboxMode;
use crate::testkit::{events_of, Fx};
use delphi_sandbox::authorized::{AuthorizedProbe, FreshHome, Snapshot};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SWEEP: &str = include_str!("../../../delphi-sandbox/tests/fixtures/red_team_sweep.sh");
const TOOLS: &str = r#""mcp__delphi__run_command""#;
const DENIED: &str = "authorized approval is not enabled in this build";

fn now() -> i64 {
    delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap()
}

fn can_run() -> bool {
    if !cfg!(target_os = "macos") {
        eprintln!("skip: the authorized profile is macOS-only; Linux refuses it, so a granted command cannot run there");
        return false;
    }
    if delphi_sandbox::Sandbox::detect().is_err() {
        eprintln!("skip: no sandbox on this machine");
        return false;
    }
    true
}

fn git(project: &Path, args: &[&str]) {
    let out = std::process::Command::new("/usr/bin/git").arg("-C").arg(project).args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"]).args(args).env_clear().env("PATH", "/usr/bin:/bin").env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn failing_probe() -> AuthorizedProbe {
    AuthorizedProbe { unavailable: Some("forced off (test)".into()), ..AuthorizedProbe::default() }
}

/// An operator home with one live record for `/bin/bash @snapshot/job.sh <args>`, the run's snapshot and throwaway home, made the way
/// the launcher makes them. The grant and the places are built from them as the daemon builds them at boot.
struct Rig {
    _t: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    snap: Snapshot,
    fresh: FreshHome,
    id: String,
    argv: Vec<String>,
}

fn rig(script_text: &str, max_uses: u32, args: &[&str]) -> Rig {
    let t = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(t.path()).unwrap();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("job.sh"), script_text).unwrap();
    fs::write(project.join("data.txt"), "snapshot data\n").unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["add", "job.sh", "data.txt"]);
    git(&project, &["commit", "-q", "-m", "x"]);
    let mut argv = vec!["/bin/bash".to_string(), "@snapshot/job.sh".to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    let home = root.join("opshome");
    fs::create_dir_all(home.join("operator")).unwrap();
    delphi_core::lock::write(&home.join("operator/ratified.lock"), &Default::default()).unwrap();
    let entry = a::Entry { tool: "shell".into(), argv: argv.clone(), files: vec!["@snapshot/job.sh".into()], runner_provenance: "project_snapshot".into() };
    let n = a::NewRecord { purpose: "S2d2b".into(), intent: "test".into(), run: "run-1".into(), expires_in_secs: a::DEFAULT_EXPIRY_SECS, max_uses, entries: vec![entry], request: None, policy: a::Policy::system() };
    let ctx = a::TokenCtx { project: Some(project.clone()), pin_tree: true, expect: Default::default(), expect_tree: None };
    let rec = a::build_record_with(&home, &n, &ctx, now(), &delphi_core::time::now_rfc3339()).unwrap();
    let id = rec.id.clone();
    a::create_audited(&home, rec.clone(), serde_json::json!({"id": rec.id, "manifest_hash": rec.manifest_hash, "run": rec.run, "by": "operator", "source": "operator"})).unwrap();
    let snap = Snapshot::build(&project, &root).unwrap();
    let fresh = FreshHome::create(&root).unwrap();
    Rig { _t: t, root, home, snap, fresh, id, argv }
}

/// As [`rig`], with one manifest entry per set of arguments, all on the one pinned script.
fn rig_multi(script_text: &str, max_uses: u32, arg_sets: &[&[&str]]) -> Rig {
    let t = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(t.path()).unwrap();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("job.sh"), script_text).unwrap();
    fs::write(project.join("data.txt"), "snapshot data\n").unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["add", "job.sh", "data.txt"]);
    git(&project, &["commit", "-q", "-m", "x"]);
    let home = root.join("opshome");
    fs::create_dir_all(home.join("operator")).unwrap();
    delphi_core::lock::write(&home.join("operator/ratified.lock"), &Default::default()).unwrap();
    let mut entries = vec![];
    let mut first = vec![];
    for (i, args) in arg_sets.iter().enumerate() {
        let mut argv = vec!["/bin/bash".to_string(), "@snapshot/job.sh".to_string()];
        argv.extend(args.iter().map(|a| a.to_string()));
        if i == 0 {
            first = argv.clone();
        }
        entries.push(a::Entry { tool: "shell".into(), argv, files: vec!["@snapshot/job.sh".into()], runner_provenance: "project_snapshot".into() });
    }
    let n = a::NewRecord { purpose: "S2d2b".into(), intent: "test".into(), run: "run-1".into(), expires_in_secs: a::DEFAULT_EXPIRY_SECS, max_uses, entries, request: None, policy: a::Policy::system() };
    let ctx = a::TokenCtx { project: Some(project.clone()), pin_tree: true, expect: Default::default(), expect_tree: None };
    let rec = a::build_record_with(&home, &n, &ctx, now(), &delphi_core::time::now_rfc3339()).unwrap();
    let id = rec.id.clone();
    a::create_audited(&home, rec.clone(), serde_json::json!({"id": rec.id, "manifest_hash": rec.manifest_hash, "run": rec.run, "by": "operator", "source": "operator"})).unwrap();
    let snap = Snapshot::build(&project, &root).unwrap();
    let fresh = FreshHome::create(&root).unwrap();
    Rig { _t: t, root, home, snap, fresh, id, argv: first }
}

fn trivial(max_uses: u32) -> Rig {
    rig("#!/bin/bash\necho job-ran\nexit 0\n", max_uses, &[])
}

impl Rig {
    fn grant(&self) -> Grant {
        let (bytes, _anchor) = launch_granted_for(&self.home, &self.id, "run-1", now(), &a::Policy::system(), Some((self.snap.path(), self.snap.tree_hash()))).unwrap();
        Grant::from_boot(&bytes, std::process::id(), now(), a::Policy::system()).unwrap()
    }

    /// The engine of a granted run: the Builder seat's stand-in program, the grant and the places the daemon attaches at boot.
    fn engine(&self, prog: String, grant: Grant) -> Fx {
        let f = fx_with(prog, SandboxMode::Off, vec![]);
        f.engine.grant.set(Arc::new(grant)).unwrap();
        f.engine.places.set(Arc::new(RunPlaces::attach(self.snap.path(), self.snap.tree_hash(), self.fresh.path()).unwrap())).unwrap();
        f
    }

    fn real(&self) -> u32 {
        ledger_count(&self.home, &self.id).unwrap()
    }

    fn dry(&self) -> u32 {
        dry_count(&self.home, &self.id).unwrap()
    }
}

fn run_cmd(argv: &[String]) -> (&'static str, serde_json::Value) {
    ("run_command", serde_json::json!({"argv": argv}))
}

// ---------------------------------------------------------------------------------------------- AC-1: a matching card is approved

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_matching_card_is_approved_run_in_the_forced_sandbox_and_logged_as_used() {
    if !can_run() {
        return;
    }
    let r = trivial(3);
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    let f = r.engine(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&r.argv)]), r.grant());
    let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
    let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
    assert!(resp.contains("job-ran") && resp.contains("exit status 0"), "the model saw the command's output: {resp}");
    // exactly one authorization.used, approved, naming the record, with both vectors; one use in the REAL ledger, none in the dry one
    let used = events_of(&f, USED);
    assert_eq!(used.len(), 1);
    let p = &used[0].payload;
    assert_eq!((p["authorization_id"].as_str(), p["approved"].as_bool(), p["sandbox"].as_str(), p["uses_left"].as_u64()), (Some(r.id.as_str()), Some(true), Some("forced"), Some(2)));
    assert_eq!(p["argv"], serde_json::json!(r.argv));
    assert_eq!(p["resolved_argv"][1], r.snap.path().join("job.sh").display().to_string());
    assert_eq!(events_of(&f, RAN).len(), 1);
    assert_eq!((r.real(), r.dry()), (1, 0));
    // it never went through a card, the dry path or `exec.run`
    assert!(events_of(&f, "approval.requested").is_empty() && events_of(&f, "action.intent").is_empty() && events_of(&f, "action.result").is_empty());
    assert!(events_of(&f, WOULD_USE).is_empty());
    // a granted run gives the Builder no worktree and raises no spec-gate card: the snapshot is the project and nothing is written
    assert!(events_of(&f, "worktree.created").is_empty() && f.engine.exec.known_worktree("t_0001").is_none());
    r.snap.verify().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_the_flag_there_is_no_grant_and_the_same_card_is_not_approved_even_with_a_valid_record() {
    if !can_run() {
        return;
    }
    // The record is in the operator's home and valid; the daemon was not started with a grant (no `--on-card authorization:` launcher:
    // an interactive session, a plain `delphi run`, a daemon an agent started). Nothing reads the store, nothing is used.
    let r = trivial(3);
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    let f = fx_with(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&r.argv)]), SandboxMode::Off, vec![]);
    let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
    assert!(events_of(&f, USED).is_empty() && events_of(&f, RAN).is_empty());
    assert_eq!((r.real(), r.dry()), (0, 0));
    let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
    assert!(!resp.contains("job-ran"), "{resp}");
    assert!(f.engine.grant.get().is_none() && f.engine.places.get().is_none());
}

// ---------------------------------------------------------------------------------------------- AC-3: everything else stays denied

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_card_that_is_not_the_matching_run_command_stays_denied_and_nothing_runs() {
    if !can_run() {
        return;
    }
    let r0 = trivial(5);
    let mut near = r0.argv.clone();
    near.push("extra".into());
    let mut swapped = r0.argv.clone();
    swapped[0] = "/usr/bin/env".into();
    drop(r0);
    #[allow(clippy::type_complexity)]
    let cases: Vec<(&str, &str, serde_json::Value)> = vec![
        ("another program", "run_command", serde_json::json!({"argv": ["/usr/bin/touch", "RT_RAN"]})),
        ("a near miss (an extra argument)", "run_command", serde_json::json!({"argv": near})),
        ("a card holding the resolved path", "run_command", serde_json::json!({"argv": ["/bin/bash", "RESOLVED"]})),
        ("another interpreter, the same script", "run_command", serde_json::json!({"argv": swapped})),
        ("a network tool", "run_command", serde_json::json!({"argv": ["/usr/bin/curl", "http://127.0.0.1:9/RT_RAN"]})),
        ("a write into .git/hooks", "run_command", serde_json::json!({"argv": ["/usr/bin/touch", ".git/hooks/pre-commit"]})),
        ("a write into the configuration", "run_command", serde_json::json!({"argv": ["/usr/bin/touch", "config.toml"]})),
        ("a merge or a pull request by git", "run_command", serde_json::json!({"argv": ["/usr/bin/git", "push", "origin", "main"]})),
        ("gh", "run_command", serde_json::json!({"argv": ["/usr/bin/gh", "pr", "merge", "1"]})),
        ("a shell line", "shell", serde_json::json!({"command": "touch RT_RAN"})),
        ("the merge tool", "merge", serde_json::json!({})),
        ("a pull request tool", "pull_request", serde_json::json!({})),
        ("dispatch", "dispatch_agent", serde_json::json!({"role": "investigator", "task": "x"})),
        ("a file write tool", "write_file", serde_json::json!({"path": "specs/009-x.md", "content": "x"})),
        ("a connector tool", "connector", serde_json::json!({})),
        ("argv that is not strings", "run_command", serde_json::json!({"argv": [1, 2]})),
        ("no argv", "run_command", serde_json::json!({})),
    ];
    for (label, tool, arguments) in cases {
        let r = trivial(5);
        let arguments = if arguments.to_string().contains("RESOLVED") { serde_json::json!({"argv": ["/bin/bash", r.snap.path().join("job.sh").display().to_string()]}) } else { arguments };
        let scripts = tempfile::tempdir().unwrap();
        let args = scripts.path().join("args");
        let f = r.engine(script_n(scripts.path(), TOOLS, &args, vec![(tool, arguments)]), r.grant());
        let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
        let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
        assert!(!resp.contains("job-ran") && !resp.contains("exit status"), "{label}: it ran: {resp}");
        assert!(resp.contains("denied") || resp.contains("not one of the tools") || resp.contains("not available") || resp.contains("not offered") || resp.contains("unknown tool"), "{label}: not refused: {resp}");
        assert!(events_of(&f, USED).is_empty() && events_of(&f, RAN).is_empty(), "{label}");
        assert!(events_of(&f, "action.intent").is_empty() && events_of(&f, "approval.requested").is_empty(), "{label}: a card or exec.run");
        let found = std::process::Command::new("/usr/bin/find").arg(&r.root).args(["-name", "RT_RAN"]).output().unwrap();
        assert!(found.stdout.is_empty(), "{label}: the command ran");
        assert_eq!((r.real(), r.dry()), (0, 0), "{label}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_that_does_not_match_gets_the_one_fixed_denial_whatever_it_was() {
    if !can_run() {
        return;
    }
    let mut replies = vec![];
    for which in 0..3 {
        let r = trivial(5);
        let argv = match which {
            0 => vec!["/usr/bin/touch".to_string(), "RT_RAN".to_string()],
            1 => vec!["/bin/ls".to_string()],
            _ => {
                let mut v = r.argv.clone();
                v.push("x".into());
                v
            }
        };
        let scripts = tempfile::tempdir().unwrap();
        let args = scripts.path().join("args");
        let f = r.engine(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&argv)]), r.grant());
        let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
        let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
        assert!(resp.contains(DENIED), "{resp}");
        assert_eq!(events_of(&f, NO_MATCH).len(), 1, "the distinction is in the log only");
        let nonce = resp.split("nonce=\\\"").nth(1).and_then(|x| x.split('\\').next()).unwrap_or("").to_string();
        replies.push(resp.replace(&nonce, "N"));
    }
    assert!(replies.windows(2).all(|w| w[0] == w[1]), "a model could learn what the manifest holds: {replies:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_matching_card_is_refused_for_each_state_of_the_record_or_the_run_that_is_wrong() {
    if !can_run() {
        return;
    }
    // wrong hash: the pinned script was changed after the launch
    let r = trivial(3);
    let g = r.grant();
    let (dir, script) = (r.snap.path().to_path_buf(), r.snap.path().join("job.sh"));
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&script, "#!/bin/bash\necho swapped\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o444)).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    // the places cannot even attach to a tree that is not the one the launcher hashed
    assert!(RunPlaces::attach(r.snap.path(), "0".repeat(64).as_str(), r.fresh.path()).is_err());
    let f = fx_with(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&r.argv)]), SandboxMode::Off, vec![]);
    f.engine.grant.set(Arc::new(g)).unwrap();
    // attached to the tree as it now is (what a daemon that booted late would see): the use-time pin check still refuses
    f.engine.places.set(Arc::new(RunPlaces::attach(r.snap.path(), &delphi_sandbox::authorized::tree_hash_of(r.snap.path()).unwrap(), r.fresh.path()).unwrap())).unwrap();
    let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
    let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
    assert!(resp.contains(DENIED) && !resp.contains("swapped"), "wrong hash: {resp}");
    assert_eq!((events_of(&f, USED).len(), events_of(&f, REFUSED_USE).len(), r.real()), (0, 1, 0));

    // expired
    let r = trivial(3);
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    let f = r.engine(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&r.argv)]), r.grant());
    f.advance(a::DEFAULT_EXPIRY_SECS + 60);
    let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
    let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
    assert!(resp.contains(DENIED) && !resp.contains("job-ran"), "expired: {resp}");
    assert!(events_of(&f, REFUSED_USE)[0].payload["reason"].as_str().unwrap().contains("expired"));
    assert_eq!((events_of(&f, USED).len(), r.real()), (0, 0));

    // revoked mid-run
    let r = trivial(3);
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    let f = r.engine(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&r.argv)]), r.grant());
    a::revoke_audited(&r.home, &r.id, &delphi_core::time::now_rfc3339()).unwrap();
    let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
    let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
    assert!(resp.contains(DENIED) && !resp.contains("job-ran"), "revoked: {resp}");
    assert!(events_of(&f, REFUSED_USE)[0].payload["reason"].as_str().unwrap().contains("revoked"));
    assert_eq!((events_of(&f, USED).len(), r.real()), (0, 0));

    // the sandbox probe fails: refused, never run unsandboxed
    let r = trivial(3);
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    let f = r.engine(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&r.argv)]), r.grant().with_probe(failing_probe));
    let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
    let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
    assert!(resp.contains(DENIED) && !resp.contains("job-ran"), "probe: {resp}");
    assert!(events_of(&f, REFUSED_USE)[0].payload["reason"].as_str().unwrap().contains("probe"));
    assert_eq!((events_of(&f, USED).len(), events_of(&f, "action.intent").len(), r.real()), (0, 0, 0));

    // a daemon with a grant and no places runs nothing anywhere else
    let r = trivial(3);
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    let f = fx_with(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&r.argv)]), SandboxMode::Off, vec![]);
    f.engine.grant.set(Arc::new(r.grant())).unwrap();
    let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
    let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
    assert!(resp.contains(DENIED) && !resp.contains("job-ran"), "no places: {resp}");
    assert_eq!((events_of(&f, USED).len(), events_of(&f, "action.intent").len(), r.real()), (0, 0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_identical_card_in_a_one_use_record_is_denied() {
    if !can_run() {
        return;
    }
    let r = trivial(1);
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    let f = r.engine(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&r.argv), run_cmd(&r.argv)]), r.grant());
    let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
    let first = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
    let second = fs::read_to_string(args.with_extension("resp2")).unwrap_or_default();
    assert!(first.contains("job-ran"), "{first}");
    assert!(second.contains(DENIED) && !second.contains("job-ran"), "{second}");
    assert_eq!((events_of(&f, USED).len(), r.real()), (1, 1));
    assert!(events_of(&f, REFUSED_USE)[0].payload["reason"].as_str().unwrap().contains("no uses left"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_concurrent_seats_on_a_one_use_record_run_exactly_one() {
    if !can_run() {
        return;
    }
    let r = trivial(1);
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    let f = r.engine(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&r.argv)]), r.grant());
    let (a1, a2) = tokio::join!(f.engine.run_task(&f.m, "builder", PROMPT), f.engine.run_task(&f.m, "builder", PROMPT));
    let _ = (a1, a2);
    assert_eq!(events_of(&f, USED).len(), 1, "{:?}", events_of(&f, REFUSED_USE).iter().map(|e| e.payload.to_string()).collect::<Vec<_>>());
    assert_eq!(r.real(), 1);
}

// ---------------------------------------------------------------------------------------------- the taint rule

#[test]
fn a_span_at_federated_peer_taint_or_lower_is_never_approved_and_every_other_refusal_is_in_force() {
    use delphi_core::provenance::Tier;
    for (tier, tainted) in [(Tier::L0, true), (Tier::L1, true), (Tier::L2, false), (Tier::L3, false), (Tier::L4, false), (Tier::L5, false)] {
        assert_eq!(pre_refusal(true, tier, true).is_some(), tainted, "{tier:?}");
        // a tainted span is refused whatever else is in order
        assert!(pre_refusal(true, tier, false).is_some());
    }
    assert!(pre_refusal(false, Tier::L5, true).unwrap().contains("manifest"), "a role that may not run commands");
    assert!(pre_refusal(true, Tier::L5, false).unwrap().contains("no snapshot"), "no places, no command");
    assert_eq!(pre_refusal(true, Tier::L3, true), None);
}

// ---------------------------------------------------------------------------------------------- the acceptance run, by the seat

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_red_team_sweep_runs_through_the_seat_with_all_22_attempts_blocked_and_one_use() {
    if !can_run() {
        return;
    }
    let r = rig(SWEEP, 3, &["@snapshot", "@scratch", "@canary", "@operator_home"]);
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    let tree = r.snap.tree_hash().to_string();
    let f = r.engine(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&r.argv)]), r.grant());
    let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
    let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
    assert!(resp.contains("SWEEP PASSED: 22 attempts, every one blocked"), "{resp}");
    assert!(!resp.contains("SUCCEEDED") && resp.contains("exit status 0"), "{resp}");
    assert!(resp.matches("blocked ").count() >= 22, "22 blocked lines");
    let used = events_of(&f, USED);
    assert_eq!(used.len(), 1);
    assert_eq!(used[0].payload["tree_hash"], serde_json::json!(tree));
    assert_eq!(used[0].payload["argv"], serde_json::json!(r.argv));
    assert_eq!((r.real(), r.dry()), (1, 0), "the real ledger holds one use and the dry ledger none");
    r.snap.verify().unwrap();
    // the home and the snapshot are discarded with their owners
    let (home, root) = (r.fresh.path().to_path_buf(), r.snap.root().to_path_buf());
    drop(f);
    let Rig { fresh, snap, .. } = r;
    drop(fresh);
    drop(snap);
    assert!(!home.exists() && !root.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_weakened_profile_fails_the_sweep_through_the_seat() {
    if !can_run() {
        return;
    }
    let r = rig(SWEEP, 3, &["@snapshot", "@scratch", "@canary", "@operator_home"]);
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    let f = r.engine(script_n(scripts.path(), TOOLS, &args, vec![run_cmd(&r.argv)]), r.grant().with_weakened_profile(|spec| spec.scratch_noexec = false));
    let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
    let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
    assert!(resp.contains("SWEEP FAILED") && resp.contains("SUCCEEDED exec_preplaced_file_in_scratch") && !resp.contains("SWEEP PASSED"), "{resp}");
    assert!(!resp.contains("exit status 0"), "the run reports failure: {resp}");
    assert_eq!((events_of(&f, USED).len(), r.real()), (1, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_seat_is_not_offered_a_file_write_tool_and_is_denied_one_it_calls_in_a_granted_run() {
    // TA-5, GOVERNANCE §5.12 ("a run that offers no file-write tool"): checked here for the Builder, the one role a run carries a command for.
    if !can_run() {
        return;
    }
    let r = trivial(3);
    let scripts = tempfile::tempdir().unwrap();
    let args = scripts.path().join("args");
    let f = r.engine(script_n(scripts.path(), TOOLS, &args, vec![("write_file", serde_json::json!({"path": "x.md", "content": "x"})), run_cmd(&r.argv)]), r.grant());
    let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
    let first = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
    assert!(first.contains("not one of the tools") || first.contains("not available in a run that holds an operator authorization"), "{first}");
    assert!(!first.contains("exit status"), "{first}");
    let started = &events_of(&f, "harness.started")[0].payload;
    let offered: Vec<String> = started["bridge"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(String::from)).collect();
    assert!(offered.iter().all(|n| crate::authorization_grant::tool_allowed(n)), "{offered:?}");
    assert!(offered.iter().any(|n| n == "run_command") && !offered.iter().any(|n| n.contains("write") || n == "shell" || n == "note"), "{offered:?}");
}

// ---------------------------------------------------------------------------------------------- red-team of #608: the guards at the call site

// The taint rule at its call site: a span whose ceiling is `federated_peer` or lower is refused even with a valid grant and a matching card.
// (`pre_refusal` is tested over every tier above; this fails if the engine stops passing the span's own ceiling to it.) A harness seat never
// gets this far with a tainted span (its launch is refused), so the call is made by an API-served Builder.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_span_tainted_to_l0_or_l1_calls_the_matching_command_under_a_grant_and_is_denied_while_a_trusted_one_runs() {
    use crate::model::Step;
    use crate::testkit::setup;
    use delphi_core::provenance::Tier;
    if !can_run() {
        return;
    }
    for (tier, runs) in [(Tier::L0, false), (Tier::L1, false), (Tier::L3, true)] {
        let r = trivial(3);
        let f = setup(vec![Step::tool("c1", "run_command", serde_json::json!({"argv": r.argv})), Step::text("done")], 5.0, |_| {});
        f.engine.grant.set(Arc::new(r.grant())).unwrap();
        f.engine.places.set(Arc::new(RunPlaces::attach(r.snap.path(), r.snap.tree_hash(), r.fresh.path()).unwrap())).unwrap();
        let task = f.m.next_task_id();
        let _ = f.engine.run_task_woken(&f.m, task, "builder", "Run the pinned job.", tier).await;
        let results: Vec<String> = events_of(&f, "tool.result").iter().map(|e| e.payload["content"].to_string()).collect();
        if runs {
            assert!(results.iter().any(|x| x.contains("job-ran")), "{tier:?}: a span above federated_peer is approved: {results:?}");
            assert_eq!((events_of(&f, USED).len(), r.real()), (1, 1));
        } else {
            assert!(!results.iter().any(|x| x.contains("job-ran")), "{tier:?}: {results:?}");
            let why = events_of(&f, REFUSED_USE);
            assert!(why.iter().any(|e| e.payload["reason"].as_str().unwrap_or("").contains("tainted")), "{tier:?}: {:?} {results:?}", why.iter().map(|e| e.payload.to_string()).collect::<Vec<_>>());
            assert_eq!((events_of(&f, USED).len(), r.real()), (0, 0), "{tier:?}: no use burned");
        }
    }
}

// The role guard: a role whose manifest does not run commands is refused even if it calls `run_command` with a grant, places and the
// matching card (a model is not trusted to call only what it was offered).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_role_that_may_not_run_commands_calls_run_command_under_a_grant_and_is_denied() {
    use crate::model::Step;
    use crate::testkit::setup;
    if !can_run() {
        return;
    }
    let r = trivial(3);
    let f = setup(vec![Step::tool("c1", "run_command", serde_json::json!({"argv": r.argv})), Step::text("done")], 5.0, |_| {});
    f.engine.grant.set(Arc::new(r.grant())).unwrap();
    f.engine.places.set(Arc::new(RunPlaces::attach(r.snap.path(), r.snap.tree_hash(), r.fresh.path()).unwrap())).unwrap();
    let _ = f.engine.run_task(&f.m, "architect", "Draft a spec").await;
    let results: Vec<String> = events_of(&f, "tool.result").iter().map(|e| e.payload["content"].to_string()).collect();
    assert!(!results.iter().any(|x| x.contains("job-ran")), "the architect ran a command: {results:?}");
    assert_eq!((events_of(&f, USED).len(), events_of(&f, RAN).len(), r.real()), (0, 0, 0));
}

// The one-at-a-time lock: the scratch directory is shared, so two seats that hold two DIFFERENT matching cards of a multi-use record run one
// after the other. The script logs start and end into scratch; interleaved runs would show start-A start-B.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_seats_with_two_different_matching_cards_run_one_at_a_time_in_the_shared_scratch() {
    if !can_run() {
        return;
    }
    let script = "#!/bin/bash\nlog=\"$1/order.log\"\necho \"start-$2\" >> \"$log\"\nt=$SECONDS; while [ $((SECONDS-t)) -lt 2 ]; do :; done\necho \"end-$2\" >> \"$log\"\nwhile IFS= read -r l; do echo \"$l\"; done < \"$log\"\n";
    let r = rig_multi(script, 5, &[&["@scratch", "A"], &["@scratch", "B"]]);
    let mk = |tag: &str| {
        let mut v = r.argv.clone();
        v[3] = tag.to_string();
        v
    };
    let (dir, mut progs) = (tempfile::tempdir().unwrap(), vec![]);
    for tag in ["A", "B"] {
        let d = dir.path().join(tag);
        fs::create_dir_all(&d).unwrap();
        let args = d.join("args");
        progs.push((script_n(&d, TOOLS, &args, vec![run_cmd(&mk(tag))]), args));
    }
    // one program that behaves as seat A for the first launch and as seat B for the second (the version probe is answered by A)
    let wrapper = dir.path().join("wrapper.sh");
    fs::write(&wrapper, format!("#!/bin/sh\ncase \"${{1-}}\" in --version) exec '{a}' \"$@\";; esac\nif mkdir '{lock}' 2>/dev/null; then exec '{a}' \"$@\"; else exec '{b}' \"$@\"; fi\n", a = progs[0].0, b = progs[1].0, lock = dir.path().join("first.lock").display())).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let f = r.engine(wrapper.display().to_string(), r.grant());
    let (x, y) = tokio::join!(f.engine.run_task(&f.m, "builder", PROMPT), f.engine.run_task(&f.m, "builder", PROMPT));
    let _ = (x, y);
    assert_eq!((events_of(&f, USED).len(), r.real()), (2, 2), "both cards ran: {:?}", events_of(&f, REFUSED_USE).iter().map(|e| e.payload.to_string()).collect::<Vec<_>>());
    let outs: Vec<String> = progs.iter().map(|(_, a)| fs::read_to_string(a.with_extension("resp")).unwrap_or_default()).collect();
    let longest = outs.iter().max_by_key(|o| o.len()).unwrap();
    // the later run printed the whole log: its two lines come after the earlier run's two, never between them
    let log: Vec<&str> = longest.split("\\n").filter(|l| l.starts_with("start-") || l.starts_with("end-")).collect();
    assert_eq!(log.len(), 4, "{longest}");
    assert!(log[0].starts_with("start-") && log[1] == log[0].replace("start-", "end-") && log[2].starts_with("start-") && log[3] == log[2].replace("start-", "end-"), "interleaved: {log:?}");
}
