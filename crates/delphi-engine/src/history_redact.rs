//! What may be kept of an operator's prompt (`specs/102` §3.1 redaction, §3.4 the paste rule), slice S3.
//!
//! * **The daemon decides what was pasted** (§3.4). A host's or the console's mark can only **add** pasted status. Unless the daemon
//!   itself saw a span typed ([`Typing::Typed`]), a span is treated as pasted when it is over `paste_chars` characters, or holds a run
//!   of 40 or more characters at 4.5 bits per character or more, or is multi-line code-like text (three or more lines, at least half
//!   of the span's lines, with brackets, indentation or a `key: value` shape). A high-entropy run is hashed even for typed text.
//! * **A pasted block is stored as a keyed hash and a length, never its text.** The prompt keeps a placeholder where the block was.
//!   The hash is an HMAC under the local key (`history_pointer::span_hmac`), so a short secret cannot be recovered by guessing.
//! * **Redaction** runs the core redactor (`delphi_core::redact`) and the ten shapes of the fixed corpus (§3.1): a private-key block,
//!   a cloud access key pair, a bearer token, a JWT, a password inside a URL, a token split across two lines, a base64 fragment, a long
//!   hex string, a `name: value` secret in a non-standard format, and a secret inside a code fence. It is best effort and says so (§13).
//! * Control characters are stripped before anything is kept, so a stored escape sequence cannot reach a terminal.
//! * **The thresholds are hypotheses** to calibrate from use (OD-3). The rule errs toward not storing.
//!
//! Nothing here reaches the network, calls a model, or reads anything but the text it is given.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

use delphi_core::redact::{Redactor, MASK};

/// A prompt is kept to this many characters; longer is cut, with the keyed hash of the full text (§3.1).
pub const PROMPT_MAX_CHARS: usize = 2000;
/// A reply field (`asked`, `did`, `open`) is kept to this many characters (§3.1).
pub const REPLY_FIELD_MAX_CHARS: usize = 200;
/// `[history] paste_chars` default (§3.4, a hypothesis).
pub const DEFAULT_PASTE_CHARS: usize = 400;
/// A run this long at [`ENTROPY_BITS`] or more reads as a pasted secret (§3.4, a hypothesis).
pub const ENTROPY_RUN: usize = 40;
pub const ENTROPY_BITS: f64 = 4.5;

/// What the daemon knows of how the text arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typing {
    /// The daemon itself saw the input at typing rate, not in one burst. Only the daemon's own console path can say this.
    Typed,
    /// Nothing says it was typed. This is the answer for a prompt that arrived whole, for a host's report, and today for every
    /// prompt: no input-timing evidence reaches the daemon yet, so the length, entropy and code-like rules all apply.
    NotSeen,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PastedBlock {
    /// Lowercase hex HMAC-SHA256 of the block's text under the local key.
    pub hash: String,
    /// Its length in characters.
    pub chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scrubbed {
    pub text: String,
    pub pasted: Vec<PastedBlock>,
    /// Redaction changed something.
    pub redacted: bool,
    /// The text was cut to the cap.
    pub cut: bool,
    /// The keyed hash of the full (redacted, uncut) text, present only when `cut`.
    pub full_hash: Option<String>,
}

pub struct Input<'a> {
    pub text: &'a str,
    /// Blocks a console or a host marked as pasted. A mark only adds.
    pub marked: &'a [String],
    pub typing: Typing,
    pub paste_chars: usize,
}

// ---------------------------------------------------------------------------------------------------------------------------
// Characters

/// Drop control characters (keeping newline and tab) so that a stored escape sequence cannot act on a terminal.
pub fn strip_controls(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect()
}

fn commas(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Shannon entropy of a string, in bits per character.
pub fn entropy_bits(s: &str) -> f64 {
    let mut counts: HashMap<char, usize> = HashMap::new();
    let mut n = 0usize;
    for c in s.chars() {
        *counts.entry(c).or_default() += 1;
        n += 1;
    }
    if n == 0 {
        return 0.0;
    }
    counts.values().map(|&k| {
        let p = k as f64 / n as f64;
        -p * p.log2()
    }).sum()
}

// ---------------------------------------------------------------------------------------------------------------------------
// Redaction: the core redactor and the ten shapes

struct Shapes {
    core: Redactor,
    split_token: Regex,
    jwt: Regex,
    bearer: Regex,
    url_password: Regex,
    base64: Regex,
    hex: Regex,
    named_secret: Regex,
}

fn shapes() -> &'static Shapes {
    static S: OnceLock<Shapes> = OnceLock::new();
    S.get_or_init(|| Shapes {
        core: Redactor::new(),
        split_token: Regex::new(r"\b(?:sk-ant-|sk-|gh[pousr]_|github_pat_|xox[baprs]-|AKIA|ASIA|AIza)[A-Za-z0-9_\-]{4,}(?:[ \t]*\r?\n[ \t]*[A-Za-z0-9_\-]{4,})+").unwrap(),
        jwt: Regex::new(r"\beyJ[A-Za-z0-9_\-]{5,}\.[A-Za-z0-9_\-]{5,}\.[A-Za-z0-9_\-]{5,}").unwrap(),
        bearer: Regex::new(r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/=\-]{8,}").unwrap(),
        url_password: Regex::new(r"(\b[A-Za-z][A-Za-z0-9+.\-]*://[^\s:/@]+:)[^\s@/]+(@)").unwrap(),
        base64: Regex::new(r"[A-Za-z0-9+/]{40,}={0,2}").unwrap(),
        hex: Regex::new(r"\b[0-9A-Fa-f]{32,}\b").unwrap(),
        named_secret: Regex::new(r#"(?i)\b([\w.\-]*(?:secret|token|password|passwd|passphrase|pwd|api[_ \-]?key|apikey|credential|auth[_\-]?key)[\w.\-]*)(\s*(?::|=>|->|=)\s*|\s+is\s+)(["']?)([^\s"']{6,})"#).unwrap(),
    })
}

/// Redact `text`: the core redactor and the ten shapes of §3.1. Returns the masked text and whether anything changed.
pub fn redact(text: &str) -> (String, bool) {
    let s = shapes();
    let mut out = text.to_string();
    // A token split over two lines first, so the core patterns do not see half of it and leave the other half.
    out = s.split_token.replace_all(&out, MASK).into_owned();
    out = s.jwt.replace_all(&out, MASK).into_owned();
    out = s.bearer.replace_all(&out, format!("${{1}} {MASK}").as_str()).into_owned();
    out = s.url_password.replace_all(&out, format!("${{1}}{MASK}${{2}}").as_str()).into_owned();
    out = s.core.redact_str(&out).0;
    out = s.named_secret.replace_all(&out, format!("${{1}}${{2}}${{3}}{MASK}").as_str()).into_owned();
    // A base64 fragment: only a run that mixes upper case, lower case and digits (a long path of plain words is left alone).
    out = s
        .base64
        .replace_all(&out, |c: &regex::Captures| {
            let m = &c[0];
            if m.chars().any(|x| x.is_ascii_digit()) && m.chars().any(|x| x.is_ascii_uppercase()) && m.chars().any(|x| x.is_ascii_lowercase()) {
                MASK.to_string()
            } else {
                m.to_string()
            }
        })
        .into_owned();
    out = s.hex.replace_all(&out, MASK).into_owned();
    let changed = out != text;
    (out, changed)
}

// ---------------------------------------------------------------------------------------------------------------------------
// The paste rule

struct Span {
    text: String,
    /// A blank-line separator (kept as written), not a span to judge.
    sep: bool,
}

/// Split into fenced code blocks, paragraphs (runs of lines between blank lines) and the blank-line separators between them.
fn spans(text: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut cur = String::new();
    let mut in_fence = false;
    let flush = |cur: &mut String, out: &mut Vec<Span>| {
        if !cur.is_empty() {
            out.push(Span { text: std::mem::take(cur), sep: false });
        }
    };
    for line in text.split_inclusive('\n') {
        let fence = line.trim_start().starts_with("```");
        if in_fence {
            cur.push_str(line);
            if fence {
                in_fence = false;
                flush(&mut cur, &mut out);
            }
        } else if fence {
            flush(&mut cur, &mut out);
            cur.push_str(line);
            in_fence = true;
        } else if line.trim().is_empty() {
            flush(&mut cur, &mut out);
            out.push(Span { text: line.to_string(), sep: true });
        } else {
            cur.push_str(line);
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn kv_shape() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[\w.\-]+\s*[:=]\s*\S").unwrap())
}

fn code_like_line(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && (line.starts_with("  ") || line.starts_with('\t') || t.chars().any(|c| "{}[]();<>".contains(c)) || kv_shape().is_match(t))
}

/// Three or more lines, at least half of the span's non-empty lines, with brackets, indentation or a `key: value` shape.
pub fn code_like(span: &str) -> bool {
    let lines: Vec<&str> = span.lines().filter(|l| !l.trim().is_empty()).collect();
    let hits = lines.iter().filter(|l| code_like_line(l)).count();
    hits >= 3 && hits * 2 >= lines.len()
}

/// A whitespace-delimited run of [`ENTROPY_RUN`] characters or more at [`ENTROPY_BITS`] bits per character or more.
pub fn high_entropy_run(span: &str) -> bool {
    span.split_whitespace().any(|w| w.chars().count() >= ENTROPY_RUN && entropy_bits(w) >= ENTROPY_BITS)
}

/// Should this span be treated as pasted?
pub fn treated_as_pasted(span: &str, typing: Typing, paste_chars: usize) -> bool {
    if high_entropy_run(span) {
        return true; // never kept, typed or not
    }
    if typing == Typing::Typed {
        return false;
    }
    span.chars().count() > paste_chars || code_like(span)
}

fn placeholder(chars: usize, hash: &str) -> String {
    format!("[pasted: {} characters, {}]", commas(chars), &hash[..hash.len().min(12)])
}

/// Apply the paste rule, then redaction, then the cap. `key` is the local history key (`history_pointer::load_or_create_key`).
pub fn scrub(key: &[u8], input: &Input) -> Scrubbed {
    let mut pasted: Vec<PastedBlock> = Vec::new();
    let mut text: String = input.text.replace('\0', "");
    // Marks first, longest first: a mark only adds pasted status.
    let mut marks: Vec<&String> = input.marked.iter().filter(|m| !m.trim().is_empty()).collect();
    marks.sort_by_key(|m| std::cmp::Reverse(m.len()));
    for m in marks {
        let block = m.trim_end_matches(['\n', '\r']);
        if block.is_empty() || !text.contains(block) {
            continue;
        }
        let hash = crate::history_pointer::span_hmac(key, block);
        let ph = placeholder(block.chars().count(), &hash);
        let n = text.matches(block).count();
        for _ in 0..n {
            pasted.push(PastedBlock { hash: hash.clone(), chars: block.chars().count() });
        }
        text = text.replace(block, &ph);
    }
    // Then the daemon's own rule over each span.
    let mut kept = String::new();
    for sp in spans(&text) {
        if sp.sep || !treated_as_pasted(&sp.text, input.typing, input.paste_chars) {
            kept.push_str(&sp.text);
            continue;
        }
        let body = sp.text.trim_end_matches(['\n', '\r']);
        let tail = &sp.text[body.len()..];
        let hash = crate::history_pointer::span_hmac(key, body);
        kept.push_str(&placeholder(body.chars().count(), &hash));
        kept.push_str(tail);
        pasted.push(PastedBlock { hash, chars: body.chars().count() });
    }
    let (masked, redacted) = redact(&strip_controls(&kept));
    let full_chars = masked.chars().count();
    if full_chars > PROMPT_MAX_CHARS {
        let full_hash = crate::history_pointer::span_hmac(key, &masked);
        let cut: String = masked.chars().take(PROMPT_MAX_CHARS).collect();
        return Scrubbed { text: cut, pasted, redacted, cut: true, full_hash: Some(full_hash) };
    }
    Scrubbed { text: masked, pasted, redacted, cut: false, full_hash: None }
}

/// A short reply field (`asked`, `did`, `open`): control characters stripped, redacted, cut to the cap. No hash is kept.
pub fn scrub_field(text: &str) -> String {
    let (masked, _) = redact(&strip_controls(text));
    masked.chars().take(REPLY_FIELD_MAX_CHARS).collect::<String>().trim().to_string()
}

#[cfg(test)]
#[path = "history_redact_tests.rs"]
mod tests;
