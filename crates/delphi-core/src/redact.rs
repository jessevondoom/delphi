//! Deterministic redaction before anything reaches disk or a client (`specs/005` §2.3).
//!
//! Masks known secret formats and every runtime-resolved credential literal registered with
//! [`Redactor::register_literal`] (`specs/009` §4.2), in raw, base64 and URL-encoded forms. Literals
//! shorter than [`MIN_BARE_LITERAL`] characters are matched only in key/value context (`key=value`,
//! `key: value`) to avoid false positives.
//!
//! Signed reasoning (`[H-003]`): a `thinking` block is never altered while keeping its signature. If
//! redaction changes it, the persisted copy drops the signature, gets `replayable: false` and
//! `signature_dropped_reason: "redacted"`. `redacted_thinking` (opaque provider data) and every
//! `provider_meta` are never scanned or altered.

use crate::event::EventEnvelope;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use base64::Engine;
use regex::Regex;
use serde_json::Value;

pub const MASK: &str = "[REDACTED]";
/// Literals shorter than this are matched only in key/value context.
pub const MIN_BARE_LITERAL: usize = 12;

pub struct Redactor {
    patterns: Vec<Regex>,
    kv_names: Vec<Regex>,
    /// Literal variants matched anywhere.
    bare: Vec<String>,
    /// Short literal variants matched only after `=` or `:`.
    short: Vec<Regex>,
}

impl Default for Redactor {
    fn default() -> Self {
        Redactor::new()
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

impl Redactor {
    pub fn new() -> Redactor {
        let patterns = [
            r"sk-ant-[A-Za-z0-9_\-]{20,}",
            r"sk-[A-Za-z0-9]{32,}",
            r"AIza[0-9A-Za-z_\-]{35}",
            r"gh[pousr]_[A-Za-z0-9]{20,}",
            r"github_pat_[A-Za-z0-9_]{20,}",
            r"AKIA[0-9A-Z]{16}",
            r"xox[baprs]-[A-Za-z0-9\-]{10,}",
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
        ]
        .iter()
        .map(|p| Regex::new(p).unwrap())
        .collect();
        let mut r = Redactor { patterns, kv_names: Vec::new(), bare: Vec::new(), short: Vec::new() };
        // `.env`-style assignments for any name that looks like a credential.
        r.kv_names.push(Regex::new(r"\b([A-Z][A-Z0-9_]*(?:API_KEY|SECRET_KEY|SECRET|TOKEN|PASSWORD|PASSWD))(\s*=\s*)([^\s'\x22]+)").unwrap());
        r
    }

    /// Mask `NAME=value` for an explicitly configured variable name.
    pub fn register_name(&mut self, name: &str) {
        self.kv_names.push(Regex::new(&format!(r"\b({})(\s*=\s*)([^\s'\x22]+)", regex::escape(name))).unwrap());
    }

    /// Register a runtime-resolved credential literal. Empty strings are ignored.
    pub fn register_literal(&mut self, literal: &str) {
        if literal.is_empty() {
            return;
        }
        let mut variants = vec![literal.to_string()];
        for v in [STANDARD.encode(literal), STANDARD_NO_PAD.encode(literal), URL_SAFE_NO_PAD.encode(literal), percent_encode(literal)] {
            if !variants.contains(&v) {
                variants.push(v);
            }
        }
        for v in variants {
            if literal.chars().count() >= MIN_BARE_LITERAL {
                if !self.bare.contains(&v) {
                    self.bare.push(v);
                }
            } else {
                self.short.push(Regex::new(&format!(r"([=:]\s*[\x22']?){}", regex::escape(&v))).unwrap());
            }
        }
        // Longest first so a literal that contains another is masked whole.
        self.bare.sort_by_key(|b| std::cmp::Reverse(b.len()));
    }

    /// Redact one string. Returns the masked text and whether anything changed.
    pub fn redact_str(&self, input: &str) -> (String, bool) {
        let mut s = input.to_string();
        for p in &self.patterns {
            s = p.replace_all(&s, MASK).into_owned();
        }
        for k in &self.kv_names {
            s = k.replace_all(&s, format!("${{1}}${{2}}{}", MASK).as_str()).into_owned();
        }
        for b in &self.bare {
            if s.contains(b.as_str()) {
                s = s.replace(b.as_str(), MASK);
            }
        }
        for sh in &self.short {
            s = sh.replace_all(&s, format!("${{1}}{}", MASK).as_str()).into_owned();
        }
        let changed = s != input;
        (s, changed)
    }

    /// Redact every string in `v` in place. Returns whether anything changed. Object keys,
    /// `provider_meta`, and `redacted_thinking` blocks are left alone; `thinking` blocks follow the
    /// signed-reasoning rule.
    pub fn redact_value(&self, v: &mut Value) -> bool {
        match v {
            Value::String(s) => {
                let (out, changed) = self.redact_str(s);
                if changed {
                    *s = out;
                }
                changed
            }
            Value::Array(items) => items.iter_mut().fold(false, |acc, i| self.redact_value(i) | acc),
            Value::Object(map) => match map.get("type").and_then(|t| t.as_str()) {
                Some("redacted_thinking") => false,
                Some("thinking") => self.redact_thinking(map),
                _ => {
                    let mut changed = false;
                    for (k, child) in map.iter_mut() {
                        if k != "provider_meta" {
                            changed |= self.redact_value(child);
                        }
                    }
                    changed
                }
            },
            _ => false,
        }
    }

    fn redact_thinking(&self, map: &mut serde_json::Map<String, Value>) -> bool {
        let mut changed = false;
        for (k, child) in map.iter_mut() {
            if k != "provider_meta" {
                changed |= self.redact_value(child);
            }
        }
        if changed {
            if let Some(meta) = map.get_mut("provider_meta").and_then(|m| m.as_object_mut()) {
                meta.remove("signature");
                meta.remove("thought_signature");
                meta.remove("encrypted_content");
            }
            map.insert("replayable".into(), Value::Bool(false));
            map.insert("signature_dropped_reason".into(), Value::String("redacted".into()));
        }
        changed
    }

    /// Redact an event's payload (the only free-form part). Returns whether anything changed.
    pub fn redact_event(&self, e: &mut EventEnvelope) -> bool {
        self.redact_value(&mut e.payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn known_formats_are_masked() {
        let r = Redactor::new();
        let secret_key = format!("sk-ant-{}", "a1B2c3D4e5F6g7H8i9J0k1L2");
        let gh = format!("ghp_{}", "x".repeat(30));
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIB\nabc\n-----END RSA PRIVATE KEY-----";
        let text = format!("k={} gh={} {} ANTHROPIC_API_KEY=abc123 DB_PASSWORD=hunter2 PATH=/usr/bin", secret_key, gh, pem);
        let (out, changed) = r.redact_str(&text);
        assert!(changed);
        for leaked in [secret_key.as_str(), gh.as_str(), "MIIB", "abc123", "hunter2"] {
            assert!(!out.contains(leaked), "{} leaked in {}", leaked, out);
        }
        assert!(out.contains("PATH=/usr/bin"), "unrelated assignments stay: {}", out);
        assert!(out.contains("ANTHROPIC_API_KEY=[REDACTED]"));
    }

    #[test]
    fn runtime_literals_in_raw_base64_and_url_forms() {
        let mut r = Redactor::new();
        let lit = "correct-horse/battery+staple==";
        r.register_literal(lit);
        let b64 = STANDARD.encode(lit);
        let url = percent_encode(lit);
        for form in [lit.to_string(), b64, url] {
            let (out, changed) = r.redact_str(&format!("header: {} end", form));
            assert!(changed && !out.contains(&form), "{} not masked: {}", form, out);
        }
    }

    #[test]
    fn short_literals_match_only_in_key_value_context() {
        let mut r = Redactor::new();
        r.register_literal("hunter2");
        let (out, changed) = r.redact_str("the hunter2 forum thread");
        assert!(!changed, "{}", out);
        for text in ["password=hunter2", "password: hunter2", "password = \"hunter2\""] {
            let (out, changed) = r.redact_str(text);
            assert!(changed && !out.contains("hunter2"), "{}", out);
        }
    }

    #[test]
    fn configured_names_are_masked() {
        let mut r = Redactor::new();
        r.register_name("MY_SERVICE_CRED");
        assert_eq!(r.redact_str("MY_SERVICE_CRED=abc").0, "MY_SERVICE_CRED=[REDACTED]");
    }

    #[test]
    fn signed_thinking_loses_signature_when_redacted() {
        let mut r = Redactor::new();
        r.register_literal("super-secret-credential-123");
        let mut block = json!({"type": "thinking", "thought": "use super-secret-credential-123 here", "replayable": true,
            "provider_meta": {"provider": "anthropic", "signature": "SIGDATA"}});
        assert!(r.redact_value(&mut block));
        assert!(!block["thought"].as_str().unwrap().contains("super-secret"));
        assert_eq!(block["replayable"], json!(false));
        assert_eq!(block["signature_dropped_reason"], json!("redacted"));
        assert!(block["provider_meta"].get("signature").is_none());
        assert_eq!(block["provider_meta"]["provider"], json!("anthropic"));
    }

    #[test]
    fn untouched_thinking_keeps_signature_and_flags() {
        let r = Redactor::new();
        let original = json!({"type": "thinking", "thought": "plain", "replayable": true, "provider_meta": {"signature": "SIGDATA"}});
        let mut block = original.clone();
        assert!(!r.redact_value(&mut block));
        assert_eq!(block, original);
    }

    #[test]
    fn redacted_thinking_and_provider_meta_are_opaque() {
        let mut r = Redactor::new();
        r.register_literal("opaque-looks-secret-1234");
        let original = json!({"block": {"type": "redacted_thinking", "replayable": true, "provider_meta": {"data": "opaque-looks-secret-1234"}},
            "other": {"provider_meta": {"signature": "opaque-looks-secret-1234"}}});
        let mut v = original.clone();
        assert!(!r.redact_value(&mut v));
        assert_eq!(v, original);
    }

    #[test]
    fn blocks_inside_arrays_and_nested_payloads_are_reached() {
        let mut r = Redactor::new();
        r.register_literal("super-secret-credential-123");
        let mut e = EventEnvelope::new("m", "agent.block.complete", json!({"blocks": [
            {"type": "text", "text": "x super-secret-credential-123"},
            {"type": "thinking", "thought": "super-secret-credential-123", "provider_meta": {"signature": "S"}}]}));
        assert!(r.redact_event(&mut e));
        let s = serde_json::to_string(&e.payload).unwrap();
        assert!(!s.contains("super-secret"), "{}", s);
        assert!(!s.contains("\"signature\""), "{}", s);
    }
}
