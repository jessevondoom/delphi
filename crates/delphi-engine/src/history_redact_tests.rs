use super::*;

const KEY: &[u8] = &[7u8; 32];

/// A deterministic base64-looking run that is not any real credential.
fn noise(n: usize, seed: u64) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ALPHABET[((x >> 33) % 64) as usize] as char
        })
        .collect()
}

/// The fixed corpus of secret shapes of `specs/102` §3.1, one per shape: `(shape, text, the part that must not survive)`. Every value is
/// an obviously fake fixture. The S7 audit runs the corpus again with shapes the author did not choose; a shape this list lacks is a
/// known gap (§13).
fn corpus() -> Vec<(&'static str, String, String)> {
    let mixed = "aB3dE6fG9h".repeat(5);
    let hex = "deadbeef".repeat(8);
    vec![
        ("private-key block", "here is my key\n-----BEGIN RSA PRIVATE KEY-----\nMIIFAKEFAKEFAKEFAKE\n-----END RSA PRIVATE KEY-----\nthanks".into(), "MIIFAKEFAKEFAKEFAKE".into()),
        ("cloud access key pair", format!("aws key AKIAFAKEFAKEFAKE0000 secret {mixed}"), "AKIAFAKEFAKEFAKE0000".into()),
        ("bearer token", "curl -H 'Authorization: Bearer abcDEF123456789xyz' https://example.test".into(), "abcDEF123456789xyz".into()),
        ("JWT", "token eyJhbGciOiJGQUtFIn0.eyJmYWtlIjoiZmFrZSJ9.ZmFrZXNpZ25hdHVyZQ end".into(), "eyJmYWtlIjoiZmFrZSJ9".into()),
        ("password inside a URL", "clone https://deploy:hunter2hunter2@git.example.test/repo.git please".into(), "hunter2hunter2".into()),
        ("token split across two lines", "my key is sk-ant-FAKEFAKEFAKE\nFAKEFAKEFAKE0000 ok".into(), "FAKEFAKEFAKE0000".into()),
        ("base64 fragment", format!("blob: {mixed} end"), mixed.clone()),
        ("long hex string", format!("digest {hex} done"), hex.clone()),
        ("name: value secret in a non-standard format", "internal_secret_phrase: swordfish99 and nothing else".into(), "swordfish99".into()),
        ("secret inside a code fence", "run this:\n```env\ndb_password: s3cretValue77\n```\nthen stop".into(), "s3cretValue77".into()),
    ]
}

#[test]
fn the_corpus_has_the_ten_shapes_of_the_spec() {
    assert_eq!(corpus().len(), 10);
}

#[test]
fn each_secret_shape_is_masked_by_redact() {
    for (shape, text, secret) in corpus() {
        let (out, changed) = redact(&text);
        assert!(changed, "{shape}: nothing was redacted");
        assert!(!out.contains(&secret), "{shape}: the secret survived redaction: {out}");
    }
}

#[test]
fn each_secret_shape_leaves_no_trace_in_a_stored_prompt() {
    for typing in [Typing::Typed, Typing::NotSeen] {
        for (shape, text, secret) in corpus() {
            let s = scrub(KEY, &Input { text: &text, marked: &[], typing, paste_chars: DEFAULT_PASTE_CHARS });
            assert!(!s.text.contains(&secret), "{shape} ({typing:?}): the secret was stored: {}", s.text);
            assert!(s.redacted || !s.pasted.is_empty(), "{shape} ({typing:?}): neither redacted nor hashed");
        }
    }
}

#[test]
fn ordinary_words_and_short_paths_are_left_alone() {
    let t = "Please look at crates/delphi-engine/src/history_pointer.rs and tell me what the S1 slice does about commit abc1234.";
    let s = scrub(KEY, &Input { text: t, marked: &[], typing: Typing::NotSeen, paste_chars: DEFAULT_PASTE_CHARS });
    assert_eq!(s.text, t);
    assert!(s.pasted.is_empty() && !s.redacted && !s.cut);
}

#[test]
fn an_unmarked_long_key_paste_is_a_hash_and_a_length_not_text() {
    // Acceptance 2: a 1,500-character key pasted with no paste mark, and not typed at typing rate.
    let key = noise(1500, 1);
    let text = format!("use this key {key} for the call");
    let s = scrub(KEY, &Input { text: &text, marked: &[], typing: Typing::NotSeen, paste_chars: DEFAULT_PASTE_CHARS });
    assert!(!s.text.contains(&key[..40]), "{}", s.text);
    assert_eq!(s.pasted.len(), 1);
    assert!(s.text.contains("[pasted: ") && s.text.contains("characters, "), "{}", s.text);
    assert!(s.pasted[0].chars > 1400, "{:?}", s.pasted);
    assert_eq!(s.pasted[0].hash, crate::history_pointer::span_hmac(KEY, text.trim_end_matches('\n')));
}

#[test]
fn a_high_entropy_run_is_hashed_even_when_the_text_was_typed() {
    let secret = noise(60, 2);
    let text = format!("the password is {secret} ok");
    assert!(entropy_bits(&secret) >= ENTROPY_BITS, "the fixture must be high entropy: {}", entropy_bits(&secret));
    let s = scrub(KEY, &Input { text: &text, marked: &[], typing: Typing::Typed, paste_chars: DEFAULT_PASTE_CHARS });
    assert!(!s.text.contains(&secret[..30]), "{}", s.text);
    assert_eq!(s.pasted.len(), 1);
}

#[test]
fn a_long_paragraph_is_hashed_unless_the_daemon_saw_it_typed() {
    // The boundary the spec leaves to calibration: paste_chars. A typed paragraph of the same length is kept up to the cap.
    let para = "word ".repeat(120); // 600 characters, low entropy, not code
    let unseen = scrub(KEY, &Input { text: &para, marked: &[], typing: Typing::NotSeen, paste_chars: 400 });
    assert_eq!(unseen.pasted.len(), 1, "an unseen 600-character paragraph is hashed");
    assert!(!unseen.text.contains("word word"));
    let typed = scrub(KEY, &Input { text: &para, marked: &[], typing: Typing::Typed, paste_chars: 400 });
    assert!(typed.pasted.is_empty(), "a typed paragraph is kept");
    assert!(typed.text.contains("word word"));
    // At or under the threshold nothing is hashed.
    let short = "word ".repeat(70); // 350
    assert!(scrub(KEY, &Input { text: &short, marked: &[], typing: Typing::NotSeen, paste_chars: 400 }).pasted.is_empty());
    // And a raised threshold keeps the same paragraph.
    assert!(scrub(KEY, &Input { text: &para, marked: &[], typing: Typing::NotSeen, paste_chars: 1000 }).pasted.is_empty());
}

#[test]
fn multi_line_code_is_hashed_and_a_short_list_is_not() {
    let code = "fn main() {\n    let x = vec![1, 2];\n    println!(\"{:?}\", x);\n}\n";
    let s = scrub(KEY, &Input { text: code, marked: &[], typing: Typing::NotSeen, paste_chars: 400 });
    assert_eq!(s.pasted.len(), 1);
    assert!(!s.text.contains("println"));
    assert!(s.text.ends_with('\n'), "the trailing newline stays outside the placeholder");
    let list = "First, read the spec.\n\nThen build the slice.\n\nThen tell me what is left.\n";
    assert!(scrub(KEY, &Input { text: list, marked: &[], typing: Typing::NotSeen, paste_chars: 400 }).pasted.is_empty());
}

#[test]
fn a_fenced_block_is_judged_as_one_span() {
    let text = "look at this\n\n```\nalpha: 1\nbeta: 2\ngamma: 3\n```\n\nand then say what it means\n";
    let s = scrub(KEY, &Input { text, marked: &[], typing: Typing::NotSeen, paste_chars: 400 });
    assert_eq!(s.pasted.len(), 1);
    assert!(s.text.contains("look at this") && s.text.contains("and then say what it means"));
    assert!(!s.text.contains("gamma"));
}

#[test]
fn a_mark_can_only_add_pasted_status() {
    let text = "Here is the note I was given: remember to rotate the thing. And my own words after.";
    let mark = vec!["remember to rotate the thing.".to_string()];
    let s = scrub(KEY, &Input { text, marked: &mark, typing: Typing::Typed, paste_chars: 400 });
    assert_eq!(s.pasted.len(), 1);
    assert!(!s.text.contains("rotate"));
    assert!(s.text.contains("my own words after"));
    // A mark that is not in the text changes nothing, and a mark never removes the daemon's own decision.
    let key = noise(1500, 3);
    let other = vec!["not present".to_string(), key.clone()];
    let t2 = format!("secret {key}");
    let s2 = scrub(KEY, &Input { text: &t2, marked: &other, typing: Typing::Typed, paste_chars: 400 });
    assert!(!s2.text.contains(&key[..30]));
}

#[test]
fn a_prompt_over_the_cap_is_cut_with_the_hash_of_the_full_text() {
    let text = "plain words here. ".repeat(200); // 3,600 characters, but spans are one paragraph: typed so it is not hashed
    let s = scrub(KEY, &Input { text: &text, marked: &[], typing: Typing::Typed, paste_chars: 400 });
    assert!(s.cut);
    assert_eq!(s.text.chars().count(), PROMPT_MAX_CHARS);
    assert_eq!(s.full_hash.as_deref(), Some(crate::history_pointer::span_hmac(KEY, &text).as_str()));
    let short = scrub(KEY, &Input { text: "short", marked: &[], typing: Typing::Typed, paste_chars: 400 });
    assert!(!short.cut && short.full_hash.is_none());
}

#[test]
fn control_characters_and_escape_sequences_are_stripped() {
    let t = "hello \u{1b}[31mred\u{1b}[0m world\u{7}\u{0}\nline two\ttabbed";
    let s = scrub(KEY, &Input { text: t, marked: &[], typing: Typing::Typed, paste_chars: 400 });
    assert!(!s.text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\t')), "{:?}", s.text);
    assert!(s.text.contains("line two\ttabbed"));
    assert_eq!(scrub_field("a\u{1b}[2Jb\u{7}"), "a[2Jb");
}

#[test]
fn the_output_is_deterministic_and_the_hash_depends_on_the_key() {
    let key = noise(500, 4);
    let t = format!("x {key} y");
    let a = scrub(KEY, &Input { text: &t, marked: &[], typing: Typing::NotSeen, paste_chars: 400 });
    let b = scrub(KEY, &Input { text: &t, marked: &[], typing: Typing::NotSeen, paste_chars: 400 });
    assert_eq!(a, b);
    let c = scrub(&[9u8; 32], &Input { text: &t, marked: &[], typing: Typing::NotSeen, paste_chars: 400 });
    assert_ne!(a.pasted[0].hash, c.pasted[0].hash, "a keyed hash differs under another key (a plain hash would not)");
}

#[test]
fn a_reply_field_is_redacted_and_cut_to_its_cap() {
    let long = "x".repeat(500);
    assert_eq!(scrub_field(&long).chars().count(), REPLY_FIELD_MAX_CHARS);
    assert!(!scrub_field("the token is Bearer abcDEF123456789xyz").contains("abcDEF123456789xyz"));
}

#[test]
fn thousands_are_written_with_commas() {
    assert_eq!(commas(1500), "1,500");
    assert_eq!(commas(999), "999");
    assert_eq!(commas(1_234_567), "1,234,567");
}
