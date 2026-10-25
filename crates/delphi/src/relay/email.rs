//! The email channel (`specs/014` §3): plain-text cards out, replies in.
//!
//! Everything about a message that decides whether it counts is decided here and nowhere else:
//!
//! * a reply is read **only for its first line of new text**; quoted history, signatures and everything below
//!   are never looked at, so a quoted old code cannot answer a new card;
//! * the `From` header is trivially forged, so it is only a filter. The sender counts as verified only if the
//!   receiving provider's own `Authentication-Results` header (the topmost one from the configured server)
//!   reports a pass aligned with the `From` domain;
//! * auto-replies, bounces and bulk mail are recognised and never acted on or answered.
//!
//! The transport (SMTP and IMAP) is behind [`MailTransport`] and is the only part that touches a network.

use super::{Action, Channel, Inbound, Outbound};
use mail_parser::MessageParser;

#[derive(Debug, Clone)]
pub struct EmailConfig {
    /// The dedicated Delphi mailbox: cards are sent from it and replies are read from it.
    pub mailbox: String,
    /// The operator's own address: where cards go.
    pub operator: String,
    /// The `authserv-id` the receiving provider puts in `Authentication-Results` (for Gmail, `mx.google.com`).
    /// Unset means "the topmost such header, whatever server wrote it", which is weaker.
    pub authserv_id: Option<String>,
}

/// A message to send. The transport does the wire encoding.
#[derive(Debug, Clone, PartialEq)]
pub struct OutMail {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub message_id: String,
    pub headers: Vec<(String, String)>,
}

pub trait MailTransport {
    fn send(&mut self, m: &OutMail) -> Result<(), String>;
    /// New messages since the last call, as raw RFC 5322 bytes. A message returned here is marked read, so it is
    /// not returned again.
    fn fetch_new(&mut self) -> Result<Vec<Vec<u8>>, String>;
}

pub struct EmailChannel<T: MailTransport> {
    pub cfg: EmailConfig,
    pub transport: T,
}

fn pct(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

fn domain_of(addr: &str) -> String {
    addr.rsplit_once('@').map(|(_, d)| d.to_lowercase()).unwrap_or_default()
}

/// The text and `mailto:` links for a card. Plain text only: no HTML, no images, no tracking links.
pub fn compose(cfg: &EmailConfig, m: &Outbound) -> OutMail {
    let mut body = m.text.clone();
    if !m.actions.is_empty() {
        body.push_str("\nTo answer, reply to this message with exactly one of these as the very first line (nothing before it):\n\n");
        for a in &m.actions {
            body.push_str(&format!("  {}\n", a.reply));
        }
        body.push_str("\nOr use a link; it opens a pre-filled reply that you only have to send:\n\n");
        for a in &m.actions {
            body.push_str(&format!("  {}: {}\n", a.label, mailto(&cfg.mailbox, &m.subject, a)));
        }
        body.push_str("\nEach code works once. If you did not expect this message, ignore it; nothing happens without a reply.\n");
    }
    let id = delphi_core::server::random_token().map(|t| t[..16].to_string()).unwrap_or_else(|_| "0".repeat(16));
    OutMail {
        to: cfg.operator.clone(),
        subject: m.subject.clone(),
        body,
        message_id: format!("<delphi.{}@{}>", id, domain_of(&cfg.mailbox)),
        // Marks our mail as machine-made so an out-of-office reply is not generated for it (RFC 3834).
        headers: vec![("Auto-Submitted".into(), "auto-generated".into()), ("X-Auto-Response-Suppress".into(), "All".into())],
    }
}

fn mailto(mailbox: &str, subject: &str, a: &Action) -> String {
    format!("mailto:{}?subject={}&body={}", mailbox, pct(&format!("Re: {}", subject)), pct(&a.reply))
}

impl<T: MailTransport> Channel for EmailChannel<T> {
    fn send(&mut self, m: &Outbound) -> Result<(), String> {
        let mail = compose(&self.cfg, m);
        self.transport.send(&mail)
    }

    fn poll(&mut self) -> Result<Vec<Inbound>, String> {
        let raws = self.transport.fetch_new()?;
        Ok(raws.iter().filter_map(|r| parse_inbound(r, &self.cfg)).collect())
    }
}

/// The first non-blank line of a reply's new text; empty if that line is quoted.
pub fn first_line(body: &str) -> String {
    let line = body.lines().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("");
    if line.starts_with('>') {
        return String::new();
    }
    line.chars().take(200).collect()
}

fn unfold(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).replace("\r\n", "").replace('\n', "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Does one `Authentication-Results` value show a pass that is aligned with the `From` domain?
pub fn auth_pass(value: &str, from_domain: &str) -> bool {
    let aligned = |d: &str| {
        let d = d.trim_matches(|c: char| c == '"' || c == '<' || c == '>').to_lowercase();
        !d.is_empty() && !from_domain.is_empty() && (d == from_domain || from_domain.ends_with(&format!(".{}", d)) || d.ends_with(&format!(".{}", from_domain)))
    };
    value.split(';').skip(1).any(|seg| {
        let seg = seg.trim();
        let lower = seg.to_lowercase();
        let prop = |key: &str| lower.split_whitespace().find_map(|w| w.strip_prefix(key).map(String::from));
        if lower.starts_with("dmarc=pass") {
            true
        } else if lower.starts_with("dkim=pass") {
            prop("header.d=").is_some_and(|d| aligned(&d))
        } else if lower.starts_with("spf=pass") {
            prop("smtp.mailfrom=").is_some_and(|m| aligned(&domain_of(&m)))
        } else {
            false
        }
    })
}

/// The `authserv-id` at the start of an `Authentication-Results` value.
fn authserv(value: &str) -> String {
    value.split(';').next().unwrap_or("").split_whitespace().next().unwrap_or("").to_lowercase()
}

pub fn parse_inbound(raw: &[u8], cfg: &EmailConfig) -> Option<Inbound> {
    let msg = MessageParser::new().parse(raw)?;
    let sender = msg.from().and_then(|f| f.first()).and_then(|a| a.address()).unwrap_or("").trim().to_lowercase();
    let id = msg.message_id().map(String::from).unwrap_or_else(|| blake3::hash(raw).to_hex()[..32].to_string());
    let headers = msg.headers();
    let text_of = |name: &str| headers.get(name).map(|h| unfold(h.raw_value()).to_lowercase());
    let auto_generated = text_of("auto-submitted").is_some_and(|v| v != "no")
        || text_of("precedence").is_some_and(|v| ["bulk", "junk", "list"].contains(&v.as_str()))
        || text_of("x-auto-response-suppress").is_some()
        || text_of("list-id").is_some()
        || text_of("return-path").is_some_and(|v| v == "<>")
        || sender.starts_with("mailer-daemon@")
        || sender.starts_with("postmaster@");
    let from_domain = domain_of(&sender);
    // The topmost header from the configured server (or the topmost of any): a header further down could have
    // been written by the sender, and a conforming provider removes or renames any that claim its own name.
    let ar = headers.all("authentication-results").map(|h| unfold(h.raw_value())).find(|v| cfg.authserv_id.as_ref().is_none_or(|want| authserv(v) == want.to_lowercase()));
    let authenticated = ar.is_some_and(|v| auth_pass(&v, &from_domain));
    let text = first_line(msg.body_text(0).as_deref().unwrap_or(""));
    Some(Inbound { id, sender, authenticated, auto_generated, text })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::{parse_verdict, Verdict};

    fn cfg() -> EmailConfig {
        EmailConfig { mailbox: "delphi.relay@example.net".into(), operator: "me@example.com".into(), authserv_id: Some("mx.example.net".into()) }
    }

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn mail(headers: &str, body: &str) -> Vec<u8> {
        format!("From: Me <me@example.com>\r\nTo: delphi.relay@example.net\r\nSubject: Re: [delphi:ap_1] write x\r\nMessage-ID: <m1@example.com>\r\nAuthentication-Results: mx.example.net; dkim=pass header.d=example.com header.s=s1; spf=pass smtp.mailfrom=me@example.com; dmarc=pass header.from=example.com\r\n{}\r\n{}", headers, body).into_bytes()
    }

    #[test]
    fn a_plain_reply_from_the_operator_is_read_as_a_verdict() {
        let m = parse_inbound(&mail("Content-Type: text/plain; charset=utf-8\r\n", &format!("APPROVE {}\r\n\r\nOn Mon, delphi wrote:\r\n> old text\r\n", TOKEN)), &cfg()).unwrap();
        assert_eq!((m.sender.as_str(), m.authenticated, m.auto_generated, m.id.as_str()), ("me@example.com", true, false, "m1@example.com"));
        assert_eq!(parse_verdict(&m.text), Some((Verdict::Approve, TOKEN.into())));
    }

    #[test]
    fn only_the_first_line_counts_so_quoted_history_and_signatures_never_answer_anything() {
        let top_post = parse_inbound(&mail("", &format!("sounds good, thanks\r\n\r\n> APPROVE {}\r\n", TOKEN)), &cfg()).unwrap();
        assert_eq!(parse_verdict(&top_post.text), None);
        let quoted_first = parse_inbound(&mail("", &format!("> APPROVE {}\r\n", TOKEN)), &cfg()).unwrap();
        assert_eq!(quoted_first.text, "");
        let reply_below = parse_inbound(&mail("", &format!("On Mon, Delphi wrote:\r\n> stuff\r\nAPPROVE {}\r\n", TOKEN)), &cfg()).unwrap();
        assert_eq!(parse_verdict(&reply_below.text), None, "a reply written below the quote is not accepted");
        let blank_first = parse_inbound(&mail("", &format!("\r\n\r\n  DENY {}  \r\nSent from my phone", TOKEN)), &cfg()).unwrap();
        assert_eq!(parse_verdict(&blank_first.text), Some((Verdict::Deny, TOKEN.into())));
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn a_forged_from_without_a_matching_pass_is_not_authenticated() {
        let forged = format!("From: Me <me@example.com>\r\nMessage-ID: <f@x>\r\nAuthentication-Results: mx.example.net; dkim=fail header.d=example.com; spf=softfail smtp.mailfrom=me@example.com; dmarc=fail\r\n\r\nAPPROVE {}\r\n", TOKEN);
        assert!(!parse_inbound(forged.as_bytes(), &cfg()).unwrap().authenticated);
        let none = format!("From: Me <me@example.com>\r\nMessage-ID: <f@x>\r\n\r\nAPPROVE {}\r\n", TOKEN);
        assert!(!parse_inbound(none.as_bytes(), &cfg()).unwrap().authenticated, "no result header at all");
        // A pass that is for somebody else's domain does not vouch for the From domain.
        let other = format!("From: Me <me@example.com>\r\nMessage-ID: <f@x>\r\nAuthentication-Results: mx.example.net; dkim=pass header.d=attacker.example; spf=pass smtp.mailfrom=bounce@attacker.example\r\n\r\nAPPROVE {}\r\n", TOKEN);
        assert!(!parse_inbound(other.as_bytes(), &cfg()).unwrap().authenticated);
    }

    #[test]
    fn a_header_the_sender_wrote_does_not_vouch_for_them() {
        // The sender planted a passing header naming a different server, below the real (failing) one.
        let raw = format!("Authentication-Results: mx.example.net; dmarc=fail\r\nAuthentication-Results: evil.example; dmarc=pass\r\nFrom: Me <me@example.com>\r\nMessage-ID: <f@x>\r\n\r\nAPPROVE {}\r\n", TOKEN);
        assert!(!parse_inbound(raw.as_bytes(), &cfg()).unwrap().authenticated, "only the configured server's header is read");
        // The sender planted a passing header with the right name *below* the provider's failing one: the topmost wins.
        let raw = format!("Authentication-Results: mx.example.net; dmarc=fail\r\nAuthentication-Results: mx.example.net; dmarc=pass\r\nFrom: Me <me@example.com>\r\nMessage-ID: <f@x>\r\n\r\nAPPROVE {}\r\n", TOKEN);
        assert!(!parse_inbound(raw.as_bytes(), &cfg()).unwrap().authenticated);
        // With no server configured, the topmost header counts, whoever wrote it.
        let mut loose = cfg();
        loose.authserv_id = None;
        let raw = format!("Authentication-Results: other.example; dmarc=pass\r\nFrom: Me <me@example.com>\r\nMessage-ID: <f@x>\r\n\r\nAPPROVE {}\r\n", TOKEN);
        assert!(parse_inbound(raw.as_bytes(), &loose).unwrap().authenticated);
    }

    #[test]
    fn dkim_or_spf_aligned_with_the_from_domain_is_enough_without_dmarc() {
        assert!(auth_pass("mx.example.net; dkim=pass header.d=example.com header.s=a", "example.com"));
        assert!(auth_pass("mx.example.net; dkim=pass header.d=mail.example.com", "example.com"));
        assert!(auth_pass("mx.example.net; spf=pass smtp.mailfrom=me@example.com", "example.com"));
        assert!(!auth_pass("mx.example.net; dkim=pass header.d=example.org", "example.com"));
        assert!(!auth_pass("mx.example.net; spf=pass smtp.mailfrom=me@example.com", "example.org"));
        assert!(!auth_pass("mx.example.net; dkim=none; spf=neutral", "example.com"));
        assert!(!auth_pass("mx.example.net", "example.com"));
    }

    #[test]
    fn out_of_office_bounces_and_bulk_mail_are_recognised_and_never_acted_on() {
        for h in ["Auto-Submitted: auto-replied\r\n", "Precedence: bulk\r\n", "X-Auto-Response-Suppress: All\r\n", "List-Id: <x.example.com>\r\n", "Return-Path: <>\r\n"] {
            assert!(parse_inbound(&mail(h, &format!("APPROVE {}\r\n", TOKEN)), &cfg()).unwrap().auto_generated, "{}", h);
        }
        assert!(!parse_inbound(&mail("Auto-Submitted: no\r\n", "hi\r\n"), &cfg()).unwrap().auto_generated);
        let bounce = format!("From: MAILER-DAEMON@example.net\r\nMessage-ID: <b@x>\r\n\r\nAPPROVE {}\r\n", TOKEN);
        assert!(parse_inbound(bounce.as_bytes(), &cfg()).unwrap().auto_generated);
    }

    #[test]
    fn html_only_replies_and_encoded_bodies_are_handled_and_attachments_are_ignored() {
        let html = mail("Content-Type: text/html; charset=utf-8\r\n", &format!("<html><body><p>APPROVE {}</p><p>thanks</p></body></html>\r\n", TOKEN));
        let m = parse_inbound(&html, &cfg()).unwrap();
        assert_eq!(parse_verdict(&m.text), Some((Verdict::Approve, TOKEN.into())), "the visible first line of an HTML reply");
        let qp = mail("Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n", &format!("DENY {}=\r\n\r\n", TOKEN[..31].to_string() + "=66"));
        assert_eq!(parse_verdict(&parse_inbound(&qp, &cfg()).unwrap().text), Some((Verdict::Deny, TOKEN.into())));
        let multi = format!("From: Me <me@example.com>\r\nMessage-ID: <mm@x>\r\nAuthentication-Results: mx.example.net; dmarc=pass\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nDENY {}\r\n--b\r\nContent-Type: text/plain; name=a.txt\r\nContent-Disposition: attachment\r\n\r\nAPPROVE ffffffffffffffffffffffffffffffff\r\n--b--\r\n", TOKEN);
        let m = parse_inbound(multi.as_bytes(), &cfg()).unwrap();
        assert_eq!(parse_verdict(&m.text), Some((Verdict::Deny, TOKEN.into())), "an attachment is never read as the reply");
    }

    #[test]
    fn a_message_without_an_id_still_gets_a_stable_one_and_garbage_does_not_panic() {
        let raw = b"From: me@example.com\r\n\r\nhello\r\n";
        let a = parse_inbound(raw, &cfg()).unwrap();
        let b = parse_inbound(raw, &cfg()).unwrap();
        assert_eq!(a.id, b.id);
        for junk in [&b""[..], &b"\x00\xff\xfe"[..], &b"From:\r\n\r\n"[..], &[b'a'; 100_000][..]] {
            let _ = parse_inbound(junk, &cfg());
        }
    }

    #[test]
    fn a_card_is_composed_as_plain_text_with_prefilled_reply_links_and_no_way_to_approve_red() {
        let card = serde_json::json!({"approval_id": "ap_1", "task_id": "t_1", "role": "architect", "tier": "amber", "tool": "write_file", "summary": "write specs/009.md", "payload_blake3": "abcdef0123456789", "timeout_secs": 600});
        let o = crate::relay::card_message(&card, TOKEN, false, crate::relay::Detail::Summary, false);
        let mail = compose(&cfg(), &o);
        assert_eq!(mail.to, "me@example.com");
        assert!(mail.message_id.starts_with("<delphi.") && mail.message_id.ends_with("@example.net>"));
        assert!(mail.headers.iter().any(|(k, v)| k == "Auto-Submitted" && v == "auto-generated"));
        assert!(mail.body.contains(&format!("APPROVE {}", TOKEN)) && mail.body.contains(&format!("DENY {}", TOKEN)));
        assert!(mail.body.contains("mailto:delphi.relay@example.net?subject=Re%3A%20%5Bdelphi%3Aap_1%5D") && mail.body.contains(&format!("body=APPROVE%20{}", TOKEN)));
        assert!(!mail.body.contains("<html") && !mail.body.contains("http://") && !mail.body.contains("https://"), "plain text, no web links or trackers");
        let red = compose(&cfg(), &crate::relay::card_message(&card, TOKEN, true, crate::relay::Detail::Summary, false));
        assert!(!red.body.contains("APPROVE ") || red.body.contains("cannot be approved by a reply"));
        assert!(!red.body.contains(&format!("APPROVE {}", TOKEN)), "a Red card offers no approve line or link");
    }

    struct FakeMail {
        sent: Vec<OutMail>,
        inbox: Vec<Vec<u8>>,
    }

    impl MailTransport for FakeMail {
        fn send(&mut self, m: &OutMail) -> Result<(), String> {
            self.sent.push(m.clone());
            Ok(())
        }
        fn fetch_new(&mut self) -> Result<Vec<Vec<u8>>, String> {
            Ok(std::mem::take(&mut self.inbox))
        }
    }

    #[test]
    fn the_channel_sends_through_the_transport_and_reduces_what_it_receives() {
        let mut ch = EmailChannel { cfg: cfg(), transport: FakeMail { sent: vec![], inbox: vec![mail("", &format!("APPROVE {}\r\n", TOKEN)), b"\x00junk".to_vec()] } };
        let got = ch.poll().unwrap();
        assert_eq!(got.len(), 1, "bytes that are not a message at all are dropped, never a crash");
        assert!(got[0].authenticated && parse_verdict(&got[0].text).is_some());
        assert!(ch.poll().unwrap().is_empty(), "read mail is not returned twice");
        ch.send(&Outbound { subject: "s".into(), text: "t".into(), actions: vec![], card: None }).unwrap();
        assert_eq!(ch.transport.sent.len(), 1);
    }
}
