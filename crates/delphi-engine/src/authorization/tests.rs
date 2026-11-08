use super::*;
use std::os::unix::fs::PermissionsExt;

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
        NewRecord { purpose: "parser tests".into(), intent: "tests".into(), run: "run-1".into(), expires_in_secs: DEFAULT_EXPIRY_SECS, max_uses: DEFAULT_USES, entries, request: None }
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
    Who { kind: kind.clone(), label: "red-team/claude/opus".into(), role: Some("red-team".into()), model: "claude/opus".into(), span: Some("s1".into()), task: Some(run.into()), taint, run: run.into() }
}

fn admit_at(log: &[EventEnvelope], op: &[EventEnvelope], w: &Who, req: &Value, secs: i64) -> Outcome {
    let now = at(secs);
    admit(log, op, w, parse_request(req), &Clock { now_unix: 1_800_000_000 + secs, now_rfc: &now })
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
    assert!(check_entry(&f.entry()).is_ok());
    let bad = |mutate: &dyn Fn(&mut Entry)| {
        let mut e = f.entry();
        mutate(&mut e);
        check_entry(&e)
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
    assert_eq!(check_entry(&alias).unwrap().tool, "shell");
}

#[test]
fn the_deny_list_stops_merges_credentials_network_dispatch_and_protected_paths() {
    let f = fix();
    let denied = |argv: Vec<&str>| {
        let e = Entry { tool: "shell".into(), argv: argv.into_iter().map(String::from).collect(), files: vec![], runner_provenance: "project_snapshot".into() };
        matches!(check_entry(&e), Err(EntryError::Denied(_)))
    };
    for bin in ["/usr/bin/git", "/usr/local/bin/gh", "/usr/bin/curl", "/usr/bin/ssh", "/usr/local/bin/delphi", "/usr/local/bin/claude", "/usr/bin/security", "/bin/rm", "/usr/bin/cargo", "/usr/bin/npm"] {
        assert!(denied(vec![bin, "x"]), "{bin}");
    }
    for arg in ["/repo/.git/hooks/pre-commit", "--out=/repo/.git/config", "/srv/u/.delphi/operator/authorizations.json", "/srv/u/.delphi/operator/ratified.lock", "/repo/.githooks/x", "/repo/config.toml", "/srv/u/.ssh/id_rsa", "https://example.com/x", "git@github.com:a/b.git", "/repo/server.pem"] {
        assert!(denied(vec![&f.bin, "-I", arg]), "{arg}");
    }
    // python -m pip / http would fetch or serve
    for m in ["pip", "http.server", "urllib.request", "socket"] {
        assert!(denied(vec![&f.bin, "-I", "-m", m]), "{m}");
    }
    assert!(deny_reason(&f.entry()).is_none());
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
    let r = parse_request(&v).unwrap();
    assert_eq!(r.context.chars().count(), MAX_CONTEXT);
    assert!(!r.context.contains('\x1b'));
    for k in ["intent", "purpose", "context", "risk", "run_plan", "manifest"] {
        let mut w = f.request_json();
        w.as_object_mut().unwrap().remove(k);
        assert!(matches!(parse_request(&w), Err(ParseError::Invalid(_))), "{k} is required");
    }
    // 5 entries at most
    let mut w = f.request_json();
    w["manifest"] = json!((0..6).map(|n| json!({"tool": "shell", "argv": [f.bin, "-I", f.script, format!("{n}")], "files": [f.script], "runner_provenance": "project_snapshot"})).collect::<Vec<_>>());
    assert!(matches!(parse_request(&w), Err(ParseError::Invalid(m)) if m.contains("1 to 5")));
    w["manifest"].as_array_mut().unwrap().pop();
    assert!(parse_request(&w).is_ok());
    // unknown fields on an entry (an agent cannot hand in a binary hash, an env or a cwd)
    let mut w = f.request_json();
    w["manifest"][0]["env"] = json!({"PATH": "/tmp"});
    assert!(matches!(parse_request(&w), Err(ParseError::Invalid(_))));
    // the same action twice
    let mut w = f.request_json();
    w["manifest"] = json!([f.entry(), f.entry()]);
    assert!(matches!(parse_request(&w), Err(ParseError::Invalid(m)) if m.contains("twice")));
    // a denied entry carries the hash of the manifest as asked for
    let mut w = f.request_json();
    w["manifest"][0]["argv"] = json!(["/usr/bin/git", "push"]);
    assert!(matches!(parse_request(&w), Err(ParseError::Denied { .. })));
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
    let mh = manifest_hash(&parse_request(&f.request_json()).unwrap().manifest);
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
    add_record(&f.home, r.clone()).unwrap();
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
    serde_json::to_vec_pretty(&Store { version: 1, records: vec![r] }).unwrap()
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
    add_record(&f.home, r).unwrap();
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
    add_record(&f.home, r1).unwrap();
    fs::write(store_path(&f.home), forged_store(&f)).unwrap();
    let r2 = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_001, &at(1)).unwrap();
    let err = add_record(&f.home, r2).unwrap_err();
    assert!(err.contains("does not match its pin"), "{err}");
    assert!(revoke(&f.home, "ta-forged0001", &at(2)).is_err(), "nor is revoke a way to re-pin a tampered store");
}

#[test]
fn revocation_is_irreversible() {
    let f = fix();
    let r = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_000, &at(0)).unwrap();
    let id = r.id.clone();
    add_record(&f.home, r).unwrap();
    let r = revoke(&f.home, &id, &at(5)).unwrap();
    assert_eq!(r.revoked.as_deref(), Some(at(5).as_str()));
    assert!(revoke(&f.home, &id, &at(6)).unwrap_err().contains("already revoked"));
    let s = load(&f.home).unwrap();
    assert!(s.records[0].is_revoked());
    // the only way to a live grant is a NEW record
    let r2 = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_100, &at(100)).unwrap();
    assert_ne!(r2.id, id);
    add_record(&f.home, r2).unwrap();
    let s = load(&f.home).unwrap();
    assert!(s.records[0].is_revoked() && !s.records[1].is_revoked());
    assert!(revoke(&f.home, "ta-nonexistent", &at(7)).is_err());
}

#[test]
fn the_review_diffs_against_earlier_grants_and_replays_recorded_cards_as_data() {
    let f = fix();
    let r = build_record(&f.home, &f.new_record(vec![f.entry()]), 1_800_000_000, &at(0)).unwrap();
    add_record(&f.home, r.clone()).unwrap();
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

#[test]
fn nothing_consumes_a_record_in_this_slice() {
    // S1 grants no capability: no module that decides a card, runs a command or starts a run may read the store. The store's
    // readers are this module, and the operator's `delphi test` commands (crates/delphi/src/test_cmd.rs) and `delphi doctor`.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let needles = ["authorization::load", "authorization::mutate", "authorization::Store", "authorization::Record", "authorization::add_record", "authorization::revoke", "authorization::store_path", "authorization::load_or_ignore"];
    let mut files = vec![];
    for e in walkdir::WalkDir::new(&src).into_iter().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "rs")) {
        files.push(e.path().to_path_buf());
    }
    assert!(files.len() > 50);
    for p in files {
        if p.starts_with(src.join("authorization")) || p.file_name().is_some_and(|n| n == "authorization.rs") {
            continue;
        }
        let text = fs::read_to_string(&p).unwrap();
        for n in needles {
            assert!(!text.contains(n), "{} reads the authorization store ({n}); S1 grants no capability, an authorization cannot be consumed before S2", p.display());
        }
    }
    // and the engine's approval path never names an authorization at all
    for f in ["approval.rs", "exec.rs", "hooks.rs", "broker.rs"] {
        if let Ok(t) = fs::read_to_string(src.join(f)) {
            assert!(!t.to_lowercase().contains("authorization"), "{f} must not know about authorizations in S1");
        }
    }
}
