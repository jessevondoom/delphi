//! `delphi relay`: run the relay for one workspace (`specs/014`).
//!
//! The relay is its own process, started by the operator in a terminal (a `tmux` pane is the usual place on a
//! server): the daemon gives a relay token only to a peer with a controlling terminal, the same rule as the
//! console, so no secret sits in a file. If the daemon restarts, the relay attaches again by itself.
//!
//! Its settings come from the **ratified** copy of `config.toml` (`[relay]` is one of the pinned blocks), so
//! editing the file cannot add an address that may answer a card until the operator ratifies it.

use super::email::{EmailChannel, EmailConfig};
use super::net::{Encryption, ImapSmtp, NetConfig};
use super::{Config, Daemon, Detail, Relay, ResolveFail};
use delphi_core::client::{Client, ClientError};
use delphi_core::proto::{code, Scope};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct EmailSettings {
    pub cfg: EmailConfig,
    pub net: NetConfig,
    /// A credential reference (`keychain:…`, `env:…`, `command:[…]`); the password is resolved at start.
    pub credential: String,
}

#[derive(Debug)]
pub struct Settings {
    pub core: Config,
    pub poll: Duration,
    pub email: Option<EmailSettings>,
}

fn str_of<'a>(t: &'a toml::Value, k: &str) -> Option<&'a str> {
    t.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

fn need<'a>(t: &'a toml::Value, k: &str, table: &str) -> Result<&'a str, String> {
    str_of(t, k).ok_or_else(|| format!("[{}] needs `{}`", table, k))
}

/// Read `[relay]` from a config's text. `Ok(None)` when there is no `[relay]` table.
pub fn settings_from(text: &str) -> Result<Option<Settings>, String> {
    let root: toml::Value = toml::from_str(text).map_err(|e| format!("config.toml is not valid TOML: {}", e))?;
    let Some(r) = root.get("relay") else { return Ok(None) };
    let channel = str_of(r, "channel").unwrap_or("email");
    if channel != "email" {
        return Err(format!("[relay] channel `{}` is not available; only `email` is built", channel));
    }
    let e = r.get("email").ok_or("[relay] channel = \"email\" needs a [relay.email] table")?;
    let mailbox = need(e, "mailbox", "relay.email")?.to_lowercase();
    let operator = need(e, "operator", "relay.email")?.to_lowercase();
    if !operator.contains('@') || !mailbox.contains('@') {
        return Err("[relay.email] `mailbox` and `operator` must be email addresses".into());
    }
    if operator == mailbox {
        return Err("[relay.email] `operator` must not be the relay's own mailbox: the relay would answer itself".into());
    }
    let mut allow = vec![operator.clone()];
    allow.extend(e.get("also_allow").and_then(|a| a.as_array()).into_iter().flatten().filter_map(|v| v.as_str()).map(|s| s.trim().to_lowercase()));
    let mut core = Config::new("email", allow);
    core.detail = match str_of(r, "detail").unwrap_or("summary") {
        "summary" => Detail::Summary,
        "full" => Detail::Full,
        other => return Err(format!("[relay] detail takes summary or full, not `{}`", other)),
    };
    core.require_auth = r.get("require_auth").and_then(|v| v.as_bool()).unwrap_or(true);
    core.notify_tasks = r.get("notify_tasks").and_then(|v| v.as_bool()).unwrap_or(true);
    let int = |k: &str| r.get(k).and_then(|v| v.as_integer()).filter(|v| *v > 0).map(|v| v as usize);
    if let Some(v) = int("max_outstanding") {
        core.max_outstanding = v;
    }
    if let Some(v) = int("max_per_hour") {
        core.max_per_hour = v;
    }
    let poll = Duration::from_secs(int("poll_secs").unwrap_or(30).max(5) as u64);
    let tls = match str_of(e, "tls").unwrap_or("implicit") {
        "implicit" => Encryption::Implicit,
        "starttls" => Encryption::StartTls,
        "none" => Encryption::None,
        other => return Err(format!("[relay.email] tls takes implicit, starttls or none, not `{}`", other)),
    };
    let port = |k: &str, implicit: u16, starttls: u16, plain: u16| e.get(k).and_then(|v| v.as_integer()).filter(|v| (1..=65535).contains(v)).map(|v| v as u16).unwrap_or(match tls {
        Encryption::Implicit => implicit,
        Encryption::StartTls => starttls,
        Encryption::None => plain,
    });
    let net = NetConfig {
        imap_host: need(e, "imap_host", "relay.email")?.to_string(),
        imap_port: port("imap_port", 993, 143, 143),
        smtp_host: need(e, "smtp_host", "relay.email")?.to_string(),
        smtp_port: port("smtp_port", 465, 587, 25),
        username: str_of(e, "username").unwrap_or(&mailbox).to_string(),
        password: String::new(),
        tls,
    };
    let cfg = EmailConfig { mailbox, operator, authserv_id: str_of(e, "authserv_id").map(String::from) };
    Ok(Some(Settings { core, poll, email: Some(EmailSettings { cfg, net, credential: need(e, "credential", "relay.email")?.to_string() }) }))
}

/// The relay's connection to the daemon's relay socket.
pub struct Link {
    cmd: Client,
}

impl Daemon for Link {
    fn resolve(&mut self, approval_id: &str, payload_blake3: &str, allow: bool, channel: &str, sender: &str) -> Result<(), ResolveFail> {
        match self.cmd.call("approval.relay_resolve", json!({"approval_id": approval_id, "payload_blake3": payload_blake3, "decision": if allow { "allow" } else { "deny" }, "channel": channel, "sender": sender})) {
            Ok(_) => Ok(()),
            Err(ClientError::Rpc { code: c, message, .. }) if c == code::NOT_FOUND => Err(ResolveFail::Gone(message)),
            Err(ClientError::Rpc { code: c, message, .. }) if c == code::FORBIDDEN => Err(ResolveFail::Forbidden(message)),
            Err(ClientError::Rpc { message, .. }) => Err(ResolveFail::Invalid(message)),
            Err(e) => Err(ResolveFail::Down(e.to_string())),
        }
    }
}

type Events = mpsc::Receiver<Option<Value>>;

/// Attach to the relay socket: a token (needs this process to have a terminal), a command connection, and an
/// event connection read by a thread. Also returns the cards already waiting.
fn attach(sock: &Path) -> Result<(Link, Events, Vec<Value>), String> {
    let mut first = Client::connect(sock).map_err(|e| format!("no daemon is listening ({}); start `delphi` for this workspace first", e))?;
    first.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let token = first.issue_token(Scope::Relay).map_err(|e| match e {
        ClientError::Rpc { code: c, .. } if c == code::TTY_REQUIRED => "the daemon only gives a relay token to a process attached to a terminal: run `delphi relay` in a terminal or a tmux pane".to_string(),
        ClientError::Rpc { code: c, message, .. } if c == code::CONFIRMATION_REQUIRED => format!("waiting for the operator: {}", message),
        other => other.to_string(),
    })?;
    first.hello(&token).map_err(|e| e.to_string())?;
    let mut ev = Client::connect(sock).map_err(|e| e.to_string())?;
    ev.hello(&token).map_err(|e| e.to_string())?;
    ev.call("events.subscribe", json!({})).map_err(|e| e.to_string())?;
    let pending = first.call("approval.list", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    first.set_read_timeout(None).ok();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || loop {
        match ev.next_notification() {
            Ok(n) if n["method"] == "event" => {
                if tx.send(Some(n["params"].clone())).is_err() {
                    return;
                }
            }
            Ok(_) => {}
            Err(_) => {
                let _ = tx.send(None);
                return;
            }
        }
    });
    Ok((Link { cmd: first }, rx, pending))
}

fn resolve_password(credential: &str) -> Result<String, String> {
    use delphi_adapter::credential::{CredentialRef, Resolved, Resolver};
    let r = CredentialRef::parse(credential)?;
    let resolver = Resolver::system(std::sync::Arc::new(|_: &str| {}));
    match resolver.resolve(&r) {
        Ok(Resolved::Token(s)) => Ok(s.expose().to_string()),
        Ok(_) => Err("the credential did not give a password (use keychain:, env: or command:)".into()),
        Err(e) => Err(format!("could not read the mailbox password: {}", e)),
    }
}

fn load(ws_dir: &Path) -> Result<Settings, String> {
    let live = std::fs::read_to_string(ws_dir.join("config.toml")).map_err(|e| format!("cannot read config.toml: {}", e))?;
    // The ratified copy of the access blocks, which now include [relay]: a local edit has no effect until ratified.
    let (text, notices) = delphi_engine::boot::effective_config(ws_dir, &live)?;
    for n in notices {
        eprintln!("delphi relay: note: {}", n);
    }
    settings_from(&text)?.ok_or_else(|| "there is no [relay] table in the ratified config. Add one to config.toml, then `/ratify` it in the console (see specs/014).".to_string())
}

/// `delphi relay [check]`. Returns the process exit code.
pub fn run(home: &Path, ws_dir: &Path, args: &[String]) -> i32 {
    let mut s = match load(ws_dir) {
        Ok(s) => s,
        Err(m) => {
            eprintln!("delphi relay: {}", m);
            return 2;
        }
    };
    let Some(mut es) = s.email.take() else { return 2 };
    es.net.password = match resolve_password(&es.credential) {
        Ok(p) => p,
        Err(m) => {
            eprintln!("delphi relay: {}", m);
            return 2;
        }
    };
    if let Err(m) = es.net.validate() {
        eprintln!("delphi relay: {}", m);
        return 2;
    }
    let transport = ImapSmtp { cfg: es.net.clone(), from: es.cfg.mailbox.clone() };
    if args.first().map(|a| a.as_str()) == Some("check") {
        return match transport.check() {
            Ok(()) => {
                println!("delphi relay: the mailbox {} accepts the login for sending and reading", es.cfg.mailbox);
                println!("  cards go to {}; replies are accepted from {}", es.cfg.operator, s.core.allow.join(", "));
                if es.cfg.authserv_id.is_none() {
                    println!("  note: no authserv_id is set, so the topmost Authentication-Results header counts whoever wrote it; set it to your provider's id (for Gmail, mx.google.com)");
                }
                0
            }
            Err(m) => {
                eprintln!("delphi relay: {}", m);
                1
            }
        };
    }
    let l = match delphi_core::spawn::layout(home, ws_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("delphi relay: {}", e);
            return 2;
        }
    };
    let chan = EmailChannel { cfg: es.cfg.clone(), transport };
    serve(&l.relay_sock, s.core, s.poll, chan)
}

/// Run until killed: attach, announce waiting cards, then forward events and poll for replies; on a lost
/// connection attach again.
fn serve<C: super::Channel>(sock: &Path, core: Config, poll: Duration, mut chan: C) -> i32 {
    // Replies already sitting in the inbox when we start belong to cards that no longer exist: read and drop them.
    if let Err(e) = chan.poll() {
        eprintln!("delphi relay: cannot read the mailbox: {}", e);
        return 1;
    }
    let mut relay: Option<Relay<C, Link>> = None;
    let mut chan_slot = Some(chan);
    let mut backoff = Duration::from_secs(2);
    let mut said_waiting = false;
    println!("delphi relay: ready ({} → {}); cards will be sent as they are raised", core.channel, core.allow.join(", "));
    loop {
        let (link, rx, pending) = match attach(sock) {
            Ok(v) => {
                backoff = Duration::from_secs(2);
                said_waiting = false;
                v
            }
            Err(m) => {
                if !said_waiting {
                    eprintln!("delphi relay: {}; trying again", m);
                    said_waiting = true;
                }
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
                continue;
            }
        };
        let r = match relay.take() {
            Some(mut r) => {
                r.daemon = link;
                r
            }
            None => Relay::new(core.clone(), chan_slot.take().expect("the channel is kept between attachments"), link),
        };
        let mut r = r;
        r.sync_pending(&pending, Instant::now());
        let mut last_poll = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(Some(e)) => r.on_event(&e, Instant::now()),
                Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let now = Instant::now();
            r.expire(now);
            if now.saturating_duration_since(last_poll) >= poll {
                last_poll = now;
                r.poll_once(now);
            }
        }
        eprintln!("delphi relay: lost the daemon; attaching again");
        relay = Some(r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
[relay]
channel = "email"
detail = "summary"
poll_secs = 20
[relay.email]
mailbox = "Delphi.Relay@gmail.com"
operator = "Me@Example.com"
also_allow = ["Phone@Example.net"]
imap_host = "imap.gmail.com"
smtp_host = "smtp.gmail.com"
credential = "keychain:delphi-relay-mail"
authserv_id = "mx.google.com"
"#;

    #[test]
    fn a_good_config_gives_a_lowercased_allowlist_and_the_standard_ports() {
        let s = settings_from(GOOD).unwrap().unwrap();
        assert_eq!(s.core.allow, vec!["me@example.com", "phone@example.net"]);
        assert_eq!((s.core.detail, s.core.require_auth, s.core.notify_tasks), (Detail::Summary, true, true));
        assert_eq!(s.poll, Duration::from_secs(20));
        let e = s.email.unwrap();
        assert_eq!((e.net.imap_port, e.net.smtp_port, e.net.tls), (993, 465, Encryption::Implicit));
        assert_eq!((e.net.username.as_str(), e.cfg.mailbox.as_str(), e.cfg.authserv_id.as_deref()), ("delphi.relay@gmail.com", "delphi.relay@gmail.com", Some("mx.google.com")));
        assert_eq!(e.credential, "keychain:delphi-relay-mail");
        let st = settings_from(&GOOD.replace("[relay.email]", "[relay.email]\ntls = \"starttls\"")).unwrap().unwrap().email.unwrap();
        assert_eq!((st.net.imap_port, st.net.smtp_port), (143, 587));
    }

    #[test]
    fn no_relay_table_means_no_relay_and_mistakes_are_named() {
        assert!(settings_from("[budget]\nmission_budget_usd = 5\n").unwrap().is_none());
        for (edit, want) in [
            (("channel = \"email\"", "channel = \"slack\""), "only `email` is built"),
            (("detail = \"summary\"", "detail = \"everything\""), "summary or full"),
            (("operator = \"Me@Example.com\"", "operator = \"delphi.relay@gmail.com\""), "the relay would answer itself"),
            (("operator = \"Me@Example.com\"", "operator = \"not-an-address\""), "must be email addresses"),
            (("imap_host = \"imap.gmail.com\"\n", ""), "needs `imap_host`"),
            (("credential = \"keychain:delphi-relay-mail\"\n", ""), "needs `credential`"),
        ] {
            let e = settings_from(&GOOD.replace(edit.0, edit.1)).unwrap_err();
            assert!(e.contains(want), "{:?} gave {}", edit, e);
        }
        assert!(settings_from("[relay]\nchannel = \"email\"\n").unwrap_err().contains("[relay.email]"));
        assert!(settings_from(&GOOD.replace("[relay.email]", "[relay.email]\ntls = \"maybe\"")).unwrap_err().contains("implicit, starttls or none"));
    }

    #[test]
    fn the_relay_table_is_read_from_the_ratified_copy_and_a_local_edit_adds_no_approver() {
        // The pinned blocks include [relay]; effective_config keeps the ratified ones when the live file differs.
        let t = tempfile::tempdir().unwrap();
        let ws = t.path();
        std::fs::create_dir_all(ws.join("local/ratified")).unwrap();
        let live = GOOD.replace("Me@Example.com", "attacker@example.com");
        // Nothing is ratified, so no [relay] settings are trusted at all.
        let (text, notices) = delphi_engine::boot::effective_config(ws, &live).unwrap();
        assert!(settings_from(&text).unwrap().is_none(), "an unratified [relay] is ignored: {}", text);
        assert!(!notices.is_empty() || !text.contains("attacker@example.com"));
    }
}
