//! Tests for the Xenia admin command line. Every store is a real git repository driven by the real library code; every
//! roster, statement and record is really signed. The attacker is a script, an agent or a pipe that wants a signature
//! without a person, a hostile store address, and a store host that lies about who is an admin.

use super::*;
use delphi_xenia::store::StorePaths;
use crate::xenia_admin_approve::{Terminal, TermApprover, Why};
use delphi_core::xenia_fixture::{self as fxt, Fx};
use delphi_core::xenia_admin::{Admin, AdminApprover, AdminDecision, AdminPreview};
use std::collections::VecDeque;

// ---------------------------------------------------------------------------------------------------------
// A terminal under the test's control

enum Ans {
    /// Read the prompt and type what it asks for, as a careful person would.
    Honest,
    Text(&'static str),
    /// End of input.
    Eof,
}

struct Term {
    interactive: Result<(), String>,
    script: VecDeque<Ans>,
    /// What happens when the script is used up.
    default: fn() -> Ans,
    shown: String,
    asked: Vec<String>,
    codes: u32,
    /// Refuse the digest challenge of any preview that shows this text.
    decline_if: Option<String>,
}

impl Term {
    fn honest() -> Term {
        Term { interactive: Ok(()), script: VecDeque::new(), default: || Ans::Honest, shown: String::new(), asked: Vec::new(), codes: 0, decline_if: None }
    }

    fn script(a: Vec<Ans>) -> Term {
        Term { interactive: Ok(()), script: a.into(), default: || Ans::Eof, shown: String::new(), asked: Vec::new(), codes: 0, decline_if: None }
    }

    fn pipe() -> Term {
        Term { interactive: Err("approving is a human act: run this in a terminal (it has no flag, environment variable, file or pipe that answers for you)".into()), ..Term::honest() }
    }
}

/// What a careful person types for a prompt of this library.
fn honest_answer(prompt: &str) -> String {
    if let Some(i) = prompt.find("type `") {
        let rest = &prompt[i + 6..];
        return rest[..rest.find('`').unwrap()].to_string();
    }
    let i = prompt.rfind("type ").unwrap() + 5;
    let rest = &prompt[i..];
    if let Some(j) = rest.find(" (the first 8") {
        return rest[..j].to_string();
    }
    rest[..rest.find("; anything").unwrap()].to_string()
}

impl Terminal for Term {
    fn interactive(&self) -> Result<(), String> {
        self.interactive.clone()
    }

    fn show(&mut self, text: &str) {
        self.shown.push_str(text);
    }

    fn ask(&mut self, prompt: &str) -> Option<String> {
        self.asked.push(prompt.to_string());
        if let (Some(d), true) = (&self.decline_if, prompt.contains("(the first 8")) {
            if self.shown.rsplit("=== ").next().is_some_and(|last| last.contains(d.as_str())) {
                return Some("no".into());
            }
        }
        match self.script.pop_front().unwrap_or_else(self.default) {
            Ans::Honest => Some(honest_answer(prompt)),
            Ans::Text(t) => Some(t.to_string()),
            Ans::Eof => None,
        }
    }

    fn fresh_code(&mut self) -> String {
        self.codes += 1;
        format!("T{:03}", self.codes)
    }
}

// ---------------------------------------------------------------------------------------------------------
// A store with people in it

fn b64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() {
            out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// Make `home`'s install key the test key with this seed byte.
fn install_key(home: &Path, seed: u8) {
    use std::os::unix::fs::PermissionsExt;
    let p = crate::xenia_cmd::key_path(home);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::set_permissions(p.parent().unwrap(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let _ = std::fs::remove_file(&p);
    std::fs::write(&p, format!("xenia-key/1\n{}\n", b64url(&[seed; 32]))).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(Identity::load(&p).unwrap().did(), delphi_xenia::testkit::key(seed).did(), "the key file is the test key");
}

fn seed_of(fx: &Fx, who: &str) -> u8 {
    (0u8..=255).find(|s| delphi_xenia::testkit::key(*s).did() == fx.id(who).did()).expect("a test seed")
}

struct W {
    _tmp: tempfile::TempDir,
    fx: Fx,
    home: PathBuf,
}

impl W {
    fn new() -> W {
        let tmp = tempfile::tempdir().unwrap();
        let (fx, _head) = Fx::installed(tmp.path(), &[]);
        let home = tmp.path().join("home");
        W { _tmp: tmp, fx, home }
    }

    fn env(&self, plus: i64) -> Env {
        Env { home: self.home.clone(), allow_file: true, now: fxt::now() + plus, cwd: self.fx.dir.clone() }
    }

    /// Run `args` as `who` with this terminal. Returns exit code, stdout, stderr.
    fn run_at(&self, who: &str, plus: i64, term: &mut Term, args: &[&str]) -> (i32, String, String) {
        install_key(&self.home, seed_of(&self.fx, who));
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(&args, &self.env(plus), &mut Io { out: &mut out, err: &mut err, term });
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn run(&self, who: &str, term: &mut Term, args: &[&str]) -> (i32, String, String) {
        self.run_at(who, 0, term, args)
    }

    fn host_head(&self) -> String {
        self.fx.remote.host_head()
    }
}

fn j(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON ({e}): {s}"))
}


fn sample_preview(w: &W) -> AdminPreview {
    let admin = Admin::new(&w.fx.store, fxt::cfg());
    let newcomer = delphi_xenia::identity::Identity::generate().unwrap().did();
    admin.propose(delphi_xenia::admin::Operation::Add(delphi_xenia::admin::NewMember { did: newcomer, person: "zed".into(), handle: "zed".into(), admin: false, steward_areas: vec![] })).unwrap();
    admin.prepare_sign(&w.fx.id("alice"), 2).unwrap().preview().clone()
}

#[test]
fn the_approver_refuses_without_a_person_and_asks_nothing() {
    let w = W::new();
    let p = sample_preview(&w);
    let mut t = Term::pipe();
    let mut a = TermApprover::new(&mut t);
    assert_eq!(AdminApprover::decide(&mut a, &p), AdminDecision::Deny);
    assert!(matches!(a.why(), Some(Why::NotInteractive(_))));
    assert!(t.shown.is_empty() && t.asked.is_empty(), "no preview is printed and no question is asked");
}

#[test]
fn an_answer_from_an_earlier_prompt_does_not_answer_a_later_one() {
    let w = W::new();
    let p = sample_preview(&w);
    // First prompt: honest. Its answer carries code T001. The replay at the second prompt (code T002) is refused.
    let mut t = Term::honest();
    let mut a = TermApprover::new(&mut t);
    assert!(matches!(AdminApprover::decide(&mut a, &p), AdminDecision::Approve(_)));
    let first_answer = honest_answer(t.asked.last().unwrap());
    assert!(first_answer.ends_with("-T001"));
    let mut replay = Term::script(vec![Ans::Text(Box::leak(first_answer.into_boxed_str()))]);
    replay.codes = 1;
    let mut a = TermApprover::new(&mut replay);
    assert_eq!(AdminApprover::decide(&mut a, &p), AdminDecision::Deny);
    assert_eq!(a.why(), Some(&Why::WrongChallenge));
}

#[test]
fn rotate_has_no_default_and_the_two_kinds_are_exclusive() {
    let w = W::new();
    let (old, new) = (w.fx.id("dave").did(), delphi_xenia::identity::Identity::generate().unwrap().did());
    let before = w.host_head();
    for extra in [vec![], vec!["--routine", "--compromised"]] {
        let mut args = vec!["rotate", old.as_str(), new.as_str()];
        args.extend(extra);
        let (c, out, err) = w.run("alice", &mut Term::honest(), &args);
        assert_eq!(c, 2, "{out}{err}");
        assert!(err.contains("--routine") && err.contains("--compromised"), "{err}");
    }
    assert_eq!(w.host_head(), before, "no proposal branch was pushed");
    assert!(!w.fx.remote.git(&["ls-remote", w.fx.remote.host_path().to_str().unwrap()]).contains("roster-proposal"));
}

#[test]
fn an_admin_proposes_two_admins_sign_one_publishes_and_a_fresh_install_accepts_it() {
    let w = W::new();
    let newcomer = delphi_xenia::identity::Identity::generate().unwrap().did();
    let (c, out, err) = w.run("alice", &mut Term::pipe(), &["add", &newcomer, "--person", "zed", "--handle", "zed", "--json"]);
    assert_eq!(c, 0, "{out}{err}");
    let n = j(&out)["proposal"]["n"].as_u64().unwrap();
    assert_eq!(n, 2);
    // Showing it needs no key and no terminal.
    let (c, out, _) = w.run("erin", &mut Term::pipe(), &["show", "2", "--json"]);
    assert_eq!(c, 0, "{out}");
    assert_eq!(j(&out)["proposal"]["quorum"]["have"], 0);
    // Two admins sign, each seeing the exact epoch bytes and the domain.
    for who in ["alice", "bob"] {
        let mut t = Term::honest();
        let (c, out, err) = w.run(who, &mut t, &["sign", "2", "--json"]);
        assert_eq!(c, 0, "{who}: {out}{err}");
        assert!(t.shown.contains(&newcomer) && t.shown.contains("the key will sign exactly these domains"), "{}", t.shown);
    }
    let (c, out, _) = w.run("alice", &mut Term::pipe(), &["show", "2", "--json"]);
    assert_eq!((c, j(&out)["proposal"]["quorum"]["have"].as_u64()), (0, Some(2)));
    let (c, out, err) = w.run_at("alice", 120, &mut Term::honest(), &["publish", "2", "--json"]);
    assert_eq!(c, 0, "{out}{err}");
    // A refresh a minute later also works and walks the history first.
    let mut t = Term::honest();
    let (c, out, err) = w.run_at("alice", 600, &mut t, &["refresh", "--validity-days", "7", "--json"]);
    assert_eq!(c, 0, "{out}{err}");
    assert!(t.shown.contains("history walked from"), "{}", t.shown);
    // A second, fresh install joins at epoch 1 and sees epoch 2 after syncing.
    let fresh = w.fx.fresh_install("fresh");
    let synced = fresh.sync_verified(&VerifyConfig::at(fxt::now() + 700)).unwrap();
    assert!(matches!(synced, delphi_core::xenia_sync::VerifiedSync::Accepted { .. }), "{synced:?}");
    assert_eq!(fresh.accepted_head().unwrap().unwrap(), w.host_head());
    // A stale proposal number is refused by the library.
    let (c, out, _) = w.run("carol", &mut Term::honest(), &["sign", "2", "--json"]);
    assert_eq!(c, 1, "{out}");
}

#[test]
fn a_compromise_rotation_and_a_removal_for_cause_carry_their_flags_through_the_prompt() {
    let w = W::new();
    let (old, new) = (w.fx.id("dave").did(), delphi_xenia::identity::Identity::generate().unwrap().did());
    let (c, out, err) = w.run("alice", &mut Term::pipe(), &["rotate", &old, &new, "--compromised", "--json"]);
    assert_eq!(c, 0, "{out}{err}");
    assert!(j(&out)["proposal"]["flags"].as_array().unwrap().iter().any(|f| f == "voids-tombstones"), "{out}");
    assert!(j(&out)["proposal"]["reminders"].as_array().unwrap().iter().any(|f| f == "compromised-key-records"));
    // Signing it asks for the flag by name; a wrong word refuses and nothing is pushed.
    let before = w.host_head();
    let mut t = Term::script(vec![Ans::Text("fine")]);
    let (c, out, _) = w.run("alice", &mut t, &["sign", "2", "--json"]);
    assert_eq!((c, j(&out)["reason"].as_str()), (1, Some("flag-not-acknowledged")), "{out}");
    assert!(t.shown.contains("FLAG voids-tombstones") && t.shown.contains("compromised key"), "{}", t.shown);
    assert_eq!(w.host_head(), before);
}

// ---------------------------------------------------------------------------------------------------------
// The ceremony, end to end, on separate installs

#[test]
fn the_genesis_ceremony_builds_a_store_three_founders_sign_and_a_member_joins() {
    let tmp = tempfile::tempdir().unwrap();
    let remote = delphi_core::xenia_testkit::TestRemote::new(&tmp.path().join("host"));
    let (ha, hb, hc) = (tmp.path().join("a"), tmp.path().join("b"), tmp.path().join("c"));
    let did = |h: &Path| Identity::load_or_create(&crate::xenia_cmd::key_path(h)).unwrap().did();
    let (da, db, dc) = (did(&ha), did(&hb), did(&hc));
    let cwd = tmp.path().to_path_buf();
    let call = |home: &Path, term: &mut Term, args: &[&str]| {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let env = Env { home: home.to_path_buf(), allow_file: true, now: fxt::now(), cwd: cwd.clone() };
        let c = run_with(&args, &env, &mut Io { out: &mut o, err: &mut e, term });
        (c, String::from_utf8(o).unwrap(), String::from_utf8(e).unwrap())
    };
    let draft = tmp.path().join("draft.json");
    let f = |d: &str, p: &str| format!("did={d},person={p},handle={p},role=admin");
    let (c, out, err) = call(&ha, &mut Term::pipe(), &["init", "draft", "--org", "acme", "--store", "acme-main", "--founder", &f(&da, "alice"), "--founder", &f(&db, "bob"), "--founder", &f(&dc, "carol"), "--out", draft.to_str().unwrap(), "--json"]);
    assert_eq!(c, 0, "{out}{err}");
    let hash = j(&out)["hash"].as_str().unwrap().to_string();
    // An existing file is never overwritten.
    assert_eq!(call(&ha, &mut Term::pipe(), &["init", "draft", "--org", "acme", "--store", "acme-main", "--founder", &f(&da, "alice"), "--out", draft.to_str().unwrap()]).0, 1);
    let mut sigs = Vec::new();
    for (home, name) in [(&ha, "a"), (&hb, "b"), (&hc, "c")] {
        let out_file = tmp.path().join(format!("{name}.sig.json"));
        let mut t = Term::honest();
        let (c, out, err) = call(home, &mut t, &["init", "sign", draft.to_str().unwrap(), "--org", "acme", "--store", "acme-main", "--out", out_file.to_str().unwrap(), "--json"]);
        assert_eq!(c, 0, "{name}: {out}{err}");
        assert!(t.shown.contains(&db) && t.shown.contains(&dc), "every founder is shown to every founder: {}", t.shown);
        assert_eq!(j(&out)["draft_hash"], hash.as_str());
        sigs.push(out_file);
    }
    let s = |i: usize| sigs[i].to_str().unwrap().to_string();
    // Two of three founder signatures is not a genesis.
    let (c, out, _) = call(&ha, &mut Term::honest(), &["init", "create", draft.to_str().unwrap(), "--org", "acme", "--store", "acme-main", "--url", &remote.url(), "--sig", &s(0), "--sig", &s(1), "--json"]);
    assert_eq!((c, j(&out)["reason"].as_str()), (1, Some("missing-founder-signatures")), "{out}");
    assert!(!ha.join("xenia/acme/acme-main").exists() && !ha.join("xenia/acme").exists(), "a refused creation leaves no store directory");
    let (c, out, err) = call(&ha, &mut Term::honest(), &["init", "create", draft.to_str().unwrap(), "--org", "acme", "--store", "acme-main", "--url", &remote.url(), "--sig", &s(0), "--sig", &s(1), "--sig", &s(2), "--json"]);
    assert_eq!(c, 0, "{out}{err}");
    let join = j(&out)["join"].clone();
    assert!(join["command"].as_str().unwrap().starts_with("delphi xenia join "));
    // The founder is joined already; a member joins from the out-of-band line and syncs.
    let open = |h: &Path| delphi_core::xenia_store::Store::open(h, "acme", "acme-main", &remote.url(), true).unwrap();
    let first = open(&ha);
    let synced = first.sync_verified(&VerifyConfig::at(fxt::now())).unwrap();
    assert!(matches!(synced, delphi_core::xenia_sync::VerifiedSync::Accepted { .. }), "{synced:?}");
    let head_pair = join["head"].as_str().unwrap();
    let (epoch, hash) = delphi_xenia::pins::JoinRequest::parse_head(head_pair).unwrap();
    let req = delphi_xenia::pins::JoinRequest { url: join["url"].as_str().unwrap().into(), admin_fingerprint: join["admin"].as_str().unwrap().into(), head_epoch: epoch, head_hash: hash };
    let member = open(&hb);
    delphi_core::xenia_admin::join_store(&member, &req).unwrap();
    let synced = member.sync_verified(&VerifyConfig::at(fxt::now())).unwrap();
    assert!(matches!(synced, delphi_core::xenia_sync::VerifiedSync::Accepted { .. }), "{synced:?}");
    assert_eq!(member.accepted_head().unwrap().unwrap(), remote.host_head());
}

// ---------------------------------------------------------------------------------------------------------
// What an agent can reach

#[test]
fn the_approver_module_has_no_way_to_reach_a_model_a_daemon_the_network_or_the_environment() {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/xenia_admin_approve.rs")).unwrap();
    // Code only: comments may say what is absent.
    let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    let code = code.split("#[cfg(test)]").next().unwrap();
    for banned in [
        "std::env", "env::var", "env!(", "std::fs", "File::", "std::process", "Command", "std::net", "TcpStream", "UnixStream", "UnixListener", "tokio", "reqwest", "hyper", "delphi_engine", "delphi_adapter", "delphi_sandbox",
        "async", ".await", "spawn", "thread", "socket", "Socket", "daemon", "Daemon", "hub", "Hub", "Client", "client", "model", "Model", "bridge", "rpc", "Rpc", "provider", "Identity", "identity", "key_path", "git",
    ] {
        assert!(!code.contains(banned), "the approver must not mention `{banned}`");
    }
    // What it does use: a terminal check and the immutable previews.
    assert!(code.contains("libc::isatty(0)") && code.contains("libc::isatty(2)"));
    assert!(code.contains("delphi_core::xenia_admin::"));
}

#[test]
fn xenia_state_and_the_key_are_outside_every_zone_an_agent_can_read() {
    use delphi_core::pathguard::{Access, Guard, Zones};
    let w = W::new();
    install_key(&w.home, 1);
    let ws = w.home.join("workspaces/ws");
    let wt = w._tmp.path().join("wt");
    for d in [ws.join("local/ratified"), ws.join("team"), ws.join("base"), wt.join("src")] {
        std::fs::create_dir_all(d).unwrap();
    }
    let zone_roots: Vec<PathBuf> = vec![ws.clone(), ws.join("local"), ws.join("team"), ws.join("base"), w.home.join("workspaces"), wt.clone()];
    let paths = StorePaths::new(&w.home, "acme", "acme-main").unwrap();
    assert!(paths.is_outside_zones(&w.home, &zone_roots));
    assert!(paths.root.starts_with(w.home.join("xenia")) && !paths.root.starts_with(ws.join("team")));
    assert!(crate::xenia_cmd::key_path(&w.home).starts_with(w.home.join("xenia")) && crate::xenia_cmd::key_path(&w.home).is_file());
    // And the file tools cannot name them, however the path is spelled.
    std::os::unix::fs::symlink(crate::xenia_cmd::key_path(&w.home), wt.join("src/key-link")).unwrap();
    std::os::unix::fs::symlink(w.home.join("xenia"), wt.join("src/dir-link")).unwrap();
    let g = Guard {
        zones: Zones { local: ws.join("local"), team: ws.join("team"), base: ws.join("base"), ratified: ws.join("local/ratified"), worktree: Some(wt.clone()), projects: vec![], bundles: vec![], payload: None },
        role: "builder".into(),
        read_globs: vec!["**".into()],
        write_globs: vec!["**".into()],
        protected: Default::default(),
    };
    // The operator's switch file is in the operator zone, which no agent can read, write or propose a change to.
    std::fs::create_dir_all(w.home.join("operator")).unwrap();
    std::fs::write(w.home.join("operator/xenia.toml"), "[xenia]\nconsole_admin = false\n").unwrap();
    for a in ["operator/xenia.toml", "../operator/xenia.toml", "local/../operator/xenia.toml"] {
        for access in [Access::Read, Access::Write] {
            assert!(g.check(a, access).is_err(), "{a} must not resolve ({access:?})");
        }
    }
    let key = crate::xenia_cmd::key_path(&w.home).display().to_string();
    let attempts = vec![
        "xenia/identity.key".to_string(),
        "xenia/acme/acme-main/state/pins.toml".to_string(),
        "../xenia/identity.key".to_string(),
        "@worktree/../../../xenia/identity.key".to_string(),
        "@worktree/src/key-link".to_string(),
        "@worktree/src/dir-link/identity.key".to_string(),
        "@worktree/src/dir-link/acme/acme-main/state/pins.toml".to_string(),
        key.clone(),
        format!("@project/x/{key}"),
        "local/../xenia/identity.key".to_string(),
    ];
    for a in &attempts {
        for access in [Access::Read, Access::Write] {
            assert!(g.check(a, access).is_err(), "{a} must not resolve ({access:?})");
        }
    }
}

#[test]
fn a_process_started_by_an_agent_seat_is_never_a_person() {
    let seat = approve::StdTerminal::new(true, fresh_code);
    assert!(seat.interactive().unwrap_err().contains("agent seat"));
    assert!(!inside_an_agent_seat() || std::env::var_os("DELPHI_BRIDGE_SOCKET").is_some() || std::env::var_os("DELPHI_BRIDGE_TOKEN").is_some());
    // SAFETY: only this test reads or writes these two variables.
    unsafe { std::env::set_var("DELPHI_BRIDGE_TOKEN", "x") };
    assert!(inside_an_agent_seat());
    unsafe { std::env::remove_var("DELPHI_BRIDGE_TOKEN") };
}

#[test]
fn the_prompt_code_is_fresh_and_from_a_fixed_alphabet() {
    let codes: std::collections::BTreeSet<String> = (0..20).map(|_| fresh_code()).collect();
    assert!(codes.len() >= 19, "{codes:?}");
    assert!(codes.iter().all(|c| c.len() == 4 && c.bytes().all(|b| b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(&b))));
}


#[test]
fn the_daemon_and_the_tool_registry_reach_only_the_limited_admin_operations() {
    // Tools an agent is offered, with and without write scope: nothing about Xenia but the read-only search.
    let reg = delphi_engine::tools::Registry::builtin();
    let mut names: Vec<String> = Vec::new();
    for write in [false, true] {
        for d in reg.defs(write) {
            names.push(d.name.clone());
            if d.name != "xenia_search" && d.name != "xenia_help" {
                assert!(!format!("{} {}", d.name, d.description).to_lowercase().contains("xenia"), "{} mentions Xenia", d.name);
            }
        }
    }
    assert!(names.iter().filter(|n| n.contains("xenia")).all(|n| n == "xenia_search" || n == "xenia_help"), "only the read-only search and the help text");
    for n in &names {
        for banned in ["sign", "publish", "endorse", "admin", "join", "withdraw", "roster", "tombstone", "dispute", "rotate", "propose", "genesis"] {
            assert!(!n.to_lowercase().contains(banned), "tool `{n}` looks like a Xenia write: {banned}");
        }
    }
    // The console socket answers exactly two admin methods; the narrator and relay lists have none.
    let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../delphi-engine/src");
    let svc = std::fs::read_to_string(engine.join("service.rs")).unwrap();
    let mut methods = Vec::new();
    for line in svc.lines() {
        let l = line.trim_start();
        if let Some(rest) = l.strip_prefix('"') {
            if let Some((m, after)) = rest.split_once('"') {
                if after.trim_start().starts_with("=>") && m.contains('.') && m.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_') {
                    methods.push(m.to_string());
                }
            }
        }
    }
    let adminish: Vec<&String> = methods.iter().filter(|m| ["admin", "roster", "rotate", "genesis", "propose", "join"].iter().any(|b| m.contains(b))).collect();
    assert_eq!(adminish, vec!["xenia.admin.prepare", "xenia.admin.decide"], "{methods:?}");
    for m in delphi_core::server::NARRATOR_METHODS.iter().chain(delphi_core::server::RELAY_METHODS.iter()) {
        assert!(!m.contains("xenia"), "{m}");
    }
    // Nothing in the engine can start a genesis, a rewrite recovery or a rotation: the identifiers are not there, so
    // the terminal-only set stays terminal-only whatever an RPC asks.
    for entry in std::fs::read_dir(&engine).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap();
        let text = text.split("#[cfg(test)]").next().unwrap_or("");
        for banned in ["init_store", "prepare_genesis_signature", "GenesisDraft", "accept_rewritten_head", "plan_rewrite"] {
            assert!(!text.contains(banned), "{name} mentions {banned}");
        }
    }
    // And the daemon's console admin code classifies by the typed diff before it builds a card.
    let core = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../delphi-core/src/xenia_admin_console.rs")).unwrap();
    assert!(core.contains("classify(&base, &cand, admin.policy.min_admins).map_err(ConsoleError::Terminal)?"));
}

#[test]
fn every_admin_signing_command_refuses_without_a_person_and_changes_nothing() {
    let w = W::new();
    let host = w.host_head();
    let sticky = std::fs::read(w.fx.store.paths.state.join("sticky.json")).unwrap();
    let cmds: Vec<Vec<&str>> = vec![
        vec!["sign", "2", "--json"],
        vec!["publish", "2", "--json"],
        vec!["refresh", "--json"],
        vec!["init", "sign", "nofile", "--org", "acme", "--store", "acme-main", "--out", "o", "--json"],
        vec!["init", "create", "nofile", "--org", "acme", "--store", "acme-main", "--url", "https://example.invalid/x", "--sig", "s", "--json"],
    ];
    for c in cmds {
        let mut t = Term::pipe();
        let (code, out, _) = w.run("alice", &mut t, &c);
        assert_eq!(code, 1, "{c:?}: {out}");
        assert_eq!(j(&out)["reason"], "not-interactive", "{c:?}");
        assert!(t.shown.is_empty() && t.asked.is_empty(), "{c:?}");
    }
    assert_eq!(w.host_head(), host);
    assert_eq!(std::fs::read(w.fx.store.paths.state.join("sticky.json")).unwrap(), sticky);
}

#[test]
fn no_flag_answers_for_a_person_and_environment_variables_are_not_read() {
    let w = W::new();
    for flag in ["--yes", "-y", "--force", "--no-confirm", "--approve", "--assume-yes", "--non-interactive", "--batch", "--yes=true"] {
        for cmd in [vec!["sign", "2"], vec!["publish", "2"], vec!["refresh"], vec!["init", "sign", "f"], vec!["init", "create", "f"]] {
            let mut args = cmd.clone();
            args.push(flag);
            let mut t = Term::honest();
            let before = w.host_head();
            let (c, out, err) = w.run("alice", &mut t, &args);
            assert_eq!(c, 2, "{args:?}: {out}{err}");
            assert!(t.shown.is_empty() && t.asked.is_empty());
            assert_eq!(w.host_head(), before);
        }
    }
    // SAFETY: only this test sets these; nothing reads them for consent, which is what is being shown.
    for (k, v) in [("DELPHI_XENIA_APPROVE", "yes"), ("XENIA_YES", "1"), ("CI", "1")] {
        unsafe { std::env::set_var(k, v) };
    }
    let mut t = Term::pipe();
    let (c, out, _) = w.run("alice", &mut t, &["refresh", "--json"]);
    assert_eq!((c, j(&out)["reason"].as_str()), (1, Some("not-interactive")));
    for k in ["DELPHI_XENIA_APPROVE", "XENIA_YES", "CI"] {
        unsafe { std::env::remove_var(k) };
    }
}

#[test]
fn genesis_refuses_a_hostile_address_and_a_signature_from_someone_who_is_not_a_founder() {
    let tmp = tempfile::tempdir().unwrap();
    let remote = delphi_core::xenia_testkit::TestRemote::new(&tmp.path().join("host"));
    let (ha, hb, hs) = (tmp.path().join("a"), tmp.path().join("b"), tmp.path().join("stranger"));
    let did = |h: &Path| Identity::load_or_create(&crate::xenia_cmd::key_path(h)).unwrap().did();
    let (da, db) = (did(&ha), did(&hb));
    let _ = did(&hs);
    let cwd = tmp.path().to_path_buf();
    let call = |home: &Path, allow: bool, term: &mut Term, args: &[&str]| {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let env = Env { home: home.to_path_buf(), allow_file: allow, now: fxt::now(), cwd: cwd.clone() };
        let c = run_with(&args, &env, &mut Io { out: &mut o, err: &mut e, term });
        (c, String::from_utf8(o).unwrap(), String::from_utf8(e).unwrap())
    };
    let draft = tmp.path().join("draft.json");
    let f = |d: &str, p: &str| format!("did={d},person={p},handle={p},role=admin");
    let (c, out, err) = call(&ha, true, &mut Term::pipe(), &["init", "draft", "--org", "acme", "--store", "acme-main", "--founder", &f(&da, "alice"), "--founder", &f(&db, "bob"), "--out", draft.to_str().unwrap(), "--json"]);
    assert_eq!(c, 0, "{out}{err}");
    let sig = |home: &Path, name: &str| {
        let p = tmp.path().join(format!("{name}.sig.json"));
        let (c, out, _) = call(home, true, &mut Term::honest(), &["init", "sign", draft.to_str().unwrap(), "--org", "acme", "--store", "acme-main", "--out", p.to_str().unwrap(), "--json"]);
        (c, out, p)
    };
    // Someone who is not a founder cannot sign the draft at all.
    let (c, out, p_s) = sig(&hs, "stranger");
    assert_eq!((c, j(&out)["reason"].as_str()), (1, Some("not-an-approver")), "{out}");
    assert!(!p_s.exists());
    let (ca, _, pa) = sig(&ha, "a");
    assert_eq!(ca, 0);
    let (cb, _, pb) = sig(&hb, "b");
    assert_eq!(cb, 0);
    // A signature file that names the other founder but carries this founder's signature does not complete the ceremony.
    let forged = tmp.path().join("forged.sig.json");
    let mut v: Value = serde_json::from_slice(&std::fs::read(&pa).unwrap()).unwrap();
    v["kid"] = json!(db);
    std::fs::write(&forged, serde_json::to_vec(&v).unwrap()).unwrap();
    let create = |url: &str, sigs: &[&Path]| {
        let mut args: Vec<String> = ["init", "create", draft.to_str().unwrap(), "--org", "acme", "--store", "acme-main", "--url", url, "--json"].iter().map(|s| s.to_string()).collect();
        for s in sigs {
            args.push("--sig".into());
            args.push(s.to_str().unwrap().into());
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        call(&ha, true, &mut Term::honest(), &args)
    };
    let (c, out, _) = create(&remote.url(), &[&pa, &forged]);
    assert_eq!(c, 1, "{out}");
    assert!(!remote.host_path().join("refs/heads/main").exists(), "nothing was pushed");
    // A hostile address is refused before a directory exists.
    for u in ["ext::sh -c touch% /tmp/never", "file:///tmp/x", "https://-oProxyCommand=evil/x", "http://example.invalid/x"] {
        let args: Vec<String> = ["init", "create", draft.to_str().unwrap(), "--org", "acme", "--store", "acme-main", "--url", u, "--sig", pa.to_str().unwrap(), "--sig", pb.to_str().unwrap(), "--json"].iter().map(|s| s.to_string()).collect();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        // The real command: file addresses are off.
        let (c, out, _) = call(&ha, false, &mut Term::honest(), &args);
        assert_eq!((c, j(&out)["reason"].as_str()), (1, Some("hostile-url")), "{u}: {out}");
    }
    assert!(!ha.join("xenia/acme").exists(), "no store directory was made");
}

// ---------------------------------------------------------------------------------------------------------
// `sync --accept-rewritten-head`

struct Rw {
    _tmp: tempfile::TempDir,
    fx: Fx,
    c2: String,
    r2: String,
    tomb: delphi_xenia::testkit_records::SignedRecord,
    sticky: Vec<u8>,
}

/// An install holding a note and a tombstone; the host then rewrites history to drop the tombstone. `signers` sign a
/// statement naming the replaced head (none: no statement at all).
fn rewritten(signers: &[&str]) -> Rw {
    use delphi_xenia::testkit_records::{self as rk, FreshSpec};
    let tmp = tempfile::tempdir().unwrap();
    let org = delphi_core::xenia_fixture::Org::three_admins();
    let alice = org.key("alice");
    let note = rk::memory(&alice, &fxt::u(0), 1, "active", "note");
    let tomb = rk::tombstone(&alice, &fxt::u(1), 1, &fxt::u(0), "withdraw");
    let (fx, c1) = Fx::installed_on(Fx::new(tmp.path(), org), &[&note, &tomb]);
    let c2 = fx.store.accepted_head().unwrap().unwrap();
    let sticky = std::fs::read(fx.store.paths.state.join("sticky.json")).unwrap();
    fx.remote.reset_hard(&c1);
    fx.remote.remove(&tomb.path);
    fx.remote.remove(&tomb.sidecar_path);
    let mut r2 = fx.commit("rewrite");
    if !signers.is_empty() {
        let mut spec = FreshSpec::for_head(&fx.org, &r2, fxt::now() - 600, 7 * fxt::DAY);
        spec.rewrite_of = Some(c2.clone());
        let keys: Vec<_> = signers.iter().map(|n| fx.id(n)).collect();
        let refs: Vec<&_> = keys.iter().collect();
        let (stmt, sigs) = spec.sign(&refs);
        fx.remote.write("roster/freshness.json", &stmt);
        fx.remote.write("roster/freshness.sigs.json", &sigs);
        r2 = fx.commit("statement");
    }
    Rw { _tmp: tmp, fx, c2, r2, tomb, sticky }
}

impl Rw {
    fn recover(&self, term: &mut Term) -> (i32, String, String) {
        let cfg = fxt::cfg();
        let outcome = self.fx.store.sync_verified(&cfg).unwrap();
        let r = delphi_core::xenia_status::StoreRef { org: "acme".into(), store: "acme-main".into(), url: self.fx.remote.url() };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = crate::xenia_recover::recover_with(&r, &self.fx.store, &cfg, &outcome, term, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn unchanged(&self, what: &str) {
        assert_eq!(self.fx.store.accepted_head().unwrap().as_deref(), Some(self.c2.as_str()), "{what}: still at the old head");
        assert_eq!(std::fs::read(self.fx.store.paths.state.join("sticky.json")).unwrap(), self.sticky, "{what}: state untouched");
    }
}

#[test]
fn a_quorum_statement_and_the_typed_commit_and_count_accept_the_rewrite_and_it_is_journaled() {
    let rw = rewritten(&["alice", "bob"]);
    let mut t = Term::honest();
    let (c, out, err) = rw.recover(&mut t);
    assert_eq!(c, 0, "{out}{err}");
    assert!(t.shown.contains(&rw.tomb.path) && t.shown.contains(&rw.tomb.sidecar_path) && t.shown.contains(&rw.c2) && t.shown.contains(&rw.r2), "{}", t.shown);
    assert!(t.asked[0].contains(&format!("accept {} 2", &rw.r2[..12])), "{:?}", t.asked);
    assert_eq!(rw.fx.store.accepted_head().unwrap().as_deref(), Some(rw.r2.as_str()));
    let log = std::fs::read_to_string(rw.fx.store.paths.state.join("rewrite-log.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 2, "{log}");
    let journal = delphi_core::xenia_status::read_journal(&rw.fx.store);
    assert!(journal.iter().any(|e| e.kind == "recovery" && e.outcome == "accepted" && e.detail["count"] == 2), "{journal:?}");
}

#[test]
fn without_an_authentic_quorum_statement_the_recovery_explains_and_changes_nothing() {
    for (what, signers) in [("no statement at all", vec![]), ("a statement signed by one of three admins", vec!["alice"])] {
        let rw = rewritten(&signers);
        let mut t = Term::honest();
        let (c, out, err) = rw.recover(&mut t);
        assert_eq!(c, 1, "{what}: {out}{err}");
        assert!(err.contains("rewrite-quorum-missing") && err.contains("Nothing was changed"), "{what}: {err}");
        assert!(t.asked.is_empty(), "{what}: no question was asked");
        rw.unchanged(what);
    }
}

#[test]
fn an_approval_for_another_commit_a_wrong_count_a_yes_or_a_pipe_is_refused() {
    let rw = rewritten(&["alice", "bob"]);
    for (what, ans) in [
        ("another commit", Ans::Text("accept 000000000000 2")),
        ("fewer paths", Ans::Text("accept 111111111111 1")),
        ("yes", Ans::Text("yes")),
        ("the word alone", Ans::Text("accept")),
        ("end of input", Ans::Eof),
    ] {
        let mut t = Term::script(vec![ans]);
        let (c, out, err) = rw.recover(&mut t);
        assert_eq!(c, 1, "{what}: {out}{err}");
        assert!(t.shown.contains(&rw.tomb.path), "{what}: the paths were listed first");
        rw.unchanged(what);
    }
    // The right commit with a count that is not the list's.
    let wrong_count = format!("accept {} 1", &rw.r2[..12]);
    let mut t = Term::script(vec![Ans::Text(Box::leak(wrong_count.into_boxed_str()))]);
    assert_eq!(rw.recover(&mut t).0, 1);
    rw.unchanged("wrong count");
    // No terminal: refused before anything is shown.
    let mut t = Term::pipe();
    let (c, _, err) = rw.recover(&mut t);
    assert_eq!(c, 1);
    assert!(err.contains("human act"), "{err}");
    assert!(t.shown.is_empty() && t.asked.is_empty());
    rw.unchanged("pipe");
    let journal = delphi_core::xenia_status::read_journal(&rw.fx.store);
    assert!(journal.iter().all(|e| e.outcome != "accepted" || e.kind != "recovery"));
}

#[test]
fn a_host_that_went_back_in_time_is_explained_as_not_recoverable() {
    let tmp = tempfile::tempdir().unwrap();
    let org = delphi_core::xenia_fixture::Org::three_admins();
    let note = delphi_xenia::testkit_records::memory(&org.key("alice"), &fxt::u(0), 1, "active", "note");
    let (fx, _c1) = Fx::installed_on(Fx::new(tmp.path(), org), &[&note]);
    let accepted = fx.store.accepted_head().unwrap().unwrap();
    // Back to the first commit, forced: the host now serves an ancestor of what we hold.
    let first = fx.remote.git(&["rev-list", "--max-parents=0", "HEAD"]).trim().to_string();
    fx.remote.reset_hard(&first);
    fx.remote.force_push();
    let cfg = fxt::cfg();
    let outcome = fx.store.sync_verified(&cfg).unwrap();
    assert!(matches!(&outcome, delphi_core::xenia_sync::VerifiedSync::NonFastForward(n) if n.kind == delphi_core::xenia_store::Divergence::Behind), "{outcome:?}");
    let r = delphi_core::xenia_status::StoreRef { org: "acme".into(), store: "acme-main".into(), url: fx.remote.url() };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut t = Term::honest();
    let c = crate::xenia_recover::recover_with(&r, &fx.store, &cfg, &outcome, &mut t, &mut out, &mut err);
    assert_eq!(c, 1);
    let err = String::from_utf8(err).unwrap();
    assert!(err.contains("OLDER view") && err.contains("cannot be recovered"), "{err}");
    assert!(t.asked.is_empty());
    assert_eq!(fx.store.accepted_head().unwrap().as_deref(), Some(accepted.as_str()));
}

#[test]
fn the_recovery_is_not_reachable_from_the_daemon_or_a_tool() {
    // The engine never names the library's recovery calls (the engine-source test above also pins this), and the CLI word
    // is the only caller.
    let cmd = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/xenia_cmd.rs")).unwrap();
    assert!(cmd.contains("crate::xenia_recover::run("));
    let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../delphi-engine/src");
    for entry in std::fs::read_dir(engine).unwrap().flatten() {
        if entry.file_name().to_string_lossy().ends_with(".rs") {
            let text = std::fs::read_to_string(entry.path()).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap_or("");
            assert!(!code.contains("accept_rewritten_head") && !code.contains("plan_rewrite"), "{:?}", entry.path());
        }
    }
}
