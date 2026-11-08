use super::*;
use delphi_core::provenance::Tier;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::os::unix::fs::PermissionsExt;

const SECRET: &[u8] = b"12345678901234567890";

struct Script {
    tty: bool,
    answers: VecDeque<String>,
    prompts: Vec<String>,
}

impl Script {
    fn new(answers: &[&str]) -> Script {
        Script { tty: true, answers: answers.iter().map(|s| s.to_string()).collect(), prompts: vec![] }
    }
}

impl Terminal for Script {
    fn interactive(&self) -> Result<(), String> {
        if self.tty { Ok(()) } else { Err("not a terminal".into()) }
    }
    fn show(&mut self, _text: &str) {}
    fn ask(&mut self, prompt: &str) -> Option<String> {
        self.prompts.push(prompt.to_string());
        self.answers.pop_front()
    }
}

struct Env {
    _t: tempfile::TempDir,
    home: PathBuf,
    ws: PathBuf,
    bin: String,
    script: String,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn env() -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(t.path()).unwrap();
    let home = root.join("home");
    let ws = home.join("workspaces/w");
    std::fs::create_dir_all(home.join("operator")).unwrap();
    std::fs::create_dir_all(ws.join("local")).unwrap();
    delphi_core::lock::write(&home.join("operator/ratified.lock"), &delphi_core::lock::LockFile::default()).unwrap();
    let bin = root.join("python3");
    std::fs::write(&bin, b"#!fake runner").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let script = root.join("t.py");
    std::fs::write(&script, b"print(1)").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    Env { home, ws, bin: bin.to_string_lossy().into(), script: script.to_string_lossy().into(), _t: t }
}

fn good_factor(_: &Path) -> Result<Vec<u8>, String> {
    Ok(SECRET.to_vec())
}

impl Env {
    fn entry(&self, n: usize) -> auth::Entry {
        auth::Entry { tool: "shell".into(), argv: vec![self.bin.clone(), "-I".into(), self.script.clone(), format!("--case={n}")], files: vec![self.script.clone()], runner_provenance: "project_snapshot".into() }
    }

    fn request(&self, entries: usize) -> Value {
        json!({"intent": "run the parser tests", "purpose": "needs a Red shell", "manifest": (0..entries).map(|n| self.entry(n)).collect::<Vec<_>>(), "context": "I was reading the parser.", "risk": "runs a script in the sandbox", "run_plan": "once"})
    }

    /// Stage a request exactly as the daemon would: through `admit`, then into a mission log.
    fn stage(&self, mission: &str, kind: auth::Kind, taint: Tier, request: &Value) -> String {
        let host = kind == auth::Kind::Host;
        let who = auth::Who { kind, label: if host { "host:claude-code/opus".into() } else { "red-team/claude/opus".into() }, role: (!host).then(|| "red-team".into()), model: "opus".into(), span: (!host).then(|| "s1".into()), task: (!host).then(|| "t1".into()), taint, run: "t1".into() };
        let rfc = delphi_core::time::now_rfc3339();
        let p = match auth::admit(&[], &[], &who, auth::parse_request(request), &auth::Clock { now_unix: now(), now_rfc: &rfc }) {
            auth::Outcome::Staged(p) => p,
            auth::Outcome::Refused { message, .. } => panic!("{message}"),
        };
        self.append(mission, auth::STAGED, p.clone());
        p["id"].as_str().unwrap().to_string()
    }

    fn append(&self, mission: &str, kind: &str, payload: Value) {
        let mut log = delphi_core::log::EventLog::open(&delphi_core::sessions::dir(&self.ws, mission), Default::default()).unwrap();
        log.append(delphi_core::event::EventEnvelope::new(mission, kind, payload)).unwrap();
    }

    fn deps(&self) -> Deps<'static> {
        Deps { now_unix: now(), factor: &good_factor }
    }

    fn code(&self) -> String {
        delphi_core::totp::code_at(SECRET, now() as u64)
    }

    fn run(&self, args: &[&str], t: &mut Script) -> i32 {
        let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        run(&self.home, &self.ws, &a, t, &self.deps())
    }

    fn store(&self) -> auth::Store {
        auth::load(&self.home).unwrap()
    }
}

fn line(e: &auth::Entry) -> String {
    auth::render_argv(&e.argv)
}

// ---------------------------------------------------------------------------------------------- review

#[test]
fn review_shows_its_items_in_the_order_of_the_spec_and_collapses_the_agents_words() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    // a card to replay against, and a poisoned log line: a request whose text carries escape sequences straight into the log
    e.append("m1", "approval.requested", json!({"tool": "run_command", "payload": {"command": line(&e.entry(0))}, "summary": "x"}));
    let mut req: auth::Request = serde_json::from_value(e.request(1)).unwrap();
    req.manifest = vec![e.entry(0)];
    req.context = "I saw this\x1b[2K\x1b[1A and so on \u{202e}reversed".into();
    req.risk = "RISK-WORDS".into();
    let (mh, rh) = (auth::manifest_hash(&req.manifest), auth::request_hash(&req));
    e.append("m2", auth::STAGED, json!({"id": "ar-0123456789", "tier": "L0", "request": req, "manifest_hash": mh, "request_hash": rh, "requester": {"kind": "agent", "label": "red-team/\x1b[31mclaude", "role": "red-team", "model": "m", "span": "s", "task": "t"}, "taint": {"tier": "L4", "name": "internal_empirical"}, "run": "t9", "staged_at": delphi_core::time::now_rfc3339()}));
    let logs = read_logs(&e.home, &e.ws);
    let s = find(&logs, &id).unwrap();
    let card = render_review(s, &logs, &auth::Store::default(), false);
    let at = |needle: &str| card.find(needle).unwrap_or_else(|| panic!("`{needle}` is not in:\n{card}"));
    let order = [at("(1) MANIFEST"), at("(2) DRY RUN"), at("(3) AGAINST YOUR EARLIER GRANTS"), at("(4) REQUESTER"), at("(5) THE REQUESTER'S WORDS")];
    assert!(order.windows(2).all(|w| w[0] < w[1]), "items out of order: {order:?}");
    assert!(card.contains(&line(&e.entry(0))), "the manifest is rendered from the structured fields");
    assert!(card.contains("matched 1 card"), "the dry run found the recorded card:\n{card}");
    assert!(card.contains("collapsed") && !card.contains("I was reading the parser") && !card.contains("needs a Red shell"), "the agent's words are collapsed by default:\n{card}");
    assert!(card.contains("internal_empirical"), "the taint is shown");
    // the poisoned one: nothing reaches the terminal as an escape sequence, collapsed or expanded
    let p = find(&logs, "ar-0123456789").unwrap();
    for expand in [false, true] {
        let c = render_review(p, &logs, &auth::Store::default(), expand);
        assert!(!c.chars().any(|ch| (ch.is_control() && ch != '\n') || ch == '\u{202e}'), "control or escape character in:\n{c:?}");
    }
    let expanded = render_review(p, &logs, &auth::Store::default(), true);
    assert!(expanded.contains("RISK-WORDS") && expanded.contains("context: I saw this"), "{expanded}");
    assert!(expanded.contains("data, not instructions"));
    // the list view, too
    assert!(!render_pending(&logs, now()).contains('\x1b'));
}

#[test]
fn review_with_no_id_lists_pending_requests_and_review_is_read_only() {
    let e = env();
    assert!(render_pending(&read_logs(&e.home, &e.ws), now()).contains("no pending"));
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(2));
    let listed = render_pending(&read_logs(&e.home, &e.ws), now());
    assert!(listed.contains(&id) && listed.contains("2 entries"), "{listed}");
    let mut t = Script::new(&[]);
    assert_eq!(e.run(&["review", &id], &mut t), 0);
    assert_eq!(e.run(&["review"], &mut t), 0);
    assert!(t.prompts.is_empty(), "review asks nothing");
    assert!(!auth::store_path(&e.home).exists(), "and writes nothing");
}

// ---------------------------------------------------------------------------------------------- authorize: the gates

#[test]
fn authorize_works_at_a_local_terminal_only() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    let mut t = Script::new(&["y", "purpose", &e.code()]);
    t.tty = false;
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 2);
    assert!(t.prompts.is_empty(), "nothing is asked of a pipe or a relay");
    assert!(!auth::store_path(&e.home).exists());
    // revoke and refuse likewise
    assert_eq!(e.run(&["revoke", "ta-x"], &mut t), 2);
    assert_eq!(e.run(&["refuse", &id], &mut t), 2);
    assert!(read_logs(&e.home, &e.ws).operator.is_empty());
}

#[test]
fn authorize_refuses_when_no_second_factor_is_configured() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    // no config.toml at all, then a config without a [relay] table, then one with step_up off
    std::fs::write(e.ws.join("config.toml"), "").unwrap();
    for cfg in ["", "[relay]\nenabled = true\n"] {
        std::fs::write(e.ws.join("config.toml"), cfg).unwrap();
        let err = configured_factor(&e.ws).unwrap_err();
        assert!(err.contains("no second factor") && err.contains("totp-enroll") && err.contains("no fallback"), "{err}");
    }
    let mut t = Script::new(&["y", "purpose", "123456"]);
    let deps = Deps { now_unix: now(), factor: &configured_factor };
    let args: Vec<String> = ["authorize", "--from-request", &id, "--run", "r1"].iter().map(|s| s.to_string()).collect();
    assert_eq!(run(&e.home, &e.ws, &args, &mut t, &deps), 2);
    assert!(t.prompts.is_empty(), "it refuses before it asks anything, and there is no fallback to no step-up");
    assert!(!auth::store_path(&e.home).exists());
}

#[test]
fn a_wrong_authenticator_code_authorizes_nothing() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    let mut t = Script::new(&["y", "my purpose", "000000"]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 2);
    assert!(!auth::store_path(&e.home).exists());
    assert!(read_logs(&e.home, &e.ws).operator.is_empty());
}

// ---------------------------------------------------------------------------------------------- authorize: retype and confirm

#[test]
fn a_host_request_is_authorized_only_by_retyping_each_argv() {
    let e = env();
    let id = e.stage("host-sess0001", auth::Kind::Host, auth::HOST_TAINT, &e.request(2));
    let want = [line(&e.entry(0)), line(&e.entry(1))];
    // CONFIRM is not accepted for a host, nor a near miss, nor a prefilled copy of the wrong entry
    for wrong in ["CONFIRM", "y", "", &format!("{} ", want[1]), &want[0].replace("-I", "-i")] {
        let mut t = Script::new(&[wrong, &want[1], "p", &e.code()]);
        assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 2, "{wrong:?}");
        assert!(!auth::store_path(&e.home).exists(), "{wrong:?} authorized something");
    }
    // each entry typed exactly (surrounding whitespace is the terminal's)
    let mut t = Script::new(&[&want[0], &format!("  {}  ", want[1]), "host asked for parser tests", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 0, "{:?}", t.prompts);
    assert!(t.prompts[0].contains("retype") && !t.prompts[0].contains(&want[0]), "the command is not offered to copy at the prompt: {:?}", t.prompts[0]);
    let s = e.store();
    assert_eq!(s.records.len(), 1);
    let r = &s.records[0];
    assert_eq!(r.request_id.as_deref(), Some(id.as_str()));
    assert!(r.request_hash.is_some() && r.requester.as_deref() == Some("host:claude-code/opus"));
    assert_eq!((r.run.as_str(), r.max_uses, r.manifest.len(), r.uses), ("r1", auth::DEFAULT_USES, 2, 0));
    assert_eq!(r.purpose, "host asked for parser tests");
    // it is a NEW record, the request is answered, and the audit log (daemon-side, not an agent's) holds the event
    assert!(auth::resolved_ids(&auth::read_audit(&e.home)).contains(&id));
    let audit = auth::read_audit(&e.home);
    assert_eq!(audit.len(), 1);
    assert_eq!((audit[0].kind.as_str(), audit[0].payload["id"].as_str(), audit[0].payload["step_up"].as_str()), (auth::CREATED, Some(r.id.as_str()), Some("totp")));
    assert!(render_pending(&read_logs(&e.home, &e.ws), now()).contains("no pending"));
    // and the same request cannot be authorized twice
    let mut t = Script::new(&[&want[0], &want[1], "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r2"], &mut t), 2);
    assert_eq!(e.store().records.len(), 1);
}

#[test]
fn an_agent_span_below_internal_empirical_may_be_retyped_or_confirmed_and_above_it_is_asked_once() {
    let e = env();
    let low = e.stage("m1", auth::Kind::Agent, Tier::L3, &e.request(2));
    let want = line(&e.entry(1));
    // one retyped, one confirmed
    let mut t = Script::new(&[&line(&e.entry(0)), "CONFIRM", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", &low, "--run", "r1"], &mut t), 0, "{:?}", t.prompts);
    assert!(t.prompts[0].contains("CONFIRM"));
    // a lowercase "confirm" is not CONFIRM
    let e2 = env();
    let low2 = e2.stage("m1", auth::Kind::Agent, Tier::L3, &e2.request(1));
    let mut t = Script::new(&["confirm", "p", &e2.code()]);
    assert_eq!(e2.run(&["authorize", "--from-request", &low2, "--run", "r1"], &mut t), 2);
    // at internal_empirical the operator is asked once
    let e3 = env();
    let hi = e3.stage("m1", auth::Kind::Agent, Tier::L4, &e3.request(2));
    let mut t = Script::new(&["y", "p", &e3.code()]);
    assert_eq!(e3.run(&["authorize", "--from-request", &hi, "--run", "r1"], &mut t), 0, "{:?}", t.prompts);
    assert_eq!(t.prompts.iter().filter(|p| p.contains("retype")).count(), 0);
    let _ = want;
}

#[test]
fn the_operator_may_narrow_the_manifest_and_shorten_the_limits_but_not_widen_them() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(3));
    let mut t = Script::new(&["y", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1", "--drop", "1", "--drop", "3", "--expires", "30m", "--uses", "3"], &mut t), 0);
    let r = &e.store().records[0];
    assert_eq!(r.manifest.len(), 1);
    assert_eq!(r.manifest[0].argv, e.entry(1).argv, "entry 2 is the one that stayed");
    assert_eq!(r.max_uses, 3);
    let (c, x) = (delphi_core::time::parse_rfc3339(&r.created).unwrap(), delphi_core::time::parse_rfc3339(&r.expires).unwrap());
    assert_eq!(x - c, 30 * 60);
    // over the maximum, no run id, every entry dropped, a bad entry number: all refused
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    for args in [vec!["--run", "r", "--expires", "25h"], vec!["--run", "r", "--uses", "51"], vec!["--run", "r", "--uses", "0"], vec!["--run", "r", "--drop", "1"], vec!["--run", "r", "--drop", "7"], vec![]] {
        let mut t = Script::new(&["y", "p", &e.code()]);
        let mut a = vec!["authorize", "--from-request", id.as_str()];
        a.extend(args.iter().copied());
        assert_eq!(e.run(&a, &mut t), 2, "{args:?}");
        assert!(!auth::store_path(&e.home).exists(), "{args:?}");
    }
    // the defaults are the first-slice ones: 2 hours, 10 uses
    let mut t = Script::new(&["y", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 0);
    let r = &e.store().records[0];
    let (c, x) = (delphi_core::time::parse_rfc3339(&r.created).unwrap(), delphi_core::time::parse_rfc3339(&r.expires).unwrap());
    assert_eq!((x - c, r.max_uses), (7200, 10));
}

#[test]
fn the_deny_list_rejects_an_entry_when_the_record_is_made() {
    let e = env();
    for argv in [json!(["/usr/bin/git", "push", "origin", "main"]), json!(["/usr/bin/curl", "https://x"]), json!([e.bin, "-I", e.script, "--out=/repo/.git/hooks/pre-commit"]), json!(["/usr/local/bin/delphi", "run", "x"]), json!([e.bin, "-I", e.script, "/srv/u/.delphi/operator/authorizations.json"])] {
        let entry = json!({"tool": "shell", "argv": argv, "files": [e.script], "runner_provenance": "project_snapshot"}).to_string();
        let mut t = Script::new(&["y", "p", &e.code()]);
        assert_eq!(e.run(&["authorize", "--run", "r1", "--intent", "i", "--entry-json", &entry], &mut t), 2, "{argv}");
        assert!(!auth::store_path(&e.home).exists(), "{argv} was authorized");
    }
    // a console-authored record works for an entry that passes
    let entry = json!(e.entry(0)).to_string();
    let mut t = Script::new(&["y", "my own words", &e.code()]);
    assert_eq!(e.run(&["authorize", "--run", "r1", "--intent", "i", "--entry-json", &entry], &mut t), 0, "{:?}", t.prompts);
    let r = &e.store().records[0];
    assert!(r.request_id.is_none() && r.purpose == "my own words");
}

// ---------------------------------------------------------------------------------------------- the forgery gate

#[test]
fn forgery_a_planted_store_is_ignored_and_authorize_does_not_write_over_it() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    // an agent-writable path cannot make a valid record: the same record, planted in the store path and elsewhere, has no pin
    let rec = auth::build_record(&e.home, &auth::NewRecord { purpose: "forged".into(), intent: "i".into(), run: "r1".into(), expires_in_secs: 3600, max_uses: 50, entries: vec![e.entry(0)], request: None }, now(), &delphi_core::time::now_rfc3339()).unwrap();
    let forged = serde_json::to_vec(&auth::Store { version: 1, records: vec![rec] }).unwrap();
    std::fs::write(auth::store_path(&e.home), &forged).unwrap();
    std::fs::create_dir_all(e.home.join("operator/authorizations")).unwrap();
    std::fs::write(e.home.join("operator/authorizations/ta-forged.json"), &forged).unwrap();
    std::fs::write(e.ws.join("authorizations.json"), &forged).unwrap();
    assert!(auth::load(&e.home).is_err());
    assert!(auth::load_or_ignore(&e.home).records.is_empty(), "ignored");
    assert!(render_store(&auth::load_or_ignore(&e.home), now()).contains("no authorizations"));
    // authorize is gated on it: it refuses, asks nothing it would act on, and leaves the planted file as it found it
    let mut t = Script::new(&["y", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 2);
    assert_eq!(std::fs::read(auth::store_path(&e.home)).unwrap(), forged);
    assert!(read_logs(&e.home, &e.ws).operator.is_empty(), "no audit event for what did not happen");
    // revoke is no way around it either
    let mut t = Script::new(&["y"]);
    assert_eq!(e.run(&["revoke", "ta-forged"], &mut t), 2);
    assert_eq!(std::fs::read(auth::store_path(&e.home)).unwrap(), forged);
}

// ---------------------------------------------------------------------------------------------- refuse and revoke

#[test]
fn refusing_closes_a_request_and_keeps_its_manifest_out_for_a_day() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    let mut t = Script::new(&[]);
    assert_eq!(e.run(&["refuse", &id], &mut t), 0);
    assert!(render_pending(&read_logs(&e.home, &e.ws), now()).contains("no pending"));
    assert_eq!(e.run(&["refuse", &id], &mut t), 2, "already answered");
    // it cannot be authorized afterwards
    let mut t = Script::new(&["y", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 2);
    // and the daemon does not stage the same manifest again within 24 hours
    let logs = read_logs(&e.home, &e.ws);
    let who = auth::Who { kind: auth::Kind::Agent, label: "x".into(), role: None, model: "m".into(), span: None, task: None, taint: Tier::L4, run: "t7".into() };
    let rfc = delphi_core::time::now_rfc3339();
    let again = auth::admit(&[], &logs.operator, &who, auth::parse_request(&e.request(1)), &auth::Clock { now_unix: now(), now_rfc: &rfc });
    assert!(matches!(&again, auth::Outcome::Refused { message, .. } if message.contains("24 hours")), "{again:?}");
}

#[test]
fn revocation_is_irreversible_and_audited() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    let mut t = Script::new(&["y", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 0);
    let ta = e.store().records[0].id.clone();
    // not without a terminal, not without a yes
    let mut t = Script::new(&["y"]);
    t.tty = false;
    assert_eq!(e.run(&["revoke", &ta], &mut t), 2);
    let mut t = Script::new(&["n"]);
    assert_eq!(e.run(&["revoke", &ta], &mut t), 2);
    assert!(!e.store().records[0].is_revoked());
    let mut t = Script::new(&["y"]);
    assert_eq!(e.run(&["revoke", &ta], &mut t), 0);
    assert!(e.store().records[0].is_revoked());
    assert!(render_store(&e.store(), now()).contains("revoked"));
    let kinds: Vec<String> = auth::read_audit(&e.home).iter().map(|x| x.kind.clone()).collect();
    assert_eq!(kinds, [auth::CREATED, auth::REVOKED]);
    // there is no way back: a second revoke fails, and so does authorizing the answered request again
    let mut t = Script::new(&["y"]);
    assert_eq!(e.run(&["revoke", &ta], &mut t), 2);
    assert!(e.store().records[0].is_revoked());
    // the pin still verifies after the change
    assert!(auth::load(&e.home).is_ok());
}

#[test]
fn the_store_file_is_owner_only_and_pinned_in_the_operator_lock() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    let mut t = Script::new(&["y", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 0);
    let mode = std::fs::metadata(auth::store_path(&e.home)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let lock = delphi_core::lock::read(&e.home.join("operator/ratified.lock")).unwrap();
    assert_eq!(lock.files.get(auth::LOCK_KEY).cloned(), Some(delphi_core::lock::hash(&std::fs::read(auth::store_path(&e.home)).unwrap())));
    // any edit to the file breaks the pin
    let mut text = std::fs::read_to_string(auth::store_path(&e.home)).unwrap();
    text = text.replace("\"max_uses\": 10", "\"max_uses\": 50");
    std::fs::write(auth::store_path(&e.home), text).unwrap();
    assert!(auth::load(&e.home).is_err());
}

#[test]
fn a_log_line_claiming_an_agent_request_below_the_gate_is_not_authorized_and_a_hostile_argv_is_not_shown_raw() {
    let e = env();
    let mut req: auth::Request = serde_json::from_value(e.request(1)).unwrap();
    req.manifest[0].argv.push("tail\x1b[2K\u{202e}".into());
    let (mh, rh) = (auth::manifest_hash(&req.manifest), auth::request_hash(&req));
    e.append("m1", auth::STAGED, json!({"id": "ar-aaaaaaaaaa", "tier": "L0", "request": req, "manifest_hash": mh, "request_hash": rh, "requester": {"kind": "agent", "label": "x", "role": "r", "model": "m", "span": "s", "task": "t"}, "taint": {"tier": "L1", "name": "federated_peer"}, "run": "t", "staged_at": delphi_core::time::now_rfc3339()}));
    let logs = read_logs(&e.home, &e.ws);
    let card = render_review(find(&logs, "ar-aaaaaaaaaa").unwrap(), &logs, &auth::Store::default(), true);
    assert!(!card.chars().any(|c| (c.is_control() && c != '\n') || c == '\u{202e}'), "{card:?}");
    let mut t = Script::new(&["y", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", "ar-aaaaaaaaaa", "--run", "r1"], &mut t), 2);
    assert!(t.prompts.is_empty() && !auth::store_path(&e.home).exists());
}

#[test]
fn durations_parse() {
    assert_eq!((parse_duration("2h"), parse_duration("90m"), parse_duration("45s"), parse_duration("600")), (Some(7200), Some(5400), Some(45), Some(600)));
    assert_eq!((parse_duration("0"), parse_duration("-5m"), parse_duration("h"), parse_duration("")), (None, None, None, None));
}
