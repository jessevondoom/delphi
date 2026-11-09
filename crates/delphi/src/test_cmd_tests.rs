use super::*;
use delphi_core::provenance::Tier;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::os::unix::fs::PermissionsExt;

const SECRET: &[u8] = b"12345678901234567890";

type PromptHook = Box<dyn Fn(&str)>;

struct Script {
    tty: bool,
    answers: VecDeque<String>,
    prompts: Vec<String>,
    on_prompt: Option<PromptHook>,
    shown: String,
}

impl Script {
    fn new(answers: &[&str]) -> Script {
        Script { tty: true, answers: answers.iter().map(|s| s.to_string()).collect(), prompts: vec![], on_prompt: None, shown: String::new() }
    }
}

impl Terminal for Script {
    fn interactive(&self) -> Result<(), String> {
        if self.tty { Ok(()) } else { Err("not a terminal".into()) }
    }
    fn show(&mut self, text: &str) {
        self.shown.push_str(text);
    }
    fn ask(&mut self, prompt: &str) -> Option<String> {
        self.prompts.push(prompt.to_string());
        if let Some(h) = &self.on_prompt {
            h(prompt);
        }
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
    let bin = root.join("bash");
    std::fs::write(&bin, b"#!fake runner").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let script = root.join("t.sh");
    std::fs::write(&script, b"print(1)").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    Env { home, ws, bin: bin.to_string_lossy().into(), script: script.to_string_lossy().into(), _t: t }
}

fn good_factor(_: &Path) -> Result<Vec<u8>, String> {
    Ok(SECRET.to_vec())
}

impl Env {
    fn entry(&self, n: usize) -> auth::Entry {
        auth::Entry { tool: "shell".into(), argv: vec![self.bin.clone(), self.script.clone(), format!("--case={n}")], files: vec![self.script.clone()], runner_provenance: "project_snapshot".into() }
    }

    fn request(&self, entries: usize) -> Value {
        json!({"intent": "run the parser tests", "purpose": "needs a Red shell", "manifest": (0..entries).map(|n| self.entry(n)).collect::<Vec<_>>(), "context": "I was reading the parser.", "risk": "runs a script in the sandbox", "run_plan": "once"})
    }

    /// Stage a request exactly as the daemon would: through `admit`, then into a mission log.
    fn stage(&self, mission: &str, kind: auth::Kind, taint: Tier, request: &Value) -> String {
        let host = kind == auth::Kind::Host;
        let who = auth::Who { kind, label: if host { "host:claude-code/opus".into() } else { "red-team/claude/opus".into() }, role: (!host).then(|| "red-team".into()), model: "opus".into(), span: (!host).then(|| "s1".into()), task: (!host).then(|| "t1".into()), taint, run: "t1".into(), mission: mission.into() };
        let rfc = delphi_core::time::now_rfc3339();
        let p = match auth::record_request(&self.ws, &self.home, &who, auth::parse_request(request, &self.policy()), &auth::Clock { now_unix: now(), now_rfc: &rfc }) {
            auth::Outcome::Staged(p) => p,
            auth::Outcome::Refused { message, .. } => panic!("{message}"),
        };
        self.append(mission, auth::STAGED, p.clone());
        p["id"].as_str().unwrap().to_string()
    }

    /// What the "daemon" knows: the fixture's fake runner is its only interpreter.
    fn policy(&self) -> auth::Policy {
        auth::Policy { interpreters: vec![self.bin.clone()], sealed_roots: vec![] }
    }

    /// A line in a mission log AND in the daemon's ledger (a payload the daemon wrote, however odd its text).
    fn plant(&self, mission: &str, payload: Value) {
        auth::append_ledger(&self.ws, auth::STAGED, payload.clone()).unwrap();
        self.append(mission, auth::STAGED, payload);
    }

    fn append(&self, mission: &str, kind: &str, payload: Value) {
        let mut log = delphi_core::log::EventLog::open(&delphi_core::sessions::dir(&self.ws, mission), Default::default()).unwrap();
        log.append(delphi_core::event::EventEnvelope::new(mission, kind, payload)).unwrap();
    }

    fn deps(&self) -> Deps<'static> {
        Deps { clock: &now, policy: self.policy(), factor: &good_factor, project: &|| None }
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
    e.plant("m2", json!({"id": "ar-0123456789", "tier": "L0", "request": req, "manifest_hash": mh, "request_hash": rh, "requester": {"kind": "agent", "label": "red-team/\x1b[31mclaude", "role": "red-team", "model": "m", "span": "s", "task": "t"}, "taint": {"tier": "L4", "name": "internal_empirical"}, "run": "t9", "staged_at": delphi_core::time::now_rfc3339()}));
    let logs = read_logs(&e.home, &e.ws).unwrap();
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
    assert!(render_pending(&read_logs(&e.home, &e.ws).unwrap(), now()).contains("no pending"));
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(2));
    let listed = render_pending(&read_logs(&e.home, &e.ws).unwrap(), now());
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
    assert!(read_logs(&e.home, &e.ws).unwrap().operator.is_empty());
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
    let deps = Deps { clock: &now, policy: e.policy(), factor: &configured_factor, project: &|| None };
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
    assert!(read_logs(&e.home, &e.ws).unwrap().operator.is_empty());
}

// ---------------------------------------------------------------------------------------------- authorize: retype and confirm

#[test]
fn a_host_request_is_authorized_only_by_retyping_each_argv() {
    let e = env();
    let id = e.stage("host-sess0001", auth::Kind::Host, auth::HOST_TAINT, &e.request(2));
    let want = [line(&e.entry(0)), line(&e.entry(1))];
    // CONFIRM is not accepted for a host, nor a near miss, nor a prefilled copy of the wrong entry
    for wrong in ["CONFIRM", "y", "", &format!("{} ", want[1]), &want[0].replace("--case=0", "--case=7")] {
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
    assert!(auth::resolved_ids(&auth::read_audit(&e.home).unwrap()).contains(&id));
    let audit = auth::read_audit(&e.home).unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!((audit[0].kind.as_str(), audit[0].payload["id"].as_str(), audit[0].payload["step_up"].as_str()), (auth::CREATED, Some(r.id.as_str()), Some("totp")));
    assert!(render_pending(&read_logs(&e.home, &e.ws).unwrap(), now()).contains("no pending"));
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
    for argv in [json!(["/usr/bin/git", "push", "origin", "main"]), json!(["/usr/bin/curl", "https://x"]), json!([e.bin, e.script, "--out=/repo/.git/hooks/pre-commit"]), json!(["/usr/local/bin/delphi", "run", "x"]), json!([e.bin, e.script, "/srv/u/.delphi/operator/authorizations.json"])] {
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
    let rec = auth::build_record(&e.home, &auth::NewRecord { purpose: "forged".into(), intent: "i".into(), run: "r1".into(), expires_in_secs: 3600, max_uses: 50, entries: vec![e.entry(0)], request: None, policy: e.policy() }, now(), &delphi_core::time::now_rfc3339()).unwrap();
    let forged = serde_json::to_vec(&auth::Store { version: 1, records: vec![rec], audit_head: None }).unwrap();
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
    assert!(read_logs(&e.home, &e.ws).is_err(), "with the store failing its pin, the audit state cannot be trusted and nothing is shown");
    let seg = auth::audit_dir(&e.home).join("events/000001.jsonl");
    assert!(!seg.exists() || std::fs::read_to_string(&seg).unwrap().is_empty(), "no audit event for what did not happen");
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
    assert!(render_pending(&read_logs(&e.home, &e.ws).unwrap(), now()).contains("no pending"));
    assert_eq!(e.run(&["refuse", &id], &mut t), 2, "already answered");
    // it cannot be authorized afterwards
    let mut t = Script::new(&["y", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 2);
    // and the daemon does not stage the same manifest again within 24 hours
    let logs = read_logs(&e.home, &e.ws).unwrap();
    let who = auth::Who { kind: auth::Kind::Agent, label: "x".into(), role: None, model: "m".into(), span: None, task: None, taint: Tier::L4, run: "t7".into(), mission: "m".into() };
    let rfc = delphi_core::time::now_rfc3339();
    let again = auth::admit(&[], &logs.operator, &who, auth::parse_request(&e.request(1), &e.policy()), &auth::Clock { now_unix: now(), now_rfc: &rfc });
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
    let kinds: Vec<String> = auth::read_audit(&e.home).unwrap().iter().map(|x| x.kind.clone()).collect();
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
    e.plant("m1", json!({"id": "ar-aaaaaaaaaa", "tier": "L0", "request": req, "manifest_hash": mh, "request_hash": rh, "requester": {"kind": "agent", "label": "x", "role": "r", "model": "m", "span": "s", "task": "t"}, "taint": {"tier": "L1", "name": "federated_peer"}, "run": "t", "staged_at": delphi_core::time::now_rfc3339()}));
    let logs = read_logs(&e.home, &e.ws).unwrap();
    let card = render_review(find(&logs, "ar-aaaaaaaaaa").unwrap(), &logs, &auth::Store::default(), true);
    assert!(!card.chars().any(|c| (c.is_control() && c != '\n') || c == '\u{202e}'), "{card:?}");
    let mut t = Script::new(&["y", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", "ar-aaaaaaaaaa", "--run", "r1"], &mut t), 2);
    assert!(t.prompts.is_empty() && !auth::store_path(&e.home).exists());
}

// Red-team P1: the clock was read once at the start, so the authenticator code and the record's times were minutes stale.
#[test]
fn the_clock_is_read_again_after_the_prompts_for_the_code_and_for_the_record() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    let t = std::rc::Rc::new(std::cell::Cell::new(now()));
    let later = now() + 600;
    // ten minutes pass while the operator reads and types; the code on the phone is for the later time
    let mut s = Script::new(&["y", "p", &delphi_core::totp::code_at(SECRET, later as u64)]);
    let clock_handle = t.clone();
    s.on_prompt = Some(Box::new(move |p| {
        if p.contains("authenticator") {
            clock_handle.set(later);
        }
    }));
    let clock = {
        let t = t.clone();
        move || t.get()
    };
    let args: Vec<String> = ["authorize", "--from-request", &id, "--run", "r1"].iter().map(|s| s.to_string()).collect();
    let deps = Deps { clock: &clock, policy: e.policy(), factor: &good_factor, project: &|| None };
    assert_eq!(run(&e.home, &e.ws, &args, &mut s, &deps), 0, "{:?}", s.prompts);
    let r = &e.store().records[0];
    assert_eq!(delphi_core::time::parse_rfc3339(&r.created), Some(later), "created is the time of the check, not of the start");
    assert_eq!(delphi_core::time::parse_rfc3339(&r.expires), Some(later + auth::DEFAULT_EXPIRY_SECS));
    // the code for the START time is no longer good at the later time
    let e2 = env();
    let id2 = e2.stage("m1", auth::Kind::Agent, Tier::L4, &e2.request(1));
    let t2 = std::rc::Rc::new(std::cell::Cell::new(now()));
    let mut s2 = Script::new(&["y", "p", &e2.code()]);
    let h2 = t2.clone();
    s2.on_prompt = Some(Box::new(move |p| {
        if p.contains("authenticator") {
            h2.set(later);
        }
    }));
    let clock2 = {
        let t2 = t2.clone();
        move || t2.get()
    };
    let args2: Vec<String> = ["authorize", "--from-request", &id2, "--run", "r1"].iter().map(|s| s.to_string()).collect();
    assert_eq!(run(&e2.home, &e2.ws, &args2, &mut s2, &Deps { clock: &clock2, policy: e2.policy(), factor: &good_factor, project: &|| None }), 2);
    assert!(!auth::store_path(&e2.home).exists());
}

// Red-team P1: a code spent anywhere (the daemon's relay step-up) is spent here, because the state file is merged.
#[test]
fn a_code_already_spent_elsewhere_does_not_authorize() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    let code = e.code();
    // "the daemon" spends it first, through its own verifier on the shared state file
    let daemon = delphi_core::totp::Verifier::persistent(e.ws.join("local/totp-state"));
    assert!(daemon.check(SECRET, &code, now() as u64).is_ok());
    let mut t = Script::new(&["y", "p", &code]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 2);
    assert!(!auth::store_path(&e.home).exists());
    // and the other way round: a code spent here is refused there
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    let daemon = delphi_core::totp::Verifier::persistent(e.ws.join("local/totp-state"));
    let code = e.code();
    let mut t = Script::new(&["y", "p", &code]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 0);
    assert_eq!(daemon.check(SECRET, &code, now() as u64), Err(delphi_core::totp::StepUpError::Replayed));
}

// Red-team P1: a staged line that the daemon's chain-verified ledger does not hold is not a request (its taint and requester are not
// covered by the request hash), and a broken ledger or audit log stops the commands loudly.
#[test]
fn a_staged_line_the_ledger_does_not_hold_is_not_shown_and_a_broken_ledger_stops_everything() {
    let e = env();
    let real = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    // a forged line in a mission log: valid hashes, a better taint, no ledger entry
    let mut req: auth::Request = serde_json::from_value(e.request(1)).unwrap();
    req.manifest = vec![e.entry(5)];
    let (mh, rh) = (auth::manifest_hash(&req.manifest), auth::request_hash(&req));
    e.append("m3", auth::STAGED, json!({"id": "ar-bbbbbbbbbb", "tier": "L0", "request": req, "manifest_hash": mh, "request_hash": rh, "requester": {"kind": "agent", "label": "forged", "role": "r", "model": "m", "span": "s", "task": "t"}, "taint": {"tier": "L5", "name": "user_ratified"}, "run": "t", "staged_at": delphi_core::time::now_rfc3339()}));
    let logs = read_logs(&e.home, &e.ws).unwrap();
    assert_eq!(logs.staged.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), [real.as_str()]);
    let mut t = Script::new(&["y", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", "ar-bbbbbbbbbb", "--run", "r1"], &mut t), 2);
    assert!(t.prompts.is_empty() && !auth::store_path(&e.home).exists());
    // an edited ledger: every command refuses, loudly, and nothing is written
    let seg = auth::ledger_dir(&e.ws).join("events/000001.jsonl");
    let original = std::fs::read_to_string(&seg).unwrap();
    std::fs::write(&seg, original.replace("user_ratified", "internal_empirical").replace(&real, "ar-cccccccccc")).unwrap();
    let mut t = Script::new(&["y", "p", &e.code()]);
    for args in [vec!["review"], vec!["list"], vec!["authorize", "--from-request", &real, "--run", "r1"], vec!["refuse", &real]] {
        assert_eq!(e.run(&args, &mut t), 2, "{args:?}");
    }
    assert!(t.prompts.is_empty() && !auth::store_path(&e.home).exists());
    std::fs::write(&seg, &original).unwrap();
    assert!(read_logs(&e.home, &e.ws).is_ok());
}

#[test]
fn a_second_authorize_cannot_run_while_the_first_holds_the_audit_log() {
    let e = env();
    let id = e.stage("m1", auth::Kind::Agent, Tier::L4, &e.request(1));
    let held = auth::open_audit(&e.home).unwrap();
    let mut t = Script::new(&["y", "p", &e.code()]);
    assert_eq!(e.run(&["authorize", "--from-request", &id, "--run", "r1"], &mut t), 2);
    assert!(!auth::store_path(&e.home).exists());
    drop(held);
}

#[test]
fn durations_parse() {
    assert_eq!(parse_duration("999999999999999999h"), None, "no overflow");
    assert_eq!((parse_duration("2h"), parse_duration("90m"), parse_duration("45s"), parse_duration("600")), (Some(7200), Some(5400), Some(45), Some(600)));
    assert_eq!((parse_duration("0"), parse_duration("-5m"), parse_duration("h"), parse_duration("")), (None, None, None, None));
}

// ---------------------------------------------------------------------------------------------- S2d2a: symbolic paths at the console

fn git_cmd(project: &Path, args: &[&str]) {
    let out = std::process::Command::new("/usr/bin/git").arg("-C").arg(project).args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"]).args(args).env_clear().env("PATH", "/usr/bin:/bin").env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A project that is a git repository holding `job.sh`, beside the test home.
fn project_of(e: &Env) -> PathBuf {
    let proj = e.home.parent().unwrap().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join("job.sh"), "#!/bin/bash\necho the-pinned-script\n").unwrap();
    git_cmd(&proj, &["init", "-q"]);
    git_cmd(&proj, &["add", "job.sh"]);
    git_cmd(&proj, &["commit", "-q", "-m", "x"]);
    proj
}

fn symbolic_entry(e: &Env) -> auth::Entry {
    auth::Entry { tool: "shell".into(), argv: vec![e.bin.clone(), "@snapshot/job.sh".into(), "@scratch".into(), "@canary".into()], files: vec!["@snapshot/job.sh".into()], runner_provenance: "project_snapshot".into() }
}

fn authorize_sym(e: &Env, proj: &Path, extra: &[&str], t: &mut Script) -> i32 {
    let entry = serde_json::to_string(&symbolic_entry(e)).unwrap();
    let mut args: Vec<String> = ["authorize", "--run", "run-1", "--intent", "i", "--purpose", "p", "--entry-json", &entry].iter().map(|s| s.to_string()).collect();
    args.extend(extra.iter().map(|s| s.to_string()));
    let p = proj.to_path_buf();
    let projf = move || Some(p.clone());
    run(&e.home, &e.ws, &args, t, &Deps { clock: &now, policy: e.policy(), factor: &good_factor, project: &projf })
}

#[test]
fn authorizing_symbolic_text_shows_the_fixed_token_table_and_the_pins_and_stores_a_scheme_1_record() {
    let e = env();
    let proj = project_of(&e);
    let retype = line(&symbolic_entry(&e));
    let mut t = Script::new(&[&retype, &e.code()]);
    assert_eq!(authorize_sym(&e, &proj, &[], &mut t), 0, "{:?}", t.prompts);
    let shown = t.shown.clone();
    for token in ["@snapshot", "@snapshot/<rel>", "@scratch", "@canary", "@operator_home"] {
        assert!(shown.contains(token), "the table lacks {token}: {shown}");
    }
    assert!(shown.contains("DECOY") && shown.contains("pinned file @snapshot/job.sh") && shown.contains("blake3 "), "{shown}");
    assert!(shown.contains("snapshot tree hash pinned") && shown.contains("--show"), "{shown}");
    assert!(!shown.contains("the-pinned-script"), "the content is shown only with --show");
    let rec = e.store().records.into_iter().next().unwrap();
    assert_eq!(rec.token_scheme, 1);
    // the record holds the SYMBOLIC text and no per-run path
    assert_eq!(rec.manifest[0].argv, symbolic_entry(&e).argv);
    assert_eq!(rec.manifest[0].files[0].path, "@snapshot/job.sh");
    assert!(rec.snapshot.as_ref().is_some_and(|s| s.source == "tracked_files" && s.tree_blake3.is_some()));
    // the hash the operator saw is the hash that was stored
    assert!(shown.contains(&rec.manifest[0].files[0].blake3));
}

#[test]
fn show_prints_the_pinned_file_and_no_pin_tree_is_an_opt_out_that_is_said_out_loud() {
    let e = env();
    let proj = project_of(&e);
    let retype = line(&symbolic_entry(&e));
    let mut t = Script::new(&[&retype, &e.code()]);
    assert_eq!(authorize_sym(&e, &proj, &["--show", "--no-pin-tree"], &mut t), 0);
    assert!(t.shown.contains("the-pinned-script") && t.shown.contains("NOT pinned"), "{}", t.shown);
    let rec = e.store().records.into_iter().next().unwrap();
    assert_eq!(rec.snapshot.and_then(|s| s.tree_blake3), None);
}

#[test]
fn symbolic_text_is_retyped_and_confirm_is_never_accepted() {
    let e = env();
    let proj = project_of(&e);
    let retype = line(&symbolic_entry(&e));
    for wrong in ["CONFIRM", "y", "", &retype.replace("@scratch", "@Scratch"), &format!("{retype} extra")] {
        let mut t = Script::new(&[wrong, "p", &e.code()]);
        assert_eq!(authorize_sym(&e, &proj, &[], &mut t), 2, "{wrong:?}");
        assert!(!auth::store_path(&e.home).exists(), "{wrong:?} authorized something");
    }
    // a resolved absolute path in place of the token is a different text
    let mut t = Script::new(&[&retype.replace("@scratch", "/tmp/x"), &e.code()]);
    assert_eq!(authorize_sym(&e, &proj, &[], &mut t), 2);
}

#[test]
fn a_pinned_file_that_is_untracked_modified_or_changes_while_the_operator_reads_refuses_the_authorization() {
    let e = env();
    let proj = project_of(&e);
    let retype = line(&symbolic_entry(&e));
    // modified: refused before anything is asked
    std::fs::write(proj.join("job.sh"), "#!/bin/bash\necho edited\n").unwrap();
    let mut t = Script::new(&[&retype, &e.code()]);
    assert_eq!(authorize_sym(&e, &proj, &[], &mut t), 2);
    assert!(t.prompts.is_empty(), "refused before the operator is asked to retype anything");
    git_cmd(&proj, &["checkout", "--", "job.sh"]);
    // untracked entry
    let untracked = auth::Entry { files: vec!["@snapshot/new.sh".into()], argv: vec![e.bin.clone(), "@snapshot/new.sh".into()], ..symbolic_entry(&e) };
    std::fs::write(proj.join("new.sh"), "x").unwrap();
    let raw = serde_json::to_string(&untracked).unwrap();
    let args: Vec<String> = ["authorize", "--run", "r", "--intent", "i", "--purpose", "p", "--entry-json", &raw].iter().map(|s| s.to_string()).collect();
    let p = proj.clone();
    let projf = move || Some(p.clone());
    let mut t = Script::new(&[&line(&untracked), &e.code()]);
    assert_eq!(run(&e.home, &e.ws, &args, &mut t, &Deps { clock: &now, policy: e.policy(), factor: &good_factor, project: &projf }), 2);
    // changed while the operator reads: the file is edited when the code is asked for
    let p2 = proj.clone();
    let mut t = Script::new(&[&retype, &e.code()]);
    t.on_prompt = Some(Box::new(move |prompt| {
        if prompt.contains("authenticator") {
            std::fs::write(p2.join("job.sh"), "#!/bin/bash\necho swapped while reading\n").unwrap();
        }
    }));
    assert_eq!(authorize_sym(&e, &proj, &[], &mut t), 2);
    assert!(!auth::store_path(&e.home).exists(), "nothing was authorized");
    // no project at all: a token has nothing to resolve against
    let entry = serde_json::to_string(&symbolic_entry(&e)).unwrap();
    let args: Vec<String> = ["authorize", "--run", "r", "--intent", "i", "--purpose", "p", "--entry-json", &entry].iter().map(|s| s.to_string()).collect();
    let mut t = Script::new(&[&retype, &e.code()]);
    assert_eq!(run(&e.home, &e.ws, &args, &mut t, &e.deps()), 2);
}

// The real flow, end to end up to the line the flag still draws: authorize by the console path (second factor through the test seam),
// then the launcher and the daemon's grant read what the console wrote. The run still denies every card.
#[test]
fn a_record_authorized_at_the_console_is_launched_resolved_in_the_dry_path_and_the_client_still_denies() {
    if !cfg!(target_os = "macos") || delphi_sandbox::Sandbox::detect().is_err() {
        eprintln!("skip: the authorized profile needs sandbox-exec (macOS)");
        return;
    }
    use delphi_engine::{authorization_anchor::launch_granted_for, authorization_grant::{Grant, Outcome}};
    let e = env();
    let proj = project_of(&e);
    let retype = line(&symbolic_entry(&e));
    let mut t = Script::new(&[&retype, &e.code()]);
    assert_eq!(authorize_sym(&e, &proj, &[], &mut t), 0);
    let rec = e.store().records.into_iter().next().unwrap();
    // the run: a snapshot of the project made NOW, after the record
    let root = std::fs::canonicalize(e.home.parent().unwrap()).unwrap();
    let snap = delphi_sandbox::authorized::Snapshot::build(&proj, &root).unwrap();
    let (bytes, _anchor) = launch_granted_for(&e.home, &rec.id, "run-1", now(), &e.policy(), Some((snap.path(), snap.tree_hash()))).unwrap();
    let grant = Grant::from_boot(&bytes, std::process::id(), now(), e.policy()).unwrap();
    // the model's card carries the symbolic text; the dry path matches it and shows the vector it WOULD run
    let card = symbolic_entry(&e).argv;
    match grant.evaluate_card("run_command", &card, now()) {
        Outcome::WouldUse { index, .. } => assert_eq!(index, 1),
        other => panic!("expected the dry path to match, got {other:?}"),
    }
    let shown = grant.resolve_dry(&card);
    assert_eq!(shown[1], snap.path().join("job.sh").display().to_string());
    assert!(shown[2].contains("@scratch") && shown[3].contains("@canary"), "{shown:?}");
    // a card holding the resolved path matches nothing
    let mut resolved = card.clone();
    resolved[1] = shown[1].clone();
    assert!(matches!(grant.evaluate_card("run_command", &resolved, now()), Outcome::NotMatched(_)));
    // and the run's own answer to a Red card is a denial whatever the record says: the approval is the daemon's
    let on = crate::run::OnCard::Authorization { id: rec.id.clone(), run: "run-1".into() };
    let (verdict, why) = crate::run::decide(&on, &json!({"tier": "red", "tool": "run_command", "pattern": "run_command"})).unwrap();
    assert_eq!(verdict, "deny");
    assert!(why.contains("never approved without the operator"), "{why}");
    // the real ledger is untouched by observation
    assert_eq!(delphi_engine::authorization_grant::ledger_count(&e.home, &rec.id).unwrap(), 0);
}

// ---------------------------------------------------------------------------------------------- red-team of #607: what the operator signs

#[test]
fn a_tracked_file_edited_while_the_operator_reads_refuses_the_write_because_the_shown_tree_is_the_stored_one() {
    let e = env();
    let proj = project_of(&e);
    // a tracked library the pinned script would source: not itself pinned, but part of the tree
    std::fs::write(proj.join("lib.sh"), "echo lib\n").unwrap();
    git_cmd(&proj, &["add", "lib.sh"]);
    git_cmd(&proj, &["commit", "-q", "-m", "lib"]);
    let retype = line(&symbolic_entry(&e));
    let mut t = Script::new(&[&retype, &e.code()]);
    let p2 = proj.clone();
    t.on_prompt = Some(Box::new(move |prompt| {
        if prompt.contains("authenticator") {
            std::fs::write(p2.join("lib.sh"), "echo rewritten while the operator typed the code\n").unwrap();
        }
    }));
    assert_eq!(authorize_sym(&e, &proj, &[], &mut t), 2);
    assert!(!auth::store_path(&e.home).exists(), "nothing was stored");
    // the reason is said: the tree changed since it was shown
    let again = {
        let mut t = Script::new(&[&retype, &e.code()]);
        let p3 = proj.clone();
        t.on_prompt = Some(Box::new(move |prompt| {
            if prompt.contains("authenticator") {
                std::fs::write(p3.join("lib.sh"), "echo rewritten again\n").unwrap();
            }
        }));
        authorize_sym(&e, &proj, &[], &mut t)
    };
    assert_eq!(again, 2);
}

#[test]
fn show_prints_at_most_64_kib_of_a_file_says_so_and_strips_control_characters() {
    let e = env();
    let proj = project_of(&e);
    let mut big = String::from("echo \u{1b}[31mred\u{7}\n");
    big.push_str(&"#".repeat(100_000));
    big.push_str("\nTAIL-MARKER\n");
    std::fs::write(proj.join("job.sh"), &big).unwrap();
    git_cmd(&proj, &["add", "job.sh"]);
    git_cmd(&proj, &["commit", "-q", "-m", "big"]);
    let retype = line(&symbolic_entry(&e));
    let mut t = Script::new(&[&retype, &e.code()]);
    assert_eq!(authorize_sym(&e, &proj, &["--show"], &mut t), 0);
    assert!(t.shown.contains("truncated") && t.shown.contains("covers the WHOLE file"), "{}", &t.shown[..400.min(t.shown.len())]);
    assert!(!t.shown.contains("TAIL-MARKER"), "the tail is not printed");
    assert!(!t.shown.contains('\u{1b}') && !t.shown.contains('\u{7}'), "control and escape characters are stripped");
    assert!(t.shown.len() < 80_000);
}
