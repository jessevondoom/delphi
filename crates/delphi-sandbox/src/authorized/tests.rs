//! Spec 092 S2b tests. The macOS tests need `sandbox-exec` and run on a real Mac (and on any macOS CI runner that has it); on a
//! machine with no sandbox they print why and skip. The Linux tests (bubblewrap) assert that the profile is REFUSED there.

use super::*;
use crate::{bwrap_args, sbpl, sbpl_str, Resolved};
use std::process::Command;
use std::sync::Mutex;

/// `probe_authorized()` reads a process-wide environment hook, so the tests that call it take turns.
static PROBE: Mutex<()> = Mutex::new(());

const SWEEP: &str = include_str!("../../tests/fixtures/red_team_sweep.sh");

fn canon(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap()
}

/// The sandbox, when this machine can run the authorized profile; otherwise the reason, printed.
fn mac_sandbox() -> Option<Sandbox> {
    if !cfg!(target_os = "macos") {
        eprintln!("skip: the authorized profile is macOS-only; Linux refuses it (see linux_refuses_the_authorized_profile)");
        return None;
    }
    match Sandbox::detect() {
        Ok(sb) => Some(sb),
        Err(e) => {
            eprintln!("skip: no sandbox on this machine ({e})");
            None
        }
    }
}

// ---------------------------------------------------------------------------------------------- the profile

struct Layout {
    _t: tempfile::TempDir,
    snap: Snapshot,
    scratch: PathBuf,
    canary: PathBuf,
    opshome: PathBuf,
    fakedelphi: PathBuf,
    port: u16,
    listener: std::net::TcpListener,
}

fn layout() -> Layout {
    let t = tempfile::tempdir().unwrap();
    let root = canon(t.path());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("sweep.sh"), SWEEP).unwrap();
    fs::write(project.join("data.txt"), "snapshot data\n").unwrap();
    let snap = Snapshot::from_files(&project, &[PathBuf::from("sweep.sh"), PathBuf::from("data.txt")], &root).unwrap();
    let scratch = root.join("scratch");
    fs::create_dir(&scratch).unwrap();
    let pre = scratch.join("preplaced-exec");
    fs::write(&pre, "#!/bin/bash\nexit 0\n").unwrap();
    fs::set_permissions(&pre, fs::Permissions::from_mode(0o755)).unwrap();
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
    let port = listener.local_addr().unwrap().port();
    Layout { _t: t, snap, scratch, canary, opshome, fakedelphi, port, listener }
}

impl Layout {
    fn spec(&self) -> SandboxSpec {
        authorized_spec(self.snap.path(), &self.scratch, INTERPRETER)
    }

    /// Run the sweep under `spec`; returns (exit code, stdout).
    fn sweep(&self, sb: &Sandbox, spec: &SandboxSpec) -> (Option<i32>, String) {
        let args: Vec<std::ffi::OsString> = vec![
            self.snap.path().join("sweep.sh").into(),
            self.snap.path().into(),
            self.scratch.clone().into(),
            self.canary.clone().into(),
            self.opshome.clone().into(),
            self.port.to_string().into(),
            self.fakedelphi.clone().into(),
        ];
        let mut cmd = sb.command(spec, INTERPRETER, args).unwrap();
        cmd.current_dir(self.snap.path());
        let out = run_bounded(cmd, &RunOptions { timeout: Duration::from_secs(60), max_output_bytes: 1 << 20, quota: vec![] }).unwrap();
        assert!(!out.timed_out, "{out:?}");
        (out.status.and_then(|s| s.code()), out.stdout)
    }
}

#[test]
fn the_sweep_passes_only_when_every_attempt_is_blocked() {
    let Some(sb) = mac_sandbox() else { return };
    let l = layout();
    let (code, out) = l.sweep(&sb, &l.spec());
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("SWEEP PASSED: 15 attempts, every one blocked"), "{out}");
    assert_eq!(out.lines().filter(|x| x.starts_with("blocked ")).count(), 15, "{out}");
    assert!(!out.contains("SUCCEEDED"), "{out}");
    // Nothing happened on disk either, and the listener never saw a connection.
    l.snap.verify().unwrap();
    assert!(!l.snap.root().join("sweep-outside").exists());
    assert!(!l.opshome.join("delphi-sweep-hole").exists());
    assert!(l.listener.set_nonblocking(true).is_ok());
    assert!(l.listener.accept().is_err(), "the sandboxed script reached the host's listener");
}

/// The sweep must FAIL, naming the attempt, when the profile has a hole; otherwise passing proves nothing.
fn weakened(name: &str, attempt: &str, weaken: impl FnOnce(&Layout, &mut SandboxSpec)) {
    let Some(sb) = mac_sandbox() else { return };
    let l = layout();
    let mut spec = l.spec();
    weaken(&l, &mut spec);
    let (code, out) = l.sweep(&sb, &spec);
    assert_eq!(code, Some(1), "{name}: the sweep did not fail on a weakened profile: {out}");
    assert!(out.contains(&format!("SUCCEEDED {attempt}")), "{name}: {attempt} not named: {out}");
    assert!(out.contains("SWEEP FAILED"), "{name}: {out}");
    assert!(!out.contains("SWEEP PASSED"), "{name}: {out}");
}

#[test]
fn the_sweep_fails_when_the_snapshot_is_writable() {
    weakened("snapshot writable", "write_new_file_in_snapshot", |l, spec| {
        // The directory modes are loosened too, so only the profile stands between the script and the write.
        let dir = l.snap.path();
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(dir.join("data.txt"), fs::Permissions::from_mode(0o644)).unwrap();
        spec.scratch.push(dir.to_path_buf());
    });
}

#[test]
fn the_sweep_fails_when_the_network_is_on() {
    weakened("network on", "network_connect", |l, spec| spec.network = Network::ProxyOnly { port: l.port });
}

#[test]
fn the_sweep_fails_when_scratch_can_execute() {
    weakened("scratch exec", "exec_preplaced_file_in_scratch", |_, spec| spec.scratch_noexec = false);
}

#[test]
fn the_sweep_fails_when_the_environment_carries_a_token() {
    weakened("env token", "read_env_for_credentials", |_, spec| {
        spec.env.insert("ANTHROPIC_API_KEY".into(), "sk-x".into());
    });
}

#[test]
fn the_sweep_fails_when_the_operators_home_is_readable() {
    weakened("home readable", "list_operator_home", |l, spec| spec.read_only.push(l.opshome.clone()));
}

#[test]
fn the_sweep_fails_when_files_outside_the_grant_are_readable() {
    weakened("outside readable", "read_canary_outside_grant", |l, spec| spec.read_only.push(l.snap.root().to_path_buf()));
}

#[test]
fn the_sweep_fails_when_a_nested_delphi_can_run() {
    weakened("nested delphi", "spawn_nested_delphi", |l, spec| {
        spec.read_only.push(l.fakedelphi.parent().unwrap().to_path_buf());
        spec.exec_only.as_mut().unwrap().push(l.fakedelphi.clone());
    });
}

#[test]
fn the_sweep_refuses_to_pass_when_it_cannot_prove_anything() {
    // A positive control: with the snapshot unreadable the script exits 2 rather than reporting a vacuous pass.
    let Some(sb) = mac_sandbox() else { return };
    let l = layout();
    let args: Vec<std::ffi::OsString> = vec![l.snap.path().join("sweep.sh").into(), l.scratch.clone().into(), l.scratch.clone().into(), l.canary.clone().into(), l.opshome.clone().into(), "9".into(), l.fakedelphi.clone().into()];
    // `data.txt` is looked for under the first argument, here the scratch dir, where it does not exist
    let mut cmd = sb.command(&l.spec(), INTERPRETER, [&args[0], &args[1], &args[2], &args[3], &args[4], &args[5], &args[6]]).unwrap();
    cmd.current_dir(l.snap.path());
    let out = run_bounded(cmd, &RunOptions::default()).unwrap();
    assert_eq!(out.status.and_then(|s| s.code()), Some(2), "{out:?}");
    assert!(out.stdout.contains("SWEEP INCONCLUSIVE"), "{out:?}");
}

// ---------------------------------------------------------------------------------------------- exec rules

fn run_in(sb: &Sandbox, spec: &SandboxSpec, program: &str, args: &[&str]) -> Option<i32> {
    let mut cmd = sb.command(spec, program, args).unwrap();
    let out = cmd.output().unwrap();
    out.status.code()
}

#[test]
fn only_the_named_interpreter_can_be_executed_and_it_cannot_execute_anything_else() {
    let Some(sb) = mac_sandbox() else { return };
    let l = layout();
    let mut spec = l.spec();
    let script = l.snap.path().join("sweep.sh");
    let _ = script;
    // bash is allowed and runs a builtin
    assert_eq!(run_in(&sb, &spec, "/bin/bash", &["-c", "exit 7"]), Some(7));
    // but it cannot start another program, even a system one that is readable
    assert_ne!(run_in(&sb, &spec, "/bin/bash", &["-c", "/usr/bin/true"]), Some(0));
    assert_ne!(run_in(&sb, &spec, "/bin/bash", &["-c", "/bin/echo hi"]), Some(0));
    // zsh is not the named interpreter
    assert_ne!(run_in(&sb, &spec, "/bin/zsh", &["-c", "exit 0"]), Some(0));
    // /bin/sh is not exec-allowed when bash is the interpreter ...
    assert_ne!(run_in(&sb, &spec, "/bin/sh", &["-c", "exit 0"]), Some(0));
    // ... and with /bin/sh as the interpreter, bash must be allowed too (sh re-executes it)
    spec.exec_only = Some(exec_literals("/bin/sh"));
    assert_eq!(run_in(&sb, &spec, "/bin/sh", &["-c", "exit 0"]), Some(0));
    // allowing only /bin/sh, without bash, does not start
    spec.exec_only = Some(vec![PathBuf::from("/bin/sh")]);
    assert_ne!(run_in(&sb, &spec, "/bin/sh", &["-c", "exit 0"]), Some(0));
}

#[test]
fn the_spec_validation_refuses_what_would_widen_the_profile() {
    let l = layout();
    // an exec allowlist next to a writable worktree (which is executable) is refused
    let mut spec = l.spec();
    spec.worktree = Some(l.scratch.clone());
    assert!(matches!(Resolved::new(&spec), Err(SandboxError::Unsupported(_))));
    // an exec entry that is not a regular file is refused
    let mut spec = l.spec();
    spec.exec_only = Some(vec![PathBuf::from("/bin")]);
    assert!(matches!(Resolved::new(&spec), Err(SandboxError::BadPath(_))));
    spec.exec_only = Some(vec![]);
    assert!(Resolved::new(&spec).is_err());
    // an ordinary spec is unchanged: the new fields default off
    let d = SandboxSpec::new();
    assert!(d.snapshot_ro.is_none() && !d.scratch_noexec && d.exec_only.is_none());
}

#[test]
fn the_profile_grants_exec_by_literal_and_never_on_scratch_or_the_snapshot() {
    let l = layout();
    let p = sbpl(&Resolved::new(&l.spec()).unwrap());
    assert!(p.contains("(allow process-exec (literal \"/bin/bash\"))"), "{p}");
    assert!(!p.contains("process-exec* (subpath"), "no subpath may be executable: {p}");
    let scratch = sbpl_str(&canon(&l.scratch));
    let snap = sbpl_str(&canon(l.snap.path()));
    assert!(p.contains(&format!("(allow file-read* file-write* (subpath {scratch}))")));
    assert!(!p.contains(&format!("process-exec* (subpath {scratch}")) && !p.contains(&format!("(process-exec (subpath {scratch}")));
    assert!(p.contains(&format!("(allow file-read* (subpath {snap}))")) && !p.contains(&format!("file-write* (subpath {snap}")));
    // network: nothing
    assert!(!p.contains("network"), "{p}");
    // dropping the no-exec flag is visible in the profile (so the tests above are not vacuous)
    let mut spec = l.spec();
    spec.scratch_noexec = false;
    assert!(sbpl(&Resolved::new(&spec).unwrap()).contains(&format!("process-exec* (subpath {scratch})")));
}

#[test]
fn linux_refuses_the_authorized_profile_rather_than_run_it_loosely() {
    let l = layout();
    let r = Resolved::new(&l.spec()).unwrap();
    // The bubblewrap builder is pure, so this runs on every platform: it cannot make a scratch directory no-exec or limit exec
    // to one file, so it says so and the caller refuses to start.
    let e = bwrap_args(&r).unwrap_err();
    assert!(matches!(&e, SandboxError::Unsupported(m) if m.contains("no-exec scratch")), "{e}");
    let mut spec = l.spec();
    spec.scratch_noexec = false;
    let e = bwrap_args(&Resolved::new(&spec).unwrap()).unwrap_err();
    assert!(matches!(&e, SandboxError::Unsupported(m) if m.contains("exec allowlist")), "{e}");
    // without the authorized fields the Linux invocation is unchanged and mounts the snapshot read-only
    let mut plain = SandboxSpec::new();
    plain.snapshot_ro = Some(l.snap.path().to_path_buf());
    let args = bwrap_args(&Resolved::new(&plain).unwrap()).unwrap();
    let s = canon(l.snap.path()).to_string_lossy().into_owned();
    assert!(args.windows(3).any(|w| w[0] == "--ro-bind" && w[1] == s && w[2] == s), "{args:?}");
}

// ---------------------------------------------------------------------------------------------- the probe

#[test]
fn the_probe_holds_all_five_canaries_under_the_real_profile() {
    let _g = PROBE.lock().unwrap_or_else(|e| e.into_inner());
    if mac_sandbox().is_none() {
        return;
    }
    let p = probe_authorized();
    assert!(p.is_ok(), "{p:?}");
    assert!(p.failed().is_empty() && p.unavailable.is_none());
    assert!(p.network_blocked && p.outside_unreadable && p.snapshot_readonly && p.scratch_noexec && p.interpreter_runs);
    assert!(p.summary().contains("all five canaries"), "{}", p.summary());
}

#[test]
fn the_probe_fails_closed_when_the_sandbox_is_forced_off() {
    let _g = PROBE.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: serialized by PROBE; no other test in this crate reads this variable.
    unsafe { std::env::set_var("DELPHI_TEST_FORCE_NO_SANDBOX", "1") };
    let p = probe_authorized();
    unsafe { std::env::remove_var("DELPHI_TEST_FORCE_NO_SANDBOX") };
    assert!(!p.is_ok());
    assert_eq!(p.failed(), vec!["unavailable"]);
    assert!(!p.network_blocked && !p.outside_unreadable && !p.snapshot_readonly && !p.scratch_noexec && !p.interpreter_runs);
    assert!(p.summary().contains("unavailable"), "{}", p.summary());
}

#[test]
fn each_canary_notices_its_own_hole() {
    let _g = PROBE.lock().unwrap_or_else(|e| e.into_inner());
    if mac_sandbox().is_none() {
        return;
    }
    let network = probe_with(INTERPRETER, |s, c| s.network = Network::ProxyOnly { port: c.port });
    assert!(!network.network_blocked && !network.is_ok(), "{network:?}");
    assert_eq!(network.failed(), vec!["network_blocked"], "{network:?}");

    let outside = probe_with(INTERPRETER, |s, c| s.read_only.push(c.snapshot.parent().unwrap().to_path_buf()));
    assert_eq!(outside.failed(), vec!["outside_unreadable"], "{outside:?}");

    let writable = probe_with(INTERPRETER, |s, c| {
        fs::set_permissions(&c.snapshot, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(c.snapshot.join("data.txt"), fs::Permissions::from_mode(0o644)).unwrap();
        s.scratch.push(c.snapshot.clone());
    });
    assert_eq!(writable.failed(), vec!["snapshot_readonly"], "{writable:?}");

    let exec = probe_with(INTERPRETER, |s, _| s.scratch_noexec = false);
    assert_eq!(exec.failed(), vec!["scratch_noexec"], "{exec:?}");

    // the interpreter is not executable: nothing ran, so nothing is proven
    let none = probe_with(INTERPRETER, |s, _| s.exec_only = Some(vec![PathBuf::from("/bin/zsh")]));
    assert!(!none.interpreter_runs && !none.is_ok(), "{none:?}");
    assert_eq!(none.failed().len(), 5, "no canary may pass when the script never ran: {none:?}");
}

#[test]
fn an_interpreter_that_does_not_exist_is_unavailable_never_ok() {
    let _g = PROBE.lock().unwrap_or_else(|e| e.into_inner());
    if mac_sandbox().is_none() {
        return;
    }
    let p = probe_with("/bin/no-such-interpreter", |_, _| {});
    assert!(!p.is_ok(), "{p:?}");
}

#[test]
fn on_linux_the_probe_says_unavailable() {
    let _g = PROBE.lock().unwrap_or_else(|e| e.into_inner());
    if !cfg!(target_os = "linux") {
        return;
    }
    let p = probe_authorized();
    assert!(!p.is_ok(), "{p:?}");
    // either there is no bubblewrap, or the profile cannot be expressed with it; both are a refusal
    assert!(p.unavailable.is_some(), "{p:?}");
    // In the Linux CI job bubblewrap is present: then the reason is the platform's, not a missing tool.
    if std::env::var("DELPHI_REQUIRE_SANDBOX").as_deref() == Ok("1") {
        assert!(p.unavailable.as_deref().unwrap_or("").contains("cannot express"), "{p:?}");
        assert_eq!(p.mode, Some("bubblewrap"));
    }
}

// ---------------------------------------------------------------------------------------------- the snapshot

fn git(dir: &Path, args: &[&str]) {
    let s = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_SYSTEM", "/dev/null").status().expect("git");
    assert!(s.success(), "git {args:?}");
}

fn repo() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let root = canon(t.path()).join("proj");
    fs::create_dir_all(root.join("dir")).unwrap();
    git(&root, &["init", "-q"]);
    fs::write(root.join("a.txt"), "a").unwrap();
    fs::write(root.join("dir/b.sh"), "#!/bin/bash\n").unwrap();
    fs::set_permissions(root.join("dir/b.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    git(&root, &["add", "a.txt", "dir/b.sh"]);
    fs::write(root.join("untracked.txt"), "secret").unwrap();
    fs::write(root.join(".env"), "TOKEN=x").unwrap();
    (t, root)
}

#[test]
fn the_snapshot_holds_tracked_files_only_read_only_and_is_discarded() {
    let (t, project) = repo();
    let parent = canon(t.path());
    let removed;
    {
        let s = Snapshot::build(&project, &parent).unwrap();
        let d = s.path();
        assert_eq!(s.file_count(), 2);
        assert_eq!(fs::read_to_string(d.join("a.txt")).unwrap(), "a");
        assert!(d.join("dir/b.sh").exists());
        assert!(!d.join("untracked.txt").exists() && !d.join(".env").exists(), "an untracked file reached the snapshot");
        assert!(!d.join(".git").exists());
        assert_eq!(fs::metadata(d.join("a.txt")).unwrap().permissions().mode() & 0o777, 0o444);
        assert_eq!(fs::metadata(d.join("dir/b.sh")).unwrap().permissions().mode() & 0o777, 0o444, "no file is executable in the snapshot");
        assert_eq!(fs::metadata(d.join("dir")).unwrap().permissions().mode() & 0o777, 0o555);
        assert_eq!(fs::metadata(d).unwrap().permissions().mode() & 0o777, 0o555);
        // not writable: not the file, not a new file in a directory
        assert!(fs::OpenOptions::new().write(true).open(d.join("a.txt")).is_err());
        assert!(fs::write(d.join("new.txt"), "x").is_err());
        assert!(fs::write(d.join("dir/new.txt"), "x").is_err());
        s.verify().unwrap();
        assert_eq!(s.tree_hash().len(), 64);
        // the project is untouched
        assert_eq!(fs::read_to_string(project.join("a.txt")).unwrap(), "a");
        removed = s.root().to_path_buf();
        // a change is noticed
        fs::set_permissions(d.join("dir"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(s.verify().is_err());
        fs::set_permissions(d.join("dir"), fs::Permissions::from_mode(0o555)).unwrap();
        s.verify().unwrap();
    }
    assert!(!removed.exists(), "the snapshot is discarded on drop");
}

#[test]
fn two_snapshots_of_the_same_tree_have_the_same_hash() {
    let (t, project) = repo();
    let parent = canon(t.path());
    let (a, b) = (Snapshot::build(&project, &parent).unwrap(), Snapshot::build(&project, &parent).unwrap());
    assert_eq!(a.tree_hash(), b.tree_hash());
    fs::write(project.join("a.txt"), "changed").unwrap();
    assert_ne!(Snapshot::build(&project, &parent).unwrap().tree_hash(), a.tree_hash());
}

#[test]
fn a_symlink_that_leaves_the_tree_is_refused_and_leaves_nothing_behind() {
    let (t, project) = repo();
    let parent = canon(t.path());
    for (name, target) in [("escape", "../outside"), ("abs", "/etc/passwd"), ("deep", "dir/../../x"), ("up", "../../../x")] {
        std::os::unix::fs::symlink(target, project.join(name)).unwrap();
        git(&project, &["add", name]);
        let e = Snapshot::build(&project, &parent).unwrap_err();
        assert!(e.contains("symlink"), "{name}: {e}");
        git(&project, &["rm", "-q", "--cached", name]);
        fs::remove_file(project.join(name)).unwrap();
    }
    let leftovers: Vec<_> = fs::read_dir(&parent).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("delphi-snapshot-")).collect();
    assert!(leftovers.is_empty(), "a refused snapshot left a directory behind: {leftovers:?}");
    // a link that stays inside the tree is kept as a link
    std::os::unix::fs::symlink("../a.txt", project.join("dir/inside")).unwrap();
    git(&project, &["add", "dir/inside"]);
    let s = Snapshot::build(&project, &parent).unwrap();
    assert_eq!(fs::read_link(s.path().join("dir/inside")).unwrap(), PathBuf::from("../a.txt"));
}

#[test]
fn a_path_with_a_git_segment_or_dotdot_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let root = canon(t.path());
    fs::create_dir_all(root.join("p/.git")).unwrap();
    fs::write(root.join("p/.git/config"), "x").unwrap();
    assert!(Snapshot::from_files(&root.join("p"), &[PathBuf::from(".git/config")], &root).is_err());
    assert!(Snapshot::from_files(&root.join("p"), &[PathBuf::from("../p/x")], &root).is_err());
    assert!(Snapshot::from_files(&root.join("p"), &[PathBuf::from("/etc/passwd")], &root).is_err());
}

// ---------------------------------------------------------------------------------------------- the fresh home

#[test]
fn a_fresh_home_is_private_clean_and_discarded() {
    let t = tempfile::tempdir().unwrap();
    let root = canon(t.path());
    let path;
    {
        let h = FreshHome::create(&root).unwrap();
        path = h.path().to_path_buf();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o700);
        fs::create_dir_all(path.join("a/b")).unwrap();
        fs::write(path.join("a/b/note.txt"), "x").unwrap();
        h.recheck().unwrap();
    }
    assert!(!path.exists());
}

#[test]
fn the_home_walk_refuses_a_symlink_a_credential_and_a_widened_directory_at_each_use() {
    let t = tempfile::tempdir().unwrap();
    let root = canon(t.path());
    let h = FreshHome::create(&root).unwrap();
    let p = h.path().to_path_buf();
    // a symlink, at the top and nested, to a file and to a directory, even one that dangles
    for (rel, target) in [("link", "/etc/passwd"), ("sub/deep-link", "/tmp"), ("dangling", "/nonexistent")] {
        fs::create_dir_all(p.join(rel).parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(target, p.join(rel)).unwrap();
        let e = h.recheck().unwrap_err();
        assert!(e.contains("symlink"), "{rel}: {e}");
        fs::remove_file(p.join(rel)).unwrap();
        h.recheck().unwrap();
    }
    // credential-named files, anywhere
    for name in ["auth.json", ".netrc", "credentials", "credentials.json", "Credentials.toml", "key.pem", "id_rsa", "id_ed25519", "_netrc"] {
        fs::create_dir_all(p.join("x")).unwrap();
        fs::write(p.join("x").join(name), "s").unwrap();
        let e = h.recheck().unwrap_err();
        assert!(e.contains("credential"), "{name}: {e}");
        fs::remove_file(p.join("x").join(name)).unwrap();
        h.recheck().unwrap();
    }
    // a home that someone widened is no longer private
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(h.recheck().unwrap_err().contains("private"));
    fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
    h.recheck().unwrap();
    // swapping the whole home for a link
    let moved = root.join("moved");
    fs::rename(&p, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &p).unwrap();
    assert!(h.recheck().is_err());
    fs::remove_file(&p).unwrap();
    fs::rename(&moved, &p).unwrap();
}

#[test]
fn another_device_a_socket_or_a_pipe_is_refused_by_the_judge() {
    // a mount cannot be made in a test; the rule is a pure function of the device numbers
    assert!(judge_home_entry("data", false, true, 1, 1).is_none());
    assert!(judge_home_entry("data", false, true, 2, 1).unwrap().contains("mount"));
    assert!(judge_home_entry("data", true, true, 1, 1).unwrap().contains("symlink"));
    assert!(judge_home_entry("sock", false, false, 1, 1).unwrap().contains("socket"));
    for n in ["auth.json", "AUTH.JSON", ".netrc", "credentials", "credentials.json", "a.pem", "id_rsa", "id_dsa.pub"] {
        assert!(judge_home_entry(n, false, true, 1, 1).is_some(), "{n}");
    }
    for n in ["notes.txt", "identity.txt", "pem", "my_id", "credential.txt"] {
        assert!(judge_home_entry(n, false, true, 1, 1).is_none(), "{n}");
    }
}

#[test]
fn the_environment_is_fixed_and_has_no_interpreter_hooks() {
    let e = authorized_env();
    assert_eq!(e.len(), 2);
    for k in ["BASH_ENV", "ENV", "HOME", "LD_PRELOAD", "PERL5OPT", "PERL5LIB", "PYTHONPATH"] {
        assert!(!e.contains_key(k), "{k}");
    }
    assert!(!e.keys().any(|k| k.starts_with("DYLD_")));
}
