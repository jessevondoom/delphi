use super::*;
use std::os::unix::fs::PermissionsExt;

thread_local! {
    static POLICY: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The policy of this test: the fixture's fake runner is the only interpreter the "daemon" knows.
fn policy() -> Policy {
    Policy { interpreters: POLICY.with(|p| p.borrow().clone()), sealed_roots: vec![] }
}

/// A tempdir with a fake runner named `bash`, a script, and a home with an operator lock, all canonical.
struct Fix {
    _t: tempfile::TempDir,
    home: PathBuf,
    bin: String,
    script: String,
}

fn fix() -> Fix {
    let t = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(t.path()).unwrap();
    let home = root.join("home");
    fs::create_dir_all(home.join("operator")).unwrap();
    lock::write(&home.join("operator/ratified.lock"), &LockFile::default()).unwrap();
    let bin = root.join("bash");
    fs::write(&bin, b"#!fake runner").unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    let script = root.join("t.sh");
    fs::write(&script, b"print(1)").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).unwrap();
    POLICY.with(|p| *p.borrow_mut() = vec![bin.to_string_lossy().into()]);
    Fix { home, bin: bin.to_string_lossy().into(), script: script.to_string_lossy().into(), _t: t }
}

impl Fix {
    fn entry(&self) -> Entry {
        Entry { tool: "shell".into(), argv: vec![self.bin.clone(), self.script.clone()], files: vec![self.script.clone()], runner_provenance: "project_snapshot".into() }
    }
    fn request_json(&self) -> Value {
        json!({"intent": "run the parser tests", "purpose": "needs a Red shell", "manifest": [self.entry()], "context": "I was reading the parser.", "risk": "runs a script in the sandbox", "run_plan": "once"})
    }
    fn new_record(&self, entries: Vec<Entry>) -> NewRecord {
        NewRecord { purpose: "parser tests".into(), intent: "tests".into(), run: "run-1".into(), expires_in_secs: DEFAULT_EXPIRY_SECS, max_uses: DEFAULT_USES, entries, request: None, policy: policy() }
    }
}

fn at(secs: i64) -> String {
    delphi_core::time::format_unix_millis(1_800_000_000 + secs, 0)
}

fn ev(kind: &str, payload: Value, secs: i64) -> EventEnvelope {
    let mut e = EventEnvelope::new("m", kind, payload);
    e.timestamp = at(secs);
    e
}

fn who(kind: Kind, taint: Tier, run: &str) -> Who {
    Who { kind: kind.clone(), label: "red-team/claude/opus".into(), role: Some("red-team".into()), model: "claude/opus".into(), span: Some("s1".into()), task: Some(run.into()), taint, run: run.into(), mission: "m".into() }
}

fn admit_at(log: &[EventEnvelope], op: &[EventEnvelope], w: &Who, req: &Value, secs: i64) -> Outcome {
    let now = at(secs);
    admit(log, op, w, parse_request(req, &policy()), &Clock { now_unix: 1_800_000_000 + secs, now_rfc: &now })
}

fn staged(o: Outcome) -> Value {
    match o {
        Outcome::Staged(p) => p,
        Outcome::Refused { message, .. } => panic!("expected staged, refused: {message}"),
    }
}

fn refusal(o: Outcome) -> (String, Option<Value>) {
    match o {
        Outcome::Refused { message, log } => (message, log),
        Outcome::Staged(_) => panic!("expected a refusal"),
    }
}

/// The request with a different argument, so its manifest hash differs.
fn variant(f: &Fix, n: usize) -> Value {
    let mut v = f.request_json();
    v["manifest"][0]["argv"] = json!([f.bin, f.script, format!("--case={n}")]);
    v
}

// ---------------------------------------------------------------------------------------------- hygiene and entries

#[test]
fn control_and_escape_characters_are_stripped_and_text_is_cut() {
    let evil = "ok\x1b[2K\x1b[1Ahidden\u{202e}evil\u{200b}\r\nnext\x07 line";
    let c = clean_text(evil, 1000);
    assert!(!c.chars().any(|ch| ch.is_control() || ch == '\u{202e}' || ch == '\u{200b}'), "{c:?}");
    assert!(c.starts_with("ok") && c.contains("next"));
    assert_eq!(clean_text(&"a".repeat(5000), MAX_CONTEXT).chars().count(), MAX_CONTEXT);
    assert_eq!(strip_control("a\tb\nc"), "a b c");
}

#[test]
fn an_entry_is_exact_or_it_is_refused() {
    let f = fix();
    assert!(check_entry(&f.entry(), &policy()).is_ok());
    let bad = |mutate: &dyn Fn(&mut Entry)| {
        let mut e = f.entry();
        mutate(&mut e);
        check_entry(&e, &policy())
    };
    for (what, r) in [
        ("relative binary", bad(&|e| e.argv[0] = "bash".into())),
        ("dot-dot", bad(&|e| e.argv[0] = format!("{}/../bash", e.argv[0]))),
        ("glob", bad(&|e| e.argv.push("tests/*.py".into()))),
        ("substitution", bad(&|e| e.argv.push("$(id)".into()))),
        ("chaining", bad(&|e| e.argv.push("&&".into()))),
        ("inline code", bad(&|e| e.argv.insert(1, "-c".into()))),
        ("control char", bad(&|e| e.argv.push("a\x1bb".into()))),
        ("unknown provenance", bad(&|e| e.runner_provenance = "built_in_run".into())),
        ("unknown tool", bad(&|e| e.tool = "edit_file".into())),
        ("no script", bad(&|e| drop(e.argv.remove(1)))),
        ("script not pinned", bad(&|e| e.files.clear())),
        ("not a runner with assertable isolation", bad(&|e| e.argv[0] = "/usr/bin/awk".into())),
        ("shell wrapper", bad(&|e| e.argv[0] = "/bin/sh".into())),
        ("env wrapper", bad(&|e| e.argv[0] = "/usr/bin/env".into())),
    ] {
        assert!(matches!(r, Err(EntryError::Invalid(_))), "{what}: {r:?}");
    }
    // The canonical tool name is used whatever alias came in.
    let mut alias = f.entry();
    alias.tool = "Run_Command".into();
    assert_eq!(check_entry(&alias, &policy()).unwrap().tool, "shell");
}

#[test]
fn the_deny_list_stops_merges_credentials_network_dispatch_and_protected_paths() {
    let f = fix();
    let denied = |argv: Vec<&str>| {
        let e = Entry { tool: "shell".into(), argv: argv.into_iter().map(String::from).collect(), files: vec![], runner_provenance: "project_snapshot".into() };
        matches!(check_entry(&e, &policy()), Err(EntryError::Denied(_)))
    };
    for bin in ["/usr/bin/git", "/usr/local/bin/gh", "/usr/bin/curl", "/usr/bin/ssh", "/usr/local/bin/delphi", "/usr/local/bin/claude", "/usr/bin/security", "/bin/rm", "/usr/bin/cargo", "/usr/bin/npm"] {
        assert!(denied(vec![bin, "x"]), "{bin}");
    }
    for arg in ["/repo/.git/hooks/pre-commit", "--out=/repo/.git/config", "/srv/u/.delphi/operator/authorizations.json", "/srv/u/.delphi/operator/ratified.lock", "/repo/.githooks/x", "/repo/config.toml", "/srv/u/.ssh/id_rsa", "https://example.com/x", "git@github.com:a/b.git", "/repo/server.pem"] {
        assert!(denied(vec![&f.bin, &f.script, arg]), "{arg}");
    }
    assert!(deny_reason(&f.entry()).is_none());
}

// Spec 092 S2b: the runner is `/bin/bash <pinned script file>`. No option may make bash run anything but that file: not a string
// (`-c`), not stdin (`-s`), not an interactive shell (`-i`), not a start-up file (`--rcfile`, `--init-file`), and the environment
// that would name one (`BASH_ENV`, `ENV`) is not in the scrubbed map.
#[test]
fn no_bash_option_can_run_code_that_is_not_the_pinned_script() {
    let f = fix();
    for opt in ["-c", "-s", "-i", "-l", "-O", "-o", "-x", "--login", "--rcfile", "--init-file", "--norc", "--noprofile", "--posix", "--rcfile=/tmp/x", "--init-file=/tmp/x", "-ic", "-sc", "-xe", "-lc"] {
        // before the script: not an absolute script path, so refused
        let mut e = f.entry();
        e.argv = vec![f.bin.clone(), opt.into(), f.script.clone()];
        assert!(check_entry(&e, &policy()).is_err(), "{opt} before the script was admitted");
        // and after it, where it would be the script's own argument, still refused so no record reads as if it used one
        let mut e = f.entry();
        e.argv = vec![f.bin.clone(), f.script.clone(), opt.into()];
        assert!(check_entry(&e, &policy()).is_err(), "{opt} after the script was admitted");
    }
    // `-c` with a string, `-s` with args, a script on stdin
    for argv in [vec![f.bin.clone(), "-c".into(), "echo hi".into()], vec![f.bin.clone(), "-s".into()], vec![f.bin.clone()], vec![f.bin.clone(), "-".into()], vec![f.bin.clone(), "t.sh".into()]] {
        let mut e = f.entry();
        e.argv = argv.clone();
        e.files = vec![];
        assert!(check_entry(&e, &policy()).is_err(), "{argv:?}");
    }
    // the script must be pinned in `files`
    let mut e = f.entry();
    e.files = vec![];
    assert!(matches!(check_entry(&e, &policy()), Err(EntryError::Invalid(m)) if m.contains("files")));
    // a long option that is not a start-up file is the script's own argument and passes
    let mut e = f.entry();
    e.argv.push("--version".into());
    assert!(check_entry(&e, &policy()).is_ok());
    // the environment carries no interpreter hook
    let env = scrubbed_env();
    for k in ["BASH_ENV", "ENV", "PERL5OPT", "PERL5LIB", "PYTHONPATH", "HOME", "LD_PRELOAD"] {
        assert!(!env.contains_key(k), "{k}");
    }
    assert!(!env.keys().any(|k| k.starts_with("DYLD_")));
}

#[test]
fn python_and_other_interpreters_are_no_longer_admitted_and_bash_is() {
    let f = fix();
    let sys = Policy::system();
    assert_eq!(sys.interpreters, vec!["/bin/bash".to_string()]);
    for py in ["/usr/bin/python3", "/usr/local/bin/python3", "/opt/homebrew/bin/python3"] {
        let mut e = f.entry();
        e.argv = vec![py.into(), "-I".into(), f.script.clone()];
        assert!(matches!(check_entry(&e, &sys), Err(EntryError::Invalid(m)) if m.contains("not an interpreter")), "{py}");
    }
    for other in ["/usr/bin/perl", "/bin/zsh", "/bin/sh", "/bin/dash", "/usr/bin/ruby", "/usr/bin/env"] {
        let mut e = f.entry();
        e.argv[0] = other.into();
        assert!(check_entry(&e, &sys).is_err(), "{other}");
    }
    // the real thing: `/bin/bash <script>`, with the script pinned
    let mut e = f.entry();
    e.argv[0] = "/bin/bash".into();
    assert!(check_entry(&e, &sys).is_ok(), "{:?}", check_entry(&e, &sys));
    // other shells in the wrapper list stay refused even when named as the runner of a fixture
    let mut e = f.entry();
    e.argv[0] = "/bin/sh".into();
    assert!(check_entry(&e, &policy()).is_err());
}

#[test]
fn the_system_bash_is_pinned_resolved_and_hashed_and_the_hash_is_taken_again() {
    let f = fix();
    let sys = Policy::system();
    let pinned = pin_interpreter("/bin/bash", &sys).unwrap();
    assert_eq!(pinned.path, "/bin/bash");
    assert_eq!(pinned.blake3, hash(&fs::read("/bin/bash").unwrap()));
    // a record made with the system policy stores that hash
    let mut n = f.new_record(vec![{
        let mut e = f.entry();
        e.argv[0] = "/bin/bash".into();
        e
    }]);
    n.policy = sys.clone();
    let r = build_record(&f.home, &n, 1_800_000_000, &at(0)).unwrap();
    assert_eq!(r.manifest[0].binary, pinned);
    assert_eq!(r.manifest[0].env, scrubbed_env());
    // taken again at use: a different file at the same path is a different hash
    let again = pin_interpreter("/bin/bash", &sys).unwrap();
    assert_eq!(again.blake3, r.manifest[0].binary.blake3);
    let swapped = Policy { interpreters: vec![f.bin.clone()], sealed_roots: vec![] };
    let other = pin_interpreter(&f.bin, &swapped).unwrap();
    assert_ne!(other.blake3, r.manifest[0].binary.blake3, "a replaced interpreter must show as another hash");
    fs::write(&f.bin, b"#!replaced").unwrap();
    assert_ne!(pin_interpreter(&f.bin, &swapped).unwrap().blake3, other.blake3);
}

#[test]
fn an_interpreter_off_the_sealed_volume_is_refused_even_if_it_is_named_bash() {
    let f = fix();
    // the fixture's `bash` lives in a temp directory: the system policy's roots do not cover it, and a symlink from a sealed path
    // to elsewhere would resolve outside them
    let strict = Policy { interpreters: vec![f.bin.clone()], sealed_roots: vec!["/bin".into(), "/usr/bin".into()] };
    let e = pin_interpreter(&f.bin, &strict).unwrap_err();
    assert!(e.contains("sealed system volume"), "{e}");
    let mut n = f.new_record(vec![f.entry()]);
    n.policy = strict;
    assert!(build_record(&f.home, &n, 1_800_000_000, &at(0)).unwrap_err().contains("sealed"));
    // a directory is not an interpreter
    assert!(pin_interpreter("/bin", &Policy::system()).is_err());
}

// Red-team P1: the runner must be an interpreter the daemon knows, whatever the request says about its provenance.
#[test]
fn a_project_local_or_self_declared_runner_is_not_an_interpreter() {
    let f = fix();
    let other = fs::canonicalize(f._t.path()).unwrap().join("tools");
    fs::create_dir_all(&other).unwrap();
    let local = other.join("bash");
    fs::write(&local, b"#!built in the run").unwrap();
    fs::set_permissions(&local, fs::Permissions::from_mode(0o755)).unwrap();
    for prov in ["project_snapshot", "daemon_installed"] {
        let mut e = f.entry();
        e.argv[0] = local.to_string_lossy().into();
        e.runner_provenance = prov.into();
        let r = check_entry(&e, &policy());
        // a `bash` that is not the pinned one is refused whether it is seen as a wrapper or as an unknown interpreter
        assert!(matches!(&r, Err(EntryError::Invalid(m)) if m.contains("not an interpreter the daemon knows") || m.contains("shell or wrapper")), "{prov}: {r:?}");
    }
    // the shipped policy knows system locations only: the fixture's runner is not one of them
    assert!(check_entry(&f.entry(), &Policy::system()).is_err());
    for p in &Policy::system().interpreters {
        assert_eq!(p, "/bin/bash", "the only admitted runner is the system bash");
    }
    // non-ASCII (confusables) is refused in argv and files
    let mut e = f.entry();
    e.argv.push("tеst".into());
    assert!(matches!(check_entry(&e, &policy()), Err(EntryError::Invalid(m)) if m.contains("non-ASCII")));
    let mut e = f.entry();
    e.files.push("/tmp/\u{0435}.py".into());
    assert!(check_entry(&e, &policy()).is_err());
}

#[test]
fn the_strip_list_covers_invisible_and_filler_characters() {
    for c in ['\u{2800}', '\u{115f}', '\u{1160}', '\u{17b4}', '\u{17b5}', '\u{180e}', '\u{2060}', '\u{2064}', '\u{206a}', '\u{206f}', '\u{061c}', '\u{00ad}', '\u{034f}', '\u{3164}', '\u{ffa0}', '\u{202e}', '\u{200b}', '\u{feff}', '\u{e0041}'] {
        assert_eq!(strip_control(&format!("a{c}b")), "ab", "U+{:04X}", c as u32);
        assert_eq!(clean_text(&format!("a{c}b"), 10), "ab");
    }
}

#[test]
fn argv_is_rendered_canonically_and_quoted_only_when_needed() {
    assert_eq!(render_argv(&["/usr/bin/python3".into(), "-I".into(), "/t/a b.py".into(), "it's".into(), "".into()]), "/usr/bin/python3 -I '/t/a b.py' 'it'\\''s' ''");
}

// ---------------------------------------------------------------------------------------------- parsing

#[test]
fn a_request_is_parsed_cleaned_and_bounded() {
    let f = fix();
    let mut v = f.request_json();
    v["context"] = json!(format!("seen\x1b[31m{}", "x".repeat(5000)));
    let r = parse_request(&v, &policy()).unwrap();
    assert_eq!(r.context.chars().count(), MAX_CONTEXT);
    assert!(!r.context.contains('\x1b'));
    for k in ["intent", "purpose", "context", "risk", "run_plan", "manifest"] {
        let mut w = f.request_json();
        w.as_object_mut().unwrap().remove(k);
        assert!(matches!(parse_request(&w, &policy()), Err(ParseError::Invalid(_))), "{k} is required");
    }
    // 5 entries at most
    let mut w = f.request_json();
    w["manifest"] = json!((0..6).map(|n| json!({"tool": "shell", "argv": [f.bin, f.script, format!("{n}")], "files": [f.script], "runner_provenance": "project_snapshot"})).collect::<Vec<_>>());
    assert!(matches!(parse_request(&w, &policy()), Err(ParseError::Invalid(m)) if m.contains("1 to 5")));
    w["manifest"].as_array_mut().unwrap().pop();
    assert!(parse_request(&w, &policy()).is_ok());
    // unknown fields on an entry (an agent cannot hand in a binary hash, an env or a cwd)
    let mut w = f.request_json();
    w["manifest"][0]["env"] = json!({"PATH": "/tmp"});
    assert!(matches!(parse_request(&w, &policy()), Err(ParseError::Invalid(_))));
    // the same action twice
    let mut w = f.request_json();
    w["manifest"] = json!([f.entry(), f.entry()]);
    assert!(matches!(parse_request(&w, &policy()), Err(ParseError::Invalid(m)) if m.contains("twice")));
    // a denied entry carries the hash of the manifest as asked for
    let mut w = f.request_json();
    w["manifest"][0]["argv"] = json!(["/usr/bin/git", "push"]);
    assert!(matches!(parse_request(&w, &policy()), Err(ParseError::Denied { .. })));
}

// ---------------------------------------------------------------------------------------------- staging rules

#[test]
fn a_request_from_an_agent_span_at_federated_peer_or_lower_is_refused_at_staging() {
    let f = fix();
    for tier in [Tier::L0, Tier::L1] {
        let (msg, log) = refusal(admit_at(&[], &[], &who(Kind::Agent, tier, "t1"), &f.request_json(), 0));
        assert!(msg.contains("cannot exist"), "{msg}");
        let log = log.expect("a taint refusal is recorded");
        assert_eq!(log["blocks_restage"], false);
        assert_eq!(log["taint"], tier.name());
    }
    // internal_generated (above the gate) stages, labelled with the daemon's taint, inert at L0
    let p = staged(admit_at(&[], &[], &who(Kind::Agent, Tier::L3, "t1"), &f.request_json(), 0));
    assert_eq!(p["tier"], "L0");
    assert_eq!(p["taint"]["name"], "internal_generated");
    assert_eq!(p["requester"]["role"], "red-team");
    assert_eq!(p["requester"]["span"], "s1");
    assert_eq!(p["state"], "pending");
}

#[test]
fn a_host_request_is_staged_inert_at_a_fixed_federated_peer_taint() {
    let f = fix();
    let mut w = who(Kind::Host, HOST_TAINT, "sess0001");
    w.label = "host:claude-code/opus".into();
    w.role = None;
    w.span = None;
    let p = staged(admit_at(&[], &[], &w, &f.request_json(), 0));
    assert_eq!(p["taint"]["name"], "federated_peer");
    assert_eq!(p["tier"], "L0");
    assert_eq!(p["requester"]["kind"], "host");
    assert_eq!(p["requester"]["label"], "host:claude-code/opus");
    let s = Staged::from_event("host-sess0001", &ev(STAGED, p, 0)).unwrap();
    assert!(s.is_host() && s.needs_retype());
}

#[test]
fn the_rate_limit_is_one_request_per_five_minutes_per_run() {
    let f = fix();
    let w = who(Kind::Agent, Tier::L4, "t1");
    let p = staged(admit_at(&[], &[], &w, &variant(&f, 1), 0));
    let log = vec![ev(STAGED, p, 0)];
    let (msg, l) = refusal(admit_at(&log, &[], &w, &variant(&f, 2), 120));
    assert!(msg.contains("rate limit"), "{msg}");
    assert!(l.is_none(), "a rate refusal does not fill the log");
    // another run is not held up by it
    assert!(matches!(admit_at(&log, &[], &who(Kind::Agent, Tier::L4, "t2"), &variant(&f, 2), 120), Outcome::Staged(_)));
    // five minutes later it is allowed
    assert!(matches!(admit_at(&log, &[], &w, &variant(&f, 2), 301), Outcome::Staged(_)));
}

#[test]
fn pending_requests_are_capped_per_run_per_session_and_per_mission() {
    let f = fix();
    // 3 per run
    let w = who(Kind::Agent, Tier::L4, "t1");
    let mut log = vec![];
    for n in 0..3 {
        let p = staged(admit_at(&log, &[], &w, &variant(&f, n), n as i64 * 400));
        log.push(ev(STAGED, p, n as i64 * 400));
    }
    let (msg, _) = refusal(admit_at(&log, &[], &w, &variant(&f, 9), 3 * 400));
    assert!(msg.contains("already pending"), "{msg}");
    // a fourth host-session request is refused likewise
    let h = who(Kind::Host, HOST_TAINT, "sess0001");
    let mut hlog = vec![];
    for n in 0..3 {
        let p = staged(admit_at(&hlog, &[], &h, &variant(&f, n), n as i64 * 400));
        hlog.push(ev(STAGED, p, n as i64 * 400));
    }
    assert!(refusal(admit_at(&hlog, &[], &h, &variant(&f, 9), 3 * 400)).0.contains("host session"));
    // 5 per mission, across runs
    let mut mlog = vec![];
    for n in 0..5 {
        let p = staged(admit_at(&mlog, &[], &who(Kind::Agent, Tier::L4, &format!("t{n}")), &variant(&f, n), 0));
        mlog.push(ev(STAGED, p, 0));
    }
    let (msg, _) = refusal(admit_at(&mlog, &[], &who(Kind::Agent, Tier::L4, "t9"), &variant(&f, 9), 0));
    assert!(msg.contains("in this mission"), "{msg}");
    // an answered request frees its slot
    let id = mlog[0].payload["id"].as_str().unwrap().to_string();
    let answered = vec![ev(REFUSED, json!({"request_id": id, "source": "operator", "manifest_hash": "other", "blocks_restage": true}), 10)];
    assert!(matches!(admit_at(&mlog, &answered, &who(Kind::Agent, Tier::L4, "t9"), &variant(&f, 9), 20), Outcome::Staged(_)));
    // and a request left unanswered for seven days expires
    assert!(matches!(admit_at(&mlog, &[], &who(Kind::Agent, Tier::L4, "t9"), &variant(&f, 9), PENDING_TTL_SECS + 10), Outcome::Staged(_)));
}

#[test]
fn the_same_manifest_is_staged_once() {
    let f = fix();
    let p = staged(admit_at(&[], &[], &who(Kind::Agent, Tier::L4, "t1"), &f.request_json(), 0));
    let id = p["id"].as_str().unwrap().to_string();
    let log = vec![ev(STAGED, p, 0)];
    // different run, different words, same manifest
    let mut v = f.request_json();
    v["context"] = json!("something else entirely");
    let (msg, l) = refusal(admit_at(&log, &[], &who(Kind::Agent, Tier::L4, "t2"), &v, 10));
    assert!(msg.contains(&id) && l.is_none(), "{msg}");
}

#[test]
fn a_refused_manifest_is_not_staged_again_for_a_day() {
    let f = fix();
    let w = who(Kind::Agent, Tier::L4, "t1");
    let mh = manifest_hash(&parse_request(&f.request_json(), &policy()).unwrap().manifest);
    // refused by the operator (in the audit log)
    let op = vec![ev(REFUSED, json!({"request_id": "ar-x", "source": "operator", "manifest_hash": mh, "blocks_restage": true}), 0)];
    let (msg, _) = refusal(admit_at(&[], &op, &w, &f.request_json(), 3600));
    assert!(msg.contains("24 hours"), "{msg}");
    assert!(matches!(admit_at(&[], &op, &w, &f.request_json(), RESTAGE_SECS + 5), Outcome::Staged(_)));
    // refused by the deny-list (in the mission log): the same
    let mut denied = f.request_json();
    denied["manifest"][0]["argv"] = json!(["/usr/bin/git", "push"]);
    let (msg, log) = refusal(admit_at(&[], &[], &w, &denied, 0));
    assert!(msg.contains("deny-list"), "{msg}");
    let log = log.expect("recorded");
    assert_eq!(log["blocks_restage"], true);
    let mlog = vec![ev(REFUSED, log, 0)];
    let w2 = who(Kind::Agent, Tier::L4, "t2");
    assert!(refusal(admit_at(&mlog, &[], &w2, &denied, 3600)).0.contains("24 hours"), "a denied manifest is never staged, nor recorded again, however often it comes back");
    // a taint refusal does not block a later request from a clean span
    let (_, l) = refusal(admit_at(&[], &[], &who(Kind::Agent, Tier::L1, "t3"), &f.request_json(), 0));
    let tlog = vec![ev(REFUSED, l.unwrap(), 0)];
    assert!(matches!(admit_at(&tlog, &[], &who(Kind::Agent, Tier::L4, "t4"), &f.request_json(), 10), Outcome::Staged(_)));
}

#[test]
fn a_staged_event_that_disagrees_with_its_own_hashes_is_not_shown() {
    let f = fix();
    let mut p = staged(admit_at(&[], &[], &who(Kind::Agent, Tier::L4, "t1"), &f.request_json(), 0));
    assert!(Staged::from_event("m", &ev(STAGED, p.clone(), 0)).is_some());
    p["request"]["manifest"][0]["argv"] = json!([f.bin, f.script, "--extra"]);
    assert!(Staged::from_event("m", &ev(STAGED, p, 0)).is_none(), "edited after staging");
}

// ---------------------------------------------------------------------------------------------- the store

#[test]
fn a_record_is_made_hashed_and_pinned() {
    let f = fix();
    let r = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_000, &at(0)).unwrap();
    assert_eq!(r.manifest[0].binary.path, f.bin);
    assert_eq!(r.manifest[0].binary.blake3, hash(b"#!fake runner"));
    assert_eq!(r.manifest[0].files[0].blake3, hash(b"print(1)"));
    assert_eq!(r.manifest[0].scratch_mode, "noexec");
    assert_eq!(r.manifest[0].cwd, CWD);
    assert_eq!(r.manifest[0].env.get("PATH").map(String::as_str), Some("/nonexistent"), "no PATH search");
    assert!(r.manifest[0].env.keys().all(|k| !k.starts_with("DYLD_") && k != "LD_PRELOAD"));
    assert_eq!(r.run, "run-1");
    assert_eq!(r.uses, 0);
    add_record(&f.home, r.clone(), None).unwrap();
    let s = load(&f.home).unwrap();
    assert_eq!(s.records, vec![r]);
    let mode = fs::metadata(store_path(&f.home)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(lock::read(&f.home.join("operator/ratified.lock")).unwrap().files.get(LOCK_KEY), Some(&hash(&fs::read(store_path(&f.home)).unwrap())));
}

#[test]
fn a_record_is_refused_outside_its_limits_and_when_the_deny_list_or_the_files_say_no() {
    let f = fix();
    let now = 1_800_000_000;
    let make = |n: NewRecord| build_record(&f.home, &n, now, &at(0));
    let mut n = f.new_record(vec![f.entry()]);
    n.run = "".into();
    assert!(make(n).unwrap_err().contains("--run"), "the run is required");
    let mut n = f.new_record(vec![f.entry()]);
    n.expires_in_secs = MAX_EXPIRY_SECS + 1;
    assert!(make(n).is_err());
    let mut n = f.new_record(vec![f.entry()]);
    n.max_uses = MAX_USES + 1;
    assert!(make(n).is_err());
    let mut n = f.new_record(vec![f.entry()]);
    n.purpose = " \x1b ".into();
    assert!(make(n).unwrap_err().contains("purpose"));
    assert!(make(f.new_record(vec![])).is_err());
    // the deny-list is enforced when the record is MADE, for an entry the operator typed too
    let mut bad = f.entry();
    bad.argv = vec!["/usr/bin/git".into(), "push".into()];
    assert!(make(f.new_record(vec![bad])).unwrap_err().contains("deny-list"));
    let mut bad = f.entry();
    bad.argv.push("--out=/repo/.git/hooks/x".into());
    assert!(make(f.new_record(vec![bad])).unwrap_err().contains("deny-list"));
    // a binary that does not exist, a world-writable script, a symlinked path and a scratch path
    let mut missing = f.entry();
    missing.argv[1] = format!("{}/none.py", f._t.path().display());
    missing.files = vec![missing.argv[1].clone()];
    assert!(make(f.new_record(vec![missing])).is_err());
    let ww = f._t.path().join("ww.py");
    fs::write(&ww, b"x").unwrap();
    fs::set_permissions(&ww, fs::Permissions::from_mode(0o666)).unwrap();
    let wwp = fs::canonicalize(&ww).unwrap().to_string_lossy().to_string();
    let mut e = f.entry();
    e.argv[1] = wwp.clone();
    e.files = vec![wwp];
    assert!(make(f.new_record(vec![e])).unwrap_err().contains("world-writable"));
    let link = fs::canonicalize(f._t.path()).unwrap().join("link.py");
    std::os::unix::fs::symlink(&f.script, &link).unwrap();
    let mut e = f.entry();
    e.argv[1] = link.to_string_lossy().into();
    e.files = vec![e.argv[1].clone()];
    assert!(make(f.new_record(vec![e])).unwrap_err().contains("not canonical"));
    let scratch = fs::canonicalize(f._t.path()).unwrap().join("scratch");
    fs::create_dir_all(&scratch).unwrap();
    let sp = scratch.join("s.py");
    fs::write(&sp, b"x").unwrap();
    let mut e = f.entry();
    e.argv[1] = sp.to_string_lossy().into();
    e.files = vec![e.argv[1].clone()];
    assert!(make(f.new_record(vec![e])).unwrap_err().contains("scratch"));
}

fn assert_ignored(f: &Fix, why: &str) {
    assert!(load(&f.home).is_err(), "{why}: load fails closed");
    assert!(load_or_ignore(&f.home).records.is_empty(), "{why}: nothing is read as a record");
}

fn forged_store(f: &Fix) -> Vec<u8> {
    let mut r = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_000, &at(0)).unwrap();
    r.id = "ta-forged0001".into();
    serde_json::to_vec_pretty(&Store { version: 1, records: vec![r], audit_head: None }).unwrap()
}

#[test]
fn forgery_an_agent_writable_path_cannot_create_a_valid_record() {
    let f = fix();
    let forged = forged_store(&f);
    // 1. a store written at the right path with no pin: ignored and logged
    write_private(&store_path(&f.home), &forged).unwrap();
    assert_ignored(&f, "no pin");
    fs::remove_file(store_path(&f.home)).unwrap();
    // 2. record files anywhere else are never read
    for p in ["operator/authorizations/ta-forged0001.json", "local/authorizations.json", "operator/ratified/authorizations.json", "authorizations.json", "workspaces/default/local/scratch/ta-forged0001.json", "workspaces/default/missions/m/scratch/authorizations.json"] {
        let path = f.home.join(p);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &forged).unwrap();
    }
    assert!(load(&f.home).unwrap().records.is_empty(), "no store, no records, whatever else is on disk");
    // 3. an honest record, then the store is replaced by an edited one
    let r = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_000, &at(0)).unwrap();
    add_record(&f.home, r, None).unwrap();
    assert_eq!(load(&f.home).unwrap().records.len(), 1);
    fs::write(store_path(&f.home), &forged).unwrap();
    assert_ignored(&f, "edited store");
    // 4. an attacker re-pins nothing: removing the pin also fails closed
    let honest = {
        fs::remove_file(store_path(&f.home)).unwrap();
        let mut l = lock::read(&f.home.join("operator/ratified.lock")).unwrap();
        l.files.remove(LOCK_KEY);
        lock::write(&f.home.join("operator/ratified.lock"), &l).unwrap();
        forged.clone()
    };
    write_private(&store_path(&f.home), &honest).unwrap();
    assert_ignored(&f, "pin removed");
    // 5. a malformed or missing lock fails closed
    fs::write(f.home.join("operator/ratified.lock"), "not json").unwrap();
    assert_ignored(&f, "malformed lock");
    fs::remove_file(f.home.join("operator/ratified.lock")).unwrap();
    assert_ignored(&f, "missing lock");
}

#[test]
fn writing_never_goes_over_a_store_that_does_not_verify() {
    let f = fix();
    let r1 = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_000, &at(0)).unwrap();
    add_record(&f.home, r1, None).unwrap();
    fs::write(store_path(&f.home), forged_store(&f)).unwrap();
    let r2 = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_001, &at(1)).unwrap();
    let err = add_record(&f.home, r2, None).unwrap_err();
    assert!(err.contains("does not match its pin"), "{err}");
    assert!(revoke(&f.home, "ta-forged0001", &at(2), None).is_err(), "nor is revoke a way to re-pin a tampered store");
}

#[test]
fn revocation_is_irreversible() {
    let f = fix();
    let r = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_000, &at(0)).unwrap();
    let id = r.id.clone();
    add_record(&f.home, r, None).unwrap();
    let r = revoke(&f.home, &id, &at(5), None).unwrap();
    assert_eq!(r.revoked.as_deref(), Some(at(5).as_str()));
    assert!(revoke(&f.home, &id, &at(6), None).unwrap_err().contains("already revoked"));
    let s = load(&f.home).unwrap();
    assert!(s.records[0].is_revoked());
    // the only way to a live grant is a NEW record
    let r2 = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_100, &at(100)).unwrap();
    assert_ne!(r2.id, id);
    add_record(&f.home, r2, None).unwrap();
    let s = load(&f.home).unwrap();
    assert!(s.records[0].is_revoked() && !s.records[1].is_revoked());
    assert!(revoke(&f.home, "ta-nonexistent", &at(7), None).is_err());
}

#[test]
fn the_review_diffs_against_earlier_grants_and_replays_recorded_cards_as_data() {
    let f = fix();
    let r = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_000, &at(0)).unwrap();
    add_record(&f.home, r.clone(), None).unwrap();
    let s = load(&f.home).unwrap();
    let same = grant_diff(&s, &[f.entry()]);
    assert!(same[0].contains("granted before") && same[0].contains(&r.id), "{same:?}");
    let mut other = f.entry();
    other.argv.push("--more".into());
    assert!(grant_diff(&s, &[other])[0].contains("new arguments"));
    assert!(grant_diff(&Store::default(), &[f.entry()]).iter().any(|l| l.contains("NEW")));
    let line = render_argv(&f.entry().argv);
    let cards = vec![
        json!({"tool": "run_command", "payload": {"command": line}}),
        json!({"tool": "shell", "payload": {"command": "\x1b[2Kecho hidden"}}),
        json!({"tool": "write_file", "payload": {"path": "x"}}),
    ];
    let out = dry_run(&cards, &[f.entry()]).join("\n");
    assert!(out.contains("2 recent") && out.contains("matched 1") && out.contains("1 card(s) match no entry"), "{out}");
    assert!(!out.contains('\x1b'));
    assert!(dry_run(&[], &[f.entry()])[0].contains("no recorded shell cards"));
}


// ---------------------------------------------------------------------------------------------- workspace-wide limits

fn real_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn host_who(session: &str) -> Who {
    Who { kind: Kind::Host, label: "host:claude-code/opus".into(), role: None, model: "opus".into(), span: None, task: None, taint: HOST_TAINT, run: session.into(), mission: format!("host-{session}") }
}

impl Fix {
    /// A workspace directory beside the home, with the `local/` the ledger lives in.
    fn ws(&self) -> PathBuf {
        let ws = self.home.join("workspaces/w");
        fs::create_dir_all(ws.join("local")).unwrap();
        ws
    }
    /// `record_request` as the daemon calls it, at `offset` seconds from now.
    fn record(&self, who: &Who, req: &Value, offset: i64) -> Outcome {
        let rfc = delphi_core::time::format_unix_millis(real_now() + offset, 0);
        record_request(&self.ws(), &self.home, who, parse_request(req, &policy()), &Clock { now_unix: real_now() + offset, now_rfc: &rfc })
    }
}

// Red-team P1: a host that sends a fresh session id with every call used to get a fresh budget each time.
#[test]
fn fresh_host_session_ids_buy_no_fresh_budget() {
    let f = fix();
    assert!(matches!(f.record(&host_who("sess00000"), &variant(&f, 0), 0), Outcome::Staged(_)));
    // another session id, seconds later: the workspace takes one host request a minute
    let (msg, log) = refusal(f.record(&host_who("sess00001"), &variant(&f, 1), 5));
    assert!(msg.contains("this workspace takes one host"), "{msg}");
    assert!(log.is_none());
    // spaced out, a new session id each time: the workspace stops at its host cap
    let mut staged_count = 1;
    let mut last = String::new();
    for n in 1..14 {
        match f.record(&host_who(&format!("sess{n:05}")), &variant(&f, n), n as i64 * 120) {
            Outcome::Staged(_) => staged_count += 1,
            Outcome::Refused { message, .. } => last = message,
        }
    }
    assert_eq!(staged_count, MAX_PENDING_HOST_WORKSPACE, "at most {MAX_PENDING_HOST_WORKSPACE} host requests wait in one workspace, whatever the session ids");
    assert!(last.contains("host requests are already pending in this workspace"), "{last}");
    assert_eq!(read_ledger(&f.ws()).unwrap().iter().filter(|e| e.kind == STAGED).count(), staged_count);
}

#[test]
fn the_workspace_as_a_whole_is_capped_for_agents_too() {
    let f = fix();
    let mut staged_count = 0;
    let mut last = String::new();
    for n in 0..(MAX_PENDING_WORKSPACE + 6) {
        // a fresh task and mission each time
        let mut w = who(Kind::Agent, Tier::L4, &format!("task{n}"));
        w.mission = format!("mission{n}");
        match f.record(&w, &variant(&f, n), 0) {
            Outcome::Staged(_) => staged_count += 1,
            Outcome::Refused { message, .. } => last = message,
        }
    }
    assert_eq!(staged_count, MAX_PENDING_WORKSPACE);
    assert!(last.contains("in this workspace"), "{last}");
}

#[test]
fn a_refusal_bars_the_manifest_across_every_mission_and_session() {
    let f = fix();
    let p = staged(f.record(&host_who("sess00000"), &variant(&f, 7), 0));
    let id = p["id"].as_str().unwrap().to_string();
    refuse_audited(&f.home, &id, p["manifest_hash"].as_str().unwrap(), "2026-11-08T00:00:00.000Z").unwrap();
    // the same manifest from another session, and from an agent in another mission, a few minutes later
    let (m1, _) = refusal(f.record(&host_who("sess00001"), &variant(&f, 7), 200));
    assert!(m1.contains("24 hours"), "{m1}");
    let mut w = who(Kind::Agent, Tier::L4, "t9");
    w.mission = "other-mission".into();
    assert!(refusal(f.record(&w, &variant(&f, 7), 200)).0.contains("24 hours"));
    // a different manifest is not barred
    assert!(matches!(f.record(&host_who("sess00002"), &variant(&f, 8), 400), Outcome::Staged(_)));
    // and the operator's answer frees the slot it held
    assert!(resolved_ids(&read_audit(&f.home).unwrap()).contains(&id));
}

// ---------------------------------------------------------------------------------------------- the audit log is verified

fn made(f: &Fix, n: usize, request: Option<&str>) -> (Record, Value) {
    let mut e = f.entry();
    e.argv.push(format!("--case={n}"));
    let mut r = build_record(&f.home, &f.new_record(vec![e]), 1_800_000_000 + n as i64, &at(n as i64)).unwrap();
    r.request_id = request.map(String::from);
    let payload = json!({"id": r.id, "request_id": r.request_id, "manifest_hash": r.manifest_hash, "run": r.run, "by": "operator", "source": "operator"});
    (r, payload)
}

fn audit_segment(f: &Fix) -> PathBuf {
    audit_dir(&f.home).join("events/000001.jsonl")
}

// Red-team P1: the audit log was read with `replay` (contiguity only), so a deleted `created` event made a request pending again.
#[test]
fn a_tampered_audit_log_is_refused_loudly_not_read_half_trusted() {
    let f = fix();
    let (r1, p1) = made(&f, 1, Some("ar-0000000001"));
    create_audited(&f.home, r1, p1).unwrap();
    let (r2, p2) = made(&f, 2, Some("ar-0000000002"));
    create_audited(&f.home, r2, p2).unwrap();
    let original = fs::read_to_string(audit_segment(&f)).unwrap();
    assert_eq!(read_audit(&f.home).unwrap().len(), 2);
    assert_eq!(load(&f.home).unwrap().audit_head.map(|h| h.seq), Some(2), "the pinned store records how far the log reaches");
    let lines: Vec<&str> = original.lines().collect();
    let bad = |what: &str, content: String| {
        fs::write(audit_segment(&f), content).unwrap();
        let e = read_audit(&f.home).unwrap_err();
        assert!(e.contains("fails verification") || e.contains("cannot be read"), "{what}: {e}");
        // and the request-staging path refuses with that message instead of treating the log as empty
        let (msg, log) = refusal(f.record(&host_who("sess00000"), &variant(&f, 40), 0));
        assert!(msg.contains("audit log") && log.is_none(), "{what}: {msg}");
        fs::write(audit_segment(&f), &original).unwrap();
        assert_eq!(read_audit(&f.home).unwrap().len(), 2, "restored: {what}");
    };
    bad("first event deleted (its request would be pending again)", format!("{}\n", lines[1]));
    bad("last event deleted (the tail was cut)", format!("{}\n", lines[0]));
    bad("log emptied", String::new());
    bad("events swapped", format!("{}\n{}\n", lines[1], lines[0]));
    bad("an event edited", format!("{}\n{}\n", lines[0], lines[1].replace("ar-0000000002", "ar-0000000009")));
    // a forged event, with no chain fields at all, to hide a pending request
    let mut forged = EventEnvelope::new("operator-audit", REFUSED, json!({"request_id": "ar-0000000003", "manifest_hash": "x", "blocks_restage": true}));
    forged.seq = 3;
    forged.timestamp = at(9);
    bad("an unchained event appended", format!("{}\n{}\n{}\n", lines[0], lines[1], serde_json::to_string(&forged).unwrap()));
    // the whole log deleted
    fs::remove_dir_all(audit_dir(&f.home)).unwrap();
    assert!(read_audit(&f.home).unwrap_err().contains("deleted"));
}

#[test]
fn an_authorization_is_audited_before_it_is_live_and_a_failed_store_change_is_rolled_back() {
    let f = fix();
    let (r, p) = made(&f, 1, Some("ar-0000000001"));
    create_audited(&f.home, r.clone(), p.clone()).unwrap();
    let audit = read_audit(&f.home).unwrap();
    assert_eq!(audit.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), [CREATED]);
    assert_eq!(load(&f.home).unwrap().records.len(), 1);
    // the same request cannot be answered twice
    let (r2, p2) = made(&f, 2, Some("ar-0000000001"));
    let e = create_audited(&f.home, r2, p2).unwrap_err();
    assert!(e.contains("already answered"), "{e}");
    // a store change that fails after the event was written leaves a rollback, and the request stays unanswered
    let mut dup = made(&f, 3, Some("ar-0000000007")).0;
    dup.id = r.id.clone();
    let err = create_audited(&f.home, dup, json!({"id": r.id, "request_id": "ar-0000000007", "by": "operator"})).unwrap_err();
    assert!(err.contains("exists"), "{err}");
    let audit = read_audit(&f.home).unwrap();
    assert_eq!(audit.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), [CREATED, CREATED, CREATE_ROLLED_BACK]);
    let resolved = resolved_ids(&audit);
    assert!(resolved.contains("ar-0000000001") && !resolved.contains("ar-0000000007"), "a rolled-back event answers nothing: {resolved:?}");
    // two authorizes at once: the second cannot open the audit log while the first holds it
    let held = open_audit(&f.home).unwrap();
    let (r4, p4) = made(&f, 4, Some("ar-0000000004"));
    let e = create_audited(&f.home, r4, p4).unwrap_err();
    assert!(e.contains("another"), "{e}");
    drop(held);
    // revoke is audited first, too
    let rec = revoke_audited(&f.home, &r.id, &at(50)).unwrap();
    assert!(rec.is_revoked());
    assert_eq!(read_audit(&f.home).unwrap().last().unwrap().kind, REVOKED);
    let e = revoke_audited(&f.home, &r.id, &at(51)).unwrap_err();
    assert!(e.contains("already revoked"), "{e}");
    assert_eq!(load(&f.home).unwrap().audit_head.unwrap().seq, read_audit(&f.home).unwrap().last().unwrap().seq, "the pin follows the log");
}

#[test]
fn a_lock_released_a_moment_late_does_not_fail_the_next_writer() {
    // A forked child can hold a copy of a just-released lock until it execs; the writer waits a moment instead of failing.
    let f = fix();
    let held = open_audit(&f.home).unwrap();
    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(60));
        drop(held);
    });
    let again = open_audit(&f.home);
    t.join().unwrap();
    assert!(again.is_ok(), "{:?}", again.err());
}

// ---------------------------------------------------------------------------------------------- a crash cannot brick the store

#[test]
fn a_crash_between_pinning_and_moving_the_store_is_recovered_and_a_planted_next_file_is_ignored() {
    let f = fix();
    add_record(&f.home, made(&f, 1, None).0, None).unwrap();
    assert!(!next_path(&f.home).exists(), "a finished change leaves no .next");
    // the crash: the new bytes are written and pinned, the rename never happened
    let mut s = load(&f.home).unwrap();
    s.records.push(made(&f, 2, None).0);
    let bytes = serde_json::to_vec_pretty(&s).unwrap();
    fs::write(next_path(&f.home), &bytes).unwrap();
    let mut l = lock::read(&f.home.join("operator/ratified.lock")).unwrap();
    l.files.insert(LOCK_KEY.into(), hash(&bytes));
    lock::write(&f.home.join("operator/ratified.lock"), &l).unwrap();
    assert_eq!(load(&f.home).unwrap().records.len(), 2, "the pinned bytes are used even though the live file is the old version");
    // the next change finishes the move and adds to it
    add_record(&f.home, made(&f, 3, None).0, None).unwrap();
    assert_eq!(load(&f.home).unwrap().records.len(), 3);
    assert!(!next_path(&f.home).exists());
    assert_eq!(hash(&fs::read(store_path(&f.home)).unwrap()), lock::read(&f.home.join("operator/ratified.lock")).unwrap().files[LOCK_KEY]);
    // a planted .next matches no pin and changes nothing
    fs::write(next_path(&f.home), forged_store(&f)).unwrap();
    assert_eq!(load(&f.home).unwrap().records.len(), 3);
    // and with no pin at all it is ignored, as a planted store is
    let mut l = lock::read(&f.home.join("operator/ratified.lock")).unwrap();
    l.files.remove(LOCK_KEY);
    lock::write(&f.home.join("operator/ratified.lock"), &l).unwrap();
    assert_ignored(&f, "pin removed");
}

#[test]
fn a_script_or_runner_another_user_could_replace_is_not_pinned() {
    let f = fix();
    let make = |e: Entry| build_record(&f.home, &f.new_record(vec![e]), 1_800_000_000, &at(0));
    fs::set_permissions(&f.script, fs::Permissions::from_mode(0o664)).unwrap();
    assert!(make(f.entry()).unwrap_err().contains("group- or world-writable"));
    fs::set_permissions(&f.script, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(make(f.entry()).is_ok());
    // a directory anyone in the group can write to
    let dir = fs::canonicalize(f._t.path()).unwrap().join("shared");
    fs::create_dir_all(&dir).unwrap();
    let s = dir.join("s.py");
    fs::write(&s, b"print(2)").unwrap();
    fs::set_permissions(&s, fs::Permissions::from_mode(0o644)).unwrap();
    let mut e = f.entry();
    e.argv[1] = s.to_string_lossy().into();
    e.files = vec![e.argv[1].clone()];
    assert!(make(e.clone()).is_ok());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o775)).unwrap();
    assert!(make(e).unwrap_err().contains("directory"));
}

// ---------------------------------------------------------------------------------------------- no consumer, workspace-wide

/// Every `.rs` file of every crate, with its path relative to the workspace root.
fn workspace_sources() -> Vec<(String, String)> {
    let root = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let mut out = vec![];
    for e in walkdir::WalkDir::new(root.join("crates")).into_iter().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "rs")) {
        let rel = e.path().strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        out.push((rel, fs::read_to_string(e.path()).unwrap()));
    }
    out
}

/// True when `text` names the module `authorization` as a path segment or an import, in any style: `crate::authorization::x`,
/// `use crate::authorization as a;`, `use delphi_engine::{authorization as a, other}`, `mod authorization;`. Matched on the word, not
/// on literal substrings.
fn names_module(text: &str) -> bool {
    text.match_indices("authorization").any(|(i, w)| {
        let before = &text[..i];
        let after = &text[i + w.len()..];
        if before.chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') || after.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            return false;
        }
        let b = before.trim_end();
        let a = after.trim_start();
        // inside a `use` statement (everything since the last `;`), a brace or comma lead is an import; elsewhere only a path counts
        let stmt = &before[before.rfind(';').map_or(0, |p| p + 1)..];
        let in_use = stmt.contains("use ");
        let lead = b.ends_with("::") || ((b.ends_with('{') || b.ends_with(',')) && in_use) || b.ends_with("mod") || b.ends_with("use");
        let trail = a.starts_with("::") || a.starts_with("as ") || a.starts_with('}') || a.starts_with(',') || a.starts_with(';');
        lead && trail
    })
}

/// True when `text` uses `item` through `prefix::` as a whole word.
fn uses_item(text: &str, prefix: &str, item: &str) -> bool {
    let needle = format!("{prefix}::{item}");
    text.match_indices(&needle).any(|(i, _)| {
        let before_ok = i == 0 || !text[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_');
        let after = text[i + needle.len()..].chars().next();
        before_ok && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

/// Why `text` (the source of `run.rs`) reaches the authorization module in a way S2a does not allow, or `None`. Allowed: the full
/// path `delphi_engine::authorization::authorize_launch` and `delphi_engine::authorization::Policy`, and nothing else: no `use` line
/// that names the module, no alias, no brace form, so `check_launch`, `this_machine` and any future consuming function stay out of reach.
fn run_rs_bypass(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        if t.starts_with("use ") && (t.contains("authorization") || t.contains("delphi_engine::{") || t.contains("delphi_engine::*")) {
            return Some(format!("imports the engine by `{}`; name the two allowed items by their full path instead", t));
        }
        if t.contains("authorization as ") || t.contains("{authorization") || t.contains("authorization::{") || t.contains("authorization::*") {
            return Some(format!("aliases or brace-imports the authorization module (`{}`)", t));
        }
    }
    for (i, _) in text.match_indices("authorization::") {
        let line_start = text[..i].rfind('\n').map_or(0, |p| p + 1);
        if text[line_start..i].trim_start().starts_with("//") {
            continue;
        }
        let item: String = text[i + "authorization::".len()..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if !text[..i].ends_with("delphi_engine::") || !["authorize_launch", "Policy"].contains(&item.as_str()) {
            return Some(format!("reaches `authorization::{item}` (only the full path to `authorize_launch` and `Policy` is allowed)"));
        }
    }
    None
}

#[test]
fn the_run_rs_guard_catches_every_way_around_it() {
    assert_eq!(run_rs_bypass("let x = delphi_engine::authorization::authorize_launch(a);\nlet p = delphi_engine::authorization::Policy::system();"), None);
    for bad in [
        "use delphi_engine::authorization as a;",
        "use delphi_engine::authorization::{authorize_launch, check_launch};",
        "use delphi_engine::authorization::check_launch;",
        "use delphi_engine::{authorization};",
        "delphi_engine::authorization::check_launch(r);",
        "delphi_engine::authorization::this_machine();",
        "delphi_engine::authorization::load(h);",
        "use crate::authorization::authorize_launch;",
        "authorization::authorize_launch(h)",
    ] {
        assert!(run_rs_bypass(bad).is_some(), "not caught: {bad}");
    }
}

// S1 grants no capability: no code that decides a card, runs a command or starts a run may read the store. Scanned over the whole
// workspace, with the allowlist of who may import the module at all, and a narrower one of who may touch the store.
#[test]
fn nothing_consumes_a_record_in_this_slice() {
    const IMPORTERS: &[&str] = &[
        "crates/delphi-engine/src/authorization.rs",
        "crates/delphi-engine/src/authorization/tests.rs",
        "crates/delphi-engine/src/engine.rs",   // stages or refuses a request
        "crates/delphi-engine/src/service.rs",  // the host socket method
        "crates/delphi-engine/src/tools.rs",    // the agent tool
        "crates/delphi/src/test_cmd.rs",        // the operator's commands
        "crates/delphi/src/test_cmd_tests.rs",
        "crates/delphi/tests/e2e.rs", // S2d2a: builds a record for the real-binary launcher test (a test, not product code)
        "crates/delphi/src/doctor.rs",          // verifies the pin
        "crates/delphi/tests/docs.rs",          // schemas
        "crates/delphi/src/run.rs",             // S2a: the launch gate only (checked below); it approves nothing
        "crates/delphi/src/run_authorization_tests.rs", // S2a: test fixtures that build records
        "crates/delphi-engine/src/authorization_grant.rs", // S2c: the daemon-side reader (grant, matching, use ledger); the one new importer
        "crates/delphi-engine/src/authorization_grant/tests.rs",
        "crates/delphi-engine/tests/daemon.rs", // S2c: boots a real daemon from a grant pipe
        // S2d1: the ledger anchor (launcher side) and the forced-sandbox runner. Neither is reachable from a run (see `run_authorized_has_no_caller_but_its_tests`).
        "crates/delphi-engine/src/authorization_anchor.rs",
        "crates/delphi-engine/src/authorization_anchor/tests.rs",
        "crates/delphi-engine/src/authorization_run.rs",
        "crates/delphi-engine/src/authorization_run/tests.rs",
        "crates/delphi-engine/src/authorization_use/tests.rs", // S2d2b: tests of the daemon check build records
    ];
    // S2c: who may name the grant module at all. The module approves nothing; this list is the exact set of its callers.
    const GRANT_USERS: &[&str] = &[
        "crates/delphi-engine/src/lib.rs",                  // `pub mod authorization_grant;`
        "crates/delphi-engine/src/authorization.rs",        // `authorize_launch` reads the use ledger
        "crates/delphi-engine/src/authorization_grant.rs",
        "crates/delphi-engine/src/authorization_grant/tests.rs",
        "crates/delphi-engine/src/authorization/tests.rs",  // this scan
        "crates/delphi-engine/tests/daemon.rs",             // boots a real daemon from a grant pipe
        "crates/delphi-engine/src/engine.rs",               // holds the grant; observes a Red card; strips the write tools
        "crates/delphi-engine/src/harness_run.rs",          // refuses a seat whose own tools are not proven off
        "crates/delphi-engine/src/core_main.rs",            // reads the grant pipe once, at boot
        "crates/delphi/src/run.rs",                         // the launcher writes the pipe (`launch_payload`)
        "crates/delphi/src/run_authorization_tests.rs",    // its tests
        "crates/delphi/src/test_cmd_tests.rs",             // S2d2a: the console-flow test reads the ledger count
        "crates/delphi/tests/e2e.rs",                      // S2d2a: asserts no real use was taken
        "crates/delphi-engine/src/authorization_anchor.rs", // S2d1: writes the launch/end events and builds the boot payload
        "crates/delphi-engine/src/authorization_anchor/tests.rs",
        "crates/delphi-engine/src/authorization_run.rs",    // S2d1: `run_authorized`, a method of the grant
        "crates/delphi-engine/src/authorization_run/tests.rs",
        "crates/delphi-engine/src/authorization_use.rs",   // S2d2b: the daemon check that approves under the grant
        "crates/delphi-engine/src/authorization_use/tests.rs",
    ];
    const STORE_READERS: &[&str] = &["crates/delphi-engine/src/authorization_use/tests.rs", "crates/delphi-engine/src/authorization_anchor.rs", "crates/delphi-engine/src/authorization_anchor/tests.rs", "crates/delphi-engine/src/authorization_run/tests.rs", "crates/delphi-engine/src/authorization.rs", "crates/delphi-engine/src/authorization/tests.rs", "crates/delphi-engine/src/authorization_grant.rs", "crates/delphi-engine/src/authorization_grant/tests.rs", "crates/delphi-engine/tests/daemon.rs", "crates/delphi/src/test_cmd.rs", "crates/delphi/src/test_cmd_tests.rs", "crates/delphi/src/run_authorization_tests.rs", "crates/delphi/src/doctor.rs", "crates/delphi/tests/docs.rs", "crates/delphi/tests/e2e.rs"];
    const STORE_ITEMS: &[&str] = &["load", "load_or_ignore", "mutate", "add_record", "revoke", "store_path", "Store", "Record", "build_record", "create_audited", "revoke_audited", "refuse_audited", "read_audit", "audit_dir", "open_audit", "append_audit", "AuditHead"];
    let sources = workspace_sources();
    assert!(sources.len() > 200, "the scan must see the whole workspace");
    for (rel, text) in &sources {
        let imports = names_module(text);
        let raw = text.contains("authorizations.json") || text.contains("authorizations.json.next");
        if !imports && !raw {
            continue;
        }
        let allowed = IMPORTERS.contains(&rel.as_str()) || rel == "crates/delphi-engine/src/lib.rs"; // `pub mod authorization;`
        assert!(allowed, "{rel} uses the authorization module or the store file; S1 grants no capability, so nothing outside the allowlist may (an authorization cannot be consumed before S2)");
        if raw {
            assert!(STORE_READERS.contains(&rel.as_str()), "{rel} names the store file");
        }
        if !STORE_READERS.contains(&rel.as_str()) && rel != "crates/delphi-engine/src/lib.rs" {
            // the request side may use request items only, under the module's own name and under any alias it gives it
            let mut prefixes = vec!["authorization".to_string()];
            for l in text.lines().filter(|l| l.contains("authorization as ")) {
                if let Some(a) = l.split("authorization as ").nth(1) {
                    prefixes.push(a.trim().trim_end_matches(';').trim().to_string());
                }
            }
            for p in &prefixes {
                for item in STORE_ITEMS {
                    assert!(!uses_item(text, p, item), "{rel} reaches the store through {p}::{item}");
                }
            }
        }
    }
    // S2c: only the files above name the grant module; the approval path, the executor, the hooks and the broker never do.
    for (rel, text) in &sources {
        if text.contains("authorization_grant") {
            assert!(GRANT_USERS.contains(&rel.as_str()), "{rel} names authorization_grant; S2c lists its callers exactly (an addition is a deliberate, reviewed change)");
        }
        if ["approval.rs", "exec.rs", "hooks.rs", "broker.rs"].iter().any(|n| rel.ends_with(n)) {
            assert!(!text.contains("authorization_grant") && !text.contains("grant.get()"), "{rel} must not know the grant");
        }
    }
    // the approval path, the executor and the hooks never name an authorization at all
    for (rel, text) in &sources {
        if ["approval.rs", "exec.rs", "hooks.rs", "broker.rs", "headless"].iter().any(|n| rel.ends_with(n)) && rel != "crates/delphi-engine/src/authorization/tests.rs" {
            assert!(!text.to_lowercase().contains("authorization:"), "{rel} must not know about `--on-card authorization:` in S1");
        }
    }
    // S2a: `run.rs` may reach the module only as `delphi_engine::authorization::authorize_launch` and `::Policy`, written out in full.
    let run_rs = &sources.iter().find(|(r, _)| r == "crates/delphi/src/run.rs").unwrap().1;
    if let Some(why) = run_rs_bypass(run_rs) {
        panic!("run.rs: {why}");
    }
    // the three request-side files import the module by path or one plain alias, never by brace or glob
    for rel in ["crates/delphi-engine/src/engine.rs", "crates/delphi-engine/src/service.rs", "crates/delphi-engine/src/tools.rs"] {
        let text = &sources.iter().find(|(r, _)| r == rel).unwrap().1;
        for bad in ["authorization::*", "authorization::{", "{authorization", "authorization as _"] {
            assert!(!text.contains(bad), "{rel} imports the authorization module by `{bad}`; use a path or a plain `as` alias so the scan can see it");
        }
    }
    // no other program (the console in ui/, a build script) names the store file
    let root = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let mut others = vec![];
    for e in walkdir::WalkDir::new(&root).into_iter().filter_entry(|e| !["node_modules", "target", ".git", ".claude", "dist"].contains(&e.file_name().to_string_lossy().as_ref())).flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let in_ui = e.path().strip_prefix(&root).is_ok_and(|p| p.starts_with("ui"));
        if e.file_type().is_file() && (name == "build.rs" || (in_ui && (name.ends_with(".ts") || name.ends_with(".tsx") || name.ends_with(".js")))) {
            others.push(e.path().to_path_buf());
        }
    }
    for p in others {
        let text = fs::read_to_string(&p).unwrap_or_default();
        assert!(!text.contains("authorizations.json"), "{} names the authorization store", p.display());
        assert!(!(text.contains("operator/") && text.contains("authorization")), "{} reads the operator home and mentions authorization", p.display());
    }
    // the scan itself sees every import style
    assert!(names_module("use delphi_engine::{authorization as a, other};"));
    assert!(names_module("use crate::authorization as a;"));
    assert!(names_module("crate::authorization::load(x)"));
    assert!(names_module("pub mod authorization;"));
    assert!(!names_module("let authorization = headers.get(\"authorization\");"));
    assert!(!names_module("request_test_authorization::x"));
    // the scan itself sees the alias style used by service.rs
    assert!(uses_item("use crate::authorization as a;\n a::load(x)", "a", "load"));
    assert!(!uses_item("a::loader()", "a", "load"));
}

// Spec 092 S2d2b connected `run_authorized` to ONE place: `RunPlaces::run` (authorization_run.rs), which only `Engine::granted_run_command`
// (authorization_use.rs) calls. Nothing else may call it: not the executor, the approval path, the launcher or any other crate, and the
// switch the earlier slices kept (`AUTHORIZED_APPROVAL_ENABLED`) no longer exists. Changing this list is a reviewed change of the approval.
#[test]
fn run_authorized_has_no_caller_but_the_daemon_check_and_its_tests() {
    const ALLOWED: &[&str] = &[
        "crates/delphi-engine/src/authorization_run.rs",       // the definition
        "crates/delphi-engine/src/authorization_run/tests.rs", // its tests
        "crates/delphi-engine/src/authorization/tests.rs",     // this scan
    ];
    const MODULE_USERS: &[&str] = &[
        "crates/delphi-engine/src/lib.rs",
        "crates/delphi-engine/src/authorization_run.rs",
        "crates/delphi-engine/src/authorization_run/tests.rs",
        "crates/delphi-engine/src/authorization/tests.rs",
        "crates/delphi-engine/src/authorization_use.rs", // the daemon check
        "crates/delphi-engine/src/authorization_use/tests.rs",
        "crates/delphi-engine/src/engine.rs", // the field that holds the places
        "crates/delphi-engine/src/core_main.rs", // attaches them at boot
    ];
    const ANCHOR_USERS: &[&str] = &["crates/delphi-engine/src/authorization_use/tests.rs", "crates/delphi/tests/e2e.rs", "crates/delphi-engine/src/lib.rs", "crates/delphi-engine/src/authorization_anchor.rs", "crates/delphi-engine/src/authorization_anchor/tests.rs", "crates/delphi-engine/src/authorization_run/tests.rs", "crates/delphi/src/run.rs", "crates/delphi-engine/src/authorization/tests.rs", "crates/delphi/src/test_cmd_tests.rs"];
    let sources = workspace_sources();
    assert!(sources.len() > 200, "the scan must see the whole workspace");
    for (rel, text) in &sources {
        // code only: a comment may talk about the function, a call may not hide in one
        let code: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect();
        let calls = code.iter().any(|l| l.contains("run_authorized"));
        assert!(!calls || ALLOWED.contains(&rel.as_str()), "{rel} names `run_authorized`; only RunPlaces::run (and its tests) may");
        let uses_module = code.iter().any(|l| l.contains("authorization_run"));
        assert!(!uses_module || MODULE_USERS.contains(&rel.as_str()), "{rel} names the module that holds `run_authorized`");
        let uses_anchor = code.iter().any(|l| l.contains("authorization_anchor"));
        assert!(!uses_anchor || ANCHOR_USERS.contains(&rel.as_str()), "{rel} names the ledger anchor module");
    }
    // by construction too: the module is private to the crate (no other crate can name it), and the anchored decrement has one caller
    let lib = &sources.iter().find(|(r, _)| r == "crates/delphi-engine/src/lib.rs").unwrap().1;
    assert!(lib.contains("\nmod authorization_run;") && !lib.contains("pub mod authorization_run"), "authorization_run is a private module");
    assert!(lib.contains("\nmod authorization_use;") && !lib.contains("pub mod authorization_use"), "authorization_use is a private module");
    for (rel, text) in &sources {
        let code: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect();
        let takes = code.iter().any(|l| l.contains("take_use_anchored"));
        let ok = ["crates/delphi-engine/src/authorization_grant.rs", "crates/delphi-engine/src/authorization_grant/tests.rs", "crates/delphi-engine/src/authorization_run.rs", "crates/delphi-engine/src/authorization/tests.rs", "crates/delphi-engine/src/authorization_anchor/tests.rs", "crates/delphi-engine/src/authorization_run/tests.rs"];
        assert!(!takes || ok.contains(&rel.as_str()), "{rel} takes a real use");
        // the places' runner is called by the daemon check only
        let runs = code.iter().any(|l| l.contains("places.run(") || l.contains("RunPlaces"));
        let ok = ["crates/delphi-engine/src/authorization_run.rs", "crates/delphi-engine/src/authorization_use.rs", "crates/delphi-engine/src/authorization_use/tests.rs", "crates/delphi-engine/src/authorization_run/tests.rs", "crates/delphi-engine/src/engine.rs", "crates/delphi-engine/src/core_main.rs", "crates/delphi-engine/src/authorization/tests.rs"];
        assert!(!runs || ok.contains(&rel.as_str()), "{rel} names the places a granted command runs in");
        // no switch: the constant the earlier slices kept is gone, and nothing outside this scan names it
        assert!(!(code.iter().any(|l| l.contains("AUTHORIZED_APPROVAL_ENABLED")) && rel != "crates/delphi-engine/src/authorization/tests.rs"), "{rel} still names the removed approval switch");
    }
    // text forms that could hide a call from the plain-substring scan: an include! of the module, or a macro that builds the name
    for (rel, text) in &sources {
        if ALLOWED.contains(&rel.as_str()) || MODULE_USERS.contains(&rel.as_str()) {
            continue;
        }
        for l in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
            assert!(!(l.contains("include!") && l.contains("authorization")), "{rel}: {l}");
            assert!(!(l.contains("run_auth") && (l.contains("concat!") || l.contains("stringify!") || l.contains("paste!") || l.contains("macro_rules!"))), "{rel}: {l}");
            assert!(!(l.contains("macro_rules!") && l.contains("authorization")), "{rel}: {l}");
        }
    }
    // the definition is a method of the grant and is the only `fn run_authorized`
    let defs: Vec<&str> = sources.iter().filter(|(r, t)| r != "crates/delphi-engine/src/authorization/tests.rs" && t.lines().any(|l| l.contains("fn run_authorized("))).map(|(r, _)| r.as_str()).collect();
    assert_eq!(defs, ["crates/delphi-engine/src/authorization_run.rs"]);
    // the engine reaches the approval through the one function, before anything else in `exec_command`; the file holding `exec_command`
    // names neither the runner nor a ledger write
    let engine = &sources.iter().find(|(r, _)| r == "crates/delphi-engine/src/engine.rs").unwrap().1;
    assert!(engine.contains("GRANTED_DENIAL") && !engine.contains("run_authorized") && !engine.contains("take_use_anchored") && !engine.contains("take_use("), "the engine reaches the approval only through granted_run_command");
    let at_exec = engine.find("async fn exec_command").unwrap();
    let hook = engine[at_exec..].find("self.granted_run_command(sc, args)").expect("exec_command asks the daemon check");
    let first_other = engine[at_exec..].find("sc.manifest.execute").unwrap();
    assert!(hook < first_other, "the daemon check is the first thing exec_command does");
    // `delphi run` (the client) has no way to approve: no constant, and `decide` has no arm that answers allow to a Red card
    let run_rs = &sources.iter().find(|(r, _)| r == "crates/delphi/src/run.rs").unwrap().1;
    assert!(!run_rs.contains("pub const AUTHORIZED_APPROVAL_ENABLED"), "the approval switch is removed");
    let grant_rs = &sources.iter().find(|(r, _)| r == "crates/delphi-engine/src/authorization_grant.rs").unwrap().1;
    assert!(grant_rs.contains("pub const GRANTED_TOOLS: &[&str] = &["), "the allowlist is a constant");
    assert!(!grant_rs.contains("\"inspect_system\""), "inspect_system is not on the allowlist of a granted run (S2d1 carry-over)");
}

// Second red-team review P1: a deny-listed host request used to skip the host rate limit and the re-stage block, so a fresh session id
// and a one-argument change per call grew the ledger without bound.
#[test]
fn denied_host_requests_are_rate_limited_like_any_other_and_cost_one_row_a_minute() {
    let f = fix();
    let denied = |n: usize| {
        let mut v = f.request_json();
        v["manifest"][0]["argv"] = json!(["/usr/bin/git", "push", format!("origin{n}")]);
        v
    };
    let ledger_rows = || read_ledger(&f.ws()).unwrap().len();
    let mut logged = 0;
    for n in 0..40 {
        // a fresh session id and a new argument each time, a second apart
        let o = f.record(&host_who(&format!("flood{n:05}")), &denied(n), n as i64);
        if matches!(o, Outcome::Refused { log: Some(_), .. }) {
            logged += 1;
        }
    }
    assert_eq!(logged, 1, "only the first denied call is recorded inside the workspace's host minute");
    assert_eq!(ledger_rows(), 1);
    // an invalid request counts the same: it is refused quietly while the minute runs
    let mut invalid = f.request_json();
    invalid["manifest"] = json!([]);
    assert!(refusal(f.record(&host_who("flood99999"), &invalid, 30)).1.is_none());
    // a minute on, one more row, and no more
    assert!(matches!(f.record(&host_who("flood00100"), &denied(100), 70), Outcome::Refused { log: Some(_), .. }));
    assert_eq!(ledger_rows(), 2);
    // the same denied manifest is not recorded again inside a day, from any session
    assert!(refusal(f.record(&host_who("flood00200"), &denied(100), 200)).0.contains("24 hours"));
    assert_eq!(ledger_rows(), 2);
    // and a pending cap in force stops a host request before it is even looked at
    let g = fix();
    for n in 0..MAX_PENDING_HOST_WORKSPACE {
        assert!(matches!(g.record(&host_who(&format!("fill{n:05}")), &variant(&g, n), n as i64 * 120), Outcome::Staged(_)));
    }
    let off = MAX_PENDING_HOST_WORKSPACE as i64 * 120 + 120;
    let mut bad = g.request_json();
    bad["manifest"][0]["argv"] = json!(["/usr/bin/git", "push"]);
    let (msg, log) = refusal(g.record(&host_who("fillfinal"), &bad, off));
    assert!(msg.contains("host requests are already pending") && log.is_none(), "{msg}");
}

// Second red-team review P2: an older store-and-pin pair must not quietly take back a record or a revocation.
#[test]
fn an_older_store_and_pin_pair_is_seen_against_the_audit_log() {
    let f = fix();
    let (r1, p1) = made(&f, 1, Some("ar-0000000001"));
    create_audited(&f.home, r1.clone(), p1).unwrap();
    let old_store = fs::read(store_path(&f.home)).unwrap();
    let old_lock = fs::read(f.home.join("operator/ratified.lock")).unwrap();
    let (r2, p2) = made(&f, 2, Some("ar-0000000002"));
    create_audited(&f.home, r2, p2).unwrap();
    let (r3, p3) = made(&f, 3, Some("ar-0000000003"));
    create_audited(&f.home, r3, p3).unwrap();
    revoke_audited(&f.home, &r1.id, &at(60)).unwrap();
    assert!(read_audit(&f.home).is_ok());
    // the roll-back: the store and its pin from before the second authorization
    fs::write(store_path(&f.home), &old_store).unwrap();
    fs::write(f.home.join("operator/ratified.lock"), &old_lock).unwrap();
    assert!(load(&f.home).is_ok(), "the old pair is self-consistent: only the audit log can tell");
    let e = read_audit(&f.home).unwrap_err();
    assert!(e.contains("disagree") || e.contains("fails verification"), "{e}");
    // a refusal advances the recorded head, so cutting it is caught as well
    let g = fix();
    let (r, p) = made(&g, 1, None);
    create_audited(&g.home, r, p).unwrap();
    refuse_audited(&g.home, "ar-0000000009", "x", &at(5)).unwrap();
    assert_eq!(load(&g.home).unwrap().audit_head.unwrap().seq, 2);
    let seg = audit_dir(&g.home).join("events/000001.jsonl");
    let text = fs::read_to_string(&seg).unwrap();
    fs::write(&seg, format!("{}\n", text.lines().next().unwrap())).unwrap();
    assert!(read_audit(&g.home).unwrap_err().contains("tail was cut"));
}

// ---------------------------------------------------------------------------------------------- S2a: the launch gate refuses

const LAUNCH_NOW: i64 = 1_800_000_100;

fn live(f: &Fix) -> Record {
    let (r, p) = made(f, 1, None);
    create_audited(&f.home, r.clone(), p).unwrap();
    r
}

fn launch(f: &Fix, id: &str) -> Result<Record, String> {
    authorize_launch(&f.home, id, "run-1", LAUNCH_NOW, &policy())
}

#[test]
fn launch_accepts_a_valid_record_and_changes_nothing() {
    let f = fix();
    let r = live(&f);
    let before = fs::read(store_path(&f.home)).unwrap();
    assert_eq!(launch(&f, &r.id).unwrap().id, r.id);
    assert_eq!(fs::read(store_path(&f.home)).unwrap(), before, "the launch gate writes nothing");
    assert_eq!(load(&f.home).unwrap().records[0].uses, 0, "and consumes no use");
}

#[test]
fn launch_refuses_an_unknown_record() {
    let f = fix();
    live(&f);
    assert!(launch(&f, "ta-nope").unwrap_err().contains("no authorization"));
}

#[test]
fn launch_refuses_a_byte_flipped_store() {
    let f = fix();
    let r = live(&f);
    let mut bytes = fs::read(store_path(&f.home)).unwrap();
    let at = bytes.iter().position(|b| *b == b'1').unwrap();
    bytes[at] = b'2';
    fs::write(store_path(&f.home), bytes).unwrap();
    assert!(launch(&f, &r.id).unwrap_err().contains("pin"));
}

#[test]
fn launch_refuses_a_store_whose_lock_entry_was_deleted() {
    let f = fix();
    let r = live(&f);
    lock::write(&f.home.join("operator/ratified.lock"), &LockFile::default()).unwrap();
    assert!(launch(&f, &r.id).unwrap_err().contains("no entry in operator/ratified.lock"));
}

#[test]
fn launch_refuses_an_edited_store_even_with_a_planted_next_file() {
    let f = fix();
    let r = live(&f);
    let mut store = load(&f.home).unwrap();
    store.records[0].max_uses = 50;
    let planted = serde_json::to_vec_pretty(&store).unwrap();
    fs::write(store_path(&f.home), &planted).unwrap();
    fs::write(f.home.join("operator/authorizations.json.next"), &planted).unwrap();
    assert!(launch(&f, &r.id).is_err(), "a `.next` that no pin covers is not a way in");
}

#[test]
fn launch_refuses_a_cut_audit_log() {
    let f = fix();
    let r = live(&f);
    fs::remove_dir_all(audit_dir(&f.home)).unwrap();
    assert!(launch(&f, &r.id).unwrap_err().contains("audit"));
}

#[test]
fn launch_refuses_a_revoked_record() {
    let f = fix();
    let r = live(&f);
    revoke_audited(&f.home, &r.id, &at(50)).unwrap();
    assert!(launch(&f, &r.id).unwrap_err().contains("revoked"));
}

fn pure(f: &Fix, r: &Record) -> Result<(), String> {
    check_launch(r, LAUNCH_NOW, "run-1", &f.home, &this_machine(), &policy())
}

fn rehash(r: &mut Record) {
    r.manifest_hash = r.computed_manifest_hash();
}

#[test]
fn check_launch_names_each_reason() {
    let f = fix();
    let (r, _) = made(&f, 1, None);
    pure(&f, &r).unwrap();
    assert!(check_launch(&r, 1_800_000_000 + DEFAULT_EXPIRY_SECS + 1, "run-1", &f.home, &this_machine(), &policy()).unwrap_err().contains("expired"));
    let mut bad = r.clone();
    bad.expires = "not a time".into();
    assert!(pure(&f, &bad).unwrap_err().contains("expiry"));
    let mut bad = r.clone();
    bad.revoked = Some(at(5));
    assert!(pure(&f, &bad).unwrap_err().contains("revoked"));
    assert!(check_launch(&r, LAUNCH_NOW, "run-2", &f.home, &this_machine(), &policy()).unwrap_err().contains("another run"));
    let other = tempfile::tempdir().unwrap();
    assert!(check_launch(&r, LAUNCH_NOW, "run-1", other.path(), &this_machine(), &policy()).unwrap_err().contains("another home"));
    assert!(check_launch(&r, LAUNCH_NOW, "run-1", &f.home, "some-other-host", &policy()).unwrap_err().contains("another machine"));
    let mut bad = r.clone();
    bad.uses = bad.max_uses;
    assert!(pure(&f, &bad).unwrap_err().contains("no uses left"));
}

#[test]
fn check_launch_runs_the_manifest_through_the_deny_list_and_the_runner_check_again() {
    let f = fix();
    let (r, _) = made(&f, 1, None);
    let mut bad = r.clone();
    bad.manifest[0].argv.push("extra".into());
    assert!(pure(&f, &bad).unwrap_err().contains("recorded hash"));
    let mut bad = r.clone();
    bad.manifest[0].argv[0] = "/usr/bin/curl".into();
    rehash(&mut bad);
    assert!(pure(&f, &bad).unwrap_err().contains("deny-list"));
    let mut bad = r.clone();
    bad.manifest[0].argv[0] = "/tmp/bash".into();
    rehash(&mut bad);
    assert!(pure(&f, &bad).unwrap_err().contains("not an interpreter"));
}

// ---------------------------------------------------------------------------------------------- S2d2a: path tokens in an entry

#[test]
fn an_entry_with_a_malformed_or_misplaced_token_is_refused_when_the_record_is_made() {
    let p = Policy::system();
    let e = |argv: &[&str], files: &[&str]| Entry { tool: "shell".into(), argv: argv.iter().map(|s| s.to_string()).collect(), files: files.iter().map(|s| s.to_string()).collect(), runner_provenance: "project_snapshot".into() };
    assert!(check_entry(&e(&["/bin/bash", "@snapshot/job.sh", "@snapshot", "@scratch", "@canary", "@operator_home"], &["@snapshot/job.sh"]), &p).is_ok());
    for (argv, files) in [
        (vec!["/bin/bash", "@snapshot/job.sh", "@unknown"], vec!["@snapshot/job.sh"]),
        (vec!["/bin/bash", "@snapshot/job.sh", "x@snapshot"], vec!["@snapshot/job.sh"]),
        (vec!["@snapshot", "@snapshot/job.sh"], vec!["@snapshot/job.sh"]),
        (vec!["/bin/bash", "@snapshot/../x"], vec!["@snapshot/../x"]),
        (vec!["/bin/bash", "@snapshot//job.sh"], vec!["@snapshot//job.sh"]),
        (vec!["/bin/bash", "@snapshot/./job.sh"], vec!["@snapshot/./job.sh"]),
        (vec!["/bin/bash", "@Snapshot/job.sh"], vec!["@Snapshot/job.sh"]),
        (vec!["/bin/bash", "@snapshot/job.sh "], vec!["@snapshot/job.sh "]),
        (vec!["/bin/bash", "@snapshot/job.sh", "@snapshot "], vec!["@snapshot/job.sh"]),
        (vec!["/bin/bash", "@snapshot/job.sh"], vec!["@scratch/x"]),
        (vec!["/bin/bash", "@snapshot/job.sh"], vec!["@snapshot"]),
        (vec!["/bin/bash", "@scratch/job.sh"], vec!["@scratch/job.sh"]),
    ] {
        assert!(check_entry(&e(&argv, &files), &p).is_err(), "{argv:?} {files:?} was accepted");
    }
}

#[test]
fn a_symbolic_record_is_hashed_with_its_scheme_and_a_scheme_change_is_seen() {
    let f = fix();
    let (r, _) = made(&f, 1, None);
    assert_eq!(r.token_scheme, 1);
    let mut old = r.clone();
    old.token_scheme = 0;
    assert!(pure(&f, &old).unwrap_err().contains("recorded hash"), "the scheme is covered by the manifest hash");
    let mut future = r.clone();
    future.token_scheme = 9;
    rehash(&mut future);
    assert!(pure(&f, &future).unwrap_err().contains("token scheme"));
}

// ---------------------------------------------------------------------------------------------- red-team of #607: the hash covers what was signed

#[test]
#[allow(clippy::type_complexity)]
fn the_manifest_hash_covers_the_tree_pin_its_absence_and_every_pinned_files_path_and_hash() {
    let f = fix();
    let (r, _) = made(&f, 1, None);
    assert_eq!(r.computed_manifest_hash(), r.manifest_hash);
    let each: Vec<(&str, Box<dyn Fn(&mut Record)>)> = vec![
        ("a tree pin added", Box::new(|r| r.snapshot = Some(SnapshotPin { source: "tracked_files".into(), tree_blake3: Some("a".repeat(64)) }))),
        ("the snapshot pin removed", Box::new(|r| r.snapshot = None)),
        ("the source changed", Box::new(|r| r.snapshot.as_mut().unwrap().source = "other".into())),
        ("a pinned file's hash", Box::new(|r| r.manifest[0].files[0].blake3 = "b".repeat(64))),
        ("a pinned file's path", Box::new(|r| r.manifest[0].files[0].path.push('x'))),
        ("a pinned file dropped", Box::new(|r| {
            r.manifest[0].files.pop();
        })),
    ];
    for (what, edit) in each {
        let mut bad = r.clone();
        edit(&mut bad);
        assert_ne!(bad.computed_manifest_hash(), r.manifest_hash, "{what} does not change the manifest hash");
        assert!(pure(&f, &bad).unwrap_err().contains("recorded hash") || pure(&f, &bad).is_err(), "{what}");
    }
    // a tree pin that is present, then removed (the --no-pin-tree look), is a different hash too
    let mut pinned = r.clone();
    pinned.snapshot = Some(SnapshotPin { source: "tracked_files".into(), tree_blake3: Some("c".repeat(64)) });
    rehash(&mut pinned);
    let mut stripped = pinned.clone();
    stripped.snapshot.as_mut().unwrap().tree_blake3 = None;
    assert_ne!(stripped.computed_manifest_hash(), pinned.manifest_hash);
    assert!(pure(&f, &stripped).unwrap_err().contains("recorded hash"));
}
