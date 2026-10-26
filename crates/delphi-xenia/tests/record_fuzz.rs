//! Deterministic mutation fuzzing of steps 4-10 (no crates, no clock: a seeded xorshift generator).
//!
//! Two properties over every mutated input:
//! 1. **Totality**: the parser and validators return a verdict and never panic or loop, whatever the bytes.
//! 2. **Soundness**: if the input is accepted, an *independent* oracle (written here, sharing no code with the
//!    parser) agrees it contains nothing the spec forbids: valid NFC UTF-8, no controls but `\n`, no bidi or
//!    tag characters, no trust key as a key, no duplicate top-level keys, counts within limits, only allowed
//!    pointer schemes with ASCII locators that carry no userinfo, query, or local-path shape.
//!
//! The tests also check that they can fail: the unmutated records are accepted, plenty of mutants are
//! accepted too (so soundness is exercised on real accepts), and for inputs whose only defect is a forbidden
//! character, deleting that character makes them valid (so the character alone was the reason).

mod support;

use delphi_xenia::frontmatter;
use delphi_xenia::layout::{Kind, RecordPath};
use delphi_xenia::limits::*;
use delphi_xenia::pointer::Scheme;
use delphi_xenia::record::*;
use delphi_xenia::sidecar::Sidecar;
use support::*;
use unicode_normalization::UnicodeNormalization;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

fn dictionary() -> Vec<Vec<u8>> {
    let strs = [
        "&a ", "*a", "!!str ", "!tag ", "<<: ", "\r", "\r\n", "\t", "\u{202E}", "\u{2066}", "\u{200B}", "\u{E0041}", "\u{0}", "\u{7}", "tier: 1\n", "  trust: x\n", "source_type: x\n",
        "endorsed: true\n", "verified_by_org: x\n", "trust_class: x\n", "---\n", "...\n", "\"", "'", "[", "]", "{", "}", "#", ": ", "\\", "%", "@", "`", "|", ">", "\u{301}", "e\u{301}", " ", "  ",
        "\n\n", "file:///etc/passwd", "delphi://x", "https://u:p@h.example/", "?token=1", "../", "&amp;", "<b>", "](x)", "null", "~", "0", "99999999999999999999", "-", "- ", "key: v\n", "tier",
        "\u{ff54}", "\u{430}", "L1-L2", "blake3:", "drive", "ssh://git@h/x#y",
    ];
    let mut out: Vec<Vec<u8>> = strs.iter().map(|s| s.as_bytes().to_vec()).collect();
    out.extend([b"\xC0\xAF".to_vec(), b"\xFF".to_vec(), b"\xED\xA0\x80".to_vec(), b"\xE2\x82".to_vec()]);
    out
}

fn mutate(rng: &mut Rng, base: &[u8], others: &[Vec<u8>], dict: &[Vec<u8>]) -> Vec<u8> {
    let mut v = base.to_vec();
    for _ in 0..1 + rng.below(3) {
        if v.is_empty() {
            v.push(b'-');
        }
        let at = rng.below(v.len() + 1);
        match rng.below(11) {
            0 => {
                let i = rng.below(v.len());
                v[i] ^= 1 << rng.below(8);
            }
            1 => {
                let i = rng.below(v.len());
                v[i] = (rng.next() & 0xff) as u8;
            }
            2 | 3 => {
                let tok = &dict[rng.below(dict.len())];
                v.splice(at..at, tok.iter().copied());
            }
            4 => {
                let end = (at + 1 + rng.below(8)).min(v.len());
                v.drain(at.min(end)..end);
            }
            5..=7 => {
                // Line-level edits: duplicate, delete, swap, or splice in a line from another record.
                let mut lines: Vec<Vec<u8>> = v.split(|b| *b == b'\n').map(<[u8]>::to_vec).collect();
                let i = rng.below(lines.len());
                match rng.below(4) {
                    0 => lines.insert(i, lines[i].clone()),
                    1 => {
                        lines.remove(i);
                    }
                    2 => {
                        let j = rng.below(lines.len());
                        lines.swap(i, j);
                    }
                    _ => {
                        let other: Vec<&[u8]> = others[rng.below(others.len())].split(|b| *b == b'\n').collect();
                        lines.insert(i, other[rng.below(other.len())].to_vec());
                    }
                }
                v = lines.join(&b'\n');
            }
            8 => v.truncate(at),
            9 => {
                // Indentation drift on one line.
                let mut lines: Vec<Vec<u8>> = v.split(|b| *b == b'\n').map(<[u8]>::to_vec).collect();
                let i = rng.below(lines.len());
                if rng.below(2) == 0 {
                    lines[i].insert(0, b' ');
                } else if lines[i].first() == Some(&b' ') {
                    lines[i].remove(0);
                }
                v = lines.join(&b'\n');
            }
            _ => {
                // Replace a short run with a dictionary token.
                let tok = &dict[rng.below(dict.len())];
                let end = (at + rng.below(6)).min(v.len());
                v.splice(at..end, tok.iter().copied());
            }
        }
    }
    v
}

fn oracle_forbidden(c: char) -> bool {
    let u = c as u32;
    (c != '\n' && c.is_control()) || (0x202A..=0x202E).contains(&u) || (0x2066..=0x2069).contains(&u) || (0xE0000..=0xE007F).contains(&u) || matches!(u, 0x200E | 0x200F | 0x061C)
}

/// Everything the oracle knows must hold of an accepted record. Returns the first violation.
fn oracle_violation(bytes: &[u8], rec: &Record, kind: Kind, id: &str) -> Option<String> {
    let Ok(text) = std::str::from_utf8(bytes) else { return Some("accepted invalid UTF-8".into()) };
    if let Some(c) = text.chars().find(|c| oracle_forbidden(*c)) {
        return Some(format!("accepted U+{:04X}", c as u32));
    }
    if text.contains('\r') {
        return Some("accepted a CR".into());
    }
    if text.nfc().collect::<String>() != text {
        return Some("accepted non-NFC text".into());
    }
    let Some(rest) = text.strip_prefix("---\n") else { return Some("accepted a file with no opening delimiter".into()) };
    let Some(end) = rest.find("\n---\n").map(|i| i + 1).or_else(|| rest.starts_with("---\n").then_some(0)) else { return Some("accepted a file with no closing delimiter".into()) };
    let mut top_keys: Vec<&str> = Vec::new();
    for line in rest[..end].lines() {
        if line.ends_with(' ') || line.is_empty() {
            return Some(format!("accepted a blank or trailing-space line {line:?}"));
        }
        let indent = line.len() - line.trim_start_matches(' ').len();
        let content = line.trim_start_matches(' ');
        let content = content.strip_prefix("- ").unwrap_or(content);
        let key = content.split(':').next().unwrap_or("");
        if TRUST_KEYS.contains(&key) {
            return Some(format!("accepted trust key {key}"));
        }
        if indent == 0 {
            if top_keys.contains(&key) {
                return Some(format!("accepted duplicate top-level key {key}"));
            }
            top_keys.push(key);
        }
        for bad in ["&", "*", "!", "<<"] {
            if content.starts_with(bad) || content.split_once(": ").is_some_and(|(_, v)| v.starts_with(bad)) {
                return Some(format!("accepted a YAML feature {bad} in {line:?}"));
            }
        }
    }
    if rec.id != id || rec.kind() != kind || rec.org != ORG || rec.store != STORE || rec.author != author_did() {
        return Some("accepted a record that disagrees with its path or pinned scope".into());
    }
    if rec.rev < 1 || rec.rev > MAX_REV {
        return Some("rev out of range".into());
    }
    let counts = [
        (rec.title.chars().count(), MAX_TITLE_CHARS),
        (rec.summary.chars().count(), MAX_SUMMARY_CHARS),
        (rec.tags.len(), MAX_TAGS),
        (rec.pointers.len(), MAX_POINTERS),
        (rec.evidence.len(), MAX_EVIDENCE),
        (rec.related.len(), MAX_RELATED),
        (rec.body.len(), MAX_BODY_BYTES),
    ];
    if counts.iter().any(|(n, max)| n > max) {
        return Some("accepted a field over its limit".into());
    }
    for text in [&rec.title, &rec.summary] {
        if text.contains("](") || text.contains('\n') || text.as_bytes().windows(2).any(|w| w[0] == b'<' && w[1].is_ascii_alphabetic()) {
            return Some("accepted a link or HTML in a plain-text field".into());
        }
    }
    for p in rec.pointers.iter().chain(&rec.evidence) {
        if !matches!(p.scheme, Scheme::Git | Scheme::Https | Scheme::Xenia) {
            return Some("accepted a scheme outside the allowlist".into());
        }
        let l = &p.locator;
        if !l.bytes().all(|b| (0x21..=0x7e).contains(&b)) || l.contains(['?', ';', '=', '&', '\\']) {
            return Some(format!("accepted a locator with forbidden characters: {l:?}"));
        }
        for prefix in ["file:", "delphi:", "/", ".", "~"] {
            if l.starts_with(prefix) {
                return Some(format!("accepted a local-looking locator {l:?}"));
            }
        }
        if p.scheme != Scheme::Xenia {
            let after = l.split_once("://").map_or("", |(_, r)| r);
            if !(l.starts_with("https://") || l.starts_with("ssh://")) || after.split('/').next().unwrap_or("").contains('@') {
                return Some(format!("accepted a locator with a bad scheme or userinfo: {l:?}"));
            }
        }
    }
    None
}

fn run(bytes: &[u8], kind: Kind, id: &str) -> Result<Record, Reject> {
    let car = Sidecar { kid: author_did().to_string(), signed_at: "2026-10-24T09:00:03Z".into(), roster_epoch: 7, hash: delphi_xenia::hash::of(bytes), sig: String::new() };
    check_parsed(pinned(), &RecordPath { kind, id: id.to_string() }, &car, bytes)
}

#[test]
fn mutated_records_never_panic_and_nothing_forbidden_is_ever_accepted() {
    let bases: Vec<(Kind, &str, Vec<u8>)> = vec![
        (Kind::Memory, ID, memory_text().into_bytes()),
        (Kind::Endorsement, OTHER_ID, endorsement_text().into_bytes()),
        (Kind::Tombstone, OTHER_ID, tombstone_text().into_bytes()),
    ];
    let texts: Vec<Vec<u8>> = bases.iter().map(|b| b.2.clone()).collect();
    let dict = dictionary();
    for (kind, id, base) in &bases {
        assert!(run(base, *kind, id).is_ok(), "premise: the unmutated {kind:?} record is accepted");
    }

    let (mut accepted, mut rejected, mut forbidden_cases, mut lenient_would_accept) = (0usize, 0usize, 0usize, 0usize);
    for seed in 1..=8u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for iteration in 0..3000 {
            let (kind, id, base) = &bases[rng.below(bases.len())];
            let bytes = mutate(&mut rng, base, &texts, &dict);
            let verdict = run(&bytes, *kind, id);
            // Deterministic: the same bytes give the same verdict.
            assert_eq!(verdict.as_ref().map(|r| r.id.clone()).map_err(|e| e.reason()), run(&bytes, *kind, id).as_ref().map(|r| r.id.clone()).map_err(|e| e.reason()));
            match &verdict {
                Ok(rec) => {
                    accepted += 1;
                    if let Some(why) = oracle_violation(&bytes, rec, *kind, id) {
                        panic!("seed {seed} iteration {iteration}: {why}\ninput: {:?}", String::from_utf8_lossy(&bytes));
                    }
                }
                Err(_) => rejected += 1,
            }
            // Inputs carrying a forbidden character or a CR must be rejected, and removing just those characters
            // must (often) turn them into accepts: the character was the whole reason.
            if let Ok(text) = std::str::from_utf8(&bytes) {
                if text.chars().any(|c| oracle_forbidden(c) && c != '\n') || text.contains('\r') {
                    forbidden_cases += 1;
                    assert!(verdict.is_err(), "seed {seed} iteration {iteration}: accepted a forbidden character: {text:?}");
                    let cleaned: String = text.chars().filter(|c| !(oracle_forbidden(*c) || *c == '\r')).collect();
                    if run(cleaned.as_bytes(), *kind, id).is_ok() {
                        lenient_would_accept += 1;
                    }
                }
            }
        }
    }
    eprintln!("fuzz: accepted {accepted}, rejected {rejected}, forbidden-character cases {forbidden_cases}, would-pass-if-lenient {lenient_would_accept}");
    assert!(accepted > 300, "soundness is only tested if mutants are sometimes accepted (got {accepted})");
    assert!(rejected > 5_000, "got {rejected}");
    assert!(forbidden_cases > 500, "the dictionary must produce forbidden characters often (got {forbidden_cases})");
    assert!(lenient_would_accept > 25, "a lenient parser would have accepted these; ours did not (got {lenient_would_accept})");
}

#[test]
fn the_frontmatter_parser_alone_is_total_over_random_bytes_and_shapes() {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    let alphabet: &[u8] = b"-:[]{}\"'\\ \n\n\n  #&*!|>%@`,.abxyz019_\t\r";
    for _ in 0..30_000 {
        let n = rng.below(400);
        let mut v = b"---\n".to_vec();
        v.extend((0..n).map(|_| alphabet[rng.below(alphabet.len())]));
        if rng.below(2) == 0 {
            v.extend_from_slice(b"\n---\n");
        }
        // Must return, not panic. Anything accepted is re-checked for the two cheapest invariants.
        if let Ok(doc) = frontmatter::parse(&v) {
            assert!(!v.contains(&b'\r') && !v.contains(&b'\t'));
            assert!(doc.front.find_key_deep(&["<<"]).is_none());
        }
    }
    // Pure binary noise, and every prefix of a valid record (truncation at each byte).
    for _ in 0..5_000 {
        let n = rng.below(300);
        let v: Vec<u8> = (0..n).map(|_| (rng.next() & 0xff) as u8).collect();
        let _ = frontmatter::parse(&v);
    }
    let t = memory_text().into_bytes();
    let mut prefixes_accepted = 0;
    for cut in 0..=t.len() {
        if run(&t[..cut], Kind::Memory, ID).is_ok() {
            prefixes_accepted += 1;
        }
    }
    // The only accepted prefixes are those that still end the body: a record is complete or it is refused.
    let body_start = std::str::from_utf8(&t).unwrap().find("Handlers").unwrap();
    assert!(prefixes_accepted > 0 && prefixes_accepted <= t.len() - body_start + 1, "{prefixes_accepted}");
    assert!(run(&t[..body_start - 4], Kind::Memory, ID).is_err(), "a record cut inside its frontmatter is refused");
}
