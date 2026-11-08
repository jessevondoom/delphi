//! Spec 092 S2d1 tests: `run_authorized` end to end, in isolation. These run only on a real Mac (and on the GitHub macOS runner, which
//! has `sandbox-exec`); anywhere else they print why and skip. They do not touch `delphi run` or the engine: `run_authorized` is not
//! called from any run (the scan in `authorization/tests.rs` fails if it is).

use super::*;
use crate::authorization_anchor::{launch_granted, RunAnchor};
use delphi_sandbox::authorized::AuthorizedProbe;
use std::process::Command;
use std::sync::Mutex;

/// The sweep floods forks on purpose and the process limit counts every process of the user, so sweeps take turns.
static FLOOD: Mutex<()> = Mutex::new(());

const SWEEP: &str = include_str!("../../../delphi-sandbox/tests/fixtures/red_team_sweep.sh");
const DYLIB: &[u8] = include_bytes!("../../../delphi-sandbox/src/authorized/probe_mark.dylib");

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
    home: PathBuf,
    snap: Snapshot,
    fresh: FreshHome,
    scratch: PathBuf,
    listener: std::net::TcpListener,
    victim: Victim,
    argv: Vec<String>,
    id: String,
    script: String,
}

struct Victim(std::process::Child);

impl Drop for Victim {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// An operator home with one live record for `/bin/bash <script> <args>`, a snapshot holding the script, a no-exec scratch directory
/// with the sweep's fixtures, and a fresh throwaway home. `args` is built from the rig once its paths exist.
fn rig(script_text: &str, max_uses: u32, args: impl Fn(&Rig0) -> Vec<String>) -> Rig {
    let t = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(t.path()).unwrap();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("job.sh"), script_text).unwrap();
    fs::write(project.join("data.txt"), "snapshot data\n").unwrap();
    let snap = Snapshot::from_files(&project, &[PathBuf::from("job.sh"), PathBuf::from("data.txt")], &root).unwrap();
    let scratch = root.join("work");
    fs::create_dir(&scratch).unwrap();
    let pre = scratch.join("preplaced-exec");
    fs::write(&pre, "#!/bin/bash\nexit 0\n").unwrap();
    fs::set_permissions(&pre, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(scratch.join("mark.dylib"), DYLIB).unwrap();
    let fifo = std::ffi::CString::new(scratch.join("fifo").as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let canary = snap.root().join("canary.txt");
    fs::write(&canary, "CANARY\n").unwrap();
    let opshome = root.join("opshome");
    fs::create_dir(&opshome).unwrap();
    fs::write(opshome.join("visible.txt"), "x").unwrap();
    let tools = root.join("tools");
    fs::create_dir(&tools).unwrap();
    let fakedelphi = tools.join("delphi");
    fs::write(&fakedelphi, "#!/bin/bash\nexit 0\n").unwrap();
    fs::set_permissions(&fakedelphi, fs::Permissions::from_mode(0o755)).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let victim = Victim(Command::new("/bin/sleep").arg("120").spawn().unwrap());
    let fresh = FreshHome::create(&root).unwrap();
    let script = snap.path().join("job.sh").to_string_lossy().into_owned();

    let r0 = Rig0 { snap: snap.path().to_path_buf(), scratch: scratch.clone(), canary, opshome, fakedelphi, port: listener.local_addr().unwrap().port(), victim: victim.0.id() };
    let mut argv = vec!["/bin/bash".to_string(), script.clone()];
    argv.extend(args(&r0));

    // The operator's home, with a record made the way `delphi test authorize` makes it (real /bin/bash, sealed-volume check on).
    let home = root.join("opshome-store");
    fs::create_dir_all(home.join("operator")).unwrap();
    delphi_core::lock::write(&home.join("operator/ratified.lock"), &Default::default()).unwrap();
    let entry = a::Entry { tool: "shell".into(), argv: argv.clone(), files: vec![script.clone()], runner_provenance: "project_snapshot".into() };
    let n = a::NewRecord { purpose: "S2d1 acceptance".into(), intent: "red-team sweep".into(), run: "run-1".into(), expires_in_secs: a::DEFAULT_EXPIRY_SECS, max_uses, entries: vec![entry], request: None, policy: a::Policy::system() };
    let rec = a::build_record(&home, &n, now(), &delphi_core::time::now_rfc3339()).unwrap();
    let id = rec.id.clone();
    a::create_audited(&home, rec.clone(), json!({"id": rec.id, "manifest_hash": rec.manifest_hash, "run": rec.run, "by": "operator", "source": "operator"})).unwrap();
    Rig { _t: t, root, home, snap, fresh, scratch, listener, victim, argv, id, script }
}

/// The paths a rig makes, for building the arguments.
struct Rig0 {
    snap: PathBuf,
    scratch: PathBuf,
    canary: PathBuf,
    opshome: PathBuf,
    fakedelphi: PathBuf,
    port: u16,
    victim: u32,
}

fn sweep_rig(max_uses: u32) -> Rig {
    rig(SWEEP, max_uses, |r| vec![r.snap.display().to_string(), r.scratch.display().to_string(), r.canary.display().to_string(), r.opshome.display().to_string(), r.port.to_string(), r.fakedelphi.display().to_string(), r.victim.to_string()])
}

fn trivial_rig(max_uses: u32) -> Rig {
    rig("#!/bin/bash\necho job-ran\nexit 0\n", max_uses, |_| vec![])
}

type Events = std::sync::Arc<Mutex<Vec<(String, Value)>>>;

impl Rig {
    /// A grant made the way the daemon makes it: through the launcher's payload (anchor event, ledger state), in this process.
    fn launch(&self) -> (Grant, RunAnchor) {
        let (bytes, anchor) = launch_granted(&self.home, &self.id, "run-1", now(), &a::Policy::system()).unwrap();
        (Grant::from_boot(&bytes, std::process::id(), now(), a::Policy::system()).unwrap(), anchor)
    }

    fn area(&self) -> Area<'_> {
        Area { snapshot: &self.snap, home: &self.fresh, scratch: &self.scratch }
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
    assert!(!r.scratch.join("mark.dylib.loaded").exists(), "a native library was loaded from scratch");
    assert_eq!(unsafe { libc::kill(r.victim.0.id() as i32, 0) }, 0, "the victim process was signalled");
    assert!(!r.root.join("opshome/delphi-sweep-hole").exists());
    r.listener.set_nonblocking(true).unwrap();
    assert!(r.listener.accept().is_err(), "the sandboxed script reached the host's listener");
    // the private copy of the script is gone with the call: only the snapshot (and the canary this test put there) remain beside it
    let mut left: Vec<String> = fs::read_dir(r.snap.root()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, vec!["canary.txt".to_string(), "snapshot".to_string()], "a use-* directory was left behind");
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
    let (bytes, _anchor) = launch_granted(&r.home, &r.id, "run-1", now(), &a::Policy::system()).unwrap();
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
    let r = rig("#!/bin/bash\necho \"pwd=$(pwd) home=${HOME-unset} path=$PATH lc=$LC_ALL\"\n", 2, |_| vec![]);
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
