use super::*;
use std::os::unix::fs::PermissionsExt;

thread_local! {
    static POLICY: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The policy of this test: the fixture's fake runner is the only interpreter the "daemon" knows.
fn policy() -> Policy {
    Policy { interpreters: POLICY.with(|p| p.borrow().clone()) }
}

/// A tempdir with a fake runner named `python3`, a script, and a home with an operator lock, all canonical.
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
    let bin = root.join("python3");
    fs::write(&bin, b"#!fake runner").unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    let script = root.join("t.py");
    fs::write(&script, b"print(1)").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).unwrap();
    POLICY.with(|p| *p.borrow_mut() = vec![bin.to_string_lossy().into()]);
    Fix { home, bin: bin.to_string_lossy().into(), script: script.to_string_lossy().into(), _t: t }
}

impl Fix {
    fn entry(&self) -> Entry {
        Entry { tool: "shell".into(), argv: vec![self.bin.clone(), "-I".into(), self.script.clone()], files: vec![self.script.clone()], runner_provenance: "project_snapshot".into() }
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
    v["manifest"][0]["argv"] = json!([f.bin, "-I", f.script, format!("--case={n}")]);
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
        ("relative binary", bad(&|e| e.argv[0] = "python3".into())),
        ("dot-dot", bad(&|e| e.argv[0] = format!("{}/../python3", e.argv[0]))),
        ("glob", bad(&|e| e.argv.push("tests/*.py".into()))),
        ("substitution", bad(&|e| e.argv.push("$(id)".into()))),
        ("chaining", bad(&|e| e.argv.push("&&".into()))),
        ("inline code", bad(&|e| e.argv.insert(2, "-c".into()))),
        ("control char", bad(&|e| e.argv.push("a\x1bb".into()))),
        ("unknown provenance", bad(&|e| e.runner_provenance = "built_in_run".into())),
        ("unknown tool", bad(&|e| e.tool = "edit_file".into())),
        ("no -I", bad(&|e| drop(e.argv.remove(1)))),
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
        assert!(denied(vec![&f.bin, "-I", arg]), "{arg}");
    }
    assert!(deny_reason(&f.entry()).is_none());
}

// Red-team P1: `python3 -I -m <anything>` ran code nobody pinned (timeit, pdb, code, trace, cProfile, unittest, pickle, pydoc, ...).
#[test]
fn no_module_is_ever_run_with_dash_m() {
    let f = fix();
    for m in ["timeit", "pdb", "code", "trace", "cProfile", "unittest", "pickle", "pydoc", "antigravity", "http.server", "pip", "this", "runpy", "idlelib"] {
        for argv in [vec![f.bin.clone(), "-I".into(), "-m".into(), m.into()], vec![f.bin.clone(), "-I".into(), f.script.clone(), "-m".into(), m.into()]] {
            let mut e = f.entry();
            e.argv = argv.clone();
            let r = check_entry(&e, &policy());
            // `-m` before a script has nothing to run; `-m` after the script is the script's own argument and is harmless to the
            // runner, but the first form must never pass
            if argv[2] == "-m" {
                assert!(matches!(r, Err(EntryError::Invalid(_)) | Err(EntryError::Denied(_))), "-m {m} was admitted: {r:?}");
            }
        }
    }
    // not even with a script named after it
    let mut e = f.entry();
    e.argv = vec![f.bin.clone(), "-I".into(), "-m".into(), "timeit".into(), f.script.clone()];
    assert!(check_entry(&e, &policy()).is_err());
}

// Red-team P1: the runner must be an interpreter the daemon knows, whatever the request says about its provenance.
#[test]
fn a_project_local_or_self_declared_runner_is_not_an_interpreter() {
    let f = fix();
    let other = fs::canonicalize(f._t.path()).unwrap().join("tools");
    fs::create_dir_all(&other).unwrap();
    let local = other.join("python3");
    fs::write(&local, b"#!built in the run").unwrap();
    fs::set_permissions(&local, fs::Permissions::from_mode(0o755)).unwrap();
    for prov in ["project_snapshot", "daemon_installed"] {
        let mut e = f.entry();
        e.argv[0] = local.to_string_lossy().into();
        e.runner_provenance = prov.into();
        let r = check_entry(&e, &policy());
        assert!(matches!(&r, Err(EntryError::Invalid(m)) if m.contains("not an interpreter the daemon knows")), "{prov}: {r:?}");
    }
    // the shipped policy knows system locations only: the fixture's runner is not one of them
    assert!(check_entry(&f.entry(), &Policy::system()).is_err());
    for p in &Policy::system().interpreters {
        assert!(p.starts_with("/usr/") || p.starts_with("/opt/homebrew/"), "{p}");
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
    for c in ['\u{2060}', '\u{2064}', '\u{206a}', '\u{206f}', '\u{061c}', '\u{00ad}', '\u{034f}', '\u{3164}', '\u{ffa0}', '\u{202e}', '\u{200b}', '\u{feff}', '\u{e0041}'] {
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
    w["manifest"] = json!((0..6).map(|n| json!({"tool": "shell", "argv": [f.bin, "-I", f.script, format!("{n}")], "files": [f.script], "runner_provenance": "project_snapshot"})).collect::<Vec<_>>());
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
    assert!(refusal(admit_at(&mlog, &[], &w2, &denied, 3600)).0.contains("deny-list"), "a denied manifest is never staged, however often it comes back");
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
    p["request"]["manifest"][0]["argv"] = json!([f.bin, "-I", f.script, "--extra"]);
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
    assert_eq!(r.manifest[0].env.get("PATH").map(String::as_str), Some(""), "no PATH search");
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
    missing.argv[2] = format!("{}/none.py", f._t.path().display());
    missing.files = vec![missing.argv[2].clone()];
    assert!(make(f.new_record(vec![missing])).is_err());
    let ww = f._t.path().join("ww.py");
    fs::write(&ww, b"x").unwrap();
    fs::set_permissions(&ww, fs::Permissions::from_mode(0o666)).unwrap();
    let wwp = fs::canonicalize(&ww).unwrap().to_string_lossy().to_string();
    let mut e = f.entry();
    e.argv[2] = wwp.clone();
    e.files = vec![wwp];
    assert!(make(f.new_record(vec![e])).unwrap_err().contains("world-writable"));
    let link = fs::canonicalize(f._t.path()).unwrap().join("link.py");
    std::os::unix::fs::symlink(&f.script, &link).unwrap();
    let mut e = f.entry();
    e.argv[2] = link.to_string_lossy().into();
    e.files = vec![e.argv[2].clone()];
    assert!(make(f.new_record(vec![e])).unwrap_err().contains("not canonical"));
    let scratch = fs::canonicalize(f._t.path()).unwrap().join("scratch");
    fs::create_dir_all(&scratch).unwrap();
    let sp = scratch.join("s.py");
    fs::write(&sp, b"x").unwrap();
    let mut e = f.entry();
    e.argv[2] = sp.to_string_lossy().into();
    e.files = vec![e.argv[2].clone()];
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
    assert!(create_audited(&f.home, r2, p2).unwrap_err().contains("already answered"));
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
    assert!(create_audited(&f.home, r4, p4).unwrap_err().contains("another"));
    drop(held);
    // revoke is audited first, too
    let rec = revoke_audited(&f.home, &r.id, &at(50)).unwrap();
    assert!(rec.is_revoked());
    assert_eq!(read_audit(&f.home).unwrap().last().unwrap().kind, REVOKED);
    assert!(revoke_audited(&f.home, &r.id, &at(51)).unwrap_err().contains("already revoked"));
    assert_eq!(load(&f.home).unwrap().audit_head.unwrap().seq, read_audit(&f.home).unwrap().last().unwrap().seq, "the pin follows the log");
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
    e.argv[2] = s.to_string_lossy().into();
    e.files = vec![e.argv[2].clone()];
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

/// True when `text` uses `item` through `prefix::` as a whole word.
fn uses_item(text: &str, prefix: &str, item: &str) -> bool {
    let needle = format!("{prefix}::{item}");
    text.match_indices(&needle).any(|(i, _)| {
        let before_ok = i == 0 || !text[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_');
        let after = text[i + needle.len()..].chars().next();
        before_ok && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
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
        "crates/delphi/src/doctor.rs",          // verifies the pin
        "crates/delphi/tests/docs.rs",          // schemas
    ];
    const STORE_READERS: &[&str] = &["crates/delphi-engine/src/authorization.rs", "crates/delphi-engine/src/authorization/tests.rs", "crates/delphi/src/test_cmd.rs", "crates/delphi/src/test_cmd_tests.rs", "crates/delphi/src/doctor.rs", "crates/delphi/tests/docs.rs"];
    const STORE_ITEMS: &[&str] = &["load", "load_or_ignore", "mutate", "add_record", "revoke", "store_path", "Store", "Record", "build_record", "create_audited", "revoke_audited", "refuse_audited", "read_audit", "audit_dir", "open_audit", "append_audit", "AuditHead"];
    let sources = workspace_sources();
    assert!(sources.len() > 200, "the scan must see the whole workspace");
    for (rel, text) in &sources {
        let imports = text.contains("authorization::") || text.contains("::authorization") || text.contains("mod authorization");
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
    // the approval path, the executor and the hooks never name an authorization at all
    for (rel, text) in &sources {
        if ["approval.rs", "exec.rs", "hooks.rs", "broker.rs", "headless", "run.rs"].iter().any(|n| rel.ends_with(n)) && rel != "crates/delphi-engine/src/authorization/tests.rs" {
            assert!(!text.to_lowercase().contains("authorization:"), "{rel} must not know about `--on-card authorization:` in S1");
        }
    }
    // the scan itself sees the alias style used by service.rs
    assert!(uses_item("use crate::authorization as a;\n a::load(x)", "a", "load"));
    assert!(!uses_item("a::loader()", "a", "load"));
}
