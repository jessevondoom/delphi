//! The app socket (`specs/120` slice K1): pairing tickets, the signed handshake, refusals and the lockout, grants read per
//! call, and revocation. AC-2 (the daemon's half), AC-3, AC-4, AC-6 (first half), AC-10.

use delphi_core::app_keys::{test_sign, KeyStore};
use delphi_core::client::{Client, ClientError};
use delphi_core::proto::code;
use delphi_core::server::{Daemon, DaemonConfig, TtyPolicy};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Fixture {
    dir: tempfile::TempDir,
    sock: PathBuf,
    narrator_sock: PathBuf,
    app_sock: PathBuf,
    daemon: Arc<Daemon>,
    /// What the server reported about the app socket, in order: (kind, payload).
    audit: Arc<Mutex<Vec<(String, Value)>>>,
}

/// An in-process daemon with an app socket, on a short path (macOS limits Unix socket paths to ~100 bytes).
async fn start(policy: TtyPolicy, tweak: impl FnOnce(&mut DaemonConfig)) -> Fixture {
    let dir = tempfile::Builder::new().prefix("da").tempdir_in("/tmp").unwrap();
    let bind = |name: &str| {
        let p = dir.path().join(name);
        (tokio::net::UnixListener::bind(&p).unwrap(), p)
    };
    let (console, sock) = bind("d.sock");
    let (narrator, narrator_sock) = bind("n.sock");
    let (app, app_sock) = bind("a.sock");
    let mut cfg = DaemonConfig::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    cfg.tty_policy = policy;
    cfg.auth_timeout = Duration::from_millis(500);
    tweak(&mut cfg);
    let daemon = Daemon::new(cfg).unwrap();
    let audit = Arc::new(Mutex::new(Vec::new()));
    let sink = audit.clone();
    daemon.set_app_audit(Arc::new(move |kind: &str, payload: Value| sink.lock().unwrap().push((kind.to_string(), payload))));
    tokio::spawn(daemon.clone().serve_with_app(console, Some(narrator), None, None, Some(app)));
    Fixture { dir, sock, narrator_sock, app_sock, daemon, audit }
}

impl Fixture {
    fn console(&self) -> Client {
        let mut c = Client::connect(&self.sock).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        c.hello(self.daemon.console_token()).unwrap();
        c
    }

    fn keys(&self) -> Vec<delphi_core::app_keys::AppKey> {
        KeyStore::at(self.dir.path()).load().unwrap()
    }

    fn refusals(&self) -> Vec<Value> {
        self.audit.lock().unwrap().iter().filter(|(k, _)| k == "app.refused").map(|(_, p)| p.clone()).collect()
    }

    fn kinds(&self) -> Vec<String> {
        self.audit.lock().unwrap().iter().map(|(k, _)| k.clone()).collect()
    }

    /// Connect to the app socket and read the challenge. `None` when the daemon closed the connection without one.
    fn knock(&self) -> Option<Knock> {
        let mut c = Client::connect(&self.app_sock).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let ch = c.next_notification().ok()?;
        assert_eq!(ch["method"], "app.challenge");
        let p = &ch["params"];
        assert_eq!((p["instance"].as_str(), p["protocol_version"].as_u64()), (Some(self.daemon.instance()), Some(1)));
        Some(Knock { c, nonce: p["nonce"].as_str().unwrap().to_string(), instance: p["instance"].as_str().unwrap().to_string(), socket: self.app_sock.to_string_lossy().into_owned() })
    }

    /// Pair the key of `seed` with a fresh ticket and return its short name.
    fn pair(&self, seed: &[u8; 32]) -> String {
        let t = self.console().call("auth.pair_ticket", json!({})).unwrap();
        let mut k = self.knock().unwrap();
        let r = k.pair(seed, t["ticket"].as_str().unwrap()).unwrap();
        assert_eq!(r["name"], t["name"]);
        r["name"].as_str().unwrap().to_string()
    }

    /// A proven session for `name`.
    fn session(&self, seed: &[u8; 32], name: &str) -> Client {
        let mut k = self.knock().unwrap();
        k.hello(seed, name).unwrap();
        k.c
    }
}

/// A connection to the app socket that has been sent its challenge.
struct Knock {
    c: Client,
    nonce: String,
    instance: String,
    socket: String,
}

impl Knock {
    fn signature(&self, seed: &[u8; 32]) -> String {
        test_sign(seed, &self.nonce, &self.instance, 1, &self.socket).1
    }

    fn pair(&mut self, seed: &[u8; 32], ticket: &str) -> Result<Value, ClientError> {
        let public_key = test_sign(seed, &self.nonce, &self.instance, 1, &self.socket).0;
        self.c.call("app.pair", json!({"ticket": ticket, "public_key": public_key, "app_version": "0.1.0"}))
    }

    fn hello(&mut self, seed: &[u8; 32], name: &str) -> Result<Value, ClientError> {
        let signature = self.signature(seed);
        self.hello_with(name, &signature)
    }

    fn hello_with(&mut self, name: &str, signature: &str) -> Result<Value, ClientError> {
        self.c.call("app.hello", json!({"name": name, "signature": signature, "protocol_version": 1}))
    }

    /// The daemon answered with a refusal and then closed the connection.
    fn refused_and_closed(mut self, r: Result<Value, ClientError>) {
        assert_eq!(rpc_code(r), Some(code::FORBIDDEN));
        assert!(matches!(self.c.call("session.ping", json!({})), Err(ClientError::Closed) | Err(ClientError::Io(_))), "the connection must be closed");
    }
}

fn rpc_code(r: Result<Value, ClientError>) -> Option<i64> {
    match r {
        Err(ClientError::Rpc { code, .. }) => Some(code),
        _ => None,
    }
}

const A: [u8; 32] = [11u8; 32];
const B: [u8; 32] = [22u8; 32];

/// AC-1 (the daemon's half) and AC-3: a ticket pairs one key, which holds `read` and `request`; the same ticket again is
/// refused, as is one past its time. The paired key then opens a session with a signature and no token.
#[tokio::test(flavor = "multi_thread")]
async fn a_ticket_pairs_one_key_once_and_only_within_its_time() {
    let f = start(TtyPolicy::Disabled, |c| c.ticket_ttl = Duration::from_millis(400)).await;
    let t = f.console().call("auth.pair_ticket", json!({})).unwrap();
    let ticket = t["ticket"].as_str().unwrap().to_string();
    assert_eq!((ticket.len(), t["grants"].clone()), (64, json!(["read", "request"])), "32 random bytes, as hex");
    assert!(f.keys().is_empty(), "a ticket is not a key");
    assert_eq!(f.console().call("app.keys", json!({})).unwrap()["pending"], json!([t["name"]]), "the terminal can watch its ticket");

    let mut k = f.knock().unwrap();
    let paired = k.pair(&A, &ticket).unwrap();
    assert_eq!((paired["name"].clone(), paired["grants"].clone()), (t["name"].clone(), json!(["read", "request"])));
    let keys = f.keys();
    assert_eq!(keys.len(), 1);
    assert_eq!((keys[0].name.as_str(), keys[0].grants.clone(), keys[0].app_version.as_str()), (t["name"].as_str().unwrap(), vec!["read".to_string(), "request".to_string()], "0.1.0"));
    // The same connection goes on to prove the key it just paired: the session is the signature's, not the ticket's.
    let name = keys[0].name.clone();
    let hello = k.hello(&A, &name).unwrap();
    assert_eq!((hello["scope"].as_str(), hello["name"].as_str(), hello["grants"].clone()), (Some("app"), Some(name.as_str()), json!(["read", "request"])));
    assert!(hello.get("token").is_none(), "there is no bearer token");
    assert_eq!(k.c.call("session.ping", json!({})).unwrap(), json!("pong"));

    // The ticket again: refused, by this key or another.
    for seed in [A, B] {
        let mut again = f.knock().unwrap();
        let r = again.pair(&seed, &ticket);
        again.refused_and_closed(r);
    }
    // A ticket past its time: refused, and spent.
    let late = f.console().call("auth.pair_ticket", json!({})).unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    let mut k = f.knock().unwrap();
    let r = k.pair(&B, late["ticket"].as_str().unwrap());
    k.refused_and_closed(r);
    // A ticket nobody issued, and a key that is not 32 bytes of hex.
    let mut k = f.knock().unwrap();
    let r = k.pair(&B, &"0".repeat(64));
    k.refused_and_closed(r);
    let good = f.console().call("auth.pair_ticket", json!({})).unwrap();
    let mut k = f.knock().unwrap();
    let r = k.c.call("app.pair", json!({"ticket": good["ticket"], "public_key": "not a key", "app_version": "0.1.0"}));
    k.refused_and_closed(r);

    assert_eq!(f.keys().len(), 1, "no refused pairing recorded a key");
    // What became of each ticket that was found, for the terminal that asked for it. The secret itself is never listed.
    let listed = f.console().call("app.keys", json!({})).unwrap();
    assert_eq!(listed["pending"], json!([]));
    assert_eq!(listed["spent"], json!([{"ticket": t["name"], "outcome": "paired", "key": t["name"]}, {"ticket": late["name"], "outcome": "expired_ticket", "key": ""}, {"ticket": good["name"], "outcome": "bad_public_key", "key": ""}]));
    assert!(!listed.to_string().contains(&ticket));
    let reasons: Vec<String> = f.refusals().iter().map(|p| p["reason"].as_str().unwrap().to_string()).collect();
    assert_eq!(reasons, ["bad_ticket", "bad_ticket", "expired_ticket", "bad_ticket", "bad_public_key"]);
    assert_eq!(f.kinds()[0], "app.paired");
    // The record is the operator's alone.
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(f.dir.path().join("operator/app-keys.json")).unwrap().permissions().mode() & 0o777, 0o600);
}

/// A key paired again (a second `delphi app`) keeps its record: a ticket never widens a key the operator narrowed.
#[tokio::test(flavor = "multi_thread")]
async fn pairing_a_recorded_key_again_changes_nothing_about_it() {
    let f = start(TtyPolicy::Disabled, |_| {}).await;
    let name = f.pair(&A);
    f.console().call("app.grant", json!({"name": name, "grant": "request", "remove": true})).unwrap();
    let t = f.console().call("auth.pair_ticket", json!({})).unwrap();
    let mut k = f.knock().unwrap();
    let again = k.pair(&A, t["ticket"].as_str().unwrap()).unwrap();
    assert_eq!((again["name"].as_str(), again["grants"].clone(), again["already_paired"].as_bool()), (Some(name.as_str()), json!(["read"]), Some(true)));
    assert_eq!(f.keys().len(), 1);
}

/// AC-2, the daemon's half: no ticket for a client that is not at a terminal, whatever token it holds; and none for any
/// scope but the console's. Revoking and granting are held to the same rule.
#[tokio::test(flavor = "multi_thread")]
async fn a_ticket_is_issued_only_to_a_console_client_at_a_terminal() {
    let f = start(TtyPolicy::Required, |_| {}).await;
    let out = run_child_without_tty(&f.sock, f.daemon.console_token());
    for m in ["auth.pair_ticket", "app.revoke", "app.grant"] {
        assert!(out.contains(&format!("PROBE {} rpc {}", m, code::TTY_REQUIRED)), "{m}: expected TTY_REQUIRED, got {out}");
    }
    assert!(out.contains("PROBE app.keys ok"), "listing changes nothing and needs no terminal: {out}");
    // The narrator socket does not offer the methods at all.
    let mut n = Client::connect(&f.narrator_sock).unwrap();
    n.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let tok = f.daemon.narrator_token().to_string();
    n.hello(&tok).unwrap();
    for m in ["auth.pair_ticket", "app.keys", "app.revoke", "app.grant", "app.pair", "app.hello"] {
        assert_eq!(rpc_code(n.call(m, json!({"name": "app-000000", "grant": "request"}))), Some(code::FORBIDDEN), "{m}");
    }
    assert!(f.keys().is_empty() && f.kinds().is_empty());

    // With an unsandboxed command running, pairing waits, as a new attachment does.
    let g = start(TtyPolicy::Disabled, |_| {}).await;
    g.daemon.amber_active.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(rpc_code(g.console().call("auth.pair_ticket", json!({}))), Some(code::CONFIRMATION_REQUIRED));
    g.daemon.amber_active.store(false, std::sync::atomic::Ordering::SeqCst);
    // And tickets do not pile up.
    let mut c = g.console();
    for _ in 0..4 {
        c.call("auth.pair_ticket", json!({})).unwrap();
    }
    assert_eq!(rpc_code(c.call("auth.pair_ticket", json!({}))), Some(code::FORBIDDEN));
}

/// Child half of the test above: a console client in a session of its own, with no controlling terminal.
#[test]
fn pair_probe_child() {
    let (Ok(sock), Ok(token)) = (std::env::var("DELPHI_PROBE_SOCK"), std::env::var("DELPHI_PROBE_TOKEN")) else { return };
    let mut c = Client::connect(Path::new(&sock)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    c.hello(&token).unwrap();
    for m in ["auth.pair_ticket", "app.keys", "app.revoke", "app.grant"] {
        match c.call(m, json!({"name": "app-000000", "grant": "request"})) {
            Ok(_) => println!("PROBE {} ok", m),
            Err(ClientError::Rpc { code, .. }) => println!("PROBE {} rpc {}", m, code),
            Err(e) => println!("PROBE {} err {}", m, e),
        }
    }
}

fn run_child_without_tty(sock: &Path, token: &str) -> String {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "pair_probe_child", "--nocapture", "--test-threads=1"]).env("DELPHI_PROBE_SOCK", sock).env("DELPHI_PROBE_TOKEN", token).stdin(std::process::Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    String::from_utf8_lossy(&cmd.output().unwrap().stdout).to_string()
}

/// AC-4: a recorded signature played again, a signature by another key, a signature for another socket or daemon, an
/// unknown name and a revoked key are each closed, and each leaves an `app.refused`. A token is no use on this socket.
#[tokio::test(flavor = "multi_thread")]
async fn a_replay_another_key_or_a_revoked_key_is_closed_and_recorded() {
    // The window is short so this test's refusals never add up to a lockout (that is the next test).
    let f = start(TtyPolicy::Disabled, |c| c.app_refusal_window = Duration::from_millis(1)).await;
    let name = f.pair(&A);
    let pause = || std::thread::sleep(Duration::from_millis(20));

    // A good exchange, recorded by an eavesdropper.
    let mut k = f.knock().unwrap();
    let recorded = k.signature(&A);
    k.hello_with(&name, &recorded).unwrap();
    assert_eq!(k.c.call("session.ping", json!({})).unwrap(), json!("pong"));
    // The path with links resolved names the same socket (`/tmp` is a link on macOS).
    let mut k = f.knock().unwrap();
    k.socket = std::fs::canonicalize(&f.app_sock).unwrap().to_string_lossy().into_owned();
    k.hello(&A, &name).unwrap();

    // 1. The recorded signature again: the nonce is new, so it covers nothing.
    let mut k = f.knock().unwrap();
    let r = k.hello_with(&name, &recorded);
    k.refused_and_closed(r);
    pause();
    // 2. Another key under the paired key's name.
    let mut k = f.knock().unwrap();
    let r = k.hello(&B, &name);
    k.refused_and_closed(r);
    pause();
    // 3. The right key, signing for another socket of the same daemon.
    let mut k = f.knock().unwrap();
    k.socket = f.narrator_sock.to_string_lossy().into_owned();
    let r = k.hello(&A, &name);
    k.refused_and_closed(r);
    pause();
    // 4. The right key, signing for another daemon.
    let mut k = f.knock().unwrap();
    k.instance = "0".repeat(32);
    let r = k.hello(&A, &name);
    k.refused_and_closed(r);
    pause();
    // 5. A name that was never paired, and one that is not a name at all (it is not written down).
    let mut k = f.knock().unwrap();
    let r = k.hello(&A, "app-ffffff");
    k.refused_and_closed(r);
    pause();
    let mut k = f.knock().unwrap();
    let r = k.hello(&A, "../../operator\n{\"x\":1}");
    k.refused_and_closed(r);
    pause();
    // 6. A revoked key, with a signature that would have been good.
    f.console().call("app.revoke", json!({"name": name})).unwrap();
    let mut k = f.knock().unwrap();
    let r = k.hello(&A, &name);
    k.refused_and_closed(r);

    let got: Vec<(String, Value)> = f.refusals().iter().map(|p| (p["reason"].as_str().unwrap().to_string(), p["name"].clone())).collect();
    let n = json!(name);
    assert_eq!(got, [("bad_signature".to_string(), n.clone()), ("bad_signature".into(), n.clone()), ("bad_signature".into(), n.clone()), ("bad_signature".into(), n.clone()), ("unknown_key".into(), json!("app-ffffff")), ("unknown_key".into(), Value::Null), ("unknown_key".into(), n)]);
    assert!(f.refusals().iter().all(|p| p["locked_out"] == false));

    // A token, of any scope, and a request for one: the connection is closed and nothing but the challenge was sent.
    for first in [json!({"jsonrpc": "2.0", "id": 1, "method": "hello", "params": {"protocol_version": 1, "token": f.daemon.console_token()}}), json!({"jsonrpc": "2.0", "id": 1, "method": "auth.issue", "params": {"scope": "console"}}), json!({"jsonrpc": "2.0", "id": 1, "method": "auth.issue", "params": {"scope": "app"}}), json!({"jsonrpc": "2.0", "id": 1, "method": "session.ping", "params": {}})] {
        let mut s = UnixStream::connect(&f.app_sock).unwrap();
        writeln!(s, "{}", first).unwrap();
        // (macOS refuses to set a timeout on a socket its peer has closed; the session already has one.)
        let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
        let mut all = String::new();
        let _ = s.read_to_string(&mut all);
        let lines: Vec<&str> = all.lines().collect();
        assert!(lines.len() == 1 && lines[0].contains("app.challenge"), "{first}: {all}");
    }
    // A protocol major the daemon does not speak is told so, and is not a refusal of the key.
    let before = f.refusals().len();
    let mut k = f.knock().unwrap();
    assert_eq!(rpc_code(k.c.call("app.hello", json!({"name": "app-ffffff", "signature": "", "protocol_version": 2}))), Some(code::VERSION_SKEW));
    assert_eq!(f.refusals().len(), before);
}

/// §3 item 4: five refusals in a minute close the socket to new connections, a good key's included, for a minute.
#[tokio::test(flavor = "multi_thread")]
async fn five_refusals_close_the_socket_to_new_connections_for_a_while() {
    let f = start(TtyPolicy::Disabled, |c| c.app_lockout = Duration::from_millis(700)).await;
    let name = f.pair(&A);
    let mut open = f.session(&A, &name);
    for i in 0..5 {
        let mut k = f.knock().unwrap_or_else(|| panic!("knock {i} should still be answered"));
        let r = k.hello(&B, &name);
        k.refused_and_closed(r);
    }
    let locked: Vec<bool> = f.refusals().iter().map(|p| p["locked_out"].as_bool().unwrap()).collect();
    assert_eq!(locked, [false, false, false, false, true]);
    assert!(f.knock().is_none(), "no challenge while the socket is closed to new connections");
    assert_eq!(f.refusals().len(), 5, "a knock on the closed socket is not recorded again");
    // A session that was already open is untouched.
    assert_eq!(open.call("session.ping", json!({})).unwrap(), json!("pong"));
    tokio::time::sleep(Duration::from_millis(900)).await;
    f.session(&A, &name).call("session.ping", json!({})).unwrap();
}

/// AC-6, first half: a method under no grant is refused; a method under a grant the key lacks is refused; and the record is
/// read for every call, so a grant given back applies to the next one. `grant` takes only names the table defines.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_calls_only_the_methods_of_the_grants_its_key_holds_now() {
    let f = start(TtyPolicy::Disabled, |_| {}).await;
    let name = f.pair(&A);
    let mut s = f.session(&A, &name);
    assert_eq!(s.call("session.ping", json!({})).unwrap(), json!("pong"));
    assert_eq!(s.call("events.subscribe", json!({})).unwrap()["subscribed"], true);
    assert_eq!(s.call("session.snapshot", json!({})).unwrap()["clients"]["app"], 1);
    for m in ["approval.resolve", "approval.relay_resolve", "budget.set", "permissions.set", "ratify.apply", "mission.rewind", "daemon.shutdown", "task.submit", "settings.set", "auth.pair_ticket", "app.keys", "app.revoke", "app.grant", "app.pair", "app.hello", "host.propose_note", "app.research_continue", "nope"] {
        match s.call(m, json!({"name": name, "grant": "request"})) {
            Err(ClientError::Rpc { code: c, message, .. }) => assert!(c == code::FORBIDDEN && message.contains("under no grant"), "{m}: {message}"),
            other => panic!("{m} must be refused, got {other:?}"),
        }
    }
    assert_eq!(f.keys()[0].grants, ["read", "request"], "nothing a session called changed its key");

    // `grant` and `--remove`: only names the table defines, only keys that exist.
    let mut con = f.console();
    for bad in ["answer", "settings", "", "*", "read,request"] {
        assert_eq!(rpc_code(con.call("app.grant", json!({"name": name, "grant": bad}))), Some(code::INVALID_PARAMS), "{bad:?}");
    }
    assert_eq!(rpc_code(con.call("app.grant", json!({"name": "app-000000", "grant": "read"}))), Some(code::NOT_FOUND));
    assert_eq!(con.call("app.grant", json!({"name": name, "grant": "read"})).unwrap()["changed"], false, "already held");

    // Take `read` away: the open session ends, and a new one may not read.
    let r = con.call("app.grant", json!({"name": name, "grant": "read", "remove": true})).unwrap();
    assert_eq!((r["grants"].clone(), r["sessions_closed"].as_u64()), (json!(["request"]), Some(1)));
    // (macOS refuses to set a timeout on a socket its peer has closed; the session already has one.)
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    assert!(matches!(s.call("session.ping", json!({})), Err(ClientError::Closed) | Err(ClientError::Io(_))));
    let mut k = f.knock().unwrap();
    assert_eq!(k.hello(&A, &name).unwrap()["grants"], json!(["request"]));
    match k.c.call("observe.state", json!({})) {
        Err(ClientError::Rpc { code: c, message, .. }) => assert!(c == code::FORBIDDEN && message.contains("`read` grant"), "{message}"),
        other => panic!("a key without `read` must not read, got {other:?}"),
    }
    // Give it back: the same connection may read on its next call.
    con.call("app.grant", json!({"name": name, "grant": "read"})).unwrap();
    assert_eq!(k.c.call("session.ping", json!({})).unwrap(), json!("pong"));
    let listed = con.call("app.keys", json!({})).unwrap();
    assert_eq!((listed["keys"][0]["name"].as_str(), listed["keys"][0]["sessions"].as_u64(), listed["keys"][0]["grants"].clone()), (Some(name.as_str()), Some(1), json!(["request", "read"])));
    assert!(listed["keys"][0].get("public_key").is_none());
    assert_eq!(f.kinds().iter().filter(|k| *k == "app.granted").count(), 2, "the removal and the giving back; the no-op is not news");
}

/// AC-10: revoking a key closes its open sessions at once, and only its own.
#[tokio::test(flavor = "multi_thread")]
async fn revoking_a_key_closes_its_open_sessions_at_once() {
    let f = start(TtyPolicy::Disabled, |_| {}).await;
    let (a, b) = (f.pair(&A), f.pair(&B));
    let (mut a1, mut a2, mut b1) = (f.session(&A, &a), f.session(&A, &a), f.session(&B, &b));
    a1.call("events.subscribe", json!({})).unwrap();
    let mut con = f.console();
    assert_eq!(rpc_code(con.call("app.revoke", json!({"name": "app-000000"}))), Some(code::NOT_FOUND));
    let r = con.call("app.revoke", json!({"name": a})).unwrap();
    assert_eq!(r["sessions_closed"], 2);
    // Both are closed without their asking anything: the next read finds the end of the stream.
    for s in [&mut a1, &mut a2] {
        // (macOS refuses to set a timeout on a socket its peer has closed; the session already has one.)
        let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
        assert!(matches!(s.next_notification(), Err(ClientError::Closed)), "a revoked key's session must be closed by the daemon");
    }
    assert_eq!(b1.call("session.ping", json!({})).unwrap(), json!("pong"), "another key's session is untouched");
    assert_eq!(f.keys().iter().map(|k| k.name.as_str()).collect::<Vec<_>>(), [b.as_str()]);
    assert!(f.audit.lock().unwrap().iter().any(|(k, p)| k == "app.revoked" && p["name"] == json!(a) && p["sessions_closed"] == 2));
    // A key removed from the record behind the daemon's back ends its session at the next call.
    std::fs::write(f.dir.path().join("operator/app-keys.json"), "{\"version\":1,\"keys\":[]}\n").unwrap();
    assert_eq!(rpc_code(b1.call("session.ping", json!({}))), Some(code::FORBIDDEN));
    // (macOS refuses to set a timeout on a socket its peer has closed; the session already has one.)
    let _ = b1.set_read_timeout(Some(Duration::from_secs(3)));
    assert!(matches!(b1.call("session.ping", json!({})), Err(ClientError::Closed) | Err(ClientError::Io(_))));
}
