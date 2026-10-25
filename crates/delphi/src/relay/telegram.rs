//! The Telegram channel (`specs/014` §5.1): cards as plain-text messages with Approve and Deny buttons, answers as
//! button taps or typed replies. The relay polls the Bot API (`getUpdates`), so no port is opened.
//!
//! What counts as who: Telegram vouches for `from.id` on every update it delivers to a bot, so that number is the
//! sender (the operator's own numeric user id; a username can change hands, an id cannot). Only a **private chat with
//! that user** counts: a message in a group, or one forwarded from elsewhere, is not an answer. As with email, the
//! sender is a filter and the single-use code is the secret.
//!
//! The bot token is a credential that sits in every request URL, so it is stripped from every error message.

use super::{Channel, Inbound, Outbound};
use serde_json::{json, Value};

pub const DEFAULT_API: &str = "https://api.telegram.org";
/// Telegram refuses a message over 4096 characters.
const MAX_TEXT: usize = 3800;

#[derive(Debug, Clone)]
pub struct TelegramConfig {
    pub api_base: String,
    /// The bot token. Never printed.
    pub bot_token: String,
    /// The operator's numeric user id: cards go to their private chat, and their replies are the ones that count.
    pub chat_id: i64,
}

impl TelegramConfig {
    pub fn validate(&self) -> Result<(), String> {
        let t = &self.bot_token;
        let shape = t.split_once(':').is_some_and(|(id, rest)| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'));
        if !shape {
            return Err("the bot token does not look like <digits>:<letters and digits> (get one from @BotFather)".into());
        }
        let host = self.api_base.split("://").nth(1).unwrap_or("").split(['/', ':']).next().unwrap_or("");
        let loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
        if !(self.api_base.starts_with("https://") || (self.api_base.starts_with("http://") && loopback)) {
            return Err("api_base must be https (plain http only for a server on this machine)".into());
        }
        if self.chat_id <= 0 {
            return Err("chat_id must be your numeric Telegram user id (a private chat), not a group".into());
        }
        Ok(())
    }
}

/// JSON in, JSON out. The one thing the channel needs from the network, so tests can stand in for it.
pub trait Http {
    fn post(&self, url: &str, body: &Value) -> Result<Value, String>;
}

pub struct ReqwestHttp {
    client: reqwest::blocking::Client,
    token: String,
}

impl ReqwestHttp {
    pub fn new(token: &str) -> Result<ReqwestHttp, String> {
        let client = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(30)).build().map_err(|e| e.without_url().to_string())?;
        Ok(ReqwestHttp { client, token: token.to_string() })
    }

    fn scrub(&self, s: &str) -> String {
        s.replace(&self.token, "<bot token>")
    }
}

impl Http for ReqwestHttp {
    fn post(&self, url: &str, body: &Value) -> Result<Value, String> {
        let resp = self.client.post(url).json(body).send().map_err(|e| self.scrub(&e.without_url().to_string()))?;
        let status = resp.status();
        let v: Value = resp.json().map_err(|e| self.scrub(&format!("unreadable reply ({}): {}", status, e.without_url())))?;
        if v["ok"] == true {
            Ok(v)
        } else {
            Err(self.scrub(&format!("Telegram refused ({}): {}", status, v["description"].as_str().unwrap_or("no description"))))
        }
    }
}

pub struct TelegramChannel<H: Http> {
    pub cfg: TelegramConfig,
    pub http: H,
    /// The next update to ask for; everything before it is acknowledged.
    offset: i64,
}

impl<H: Http> TelegramChannel<H> {
    pub fn new(cfg: TelegramConfig, http: H) -> TelegramChannel<H> {
        TelegramChannel { cfg, http, offset: 0 }
    }

    fn url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.cfg.api_base.trim_end_matches('/'), self.cfg.bot_token, method)
    }

    /// Who the bot is: used by `delphi relay check`.
    pub fn whoami(&self) -> Result<String, String> {
        let v = self.http.post(&self.url("getMe"), &json!({}))?;
        Ok(v["result"]["username"].as_str().unwrap_or("?").to_string())
    }
}

/// The text of a message: the subject, a blank line, the body, cut to what Telegram accepts.
fn message_text(m: &Outbound) -> String {
    let t = format!("{}\n\n{}", m.subject, m.text);
    if t.chars().count() <= MAX_TEXT { t } else { format!("{}…", t.chars().take(MAX_TEXT).collect::<String>()) }
}

impl<H: Http> Channel for TelegramChannel<H> {
    fn send(&mut self, m: &Outbound) -> Result<(), String> {
        let mut body = json!({"chat_id": self.cfg.chat_id, "text": message_text(m), "disable_web_page_preview": true});
        // One row of buttons. A button sends its action's reply back as `callback_data`, at most 64 bytes.
        let row: Vec<Value> = m.actions.iter().filter(|a| a.reply.trim().len() <= 64).map(|a| json!({"text": a.label, "callback_data": a.reply.trim()})).collect();
        if !row.is_empty() {
            body["reply_markup"] = json!({"inline_keyboard": [row]});
        }
        self.http.post(&self.url("sendMessage"), &body).map(|_| ())
    }

    fn poll(&mut self) -> Result<Vec<Inbound>, String> {
        let v = self.http.post(&self.url("getUpdates"), &json!({"offset": self.offset, "timeout": 0, "allowed_updates": ["message", "callback_query"]}))?;
        let mut out = Vec::new();
        for u in v["result"].as_array().into_iter().flatten() {
            let id = u["update_id"].as_i64().unwrap_or(0);
            if id >= self.offset {
                self.offset = id + 1;
            }
            let (from, chat, text, cb) = if let Some(cq) = u.get("callback_query") {
                (&cq["from"], &cq["message"]["chat"], cq["data"].as_str().unwrap_or("").to_string(), Some(cq["id"].as_str().unwrap_or("").to_string()))
            } else if let Some(msg) = u.get("message") {
                (&msg["from"], &msg["chat"], msg["text"].as_str().unwrap_or("").lines().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("").to_string(), None)
            } else {
                continue;
            };
            let from_id = from["id"].as_i64().unwrap_or(0);
            // Only a private chat with the sender counts: a group, a channel and a forwarded message do not.
            let private = chat["type"] == "private" && chat["id"].as_i64() == Some(from_id) && from_id != 0;
            if let Some(cb_id) = cb {
                // Stop the button's spinner. Best effort: the answer itself does not depend on it.
                let _ = self.http.post(&self.url("answerCallbackQuery"), &json!({"callback_query_id": cb_id, "text": "Got it"}));
            }
            out.push(Inbound { id: format!("tg:{}", id), sender: from_id.to_string(), authenticated: private, auto_generated: from["is_bot"] == true, text });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::{card_message, parse_verdict, Detail, Verdict};
    use std::cell::RefCell;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct FakeApi {
        calls: RefCell<Vec<(String, Value)>>,
        updates: RefCell<VecDeque<Value>>,
        fail: RefCell<Option<String>>,
    }

    impl Http for &FakeApi {
        fn post(&self, url: &str, body: &Value) -> Result<Value, String> {
            self.calls.borrow_mut().push((url.to_string(), body.clone()));
            if let Some(f) = self.fail.borrow().clone() {
                return Err(f);
            }
            if url.ends_with("/getUpdates") {
                let off = body["offset"].as_i64().unwrap_or(0);
                let list: Vec<Value> = self.updates.borrow().iter().filter(|u| u["update_id"].as_i64().unwrap_or(0) >= off).cloned().collect();
                return Ok(json!({"ok": true, "result": list}));
            }
            Ok(json!({"ok": true, "result": {"username": "delphi_relay_bot"}}))
        }
    }

    fn cfg() -> TelegramConfig {
        TelegramConfig { api_base: DEFAULT_API.into(), bot_token: "123456:ABC-def_GHI".into(), chat_id: 42 }
    }

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn card_msg() -> Outbound {
        let card = json!({"approval_id": "ap_1", "task_id": "t_1", "role": "architect", "tier": "amber", "tool": "write_file", "summary": "write specs/009.md", "payload_blake3": "abcdef0123456789", "timeout_secs": 600});
        card_message(&card, TOKEN, false, Detail::Summary, false)
    }

    fn private_msg(update_id: i64, from: i64, text: &str) -> Value {
        json!({"update_id": update_id, "message": {"message_id": 1, "from": {"id": from, "is_bot": false}, "chat": {"id": from, "type": "private"}, "text": text}})
    }

    #[test]
    fn a_card_is_sent_to_the_operators_private_chat_with_buttons_that_carry_the_reply() {
        let api = FakeApi::default();
        let mut ch = TelegramChannel::new(cfg(), &api);
        ch.send(&card_msg()).unwrap();
        let calls = api.calls.borrow();
        let (url, body) = &calls[0];
        assert_eq!(url, "https://api.telegram.org/bot123456:ABC-def_GHI/sendMessage");
        assert_eq!(body["chat_id"], 42);
        assert!(body["text"].as_str().unwrap().contains("write specs/009.md") && body.get("parse_mode").is_none(), "plain text: nothing in the summary is read as markup");
        let buttons = body["reply_markup"]["inline_keyboard"][0].as_array().unwrap();
        assert_eq!((buttons[0]["text"].as_str(), buttons[0]["callback_data"].as_str()), (Some("Approve"), Some(format!("APPROVE {}", TOKEN).as_str())));
        assert_eq!(buttons[1]["callback_data"], format!("DENY {}", TOKEN));
        assert!(buttons.iter().all(|b| b["callback_data"].as_str().unwrap().len() <= 64));
    }

    #[test]
    fn a_long_message_is_cut_to_what_telegram_accepts() {
        let mut m = card_msg();
        m.text = "x".repeat(10_000);
        assert!(message_text(&m).chars().count() <= MAX_TEXT + 1);
    }

    #[test]
    fn a_button_tap_and_a_typed_reply_from_a_private_chat_are_answers_and_the_tap_is_acknowledged() {
        let api = FakeApi::default();
        api.updates.borrow_mut().push_back(json!({"update_id": 10, "callback_query": {"id": "cb1", "from": {"id": 42, "is_bot": false}, "data": format!("APPROVE {}", TOKEN), "message": {"chat": {"id": 42, "type": "private"}}}}));
        api.updates.borrow_mut().push_back(private_msg(11, 42, &format!("\n  DENY {}  \nthanks", TOKEN)));
        let mut ch = TelegramChannel::new(cfg(), &api);
        let got = ch.poll().unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].id.as_str(), got[0].sender.as_str(), got[0].authenticated, got[0].auto_generated), ("tg:10", "42", true, false));
        assert_eq!(parse_verdict(&got[0].text), Some((Verdict::Approve, TOKEN.into())));
        assert_eq!(parse_verdict(&got[1].text), Some((Verdict::Deny, TOKEN.into())), "the first non-blank line of a typed message");
        assert!(api.calls.borrow().iter().any(|(u, b)| u.ends_with("/answerCallbackQuery") && b["callback_query_id"] == "cb1"));
    }

    #[test]
    fn updates_are_acknowledged_so_none_is_delivered_twice() {
        let api = FakeApi::default();
        api.updates.borrow_mut().push_back(private_msg(5, 42, "hi"));
        let mut ch = TelegramChannel::new(cfg(), &api);
        assert_eq!(ch.poll().unwrap().len(), 1);
        assert!(ch.poll().unwrap().is_empty(), "the offset moved past update 5");
        let calls = api.calls.borrow();
        assert_eq!((calls[0].1["offset"].as_i64(), calls[1].1["offset"].as_i64()), (Some(0), Some(6)));
    }

    #[test]
    fn only_a_private_chat_with_the_sender_is_authenticated() {
        let api = FakeApi::default();
        // A group message from the operator's id, a message whose chat is somebody else's, and a bot.
        api.updates.borrow_mut().push_back(json!({"update_id": 1, "message": {"from": {"id": 42, "is_bot": false}, "chat": {"id": -1001, "type": "supergroup"}, "text": format!("APPROVE {}", TOKEN)}}));
        api.updates.borrow_mut().push_back(json!({"update_id": 2, "message": {"from": {"id": 42, "is_bot": false}, "chat": {"id": 99, "type": "private"}, "text": "x"}}));
        api.updates.borrow_mut().push_back(json!({"update_id": 3, "message": {"from": {"id": 7, "is_bot": true}, "chat": {"id": 7, "type": "private"}, "text": "x"}}));
        api.updates.borrow_mut().push_back(json!({"update_id": 4, "edited_message": {"text": "ignored kind"}}));
        let mut ch = TelegramChannel::new(cfg(), &api);
        let got = ch.poll().unwrap();
        assert_eq!(got.len(), 3, "an update of another kind is skipped");
        assert!(!got[0].authenticated && !got[1].authenticated);
        assert!(got[2].auto_generated);
    }

    #[test]
    fn the_token_is_checked_for_shape_and_http_is_allowed_only_for_this_machine() {
        assert!(cfg().validate().is_ok());
        for (field, bad) in [("token", "no-colon"), ("token", "123:"), ("token", ":abc"), ("token", "12a:abc"), ("token", "123:ab c"), ("api", "http://api.telegram.org"), ("api", "ftp://x"), ("chat", "-100")] {
            let mut c = cfg();
            match field {
                "token" => c.bot_token = bad.into(),
                "api" => c.api_base = bad.into(),
                _ => c.chat_id = bad.parse().unwrap(),
            }
            assert!(c.validate().is_err(), "{} {}", field, bad);
        }
        let mut local = cfg();
        local.api_base = "http://127.0.0.1:8081".into();
        assert!(local.validate().is_ok());
    }

    #[test]
    fn a_failure_is_reported_and_the_whoami_names_the_bot() {
        let api = FakeApi::default();
        let mut ch = TelegramChannel::new(cfg(), &api);
        assert_eq!(ch.whoami().unwrap(), "delphi_relay_bot");
        *api.fail.borrow_mut() = Some("Telegram refused (401): Unauthorized".into());
        assert!(ch.send(&card_msg()).unwrap_err().contains("Unauthorized"));
        assert!(ch.poll().is_err());
    }

    // ── the real HTTP client, against a tiny server on this machine ─────────────

    fn serve_once_per_request(replies: Vec<(u16, String)>) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for (status, reply) in replies {
                let Ok((s, _)) = l.accept() else { return };
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; len];
                r.read_exact(&mut body).unwrap();
                seen2.lock().unwrap().push(format!("{}|{}", line.trim(), String::from_utf8_lossy(&body)));
                let mut w = s;
                let _ = write!(w, "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", status, reply.len(), reply);
            }
        });
        (base, seen)
    }

    #[test]
    fn the_real_client_posts_json_to_the_bot_path_and_turns_a_refusal_into_an_error_without_the_token() {
        let (base, seen) = serve_once_per_request(vec![(200, r#"{"ok":true,"result":{"username":"b"}}"#.into()), (401, r#"{"ok":false,"description":"Unauthorized"}"#.into())]);
        let mut c = cfg();
        c.api_base = base;
        let http = ReqwestHttp::new(&c.bot_token).unwrap();
        let ch = TelegramChannel::new(c.clone(), http);
        assert_eq!(ch.whoami().unwrap(), "b");
        let e = ch.whoami().unwrap_err();
        assert!(e.contains("Unauthorized") && e.contains("401") && !e.contains("ABC-def_GHI"), "{}", e);
        let seen = seen.lock().unwrap();
        assert!(seen[0].starts_with("POST /bot123456:ABC-def_GHI/getMe HTTP/1.1|"), "{}", seen[0]);
    }

    #[test]
    fn a_connection_error_does_not_leak_the_token() {
        let mut c = cfg();
        c.api_base = "http://127.0.0.1:1".into(); // nothing listens here
        let ch = TelegramChannel::new(c.clone(), ReqwestHttp::new(&c.bot_token).unwrap());
        let e = ch.whoami().unwrap_err();
        assert!(!e.contains("ABC-def_GHI") && !e.contains("123456:"), "{}", e);
    }
}
