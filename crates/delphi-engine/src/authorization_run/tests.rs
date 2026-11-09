//! Spec 092 S2d1 tests: `run_authorized` end to end, in isolation. These run only on a real Mac (and on the GitHub macOS runner, which
//! has `sandbox-exec`); anywhere else they print why and skip. They do not touch `delphi run` or the engine: `run_authorized` is not
//! called from any run (the scan in `authorization/tests.rs` fails if it is).

use super::*;
use std::path::Path;
use crate::authorization_anchor::{launch_granted_for, RunAnchor};
use delphi_sandbox::authorized::AuthorizedProbe;
use std::process::Command;
use std::sync::Mutex;

/// The sweep floods forks on purpose and the process limit counts every process of the user, so sweeps take turns.
static FLOOD: Mutex<()> = Mutex::new(());

const SWEEP: &str = include_str!("../../../delphi-sandbox/tests/fixtures/red_team_sweep.sh");

fn now() -> i64 {
    delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap()
}

fn can_run() -> bool {
    if !cfg!(target_os = "macos") {
        eprintln!("skip: the authorized profile is macOS-only; Linux refuses it, so run_authorized cannot run there");
        return false;
    }
    match Sandbox::detect() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("skip: no sandbox on this machine ({e})");
            false
        }
    }
}

struct Rig {
    _t: tempfile::TempDir,
    root: PathBuf,
    project: PathBuf,
    home: PathBuf,
    snap: Snapshot,
    fresh: FreshHome,
    minted: Minted,
    /// The SYMBOLIC vector the record holds and the card carries.
    argv: Vec<String>,
    id: String,
    /// The real path of the pinned script in this run's snapshot.
    script: String,
    policy: a::Policy,
}

fn git(project: &Path, args: &[&str]) {
    let out = Command::new("/usr/bin/git").arg("-C").arg(project).args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"]).args(args).env_clear().env("PATH", "/usr/bin:/bin").env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// An operator home with one live record for `/bin/bash @snapshot/job.sh <args>` made the way `delphi test authorize` makes it (the
/// project is a git repository with the script committed; the tree is pinned), a snapshot the real builder made from that project,
/// the places the daemon mints after the record, and a fresh throwaway home. `args` are SYMBOLIC (tokens), as the record holds them.
fn rig(script_text: &str, max_uses: u32, args: &[&str]) -> Rig {
    rig_with(script_text, max_uses, args, false)
}

/// As [`rig`]; with `own_bash` the interpreter is a copy of `/bin/bash` in the rig's own directory (a policy that names it), so a test
/// can change the interpreter's bytes. The probe still uses the real `/bin/bash`; nothing is run from the copy in those tests.
fn rig_with(script_text: &str, max_uses: u32, args: &[&str], own_bash: bool) -> Rig {
    let t = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(t.path()).unwrap();
    let (bin, policy) = if own_bash {
        let dir = root.join("interp");
        fs::create_dir(&dir).unwrap();
        let copy = dir.join("bash");
        fs::copy("/bin/bash", &copy).unwrap();
        fs::set_permissions(&copy, fs::Permissions::from_mode(0o755)).unwrap();
        let b = copy.to_string_lossy().into_owned();
        (b.clone(), a::Policy { interpreters: vec![b], sealed_roots: vec![] })
    } else {
        ("/bin/bash".to_string(), a::Policy::system())
    };
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("job.sh"), script_text).unwrap();
    fs::write(project.join("data.txt"), "snapshot data\n").unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["add", "job.sh", "data.txt"]);
    git(&project, &["commit", "-q", "-m", "x"]);
    // The record comes FIRST and holds only symbolic text: no per-run path exists yet.
    let mut argv = vec![bin.clone(), "@snapshot/job.sh".to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    let home = root.join("opshome-store");
    fs::create_dir_all(home.join("operator")).unwrap();
    delphi_core::lock::write(&home.join("operator/ratified.lock"), &Default::default()).unwrap();
    let entry = a::Entry { tool: "shell".into(), argv: argv.clone(), files: vec!["@snapshot/job.sh".into()], runner_provenance: "project_snapshot".into() };
    let n = a::NewRecord { purpose: "S2d2a acceptance".into(), intent: "red-team sweep".into(), run: "run-1".into(), expires_in_secs: a::DEFAULT_EXPIRY_SECS, max_uses, entries: vec![entry], request: None, policy: policy.clone() };
    let ctx = a::TokenCtx { project: Some(project.clone()), pin_tree: true, expect: Default::default() };
    let rec = a::build_record_with(&home, &n, &ctx, now(), &delphi_core::time::now_rfc3339()).unwrap();
    let id = rec.id.clone();
    a::create_audited(&home, rec.clone(), json!({"id": rec.id, "manifest_hash": rec.manifest_hash, "run": rec.run, "by": "operator", "source": "operator"})).unwrap();
    // The run: its snapshot and the places the daemon mints, made AFTER the record.
    let snap = Snapshot::build(&project, &root).unwrap();
    let minted = Minted::mint(&snap).unwrap();
    let fresh = FreshHome::create(&root).unwrap();
    let script = snap.path().join("job.sh").to_string_lossy().into_owned();
    Rig { _t: t, root, project, home, snap, fresh, minted, argv, id, script, policy }
}

fn sweep_rig(max_uses: u32) -> Rig {
    rig(SWEEP, max_uses, &["@snapshot", "@scratch", "@canary", "@operator_home"])
}

fn trivial_rig(max_uses: u32) -> Rig {
    rig("#!/bin/bash\necho job-ran\nexit 0\n", max_uses, &[])
}

type Events = std::sync::Arc<Mutex<Vec<(String, Value)>>>;

impl Rig {
    /// A grant made the way the daemon makes it: through the launcher's payload (anchor event, ledger state), in this process.
    fn launch(&self) -> (Grant, RunAnchor) {
        let (bytes, anchor) = launch_granted_for(&self.home, &self.id, "run-1", now(), &self.policy, Some((self.snap.path(), self.snap.tree_hash()))).unwrap();
        (Grant::from_boot(&bytes, std::process::id(), now(), self.policy.clone()).unwrap(), anchor)
    }

    fn area(&self) -> Area<'_> {
        Area { snapshot: &self.snap, home: &self.fresh, minted: &self.minted }
    }

    fn call(&self, g: &Grant) -> (Result<AuthorizedRun, Refusal>, Events) {
        let ev: Events = Default::default();
        let sink = ev.clone();
        let r = g.run_authorized(&self.argv, &self.area(), now(), &mut |k, v| {
            sink.lock().unwrap().push((k.to_string(), v));
            Ok(())
        });
        (r, ev)
    }

    /// As [`Rig::call`], and `between` runs when `authorization.used` has been logged: after the use is taken, before the exec.
    fn call_hooked(&self, g: &Grant, mut between: impl FnMut(&Rig)) -> (Result<AuthorizedRun, Refusal>, Events) {
        let ev: Events = Default::default();
        let sink = ev.clone();
        let r = g.run_authorized(&self.argv, &self.area(), now(), &mut |k, v| {
            sink.lock().unwrap().push((k.to_string(), v));
            if k == USED {
                between(self);
            }
            Ok(())
        });
        (r, ev)
    }

    fn real(&self) -> u32 {
        g::ledger_count(&self.home, &self.id).unwrap()
    }

    fn dry(&self) -> u32 {
        g::dry_count(&self.home, &self.id).unwrap()
    }
}

fn count(ev: &Events, kind: &str) -> usize {
    ev.lock().unwrap().iter().filter(|(k, _)| k == kind).count()
}

fn failing_probe() -> AuthorizedProbe {
    AuthorizedProbe { unavailable: Some("forced off (test)".into()), ..AuthorizedProbe::default() }
}

// ---------------------------------------------------------------------------------------------- the acceptance run

#[test]
fn the_red_team_sweep_runs_authorized_and_every_attempt_is_blocked() {
    if !can_run() {
        return;
    }
    let _turn = FLOOD.lock().unwrap_or_else(|e| e.into_inner());
    let r = sweep_rig(3);
    let (g, anchor) = r.launch();
    let tree = r.snap.tree_hash().to_string();
    let (res, ev) = r.call(&g);
    let res = res.unwrap_or_else(|e| panic!("refused: {e:?}"));
    // the sweep: exits 0 only when all 22 attempts were blocked
    assert_eq!(res.exit_code, Some(0), "{}\n{}", res.stdout, res.stderr);
    assert!(res.success && !res.timed_out && !res.output_capped && !res.over_quota);
    assert!(res.stdout.contains("SWEEP PASSED: 22 attempts, every one blocked"), "{}", res.stdout);
    assert_eq!(res.stdout.lines().filter(|l| l.starts_with("blocked ")).count(), 22, "{}", res.stdout);
    assert!(!res.stdout.contains("SUCCEEDED"), "{}", res.stdout);
    // exactly one use, in the REAL ledger; the dry ledger untouched
    assert_eq!(count(&ev, USED), 1);
    assert_eq!(count(&ev, RAN), 1);
    assert_eq!((res.index, res.uses_left), (1, 2));
    assert_eq!((r.real(), r.dry()), (1, 0));
    let used = ev.lock().unwrap().iter().find(|(k, _)| k == USED).unwrap().1.clone();
    assert_eq!(used["tree_hash"], json!(tree));
    assert_eq!(used["authorization_id"], json!(r.id));
    assert_eq!(used["sandbox"], json!("forced"));
    assert!(used["cwd_env"].as_str().unwrap().contains("record"), "{used}");
    assert_eq!(used["uses_left"], json!(2));
    // the snapshot is what it was; nothing the script tried left a trace (checked on disk, not from the script's report)
    assert!(res.snapshot_unchanged);
    r.snap.verify().unwrap();
    assert!(!r.snap.root().join("sweep-outside").exists() && !r.snap.root().join("sweep-new-file").exists());
    assert!(!r.minted.scratch.join("mark.dylib.loaded").exists(), "a native library was loaded from scratch");
    assert!(!r.minted.operator_home.join("delphi-sweep-hole").exists());
    // the private copy of the script is gone with the call: only the snapshot and what the daemon minted remain beside it
    let left: Vec<String> = fs::read_dir(r.snap.root()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(!left.iter().any(|n| n.starts_with("use-")), "a use-* directory was left behind: {left:?}");
    // the used event shows the symbolic vector AND the resolved one
    assert_eq!(used["argv"], json!(r.argv));
    let resolved: Vec<String> = serde_json::from_value(used["resolved_argv"].clone()).unwrap();
    assert_eq!(resolved[1], r.script);
    assert_eq!(resolved[2..], [r.snap.path().display().to_string(), r.minted.scratch.display().to_string(), r.minted.canary.display().to_string(), r.minted.operator_home.display().to_string()]);
    // the anchor: ending the run records the real ledger; the next launch finds it where it was left
    let end = anchor.end().unwrap();
    assert_eq!(end.count, 1);
    drop(r.launch());
    // the throwaway home and the snapshot are discarded with their owners
    let (home_path, snap_root) = (r.fresh.path().to_path_buf(), r.snap.root().to_path_buf());
    let Rig { fresh, snap, .. } = r;
    drop(fresh);
    drop(snap);
    assert!(!home_path.exists(), "the throwaway home was not discarded");
    assert!(!snap_root.exists(), "the snapshot was not discarded");
}

// ---------------------------------------------------------------------------------------------- negative controls

#[test]
fn a_weakened_profile_makes_the_same_call_report_the_sweep_failed_and_name_the_attempt() {
    if !can_run() {
        return;
    }
    let _turn = FLOOD.lock().unwrap_or_else(|e| e.into_inner());
    let r = sweep_rig(3);
    let (g, _anchor) = r.launch();
    let g = g.with_weakened_profile(|spec| spec.scratch_noexec = false);
    let (res, ev) = r.call(&g);
    let res = res.unwrap_or_else(|e| panic!("refused: {e:?}"));
    assert_eq!(res.exit_code, Some(1), "{}", res.stdout);
    assert!(!res.success);
    assert!(res.stdout.contains("SUCCEEDED exec_preplaced_file_in_scratch") && res.stdout.contains("SWEEP FAILED"), "{}", res.stdout);
    assert!(!res.stdout.contains("SWEEP PASSED"));
    // it still took exactly one use and said so
    assert_eq!((count(&ev, USED), r.real()), (1, 1));
}

#[test]
fn a_tampered_script_is_refused_and_takes_no_use() {
    if !can_run() {
        return;
    }
    let r = trivial_rig(3);
    let (g, _anchor) = r.launch();
    // The owner changes the pinned script after authorization (a swapped file is exactly what the hash is for).
    let dir = r.snap.path().to_path_buf();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&r.script, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&r.script, "#!/bin/bash\necho swapped\n").unwrap();
    fs::set_permissions(&r.script, fs::Permissions::from_mode(0o444)).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let (res, ev) = r.call(&g);
    let err = res.unwrap_err();
    assert!(!err.use_taken && err.reason.contains("hash changed"), "{err:?}");
    assert_eq!((r.real(), count(&ev, USED)), (0, 0));
}

#[test]
fn a_revoked_record_is_refused_and_takes_no_use() {
    if !can_run() {
        return;
    }
    let r = trivial_rig(3);
    let (g, _anchor) = r.launch();
    a::revoke_audited(&r.home, &r.id, &delphi_core::time::now_rfc3339()).unwrap();
    let (res, ev) = r.call(&g);
    let err = res.unwrap_err();
    assert!(!err.use_taken && err.reason.contains("revoked"), "{err:?}");
    assert_eq!((r.real(), count(&ev, USED), count(&ev, RAN)), (0, 0, 0));
}

#[test]
fn a_sandbox_that_is_off_refuses_before_the_use_and_a_probe_that_fails_at_the_second_look_refuses_after_it() {
    if !can_run() {
        return;
    }
    let r = trivial_rig(3);
    // forced off from the start: probe before the decrement fails, nothing is taken, nothing runs, no unsandboxed fallback
    let (g, _anchor) = r.launch();
    let g = g.with_probe(failing_probe);
    let (res, ev) = r.call(&g);
    let err = res.unwrap_err();
    assert!(!err.use_taken && err.reason.contains("failed its probe"), "{err:?}");
    assert_eq!((r.real(), count(&ev, USED), count(&ev, RAN)), (0, 0, 0));
    // off at the SECOND look (immediately before exec): the use had been taken and stays consumed, the command does not run
    let (g, _anchor2) = r.launch();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let c2 = calls.clone();
    let g = g.with_probe(move || if c2.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 { delphi_sandbox::authorized::probe_authorized() } else { failing_probe() });
    let (res, ev) = r.call(&g);
    let err = res.unwrap_err();
    assert!(err.use_taken && err.reason.contains("failed its probe"), "{err:?}");
    assert_eq!((r.real(), count(&ev, USED), count(&ev, RAN)), (1, 1, 0), "consumed, logged, never run");
}

#[test]
fn two_concurrent_calls_on_a_one_use_record_run_exactly_one() {
    if !can_run() {
        return;
    }
    let _turn = FLOOD.lock().unwrap_or_else(|e| e.into_inner());
    let r = std::sync::Arc::new(trivial_rig(1));
    let (bytes, _anchor) = launch_granted_for(&r.home, &r.id, "run-1", now(), &a::Policy::system(), Some((r.snap.path(), r.snap.tree_hash()))).unwrap();
    // Two daemons' worth of state on one record: two grants, each with the launcher's anchor.
    let grants: Vec<_> = (0..2).map(|_| std::sync::Arc::new(Grant::from_boot(&bytes, std::process::id(), now(), a::Policy::system()).unwrap())).collect();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = grants
        .into_iter()
        .map(|g| {
            let (r, b) = (r.clone(), barrier.clone());
            std::thread::spawn(move || {
                b.wait();
                let (res, ev) = r.call(&g);
                (res.map(|x| x.stdout), count(&ev, USED))
            })
        })
        .collect();
    let outs: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let ran = outs.iter().filter(|(res, _)| res.is_ok()).count();
    assert_eq!(ran, 1, "{outs:?}");
    assert_eq!(outs.iter().map(|(_, used)| *used).sum::<usize>(), 1, "{outs:?}");
    assert_eq!(r.real(), 1);
    let winner = outs.iter().find_map(|(res, _)| res.as_ref().ok()).unwrap();
    assert!(winner.contains("job-ran"), "{winner}");
}

#[test]
fn the_calls_own_working_directory_and_environment_are_never_used() {
    if !can_run() {
        return;
    }
    // A record whose script prints what it was given: the record's constants, whatever the call might have carried.
    let r = rig("#!/bin/bash\necho \"pwd=$(pwd) home=${HOME-unset} path=$PATH lc=$LC_ALL\"\n", 2, &[]);
    let (g, _anchor) = r.launch();
    let (res, _ev) = r.call(&g);
    let res = res.unwrap_or_else(|e| panic!("{e:?}"));
    assert!(res.stdout.contains(&format!("pwd={}", r.snap.path().display())), "{}", res.stdout);
    assert!(res.stdout.contains("home=unset") && res.stdout.contains("path=/nonexistent") && res.stdout.contains("lc=C"), "{}", res.stdout);
}

#[test]
fn a_ledger_cut_mid_run_refuses_the_next_use_and_nothing_runs() {
    if !can_run() {
        return;
    }
    let r = trivial_rig(5);
    let (g, _anchor) = r.launch();
    let (first, _) = r.call(&g);
    assert!(first.is_ok());
    // a same-user process deletes the whole ledger between two uses
    fs::remove_dir_all(g::ledger_dir(&r.home, &r.id)).unwrap();
    let (second, ev) = r.call(&g);
    let err = second.unwrap_err();
    assert!(!err.use_taken && err.reason.contains("not where this run last saw it"), "{err:?}");
    assert_eq!(count(&ev, USED), 0);
}

#[test]
fn a_command_that_is_not_the_manifests_is_not_run() {
    if !can_run() {
        return;
    }
    let r = trivial_rig(3);
    let (g, _anchor) = r.launch();
    let mut argv = r.argv.clone();
    argv.push("extra".into());
    let ev: Events = Default::default();
    let err = g.run_authorized(&argv, &r.area(), now(), &mut |_, _| Ok(())).unwrap_err();
    assert!(!err.use_taken && err.reason.contains("no manifest entry"), "{err:?}");
    assert_eq!(r.real(), 0);
    drop(ev);
}

#[test]
fn the_private_copy_is_hashed_and_a_log_failure_after_the_use_stays_consumed() {
    if !can_run() {
        return;
    }
    let r = trivial_rig(3);
    let (g, _anchor) = r.launch();
    let res = g.run_authorized(&r.argv, &r.area(), now(), &mut |_, _| Err("disk full".into()));
    let err = res.unwrap_err();
    assert!(err.use_taken && err.reason.contains("could not be logged"), "{err:?}");
    assert_eq!(r.real(), 1);
}

#[test]
fn a_symlink_planted_in_the_throwaway_home_or_a_changed_snapshot_refuses_the_use() {
    if !can_run() {
        return;
    }
    let r = trivial_rig(3);
    let (g, _anchor) = r.launch();
    let evil = r.fresh.path().join("evil");
    std::os::unix::fs::symlink("/etc/hosts", &evil).unwrap();
    let err = r.call(&g).0.unwrap_err();
    assert!(!err.use_taken && err.reason.contains("walk") && err.reason.contains("symlink"), "{err:?}");
    fs::remove_file(&evil).unwrap();
    fs::write(r.fresh.path().join("auth.json"), "{}").unwrap();
    let err = r.call(&g).0.unwrap_err();
    assert!(!err.use_taken && err.reason.contains("credential"), "{err:?}");
    fs::remove_file(r.fresh.path().join("auth.json")).unwrap();
    // a snapshot that is not what was built
    let dir = r.snap.path().to_path_buf();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(dir.join("planted"), "x").unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let err = r.call(&g).0.unwrap_err();
    assert!(!err.use_taken && err.reason.contains("snapshot changed"), "{err:?}");
    assert_eq!(r.real(), 0);
}

#[test]
fn the_runner_never_reaches_exec_run_and_never_runs_without_the_sandbox() {
    // The source of the runner names no unsandboxed path: no `exec.run`, no plain `Command::new` of the interpreter, and it builds its
    // command only from `Sandbox::command`.
    let src = include_str!("../authorization_run.rs");
    let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    assert!(!code.contains("exec.run") && !code.contains("exec::") && !code.contains("plain_command"));
    assert!(!code.contains("Command::new"), "the runner starts nothing but through the sandbox");
    assert!(code.contains("sb.command(&spec,") && code.contains("run_bounded("));
}

// ---------------------------------------------------------------------------------------------- tampering after the use is logged

#[test]
fn a_private_copy_changed_between_the_log_and_the_exec_is_not_run() {
    if !can_run() {
        return;
    }
    let r = trivial_rig(3);
    let (g, _anchor) = r.launch();
    let (res, ev) = r.call_hooked(&g, |r| {
        let dir = fs::read_dir(r.snap.root()).unwrap().flatten().map(|e| e.path()).find(|p| p.file_name().unwrap().to_string_lossy().starts_with("use-")).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let file = dir.join("job.sh");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&file, "#!/bin/bash\necho swapped\n").unwrap();
    });
    let err = res.unwrap_err();
    assert!(err.use_taken && err.reason.contains("private copy"), "{err:?}");
    assert_eq!((count(&ev, USED), count(&ev, RAN), r.real()), (1, 0, 1), "consumed, logged, never run");
}

#[test]
fn an_interpreter_changed_between_the_log_and_the_exec_is_not_run() {
    if !can_run() {
        return;
    }
    let r = rig_with("#!/bin/bash\necho job-ran\n", 3, &[], true);
    let (g, _anchor) = r.launch();
    let bin = r.argv[0].clone();
    let (res, ev) = r.call_hooked(&g, |_| {
        use std::io::Write as _;
        let mut f = fs::OpenOptions::new().append(true).open(&bin).unwrap();
        f.write_all(b"\0tampered").unwrap();
    });
    let err = res.unwrap_err();
    assert!(err.use_taken && err.reason.contains("interpreter"), "{err:?}");
    assert_eq!((count(&ev, USED), count(&ev, RAN)), (1, 0));
}

#[test]
fn the_places_changed_between_the_log_and_the_exec_stop_the_command() {
    if !can_run() {
        return;
    }
    let r = trivial_rig(5);
    let (g, _anchor) = r.launch();
    // a symlink planted in the throwaway home
    let evil = r.fresh.path().join("evil");
    let (res, ev) = r.call_hooked(&g, |r| std::os::unix::fs::symlink("/etc/hosts", r.fresh.path().join("evil")).unwrap());
    let err = res.unwrap_err();
    assert!(err.use_taken && err.reason.contains("walk"), "{err:?}");
    assert_eq!(count(&ev, RAN), 0);
    fs::remove_file(&evil).unwrap();
    // the snapshot written to
    let (res, ev) = r.call_hooked(&g, |r| {
        let dir = r.snap.path().to_path_buf();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(dir.join("planted"), "x").unwrap();
    });
    let err = res.unwrap_err();
    assert!(err.use_taken && err.reason.contains("snapshot"), "{err:?}");
    assert_eq!(count(&ev, RAN), 0);
}

// ---------------------------------------------------------------------------------------------- S2d2a: symbolic paths

fn used_event(ev: &Events) -> Value {
    ev.lock().unwrap().iter().find(|(k, _)| k == USED).unwrap().1.clone()
}

#[test]
fn two_runs_of_one_record_get_different_paths_and_the_same_manifest_hash() {
    if !can_run() {
        return;
    }
    let r = trivial_rig(5);
    let (g1, _a1) = r.launch();
    let (res1, ev1) = r.call(&g1);
    assert!(res1.is_ok(), "{res1:?}");
    // a second run: a new snapshot of the same project, new minted places, a new launch
    let snap2 = Snapshot::build(&r.project, &r.root).unwrap();
    let minted2 = Minted::mint(&snap2).unwrap();
    assert_ne!(snap2.path(), r.snap.path());
    assert_eq!(snap2.tree_hash(), r.snap.tree_hash(), "the same tracked files make the same tree");
    let (bytes, _anchor2) = launch_granted_for(&r.home, &r.id, "run-1", now(), &r.policy, Some((snap2.path(), snap2.tree_hash()))).unwrap();
    let g2 = Grant::from_boot(&bytes, std::process::id(), now(), r.policy.clone()).unwrap();
    let ev2: Events = Default::default();
    let sink = ev2.clone();
    let res2 = g2.run_authorized(&r.argv, &Area { snapshot: &snap2, home: &r.fresh, minted: &minted2 }, now(), &mut |k, v| {
        sink.lock().unwrap().push((k.to_string(), v));
        Ok(())
    });
    assert!(res2.is_ok(), "{res2:?}");
    let (u1, u2) = (used_event(&ev1), used_event(&ev2));
    assert_eq!(u1["argv"], u2["argv"], "the symbolic vector is the same text");
    assert_ne!(u1["resolved_argv"], u2["resolved_argv"], "the paths are minted per run");
    assert_eq!(u1["record_hash"], u2["record_hash"]);
    let rec = a::load(&r.home).unwrap().records.into_iter().find(|x| x.id == r.id).unwrap();
    assert_eq!(rec.token_scheme, 1);
    assert_eq!(rec.snapshot.as_ref().map(|s| s.source.as_str()), Some("tracked_files"));
    assert_eq!(rec.snapshot.and_then(|s| s.tree_blake3), Some(r.snap.tree_hash().to_string()));
}

#[test]
fn a_card_that_is_not_exactly_the_symbolic_text_matches_nothing_and_takes_no_use() {
    if !can_run() {
        return;
    }
    let _turn = FLOOD.lock().unwrap_or_else(|e| e.into_inner());
    let r = sweep_rig(5);
    let (g, _anchor) = r.launch();
    let abs = |p: &Path| p.display().to_string();
    let good = r.argv.clone();
    let with = |i: usize, v: &str| {
        let mut a = good.clone();
        a[i] = v.to_string();
        a
    };
    let cards: Vec<(&str, Vec<String>)> = vec![
        ("wrong token", with(3, "@snapshot")),
        ("swapped tokens", {
            let mut a = good.clone();
            a.swap(3, 4);
            a
        }),
        ("parent directory", with(1, "@snapshot/../x")),
        ("double slash", with(1, "@snapshot//job.sh")),
        ("dot slash", with(1, "@snapshot/./job.sh")),
        ("token inside text", with(2, "x@snapshot")),
        ("unknown token", with(3, "@unknown")),
        ("token in argv[0]", with(0, "@snapshot")),
        ("wrong case", with(2, "@Snapshot")),
        ("trailing space", with(2, "@snapshot ")),
        ("resolved absolute snapshot", with(2, &abs(r.snap.path()))),
        ("resolved absolute script", with(1, &r.script)),
        ("resolved scratch", with(3, &abs(&r.minted.scratch))),
        ("resolved canary", with(4, &abs(&r.minted.canary))),
        ("the real operator home as a card", with(5, &abs(&r.home))),
    ];
    for (name, argv) in cards {
        let err = g.run_authorized(&argv, &r.area(), now(), &mut |_, _| Ok(())).unwrap_err();
        assert!(!err.use_taken && err.reason.contains("no manifest entry"), "{name}: {err:?}");
        assert_eq!(r.real(), 0, "{name} took a use");
    }
    // and none of them ran anything: no use directory was made
    assert!(!fs::read_dir(r.snap.root()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("use-")));
    // the exact text still runs
    let ok = g.run_authorized(&good, &r.area(), now(), &mut |_, _| Ok(()));
    assert!(ok.is_ok(), "{ok:?}");
}

#[test]
fn a_changed_tree_or_a_tampered_pinned_file_is_refused_before_any_use() {
    if !can_run() {
        return;
    }
    // the project changes after the record was made: the launcher compares the snapshot with the pins first
    let r = trivial_rig(3);
    fs::write(r.project.join("data.txt"), "changed after the operator authorized\n").unwrap();
    git(&r.project, &["add", "data.txt"]);
    git(&r.project, &["commit", "-q", "-m", "change"]);
    let snap = Snapshot::build(&r.project, &r.root).unwrap();
    let before = a::load(&r.home).unwrap().audit_head.map(|h| h.seq);
    let err = launch_granted_for(&r.home, &r.id, "run-1", now(), &r.policy, Some((snap.path(), snap.tree_hash()))).err().unwrap();
    assert!(err.contains("tree"), "{err}");
    assert_eq!(a::load(&r.home).unwrap().audit_head.map(|h| h.seq), before, "nothing was written: no launch event");
    assert_eq!(r.real(), 0);
    // the pinned SCRIPT changes: refused at launch, still before any use
    let r = trivial_rig(3);
    fs::write(r.project.join("job.sh"), "#!/bin/bash\necho tampered\n").unwrap();
    git(&r.project, &["add", "job.sh"]);
    git(&r.project, &["commit", "-q", "-m", "tamper"]);
    let snap = Snapshot::build(&r.project, &r.root).unwrap();
    let err = launch_granted_for(&r.home, &r.id, "run-1", now(), &r.policy, Some((snap.path(), snap.tree_hash()))).err().unwrap();
    assert!(err.contains("tree") || err.contains("pinned file"), "{err}");
    assert_eq!(r.real(), 0);
}

#[test]
fn a_record_without_a_tree_pin_still_checks_each_pinned_file_by_hash() {
    let r = trivial_rig(3);
    // The --no-pin-tree record: an UNPINNED file may differ (that is the opt-out); the pinned script may not.
    let mut rec = a::load(&r.home).unwrap().records.into_iter().find(|x| x.id == r.id).unwrap();
    rec.snapshot.as_mut().unwrap().tree_blake3 = None;
    fs::write(r.project.join("data.txt"), "changed unpinned file\n").unwrap();
    git(&r.project, &["add", "data.txt"]);
    git(&r.project, &["commit", "-q", "-m", "change"]);
    let snap = Snapshot::build(&r.project, &r.root).unwrap();
    assert!(a::check_snapshot_pins(&rec, snap.path(), snap.tree_hash()).is_ok(), "no tree pin: an unpinned file may differ");
    fs::write(r.project.join("job.sh"), "#!/bin/bash\necho tampered\n").unwrap();
    git(&r.project, &["add", "job.sh"]);
    git(&r.project, &["commit", "-q", "-m", "tamper"]);
    let snap = Snapshot::build(&r.project, &r.root).unwrap();
    assert!(a::check_snapshot_pins(&rec, snap.path(), snap.tree_hash()).unwrap_err().contains("pinned file"));
}

#[test]
fn authorize_refuses_a_pinned_file_that_is_untracked_modified_a_symlink_or_missing() {
    let t = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(t.path()).unwrap();
    let project = root.join("p");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("job.sh"), "#!/bin/bash\nexit 0\n").unwrap();
    fs::write(project.join("real.sh"), "#!/bin/bash\nexit 0\n").unwrap();
    std::os::unix::fs::symlink("real.sh", project.join("link.sh")).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["add", "job.sh", "real.sh", "link.sh"]);
    git(&project, &["commit", "-q", "-m", "x"]);
    fs::write(project.join("untracked.sh"), "x").unwrap();
    let entry = |rel: &str| a::Entry { tool: "shell".into(), argv: vec!["/bin/bash".into(), format!("@snapshot/{rel}")], files: vec![format!("@snapshot/{rel}")], runner_provenance: "project_snapshot".into() };
    assert!(a::preview_pins(&project, &[entry("job.sh")]).is_ok());
    assert!(a::preview_pins(&project, &[entry("untracked.sh")]).unwrap_err().contains("untracked"));
    assert!(a::preview_pins(&project, &[entry("link.sh")]).unwrap_err().contains("not a tracked file"), "a symlink entry never resolves");
    assert!(a::preview_pins(&project, &[entry("missing.sh")]).is_err());
    fs::write(project.join("job.sh"), "#!/bin/bash\nexit 1\n").unwrap();
    assert!(a::preview_pins(&project, &[entry("job.sh")]).unwrap_err().contains("modified"));
}
