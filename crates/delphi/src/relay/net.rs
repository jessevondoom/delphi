//! The mailbox over the network: SMTP out (lettre) and IMAP in (imap), both over TLS (rustls).
//!
//! * TLS is required. Plain text is accepted only when the host is the loopback address, which is what the tests
//!   (a fake server on localhost) use; a real host with `tls = "none"` is a configuration error.
//! * Each poll opens a fresh IMAP session, reads only unseen mail in `INBOX`, skips messages over a size cap,
//!   marks what it read as seen, and logs out. It never deletes, moves or sends anything through IMAP.

use super::email::{MailTransport, OutMail};
use lettre::message::header::{Header, HeaderName, HeaderValue};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encryption {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    Implicit,
    /// Upgrade with STARTTLS (IMAP 143, SMTP 587); refused if the server does not offer it.
    StartTls,
    /// No encryption. Loopback only.
    None,
}

#[derive(Clone)]
pub struct NetConfig {
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: String,
    pub tls: Encryption,
}

/// Never prints the password, so a `{:?}` in a log or a failed assertion cannot leak it.
impl std::fmt::Debug for NetConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetConfig").field("imap_host", &self.imap_host).field("imap_port", &self.imap_port).field("smtp_host", &self.smtp_host).field("smtp_port", &self.smtp_port).field("username", &self.username).field("password", &"<redacted>").field("tls", &self.tls).finish()
    }
}

/// Messages larger than this are marked read and dropped unread: a reply is a few lines.
const MAX_MESSAGE_BYTES: u64 = 256 * 1024;
/// At most this many messages are read per poll.
const MAX_PER_POLL: usize = 50;

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

impl NetConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.tls == Encryption::None && !(is_loopback(&self.imap_host) && is_loopback(&self.smtp_host)) {
            return Err("tls = \"none\" is only allowed for a mailbox on this machine (localhost); use \"implicit\" or \"starttls\"".into());
        }
        for (what, v) in [("imap_host", &self.imap_host), ("smtp_host", &self.smtp_host), ("username", &self.username)] {
            if v.trim().is_empty() || v.chars().any(|c| c.is_control() || c.is_whitespace()) {
                return Err(format!("`{}` is empty or has odd characters", what));
            }
        }
        if self.password.is_empty() {
            return Err("the mailbox password is empty".into());
        }
        Ok(())
    }
}

pub struct ImapSmtp {
    pub cfg: NetConfig,
    /// The mailbox cards are sent from.
    pub from: String,
}

macro_rules! plain_header {
    ($ty:ident, $name:expr) => {
        #[derive(Clone)]
        struct $ty(String);
        impl Header for $ty {
            fn name() -> HeaderName {
                HeaderName::new_from_ascii_str($name)
            }
            fn parse(s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
                Ok($ty(s.to_string()))
            }
            fn display(&self) -> HeaderValue {
                HeaderValue::new(Self::name(), self.0.clone())
            }
        }
    };
}

plain_header!(AutoSubmitted, "Auto-Submitted");
plain_header!(AutoResponseSuppress, "X-Auto-Response-Suppress");

impl ImapSmtp {
    fn smtp(&self) -> Result<SmtpTransport, String> {
        let c = &self.cfg;
        let creds = Credentials::new(c.username.clone(), c.password.clone());
        let b = match c.tls {
            Encryption::Implicit => SmtpTransport::relay(&c.smtp_host).map_err(|e| e.to_string())?,
            Encryption::StartTls => SmtpTransport::starttls_relay(&c.smtp_host).map_err(|e| e.to_string())?,
            Encryption::None => SmtpTransport::builder_dangerous(&c.smtp_host),
        };
        Ok(b.port(c.smtp_port).credentials(creds).timeout(Some(std::time::Duration::from_secs(30))).build())
    }

    /// Log in to both servers without sending or reading anything: `delphi relay check`.
    pub fn check(&self) -> Result<(), String> {
        self.cfg.validate()?;
        self.smtp()?.test_connection().map_err(|e| format!("SMTP: {}", e)).and_then(|ok| if ok { Ok(()) } else { Err("SMTP: the server did not accept the connection".into()) })?;
        let mut s = self.imap_session()?;
        s.select("INBOX").map_err(|e| format!("IMAP: cannot open INBOX: {}", e))?;
        let _ = s.logout();
        Ok(())
    }

    fn imap_session(&self) -> Result<imap::Session<imap::Connection>, String> {
        let c = &self.cfg;
        let mode = match c.tls {
            Encryption::Implicit => imap::ConnectionMode::Tls,
            Encryption::StartTls => imap::ConnectionMode::StartTls,
            Encryption::None => imap::ConnectionMode::Plaintext,
        };
        let client = imap::ClientBuilder::new(c.imap_host.clone(), c.imap_port).mode(mode).tls_kind(imap::TlsKind::Rust).connect().map_err(|e| format!("IMAP connect: {}", e))?;
        client.login(&c.username, &c.password).map_err(|e| format!("IMAP login: {}", e.0))
    }
}

impl MailTransport for ImapSmtp {
    fn send(&mut self, m: &OutMail) -> Result<(), String> {
        self.cfg.validate()?;
        let mut b = Message::builder().from(self.from.parse().map_err(|e| format!("bad from address: {}", e))?).to(m.to.parse().map_err(|e| format!("bad operator address: {}", e))?).subject(m.subject.clone()).message_id(Some(m.message_id.clone()));
        for (k, v) in &m.headers {
            b = match k.as_str() {
                "Auto-Submitted" => b.header(AutoSubmitted(v.clone())),
                "X-Auto-Response-Suppress" => b.header(AutoResponseSuppress(v.clone())),
                other => return Err(format!("the transport does not support the header `{}`", other)),
            };
        }
        let msg = b.body(m.body.clone()).map_err(|e| format!("could not build the message: {}", e))?;
        self.smtp()?.send(&msg).map(|_| ()).map_err(|e| format!("SMTP send: {}", e))
    }

    fn fetch_new(&mut self) -> Result<Vec<Vec<u8>>, String> {
        self.cfg.validate()?;
        let mut s = self.imap_session()?;
        s.select("INBOX").map_err(|e| format!("IMAP select: {}", e))?;
        let mut ids: Vec<u32> = s.search("UNSEEN").map_err(|e| format!("IMAP search: {}", e))?.into_iter().collect();
        ids.sort_unstable();
        ids.truncate(MAX_PER_POLL);
        if ids.is_empty() {
            let _ = s.logout();
            return Ok(vec![]);
        }
        let set = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        // Sizes first, so a huge message is never pulled into memory.
        let sizes = s.fetch(&set, "RFC822.SIZE").map_err(|e| format!("IMAP fetch size: {}", e))?;
        let small: Vec<u32> = sizes.iter().filter(|f| f.size.is_some_and(|n| u64::from(n) <= MAX_MESSAGE_BYTES)).map(|f| f.message).collect();
        let mut out = Vec::new();
        if !small.is_empty() {
            let small_set = small.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
            let bodies = s.fetch(&small_set, "BODY.PEEK[]").map_err(|e| format!("IMAP fetch: {}", e))?;
            for f in bodies.iter() {
                if let Some(b) = f.body() {
                    out.push(b.to_vec());
                }
            }
        }
        // Everything looked at, including what was too big to read, is marked seen so it is not returned again.
        s.store(&set, "+FLAGS (\\Seen)").map_err(|e| format!("IMAP store: {}", e))?;
        let _ = s.logout();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    /// A just-enough SMTP server: EHLO, AUTH PLAIN, MAIL, RCPT, DATA, QUIT. Records what it was sent.
    fn fake_smtp(received: Arc<Mutex<Vec<String>>>, accept_auth: bool) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let received = received.clone();
                std::thread::spawn(move || {
                    let mut w = s.try_clone().unwrap();
                    let mut r = BufReader::new(s);
                    let _ = w.write_all(b"220 fake ESMTP\r\n");
                    let mut line = String::new();
                    let mut in_data = false;
                    let mut data = String::new();
                    while {
                        line.clear();
                        r.read_line(&mut line).unwrap_or(0) > 0
                    } {
                        if in_data {
                            if line == ".\r\n" {
                                in_data = false;
                                received.lock().unwrap().push(std::mem::take(&mut data));
                                let _ = w.write_all(b"250 queued\r\n");
                            } else {
                                data.push_str(&line);
                            }
                            continue;
                        }
                        let up = line.to_uppercase();
                        if up.starts_with("EHLO") || up.starts_with("HELO") {
                            let _ = w.write_all(b"250-fake\r\n250 AUTH PLAIN LOGIN\r\n");
                        } else if up.starts_with("AUTH") {
                            let _ = w.write_all(if accept_auth { b"235 ok\r\n" as &[u8] } else { b"535 bad credentials\r\n" });
                        } else if up.starts_with("MAIL") || up.starts_with("RCPT") || up.starts_with("RSET") || up.starts_with("NOOP") {
                            let _ = w.write_all(b"250 ok\r\n");
                        } else if up.starts_with("DATA") {
                            in_data = true;
                            let _ = w.write_all(b"354 go\r\n");
                        } else if up.starts_with("QUIT") {
                            let _ = w.write_all(b"221 bye\r\n");
                            return;
                        } else {
                            let _ = w.write_all(b"500 what\r\n");
                        }
                    }
                });
            }
        });
        port
    }

    /// A just-enough IMAP server holding `messages` (raw bytes), all unseen at first.
    fn fake_imap(messages: Vec<Vec<u8>>, seen: Arc<Mutex<Vec<u32>>>, password: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let messages = Arc::new(messages);
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let (messages, seen) = (messages.clone(), seen.clone());
                std::thread::spawn(move || serve_imap(s, &messages, &seen, password));
            }
        });
        port
    }

    fn serve_imap(s: TcpStream, messages: &[Vec<u8>], seen: &Arc<Mutex<Vec<u32>>>, password: &str) {
        let mut w = s.try_clone().unwrap();
        let mut r = BufReader::new(s);
        let _ = w.write_all(b"* OK fake IMAP ready\r\n");
        let mut line = String::new();
        while {
            line.clear();
            r.read_line(&mut line).unwrap_or(0) > 0
        } {
            let mut parts = line.trim_end().splitn(3, ' ');
            let (tag, cmd, rest) = (parts.next().unwrap_or(""), parts.next().unwrap_or("").to_uppercase(), parts.next().unwrap_or(""));
            let unseen: Vec<u32> = (1..=messages.len() as u32).filter(|i| !seen.lock().unwrap().contains(i)).collect();
            let ids = |set: &str| -> Vec<u32> { set.split(',').filter_map(|x| x.trim().parse().ok()).collect() };
            match cmd.as_str() {
                "LOGIN" => {
                    let ok = rest.contains(password) && !password.is_empty();
                    let _ = w.write_all(format!("{} {}\r\n", tag, if ok { "OK logged in" } else { "NO [AUTHENTICATIONFAILED] bad" }).as_bytes());
                }
                "SELECT" => {
                    let _ = w.write_all(format!("* {} EXISTS\r\n* 0 RECENT\r\n* FLAGS (\\Seen)\r\n{} OK [READ-WRITE] done\r\n", messages.len(), tag).as_bytes());
                }
                "SEARCH" => {
                    let list = unseen.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
                    let _ = w.write_all(format!("* SEARCH {}\r\n{} OK done\r\n", list, tag).as_bytes());
                }
                "FETCH" => {
                    let (set, what) = rest.split_once(' ').unwrap_or((rest, ""));
                    for i in ids(set) {
                        let m = &messages[i as usize - 1];
                        if what.contains("RFC822.SIZE") {
                            let _ = w.write_all(format!("* {} FETCH (RFC822.SIZE {})\r\n", i, m.len()).as_bytes());
                        } else {
                            let _ = w.write_all(format!("* {} FETCH (BODY[] {{{}}}\r\n", i, m.len()).as_bytes());
                            let _ = w.write_all(m);
                            let _ = w.write_all(b")\r\n");
                        }
                    }
                    let _ = w.write_all(format!("{} OK done\r\n", tag).as_bytes());
                }
                "STORE" => {
                    let (set, _) = rest.split_once(' ').unwrap_or((rest, ""));
                    seen.lock().unwrap().extend(ids(set));
                    let _ = w.write_all(format!("{} OK stored\r\n", tag).as_bytes());
                }
                "LOGOUT" => {
                    let _ = w.write_all(format!("* BYE\r\n{} OK bye\r\n", tag).as_bytes());
                    return;
                }
                _ => {
                    let _ = w.write_all(format!("{} BAD unknown\r\n", tag).as_bytes());
                }
            }
        }
        let _ = r.read(&mut [0u8; 1]);
    }

    fn cfg(smtp_port: u16, imap_port: u16, password: &str) -> NetConfig {
        NetConfig { imap_host: "127.0.0.1".into(), imap_port, smtp_host: "127.0.0.1".into(), smtp_port, username: "delphi.relay@example.net".into(), password: password.into(), tls: Encryption::None }
    }

    #[test]
    fn the_password_never_appears_in_debug_output() {
        let c = cfg(25, 143, "hunter2-secret");
        assert!(!format!("{:?}", c).contains("hunter2-secret") && format!("{:?}", c).contains("<redacted>"));
    }

    #[test]
    fn plain_text_is_refused_for_any_host_but_localhost_and_empty_fields_are_refused() {
        let mut c = cfg(25, 143, "pw");
        c.imap_host = "imap.example.com".into();
        assert!(c.validate().unwrap_err().contains("only allowed for a mailbox on this machine"));
        c.tls = Encryption::Implicit;
        assert!(c.validate().is_ok());
        for (field, bad) in [("imap_host", " "), ("username", "a b"), ("smtp_host", "x\ny")] {
            let mut d = c.clone();
            match field {
                "imap_host" => d.imap_host = bad.into(),
                "username" => d.username = bad.into(),
                _ => d.smtp_host = bad.into(),
            }
            assert!(d.validate().is_err(), "{}", field);
        }
        c.password.clear();
        assert!(c.validate().unwrap_err().contains("password is empty"));
    }

    #[test]
    fn a_card_is_delivered_with_its_headers_and_the_message_id_survives() {
        let got = Arc::new(Mutex::new(vec![]));
        let smtp = fake_smtp(got.clone(), true);
        let mut t = ImapSmtp { cfg: cfg(smtp, 1, "pw"), from: "delphi.relay@example.net".into() };
        let m = OutMail { to: "me@example.com".into(), subject: "[delphi:ap_1] write x".into(), body: "Delphi needs your decision.\nAPPROVE abc\n".into(), message_id: "<delphi.0123@example.net>".into(), headers: vec![("Auto-Submitted".into(), "auto-generated".into()), ("X-Auto-Response-Suppress".into(), "All".into())] };
        t.send(&m).unwrap();
        let wire = got.lock().unwrap().remove(0);
        assert!(wire.contains("To: me@example.com") && wire.contains("From: delphi.relay@example.net"), "{}", wire);
        assert!(wire.contains("Subject: [delphi:ap_1] write x") && wire.contains("Auto-Submitted: auto-generated") && wire.contains("X-Auto-Response-Suppress: All"), "{}", wire);
        assert!(wire.contains("Message-ID: <delphi.0123@example.net>") && wire.contains("APPROVE abc"), "{}", wire);
        let bad = OutMail { headers: vec![("X-Evil".into(), "1".into())], ..m };
        assert!(t.send(&bad).unwrap_err().contains("does not support the header"));
    }

    #[test]
    fn a_rejected_login_is_an_error_and_not_a_silent_success() {
        let got = Arc::new(Mutex::new(vec![]));
        let smtp = fake_smtp(got.clone(), false);
        let mut t = ImapSmtp { cfg: cfg(smtp, 1, "wrong"), from: "delphi.relay@example.net".into() };
        let m = OutMail { to: "me@example.com".into(), subject: "s".into(), body: "b".into(), message_id: "<x@example.net>".into(), headers: vec![] };
        assert!(t.send(&m).is_err());
        assert!(got.lock().unwrap().is_empty());
    }

    #[test]
    fn unseen_mail_is_read_once_marked_seen_and_oversize_mail_is_skipped_unread() {
        let seen = Arc::new(Mutex::new(vec![]));
        let small = b"From: me@example.com\r\nMessage-ID: <1@x>\r\n\r\nAPPROVE 0123\r\n".to_vec();
        let huge = vec![b'x'; (MAX_MESSAGE_BYTES + 10) as usize];
        let imap = fake_imap(vec![small.clone(), huge], seen.clone(), "pw");
        let mut t = ImapSmtp { cfg: cfg(1, imap, "pw"), from: "delphi.relay@example.net".into() };
        let first = t.fetch_new().unwrap();
        assert_eq!(first, vec![small], "the small message was read; the huge one was not");
        assert_eq!(*seen.lock().unwrap(), vec![1, 2], "both were marked seen, so neither comes back");
        assert!(t.fetch_new().unwrap().is_empty());
    }

    #[test]
    fn an_imap_login_failure_is_reported() {
        let imap = fake_imap(vec![], Arc::new(Mutex::new(vec![])), "right");
        let mut t = ImapSmtp { cfg: cfg(1, imap, "wrong"), from: "x@example.net".into() };
        assert!(t.fetch_new().unwrap_err().contains("IMAP login"));
    }
}
