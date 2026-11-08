//! Spec 092 S2c tests: the grant, matching, the use ledger, mid-run revocation, the per-use re-probe, tool stripping and the seat table.
//! Every test here fails against a build without the mitigation it names. None of them runs a command: the fixture "interpreter" is a
//! file that is only ever hashed.

use super::*;
use crate::authorization as a;
use delphi_sandbox::authorized::AuthorizedProbe;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Fx {
    _t: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    bin: String,
    script: String,
    id: String,
    policy: a::Policy,
}

fn now() -> i64 {
    delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap()
}

fn ok_probe() -> AuthorizedProbe {
    AuthorizedProbe { mode: Some("test"), unavailable: None, network_blocked: true, outside_unreadable: true, snapshot_readonly: true, scratch_noexec: true, interpreter_runs: true, scratch_no_map_exec: true, detail: String::new() }
}

/// A home holding one live record (`run-1`, `max_uses`) for a fake interpreter and a script, made the way `delphi test authorize` makes it.
fn fixture(max_uses: u32) -> Fx {
    let t = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(t.path()).unwrap();
    let home = root.join("home");
    fs::create_dir_all(home.join("operator")).unwrap();
    delphi_core::lock::write(&home.join("operator/ratified.lock"), &Default::default()).unwrap();
    let (bin, script) = (root.join("bash"), root.join("t.sh"));
    fs::write(&bin, b"#!fake interpreter\n").unwrap();
    fs::write(&script, b"#!/bin/bash\ntouch \"$(dirname \"$0\")/RAN\"\n").unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).unwrap();
    let (b, s) = (bin.to_string_lossy().to_string(), script.to_string_lossy().to_string());
    let policy = a::Policy { interpreters: vec![b.clone()], sealed_roots: vec![] };
    let entry = a::Entry { tool: "shell".into(), argv: vec![b.clone(), s.clone()], files: vec![s.clone()], runner_provenance: "project_snapshot".into() };
    let n = a::NewRecord { purpose: "PURPOSE-MARKER-7f3a".into(), intent: "i".into(), run: "run-1".into(), expires_in_secs: a::DEFAULT_EXPIRY_SECS, max_uses, entries: vec![entry], request: None, policy: policy.clone() };
    let rec = a::build_record(&home, &n, now(), &delphi_core::time::now_rfc3339()).unwrap();
    let id = rec.id.clone();
    a::create_audited(&home, rec.clone(), json!({"id": rec.id, "manifest_hash": rec.manifest_hash, "run": rec.run, "by": "operator", "source": "operator"})).unwrap();
    Fx { _t: t, root, home, bin: b, script: s, id, policy }
}

impl Fx {
    fn record(&self) -> a::Record {
        a::load(&self.home).unwrap().records.into_iter().find(|r| r.id == self.id).unwrap()
    }

    fn seq(&self) -> u64 {
        a::load(&self.home).unwrap().audit_head.map(|h| h.seq).unwrap_or(0)
    }

    fn grant(&self) -> Grant {
        Grant::for_test(self.record(), "run-1", &self.home, self.seq(), self.policy.clone()).with_probe(ok_probe)
    }

    fn argv(&self) -> Vec<String> {
        vec![self.bin.clone(), self.script.clone()]
    }

    /// The REAL ledger's count (S2c never writes it from a run's path).
    fn used(&self) -> u32 {
        ledger_count(&self.home, &self.id).unwrap()
    }

    /// The dry ledger's count (what observation writes).
    fn dry(&self) -> u32 {
        dry_count(&self.home, &self.id).unwrap()
    }
}

fn card(g: &Grant, tool: &str, argv: &[String]) -> Outcome {
    g.evaluate_card(tool, argv, now())
}

fn is_would_use(o: &Outcome) -> bool {
    matches!(o, Outcome::WouldUse { .. })
}

// ---------------------------------------------------------------------------------------------- matching: only one kind, exactly

#[test]
fn the_exact_command_of_the_manifest_is_the_one_match() {
    let f = fixture(3);
    let g = f.grant();
    match card(&g, "run_command", &f.argv()) {
        Outcome::WouldUse { entry, index, uses_left, .. } => assert_eq!((entry, index, uses_left), (0, 1, 2)),
        other => panic!("{other:?}"),
    }
    assert_eq!(f.dry(), 1);
    assert!(matches!(card(&g, "run_command", &f.argv()), Outcome::WouldUse { index: 2, uses_left: 1, .. }));
    assert!(!f.root.join("RAN").exists(), "the match path runs nothing");
}

#[test]
fn every_card_class_other_than_run_command_is_never_matched_under_a_valid_grant() {
    let f = fixture(5);
    let g = f.grant();
    // The same argument vector as the manifest entry, offered as each other kind of card: none matches, and nothing is consumed.
    for class in ["shell", "harness_bash", "harness_edit", "harness_connector", "dispatch_agent", "connector", "merge", "pull_request", "credential", "network", "write_file", "git_push"] {
        let o = card(&g, class, &f.argv());
        assert!(matches!(o, Outcome::NotMatched(_)), "{class}: {o:?}");
    }
    assert_eq!(f.used(), 0, "no class consumed a use");
}

#[test]
fn near_misses_of_the_command_do_not_match_and_consume_nothing() {
    let f = fixture(5);
    let g = f.grant();
    let (bin, script) = (f.bin.clone(), f.script.clone());
    let variants: Vec<(&str, Vec<String>)> = vec![
        ("path-shadow name", vec!["bash".into(), script.clone()]),
        ("bare name for both", vec!["bash".into(), "t.sh".into()]),
        ("extra argument", vec![bin.clone(), script.clone(), "--extra".into()]),
        ("reordered", vec![script.clone(), bin.clone()]),
        ("trailing space", vec![bin.clone(), format!("{script} ")]),
        ("leading space", vec![format!(" {bin}"), script.clone()]),
        ("unicode confusable", vec![bin.clone(), script.replace('t', "\u{0442}")]),
        ("dot-slash prefix", vec![bin.clone(), format!("./{}", script.trim_start_matches('/'))]),
        ("double slash", vec![bin.clone(), script.replacen('/', "//", 1)]),
        ("joined into one element", vec![format!("{bin} {script}")]),
        ("interpreter only", vec![bin.clone()]),
        ("empty", vec![]),
    ];
    for (name, v) in variants {
        let o = card(&g, "run_command", &v);
        assert!(matches!(o, Outcome::NotMatched(_)), "{name}: {o:?}");
    }
    // The right vector in the wrong environment or directory.
    let (mut env, cwd) = (a::scrubbed_env(), a::CWD);
    env.insert("BASH_ENV".into(), "/tmp/x".into());
    let o = g.evaluate(&Candidate { tool: "run_command", argv: &f.argv(), cwd, env: &env }, now());
    assert!(matches!(o, Outcome::NotMatched(_)), "different env: {o:?}");
    let o = g.evaluate(&Candidate { tool: "run_command", argv: &f.argv(), cwd: "/tmp", env: &a::scrubbed_env() }, now());
    assert!(matches!(o, Outcome::NotMatched(_)), "different cwd: {o:?}");
    assert_eq!(f.used(), 0);
}

#[test]
fn a_symlinked_or_edited_script_or_interpreter_is_refused_at_use_not_matched_on_trust() {
    // edited script: same name, new bytes
    let f = fixture(5);
    let g = f.grant();
    fs::write(&f.script, b"#!/bin/bash\necho evil\n").unwrap();
    let o = card(&g, "run_command", &f.argv());
    assert!(matches!(&o, Outcome::Refused { reason, .. } if reason.contains("not the file that was pinned")), "{o:?}");
    assert_eq!(f.used(), 0);
    // symlinked script: the path is a link to a file with the pinned bytes
    let f = fixture(5);
    let g = f.grant();
    let real = f.root.join("real.sh");
    fs::copy(&f.script, &real).unwrap();
    fs::remove_file(&f.script).unwrap();
    std::os::unix::fs::symlink(&real, &f.script).unwrap();
    let o = card(&g, "run_command", &f.argv());
    assert!(matches!(&o, Outcome::Refused { reason, .. } if reason.contains("not canonical") || reason.contains("pinned")), "{o:?}");
    assert_eq!(f.used(), 0);
    // edited interpreter
    let f = fixture(5);
    let g = f.grant();
    fs::write(&f.bin, b"#!fake interpreter, replaced\n").unwrap();
    let o = card(&g, "run_command", &f.argv());
    assert!(matches!(&o, Outcome::Refused { reason, .. } if reason.contains("interpreter")), "{o:?}");
    assert_eq!(f.used(), 0);
    // the script made group-writable (an agent could replace it)
    let f = fixture(5);
    let g = f.grant();
    fs::set_permissions(&f.script, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(matches!(card(&g, "run_command", &f.argv()), Outcome::Refused { .. }));
}

#[test]
fn the_deny_list_is_applied_again_at_use_to_a_hand_built_record() {
    // A record whose manifest names a denied binary but whose hash and pins are valid (it was not made by `build_record`).
    let f = fixture(5);
    let mut rec = f.record();
    rec.manifest[0].argv[0] = "/usr/bin/git".into();
    rec.manifest[0].binary.path = "/usr/bin/git".into();
    let g = Grant::for_test(rec, "run-1", &f.home, f.seq(), f.policy.clone()).with_probe(ok_probe);
    let o = card(&g, "run_command", &["/usr/bin/git".into(), f.script.clone()]);
    assert!(matches!(o, Outcome::Refused { .. }), "{o:?}");
    assert_eq!(f.used(), 0);
}

// ---------------------------------------------------------------------------------------------- the use ledger

#[test]
fn the_ledger_is_append_only_and_chained_and_a_tampered_one_is_refused() {
    let f = fixture(4);
    assert_eq!(f.used(), 0);
    take_use(&f.home, &f.id, "h", "run-1", 4).unwrap();
    take_use(&f.home, &f.id, "h", "run-1", 4).unwrap();
    assert_eq!(f.used(), 2);
    let dir = ledger_dir(&f.home, &f.id);
    let rep = delphi_core::log::verify_chain(&dir).unwrap();
    assert!(rep.broken.is_none() && rep.unchained == 0 && rep.head.is_some());
    // Edit the segment: the chain breaks and the count is an error, never a guess (so launch and every use refuse).
    let seg = fs::read_dir(dir.join("events")).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "jsonl")).unwrap();
    let text = fs::read_to_string(&seg).unwrap();
    fs::write(&seg, text.replacen("\"index\":1", "\"index\":9", 1)).unwrap();
    assert!(ledger_count(&f.home, &f.id).is_err());
    assert!(a::authorize_launch(&f.home, &f.id, "run-1", now(), &f.policy).is_err());
}

#[test]
fn two_threads_on_a_record_with_one_use_exactly_one_wins() {
    for _round in 0..3 {
        let f2 = fixture(1);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let (b, home, id) = (barrier.clone(), f2.home.clone(), f2.id.clone());
                std::thread::spawn(move || {
                    b.wait();
                    take_use(&home, &id, "h", "run-1", 1)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1, "{results:?}");
        assert_eq!(f2.used(), 1);
    }
}

#[test]
fn many_threads_never_take_more_uses_than_the_record_has() {
    let f = fixture(3);
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (b, home, id) = (barrier.clone(), f.home.clone(), f.id.clone());
            std::thread::spawn(move || {
                b.wait();
                take_use(&home, &id, "h", "run-1", 3).is_ok()
            })
        })
        .collect();
    let won = handles.into_iter().map(|h| h.join().unwrap()).filter(|w| *w).count();
    assert!(won <= 3, "{won} uses were taken of 3");
    assert_eq!(f.used() as usize, won, "every use that was granted is in the ledger, and no other");
}

#[test]
fn a_kill_after_the_decrement_leaves_the_use_consumed() {
    let f = fixture(2);
    let exe = std::env::current_exe().unwrap();
    let st = std::process::Command::new(exe)
        .args(["--exact", "authorization_grant::tests::kill9_helper", "--nocapture", "--test-threads=1"])
        .env("S2C_KILL_HOME", &f.home)
        .env("S2C_KILL_ID", &f.id)
        .status()
        .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(st.signal(), Some(9), "the helper must die by SIGKILL right after the decrement: {st:?}");
    assert_eq!(f.used(), 1, "the use was fsynced before the kill and stays consumed");
    // and the ledger it left still verifies
    assert!(delphi_core::log::verify_chain(&ledger_dir(&f.home, &f.id)).unwrap().broken.is_none());
}

/// Run only by the test above, in a child process: take a use, then kill itself.
#[test]
fn kill9_helper() {
    if let (Ok(home), Ok(id)) = (std::env::var("S2C_KILL_HOME"), std::env::var("S2C_KILL_ID")) {
        take_use(Path::new(&home), &id, "h", "run-1", 2).unwrap();
        // SAFETY: raising a signal on ourselves.
        unsafe { libc::raise(libc::SIGKILL) };
    }
}

// ---------------------------------------------------------------------------------------------- revocation, replay, the probe

#[test]
fn a_revocation_between_two_cards_denies_the_second() {
    let f = fixture(5);
    let g = f.grant();
    assert!(is_would_use(&card(&g, "run_command", &f.argv())));
    a::revoke_audited(&f.home, &f.id, &delphi_core::time::now_rfc3339()).unwrap();
    let o = card(&g, "run_command", &f.argv());
    assert!(matches!(&o, Outcome::Refused { reason, .. } if reason.contains("revoked")), "{o:?}");
    assert_eq!(f.dry(), 1, "the refused card consumed nothing");
}

#[test]
fn an_older_store_and_lock_pair_put_back_mid_run_is_refused() {
    let f = fixture(5);
    // A second record is made after the first: the store moves on to a later audit head.
    let earlier_store = fs::read(a::store_path(&f.home)).unwrap();
    let earlier_lock = fs::read(f.home.join("operator/ratified.lock")).unwrap();
    let n = a::NewRecord { purpose: "second".into(), intent: "i".into(), run: "run-2".into(), expires_in_secs: a::DEFAULT_EXPIRY_SECS, max_uses: 1, entries: vec![a::Entry { tool: "shell".into(), argv: f.argv(), files: vec![f.script.clone()], runner_provenance: "project_snapshot".into() }], request: None, policy: f.policy.clone() };
    let rec2 = a::build_record(&f.home, &n, now(), &delphi_core::time::now_rfc3339()).unwrap();
    a::create_audited(&f.home, rec2.clone(), json!({"id": rec2.id, "manifest_hash": rec2.manifest_hash, "run": rec2.run, "by": "operator", "source": "operator"})).unwrap();
    let g = f.grant(); // launched now: it remembers the later head
    assert!(is_would_use(&card(&g, "run_command", &f.argv())));
    // The older pair goes back (a record "quietly taken back" is the point of the attack; here it is the older head that gives it away).
    fs::write(a::store_path(&f.home), earlier_store).unwrap();
    fs::write(f.home.join("operator/ratified.lock"), earlier_lock).unwrap();
    let o = card(&g, "run_command", &f.argv());
    assert!(matches!(o, Outcome::Refused { .. }), "{o:?}");
    assert_eq!(f.dry(), 1);
}

#[test]
fn a_store_that_is_not_as_far_on_as_at_launch_is_refused_by_name() {
    let f = fixture(5);
    // A grant that remembers a later head than the store now holds.
    let g = Grant::for_test(f.record(), "run-1", &f.home, f.seq() + 5, f.policy.clone()).with_probe(ok_probe);
    let o = card(&g, "run_command", &f.argv());
    assert!(matches!(&o, Outcome::Refused { reason, .. } if reason.contains("older")), "{o:?}");
}

#[test]
fn a_record_changed_in_the_store_since_launch_is_refused() {
    let f = fixture(5);
    let g = f.grant();
    // The operator-side code is the only writer, but if the stored bytes differ from the launch copy the grant does not trust it.
    a::mutate(&f.home, |s| {
        s.records[0].purpose = "changed".into();
        Ok(())
    })
    .unwrap();
    let o = card(&g, "run_command", &f.argv());
    assert!(matches!(&o, Outcome::Refused { reason, .. } if reason.contains("changed") || reason.contains("disagree") || reason.contains("audit")), "{o:?}");
}

#[test]
fn an_expired_record_is_refused_at_use() {
    let f = fixture(5);
    let g = f.grant();
    let o = g.evaluate_card("run_command", &f.argv(), now() + a::MAX_EXPIRY_SECS + 10);
    assert!(matches!(&o, Outcome::Refused { reason, .. } if reason.contains("expired")), "{o:?}");
}

#[test]
fn the_sandbox_is_probed_before_every_use_the_first_included_and_a_failed_probe_refuses() {
    let f = fixture(5);
    let calls = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicBool::new(false));
    let (c, fl) = (calls.clone(), fail.clone());
    let g = Grant::for_test(f.record(), "run-1", &f.home, f.seq(), f.policy.clone()).with_probe(move || {
        c.fetch_add(1, Ordering::SeqCst);
        if fl.load(Ordering::SeqCst) {
            AuthorizedProbe { unavailable: Some("forced off after launch".into()), ..AuthorizedProbe::default() }
        } else {
            ok_probe()
        }
    });
    assert!(is_would_use(&card(&g, "run_command", &f.argv())));
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the first use was probed");
    fail.store(true, Ordering::SeqCst);
    let o = card(&g, "run_command", &f.argv());
    assert!(matches!(&o, Outcome::Refused { reason, .. } if reason.contains("probe")), "{o:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "every use is probed again");
    assert_eq!(f.dry(), 1, "the refused card consumed nothing");
    assert!(!f.root.join("RAN").exists(), "nothing ran, with the sandbox or without it");
}

#[test]
fn a_probe_that_is_partly_ok_is_not_ok() {
    let f = fixture(5);
    let g = Grant::for_test(f.record(), "run-1", &f.home, f.seq(), f.policy.clone()).with_probe(|| AuthorizedProbe { network_blocked: false, ..ok_probe() });
    assert!(matches!(card(&g, "run_command", &f.argv()), Outcome::Refused { .. }));
}

// ---------------------------------------------------------------------------------------------- the grant is daemon-side only

#[test]
fn the_boot_payload_is_accepted_only_from_the_launcher_that_it_names_and_only_if_intact() {
    let f = fixture(5);
    let rec = f.record();
    let bytes = encode_boot(&rec, "run-1", &f.home, f.seq()).unwrap();
    let ok = Grant::from_boot(&bytes, std::process::id(), now(), f.policy.clone()).unwrap();
    assert_eq!(ok.id(), f.id);
    assert!(Grant::from_boot(&bytes, std::process::id() + 1, now(), f.policy.clone()).unwrap_err().contains("launcher"));
    let tampered = String::from_utf8(bytes.clone()).unwrap().replace("PURPOSE-MARKER-7f3a", "PURPOSE-MARKER-0000");
    assert!(Grant::from_boot(tampered.as_bytes(), std::process::id(), now(), f.policy.clone()).unwrap_err().contains("hash"));
    assert!(Grant::from_boot(b"not json", std::process::id(), now(), f.policy.clone()).is_err());
    // A grant that is already wrong never takes hold: revoked before the daemon read it.
    a::revoke_audited(&f.home, &f.id, &delphi_core::time::now_rfc3339()).unwrap();
    assert!(Grant::from_boot(&bytes, std::process::id(), now(), f.policy.clone()).unwrap_err().contains("revoked"));
    // too large for the pipe
    let mut big = rec;
    big.purpose = "x".repeat(MAX_BOOT_BYTES);
    assert!(encode_boot(&big, "run-1", &f.home, 0).unwrap_err().contains("bytes"));
}

#[test]
fn the_grant_never_prints_and_the_debug_of_the_engine_state_holds_nothing_of_it() {
    let f = fixture(5);
    let g = f.grant();
    let shown = format!("{g:?}");
    assert!(!shown.contains("PURPOSE-MARKER") && !shown.contains(&f.id) && !shown.contains(f.home.to_str().unwrap()), "{shown}");
}

fn pipe_with(bytes: &[u8]) -> i32 {
    use std::io::Write as _;
    let mut fds = [0i32; 2];
    // SAFETY: plain pipe(2).
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    // A high descriptor number, not close-on-exec: exactly how a descriptor reaches a daemon through exec.
    let high = unsafe { libc::fcntl(fds[0], libc::F_DUPFD, 200) };
    assert!(high >= 200);
    unsafe { libc::close(fds[0]) };
    let mut w = unsafe { fs::File::from_raw_fd(fds[1]) };
    w.write_all(bytes).unwrap();
    drop(w);
    high
}

fn child_view(plain: bool) -> String {
    // What a child of the daemon sees: its environment and its open descriptors. `plain` is a bare `Command` (inherits what is not
    // close-on-exec); otherwise it is started the way `exec.run` starts one (`plain_command`, which also closes every extra descriptor).
    let script = ["-c", "/usr/bin/env; echo ---; /bin/ls /dev/fd"];
    if plain {
        let out = std::process::Command::new("/bin/sh").args(script).env_clear().output().unwrap();
        return String::from_utf8_lossy(&out.stdout).into_owned();
    }
    let cmd = delphi_sandbox::plain_command(Default::default(), &a::scrubbed_env(), "/bin/sh", script);
    delphi_sandbox::run_bounded(cmd, &Default::default()).unwrap().stdout
}

fn lists(view: &str, fd: i32) -> bool {
    view.lines().skip_while(|l| *l != "---").any(|l| l.trim() == fd.to_string())
}

#[test]
fn a_child_of_the_daemon_inherits_neither_the_grant_nor_its_descriptor() {
    let f = fixture(5);
    let bytes = encode_boot(&f.record(), "run-1", &f.home, f.seq()).unwrap();
    let fd = pipe_with(&bytes);
    // Negative control: while the descriptor is open and inheritable, a bare child DOES see it, so the checks below can fail.
    assert!(lists(&child_view(true), fd), "control: an open inheritable descriptor must show in a bare child");
    // Defence in depth that already exists: `plain_command` closes every extra descriptor even while it is open.
    assert!(!lists(&child_view(false), fd), "plain_command closes inherited descriptors");
    // The daemon's step: read once, close.
    let got = take_from_fd(fd).unwrap();
    assert_eq!(got, bytes);
    for plain in [true, false] {
        let after = child_view(plain);
        assert!(!after.contains("PURPOSE-MARKER") && !after.contains(&f.id) && !after.contains("record_hash"), "{after}");
        assert!(!lists(&after, fd), "the descriptor is closed after the read (plain={plain}):\n{after}");
    }
    // The environment of an exec.run child is exactly the scrubbed one: no grant, no home, no token.
    let after = child_view(false);
    let env: Vec<&str> = after.lines().take_while(|l| *l != "---").collect();
    assert!(env.iter().all(|l| l.starts_with("LC_ALL=") || l.starts_with("PATH=") || l.starts_with("PWD=") || l.starts_with("SHLVL=") || l.starts_with("_=")), "{env:?}");
}

#[test]
fn a_grant_pipe_with_more_than_the_limit_is_refused_and_still_closed() {
    let fd = pipe_with(&vec![b'x'; MAX_BOOT_BYTES + 100]);
    assert!(take_from_fd(fd).is_err());
    // SAFETY: probing whether the descriptor is still open.
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1, "closed even on refusal");
}

// ---------------------------------------------------------------------------------------------- tools and seats (TA-5)

#[test]
fn file_writing_and_connector_tools_are_stripped_in_a_granted_run() {
    for t in ["write_file", "edit_file", "harness_edit", "write_archive", "write_html", "write_chart", "write_report", "write_deck", "write_sheet", "write_diagram", "propose_admin_action", "propose_kb_entry", "propose_note", "apply_patch", "slack.write_file", "github.propose_change"] {
        assert!(tool_stripped(t, false).is_some(), "{t}");
    }
    assert!(tool_stripped("slack.search", true).is_some(), "any connector tool");
    for t in ["read_file", "list_dir", "search", "note", "name_thread", "request_test_authorization", "run_command", "kb_search"] {
        assert_eq!(tool_stripped(t, false), None, "{t}");
    }
}

#[test]
fn a_seat_starts_in_a_granted_run_only_where_its_own_tools_can_be_proven_off() {
    // (cli, tooled, native_empty, app_server)
    assert!(seat_native_tools_off("claude", false, true, false).is_ok());
    assert!(seat_native_tools_off("claude", true, true, false).is_err(), "a Builder keeps its file and shell tools");
    assert!(seat_native_tools_off("claude", false, false, false).is_err(), "native tools listed in the profile");
    assert!(seat_native_tools_off("copilot", false, true, false).is_ok());
    assert!(seat_native_tools_off("copilot", true, true, false).is_err());
    assert!(seat_native_tools_off("copilot", false, true, true).is_err(), "ACP Builder mode");
    for always in [false, true] {
        assert!(seat_native_tools_off("agy", always, true, false).is_err(), "agy's tools cannot be switched off");
        assert!(seat_native_tools_off("codex", always, true, false).is_err(), "codex is not proven off");
    }
    assert!(seat_native_tools_off("unknown-cli", false, true, false).is_err());
}

// ---------------------------------------------------------------------------------------------- the engine: observation only, nothing approved

mod engine {
    use super::*;
    use crate::approval::{Answer, Tier as ApprovalTier};
    use crate::engine::Asker;
    use crate::model::Step;
    use crate::testkit::{events_of, setup};

    async fn wait_for_card(f: &crate::testkit::Fx) -> crate::approval::Request {
        for _ in 0..300 {
            if let Some(r) = f.engine.broker.pending().into_iter().next() {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("no approval card appeared");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_matching_red_card_with_a_valid_grant_is_logged_as_would_use_and_is_still_denied_and_nothing_runs() {
        let g = fixture(3);
        let f = setup(vec![], 5.0, |_| {});
        f.engine.grant.set(Arc::new(g.grant())).unwrap();
        let payload = json!({"tool": "run_command", "argv": g.argv(), "ceiling": "l0"});
        let who = Asker { m: &f.m, task_id: "t1", span_id: "s1", role: "builder" };
        let matched = f.engine.observe_grant_call(&who, "run_command", &payload).await.unwrap();
        assert!(matched, "the call matched an entry (the caller denies it)");
        let ask = f.engine.ask_as(&who, ApprovalTier::Red, "run_command", "run_command", "run the pinned script", &payload);
        let answer = async {
            let card = wait_for_card(&f).await;
            // The card is raised and WAITS: the grant did not answer it. The run's own policy (`decide`) denies it.
            assert_eq!(card.tier, ApprovalTier::Red);
            f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "headless run: a Red action is never approved without the operator".into() }, "headless").unwrap();
        };
        let (denied, _) = tokio::join!(ask, answer);
        let denied = denied.unwrap();
        assert!(denied.is_some_and(|r| r.is_error && r.content.contains("denied")), "a matching card with a valid grant is DENIED");
        let would = events_of(&f, WOULD_USE);
        assert_eq!(would.len(), 1);
        let p = &would[0].payload;
        assert_eq!((p["authorization_id"].as_str(), p["index"].as_u64(), p["uses_left"].as_u64(), p["approved"].as_bool(), p["dry"].as_bool()), (Some(g.id.as_str()), Some(1), Some(2), Some(false), Some(true)));
        assert!(p["purpose"].as_str().unwrap().contains("PURPOSE-MARKER") && p["record_hash"].as_str().unwrap().len() > 20);
        assert_eq!((g.dry(), g.used()), (1, 0), "the dry use is written to the dry ledger and the real one is untouched");
        let resolved = events_of(&f, "approval.resolved");
        assert_eq!(resolved[0].payload["resolution"], "denied");
        assert!(events_of(&f, "action.intent").is_empty() && events_of(&f, "action.result").is_empty(), "exec.run / the runner was never called");
        assert!(!g.root.join("RAN").exists(), "the pinned script was never run");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_red_card_that_does_not_match_is_logged_and_asked_as_before() {
        let g = fixture(3);
        let f = setup(vec![], 5.0, |_| {});
        f.engine.grant.set(Arc::new(g.grant())).unwrap();
        let payload = json!({"tool": "run_command", "argv": [g.bin.clone(), "/etc/passwd"], "ceiling": "l0"});
        let who = Asker { m: &f.m, task_id: "t1", span_id: "s1", role: "builder" };
        let matched = f.engine.observe_grant_call(&who, "run_command", &payload).await.unwrap();
        assert!(!matched);
        let ask = f.engine.ask_as(&who, ApprovalTier::Red, "run_command", "run_command", "other", &payload);
        let answer = async {
            let card = wait_for_card(&f).await;
            f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "no".into() }, "headless").unwrap();
        };
        let (denied, _) = tokio::join!(ask, answer);
        assert!(denied.unwrap().is_some());
        assert!(events_of(&f, WOULD_USE).is_empty() && events_of(&f, NO_MATCH).len() == 1);
        assert_eq!(g.used(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refusal_at_use_is_logged_as_refused_with_its_reason() {
        let g = fixture(3);
        let f = setup(vec![], 5.0, |_| {});
        let grant = g.grant().with_probe(|| AuthorizedProbe { unavailable: Some("forced off after launch".into()), ..AuthorizedProbe::default() });
        f.engine.grant.set(Arc::new(grant)).unwrap();
        let payload = json!({"tool": "run_command", "argv": g.argv(), "ceiling": "l0"});
        let who = Asker { m: &f.m, task_id: "t1", span_id: "s1", role: "builder" };
        let matched = f.engine.observe_grant_call(&who, "run_command", &payload).await.unwrap();
        assert!(matched, "matched, then refused at use");
        let ask = f.engine.ask_as(&who, ApprovalTier::Red, "run_command", "run_command", "run", &payload);
        let answer = async {
            let card = wait_for_card(&f).await;
            f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "no".into() }, "headless").unwrap();
        };
        let _ = tokio::join!(ask, answer);
        let r = events_of(&f, REFUSED_USE);
        assert_eq!(r.len(), 1);
        assert!(r[0].payload["reason"].as_str().unwrap().contains("probe") && r[0].payload["approved"] == false);
        assert_eq!(g.used(), 0);
        assert!(!g.root.join("RAN").exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_grant_means_no_observation_at_all() {
        let g = fixture(3);
        let f = setup(vec![], 5.0, |_| {});
        let payload = json!({"tool": "run_command", "argv": g.argv(), "ceiling": "l0"});
        let who = Asker { m: &f.m, task_id: "t1", span_id: "s1", role: "builder" };
        assert!(!f.engine.observe_grant_call(&who, "run_command", &payload).await.unwrap());
        let ask = f.engine.ask_as(&who, ApprovalTier::Red, "run_command", "run_command", "run", &payload);
        let answer = async {
            let card = wait_for_card(&f).await;
            f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "no".into() }, "headless").unwrap();
        };
        let _ = tokio::join!(ask, answer);
        for k in [WOULD_USE, REFUSED_USE, NO_MATCH] {
            assert!(events_of(&f, k).is_empty(), "{k}");
        }
        assert_eq!(g.used(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn write_tools_are_absent_from_the_offer_and_denied_at_call_time_in_a_granted_run() {
        let g = fixture(3);
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-new.md", "content": "# New\nbody"})), Step::text("done")], 5.0, |_| {});
        f.engine.grant.set(Arc::new(g.grant())).unwrap();
        let out = f.engine.run_task(&f.m, "architect", "Draft a spec").await.unwrap();
        assert_eq!(out.status, crate::engine::Status::Done);
        let reqs = f.model.requests();
        let offered: Vec<String> = reqs[0].body["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert!(!offered.is_empty() && offered.iter().all(|n| tool_stripped(n, false).is_none()), "no writer is offered: {offered:?}");
        assert!(offered.iter().any(|n| n == "read_file"), "reading is still offered");
        // The model called it anyway: the call is denied by a code check, and no card was raised for it.
        let results: Vec<String> = events_of(&f, "tool.result").iter().map(|e| e.payload["content"].to_string()).collect();
        assert!(results.iter().any(|r| r.contains("denied") && r.contains("operator authorization")), "{results:?}");
        assert!(events_of(&f, "approval.requested").is_empty());
        assert!(!f.root.join("workspaces/w/team/specs/009-new.md").exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn without_a_grant_the_same_run_offers_and_asks_for_write_file_as_before() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-new.md", "content": "# New\nbody"})), Step::text("done")], 5.0, |_| {});
        let task = {
            let (e, m) = (f.engine.clone(), f.m.clone());
            tokio::spawn(async move { e.run_task(&m, "architect", "Draft a spec").await })
        };
        let card = wait_for_card(&f).await;
        assert_eq!(card.tool, "write_file");
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "no".into() }, "c").unwrap();
        task.await.unwrap().unwrap();
        let offered: Vec<String> = f.model.requests()[0].body["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert!(offered.iter().any(|n| n == "write_file"));
    }
}

// ---------------------------------------------------------------------------------------------- observation never burns the real record

#[test]
fn dry_would_uses_leave_the_real_ledger_and_the_launch_check_unchanged() {
    let f = fixture(3);
    let g = f.grant();
    let before = a::authorize_launch(&f.home, &f.id, "run-1", now(), &f.policy).unwrap();
    for i in 1..=3u32 {
        assert!(matches!(card(&g, "run_command", &f.argv()), Outcome::WouldUse { index, .. } if index == i));
    }
    // The dry ledger is bounded by max_uses too (it shows when an enabled run would have been refused) ...
    assert!(matches!(card(&g, "run_command", &f.argv()), Outcome::Refused { reason, .. } if reason.contains("dry ledger")));
    assert_eq!(f.dry(), 3);
    // ... and the real ledger does not exist, its count is 0 and the S2a launch check (which reads the real ledger) still passes.
    assert_eq!(f.used(), 0);
    assert!(!ledger_dir(&f.home, &f.id).exists(), "the real ledger was never created");
    let after = a::authorize_launch(&f.home, &f.id, "run-1", now(), &f.policy).unwrap();
    assert_eq!(before, after);
    assert!(delphi_core::log::verify_chain(&dry_ledger_dir(&f.home, &f.id)).unwrap().broken.is_none());
    // A real use (S2d's path) still works and still counts from zero.
    assert_eq!(take_use(&f.home, &f.id, "h", "run-1", 3).unwrap().index, 1);
}

#[test]
fn the_dry_and_the_real_ledger_are_separate_directories() {
    let f = fixture(3);
    assert_ne!(ledger_dir(&f.home, &f.id), dry_ledger_dir(&f.home, &f.id));
    assert!(dry_ledger_dir(&f.home, &f.id).to_string_lossy().ends_with(".dry"));
    take_dry_use(&f.home, &f.id, "h", "run-1", 3).unwrap();
    assert_eq!((f.dry(), f.used()), (1, 0));
}

// ---------------------------------------------------------------------------------------------- a claude seat with bridge tools only

mod claude_seat {
    use super::*;
    use crate::harness::tests::fake_cli;
    use crate::testkit::{events_of, setup_with_exec};

    const FINISH: &str = r#"echo '{"type":"result","subtype":"success","result":"All done.","stop_reason":"end_turn","total_cost_usd":0.03,"usage":{"input_tokens":10,"cache_creation_input_tokens":200,"cache_read_input_tokens":5000,"output_tokens":40}}'"#;

    /// A stand-in `claude` that lists `tools`, writes its arguments to `args`, then calls Delphi's bridge `run_command` over the socket
    /// the MCP config names (as the real CLI's MCP relay would) and finishes.
    fn script(dir: &Path, tools: &str, args_file: &Path, call: Option<&[String]>) -> String {
        let init = format!(r#"echo '{{"type":"system","subtype":"init","session_id":"s1","tools":[{tools}],"mcp_servers":[{{"name":"delphi","status":"connected"}}]}}'"#);
        let call = call.map(|argv| {
            let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "run_command", "arguments": {"argv": argv}}}).to_string();
            format!(
                r#"cfg=""; prev=""; for a in "$@"; do [ "$prev" = "--mcp-config" ] && cfg="$a"; prev="$a"; done
sock=$(tr ',{{}}' '\n\n\n' < "$cfg" | sed -n 's/.*"DELPHI_BRIDGE_SOCKET":"\([^"]*\)".*/\1/p')
tok=$(tr ',{{}}' '\n\n\n' < "$cfg" | sed -n 's/.*"DELPHI_BRIDGE_TOKEN":"\([^"]*\)".*/\1/p')
printf '{{"token":"%s"}}\n%s\n' "$tok" '{req}' | /usr/bin/nc -U -w 3 "$sock" > '{out}'"#,
                req = req,
                out = args_file.with_extension("resp").display()
            )
        });
        let body = format!("printf '%s\\n' \"$@\" > '{}'\n{}\n{}\n{}", args_file.display(), init, call.unwrap_or_default(), FINISH);
        fake_cli(dir, &body)
    }

    fn fx(prog: String) -> crate::testkit::Fx {
        let f = setup_with_exec(vec![], 5.0, |s| s.harness_program = prog, |c| c.sandbox = crate::exec::SandboxMode::Off);
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        crate::testkit::ratify_spec(&f);
        let text = crate::testkit::CONFIG
            .replace("[access]", "[[endpoints]]\nid = \"claude-sub\"\nfamily = \"claude\"\npath = \"harness\"\nharness = \"claude\"\ncredential = \"harness-login\"\ncost_model = \"subscription\"\n[access]")
            .replace("order = { claude = [\"direct\", \"backup\"] }", "order = { claude = [\"claude-sub\"] }")
            .replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"claude-sub\"]");
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
        f
    }

    const PROMPT: &str = "Implement @specs/011-retry.md in @project/app/src/lib.rs";

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_claude_builder_seat_with_only_bridge_tools_raises_a_run_command_that_matches_and_is_logged_as_would_use_and_still_denied() {
        let g = fixture(3);
        let scripts = tempfile::tempdir().unwrap();
        let args = scripts.path().join("args");
        let prog = script(scripts.path(), r#""mcp__delphi__run_command""#, &args, Some(&g.argv()));
        let f = fx(prog);
        f.engine.grant.set(Arc::new(g.grant())).unwrap();
        let out = f.engine.run_task(&f.m, "builder", PROMPT).await.unwrap();
        let _ = out;
        // The seat started with none of the CLI's own tools: `--tools ""`, and Delphi's `run_command` is offered over the bridge.
        let argv: Vec<String> = fs::read_to_string(&args).unwrap().lines().map(String::from).collect();
        let at = argv.iter().position(|a| a == "--tools").expect("--tools is passed");
        assert_eq!(argv[at + 1], "", "an empty tool list: no native tool");
        let started = &events_of(&f, "harness.started")[0].payload;
        assert!(started["bridge"].as_array().unwrap().iter().any(|v| v == "run_command"), "{started}");
        assert_eq!(started["native_tools"], json!([]));
        assert_eq!(started["claudeai_connectors"], false);
        // The call reached `exec_command` as a run_command that the grant matched: logged, never run.
        let would = events_of(&f, WOULD_USE);
        assert_eq!(would.len(), 1, "{:?}", events_of(&f, REFUSED_USE).iter().map(|e| e.payload.to_string()).collect::<Vec<_>>());
        assert_eq!((would[0].payload["approved"].as_bool(), would[0].payload["index"].as_u64()), (Some(false), Some(1)));
        let resp = fs::read_to_string(args.with_extension("resp")).unwrap_or_default();
        assert!(resp.contains("denied") && resp.contains("not enabled"), "the model is told it was denied: {resp}");
        assert!(!g.root.join("RAN").exists() && events_of(&f, "action.intent").is_empty(), "nothing ran");
        assert_eq!((g.dry(), g.used()), (1, 0));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cli_that_lists_a_tool_of_its_own_in_a_granted_run_is_stopped_before_the_model_acts() {
        let g = fixture(3);
        let scripts = tempfile::tempdir().unwrap();
        let prog = script(scripts.path(), r#""Bash","mcp__delphi__run_command""#, &scripts.path().join("args"), Some(&g.argv()));
        let f = fx(prog);
        f.engine.grant.set(Arc::new(g.grant())).unwrap();
        let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
        let refused = events_of(&f, REFUSED_USE);
        assert!(refused.iter().any(|e| e.payload["native_tool"] == "Bash"), "{:?}", refused.iter().map(|e| e.payload.to_string()).collect::<Vec<_>>());
        assert!(events_of(&f, WOULD_USE).is_empty());
        assert_eq!(g.dry(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_native_tool_call_seen_in_the_stream_fails_the_granted_run() {
        let g = fixture(3);
        let scripts = tempfile::tempdir().unwrap();
        let body = format!(
            "{}\necho '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"tool_use\",\"id\":\"tu1\",\"name\":\"Bash\",\"input\":{{\"command\":\"touch made.txt\"}}}}]}}}}'\nsleep 1\n{}",
            r#"echo '{"type":"system","subtype":"init","session_id":"s1"}'"#, FINISH
        );
        let prog = fake_cli(scripts.path(), &body);
        let f = fx(prog);
        f.engine.grant.set(Arc::new(g.grant())).unwrap();
        let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
        let refused = events_of(&f, REFUSED_USE);
        assert!(refused.iter().any(|e| e.payload["native_tool"] == "Bash" && e.payload["reason"].as_str().unwrap().contains("stream")), "{:?}", refused.iter().map(|e| e.payload.to_string()).collect::<Vec<_>>());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn without_a_grant_a_claude_builder_keeps_its_own_tools_and_no_run_command_over_the_bridge() {
        let scripts = tempfile::tempdir().unwrap();
        let args = scripts.path().join("args");
        let prog = script(scripts.path(), "", &args, None);
        let f = fx(prog);
        let _ = f.engine.run_task(&f.m, "builder", PROMPT).await;
        let started = &events_of(&f, "harness.started")[0].payload;
        assert_eq!(started["tools"], "standard");
        assert!(!started["bridge"].as_array().unwrap().iter().any(|v| v == "run_command"));
        let argv = fs::read_to_string(&args).unwrap();
        assert!(!argv.lines().any(|l| l == "--tools"), "{argv}");
    }
}

mod engine_cards {
    use super::*;
    use crate::approval::{Answer, Tier as ApprovalTier};
    use crate::engine::Asker;
    use crate::testkit::{events_of, setup};

    // A Red card of any other kind that reaches the card path is shown to the grant too, and is never matched.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_red_card_of_another_kind_is_logged_as_no_match_and_asked_as_before() {
        let g = fixture(3);
        let f = setup(vec![], 5.0, |_| {});
        f.engine.grant.set(Arc::new(g.grant())).unwrap();
        let payload = json!({"tool": "harness_bash", "command": "x", "argv": g.argv()});
        let who = Asker { m: &f.m, task_id: "t1", span_id: "s1", role: "builder" };
        let ask = f.engine.ask_as(&who, ApprovalTier::Red, "harness_bash", "harness_bash", "run x", &payload);
        let answer = async {
            for _ in 0..300 {
                if let Some(c) = f.engine.broker.pending().into_iter().next() {
                    f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Deny { reason: "no".into() }, "headless").unwrap();
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            panic!("no card");
        };
        let (denied, _) = tokio::join!(ask, answer);
        assert!(denied.unwrap().is_some());
        assert_eq!(events_of(&f, NO_MATCH).len(), 1);
        assert!(events_of(&f, WOULD_USE).is_empty());
        assert_eq!((g.dry(), g.used()), (0, 0));
    }
}
