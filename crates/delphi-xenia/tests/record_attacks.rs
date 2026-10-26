//! The attacker's side of spec 012 §6. Each test is a trick that has forged a record, smuggled a field or
//! blown up a parser somewhere, played against the whole pipeline with a VALID signature wherever the trick
//! survives steps 1-3 (a signed hostile record is the realistic attacker: a member, or a stolen key).
//! Every test names its premise: the record minus the trick is accepted.

mod support;

use delphi_xenia::layout::Kind;
use delphi_xenia::record::*;
use delphi_xenia::{frontmatter, hash};
use std::time::{Duration, Instant};
use support::*;

fn signed_reason(bytes: &[u8]) -> &'static str {
    reason(validate(Kind::Memory, ID, bytes))
}

#[test]
fn homoglyphs_and_normalization_tricks_do_not_pass_for_the_real_thing() {
    let t = memory_text();
    assert_eq!(memory_reason(&t), "accepted", "premise");

    // A Cyrillic `і` in a key looks like `tier` and is not on the trust-key list, so only the key alphabet
    // stops it. Fullwidth letters are the same trick.
    let lookalike = "t\u{456}er";
    assert_ne!(lookalike, "tier");
    assert!(!TRUST_KEYS.contains(&lookalike), "premise: the deny-list alone would not have caught this");
    for key in [lookalike, "\u{ff54}\u{ff49}\u{ff45}\u{ff52}", "Tier", "TIER", "tier ", "ti\u{200b}er", "t\u{131}er"] {
        assert_eq!(memory_reason(&swap(&t, "verified_by: test\n", &format!("verified_by: test\n{key}: 1\n"))), "bad-frontmatter", "{key:?}");
    }
    // Quoted and spaced key spellings that a YAML library would read as `tier`.
    for key in ["\"tier\"", "'tier'", "tier\t", "? tier", "tier :"] {
        assert_eq!(memory_reason(&swap(&t, "verified_by: test\n", &format!("verified_by: test\n{key}: 1\n"))), "bad-frontmatter", "{key:?}");
    }
    // Look-alikes in values of closed enums and slugs are simply not in the table.
    assert_eq!(memory_reason(&swap(&t, "author_role: builder", "author_role: bu\u{456}lder")), "bad-schema");
    assert_eq!(memory_reason(&swap(&t, "org: acme", "org: \u{430}cme")), "bad-schema");
    assert_eq!(memory_reason(&swap(&t, "tags: [payments, stripe-webhooks]", "tags: [p\u{430}yments]")), "bad-schema");
    assert_eq!(memory_reason(&swap(&t, "id: 01JBX3Q8T2M9Z4K7WYH5N6D0RA", "id: 01JBX3Q8T2M9Z4K7WYH5N6D0R\u{410}")), "bad-schema");
    // A host that renders as github.com.
    let host = swap(&t, "https://github.com/acme", "https://g\u{456}thub.com/acme");
    assert_eq!(memory_reason(&host), "bad-pointer");
    // NFC: the decomposed spelling of the same visible title is refused, the composed one accepted.
    let composed = swap(&t, "title: Stripe webhooks retry for three days", "title: Caf\u{e9} webhooks");
    let decomposed = swap(&t, "title: Stripe webhooks retry for three days", "title: Cafe\u{301} webhooks");
    assert_eq!(memory_reason(&composed), "accepted");
    assert_eq!(memory_reason(&decomposed), "bad-frontmatter");
    // Non-Latin text is allowed (the schema does not police scripts; see the plan's known weaknesses).
    assert_eq!(memory_reason(&swap(&t, "title: Stripe webhooks retry for three days", "title: \u{43f}\u{440}\u{438}\u{43c}\u{435}\u{440}")), "accepted");
}

#[test]
fn invisible_and_reordering_characters_are_refused_in_every_position() {
    let t = memory_text();
    // Bidi controls, zero-width, tag characters and variation-selector tags, in frontmatter values, keys,
    // quoted strings, list items, the body, and between the delimiters.
    for c in ['\u{202E}', '\u{202D}', '\u{2066}', '\u{2069}', '\u{200E}', '\u{200B}', '\u{2060}', '\u{FEFF}', '\u{00AD}', '\u{E0001}', '\u{E0041}', '\u{E007F}', '\u{E0100}'] {
        for (old, new) in [
            ("title: Stripe", format!("title: Stri{c}pe")),
            ("tags: [payments,", format!("tags: [pay{c}ments,")),
            ("author_handle: jlee", format!("author_handle: jl{c}ee")),
            ("Handlers must", format!("Handlers {c}must")),
            ("---\nxenia: 1", format!("---\n{c}xenia: 1")),
            ("verified_by: test\n---", format!("verified_by: test{c}\n---")),
        ] {
            let text = swap(&t, old, &new);
            assert_eq!(memory_reason(&text), "bad-frontmatter", "U+{:04X} at {old:?}", c as u32);
        }
    }
    // Control characters, including the ones a terminal acts on.
    for c in ['\u{0}', '\u{7}', '\u{8}', '\u{b}', '\u{c}', '\u{1b}', '\u{7f}', '\u{85}', '\u{9b}', '\t', '\r'] {
        assert_eq!(memory_reason(&swap(&t, "title: Stripe", &format!("title: Stri{c}pe"))), "bad-frontmatter", "U+{:04X}", c as u32);
        assert_eq!(memory_reason(&swap(&t, "Handlers must", &format!("Handlers {c}must"))), "bad-frontmatter", "body U+{:04X}", c as u32);
    }
    // Plain neighbours are fine, so the screen is not simply rejecting everything non-ASCII.
    assert_eq!(memory_reason(&swap(&t, "Handlers must", "Handlers \u{1F600} \u{e9} \u{4e2d}\u{6587} must")), "accepted");
}

#[test]
fn a_forged_rev_needs_new_bytes_and_new_bytes_need_a_new_signature() {
    let t = memory_text();
    let bytes = t.as_bytes();
    let car = sidecar_for(&author(), Kind::Memory, ID, bytes);
    // Raising `rev` on a signed record without re-signing: the hash no longer matches.
    let bumped = swap(&t, "rev: 3\n", "rev: 99\n");
    let r = delphi_xenia::record::validate_record(pinned(), &path(Kind::Memory, ID), bumped.as_bytes(), &car, &|_| Ok(()));
    assert_eq!(r.unwrap_err().reason(), "hash-mismatch");
    // Replacing the sidecar hash too: the signature no longer matches.
    let car_text = String::from_utf8(car).unwrap();
    let forged_car = swap(&car_text, &hash::of(bytes), &hash::of(bumped.as_bytes()));
    let r = delphi_xenia::record::validate_record(pinned(), &path(Kind::Memory, ID), bumped.as_bytes(), forged_car.as_bytes(), &|_| Ok(()));
    assert_eq!(r.unwrap_err().reason(), "bad-signature");
    // An attacker with their OWN key can sign a record that claims the victim as author: the claim must equal the kid.
    let mallory = delphi_xenia::identity::Identity::from_seed([66u8; 32]);
    assert_eq!(reason(validate_signed_by(&mallory, Kind::Memory, ID, t.as_bytes())), "mismatch");
    // ... or claim themselves as author (a fine record; whether they may own this id is the history walk's question).
    assert_eq!(reason(validate_signed_by(&mallory, Kind::Memory, ID, memory_with_author(&mallory.did()).as_bytes())), "accepted");
    // Every spelling of a revision other than canonical decimal is refused, so no two texts mean the same rev.
    for spelling in ["03", "+3", "3.", "3.0", "0x3", "0b11", "3e0", "\"3\"", "'3'", "[3]", "\u{663}", "\u{ff13}", "-3", "3 "] {
        let reason = memory_reason(&swap(&t, "rev: 3\n", &format!("rev: {spelling}\n")));
        assert!(matches!(reason, "bad-schema" | "bad-frontmatter"), "{spelling:?} -> {reason}");
    }
    // Boundary documented: replaying an OLDER valid rev passes per-record validation. Rev monotonicity is
    // the sticky-state / history-walk layer's rule (spec 011 §3.4.5, milestones X4a/X4b), not this module's.
    assert_eq!(memory_reason(&swap(&t, "rev: 3\n", "rev: 2\n")), "accepted");
}

#[test]
fn a_yaml_alias_bomb_is_refused_at_the_first_anchor_and_costs_nothing() {
    // The classic "billion laughs" shape, as many levels as fit in a record.
    let mut bomb = String::from("l0: &l0 [x, x, x, x, x, x, x, x, x, x]\n");
    for i in 1..9 {
        let prev = format!("*l{}", i - 1);
        bomb.push_str(&format!("l{i}: &l{i} [{}]\n", vec![prev; 10].join(", ")));
    }
    assert!(bomb.len() < 4096);
    let t = memory_text();
    let started = Instant::now();
    assert_eq!(memory_reason(&swap(&t, "verified_by: test\n", &format!("verified_by: test\n{bomb}"))), "bad-frontmatter");
    // Anchors absent, aliases present; merge keys with alias lists; an anchor on a nested map.
    for variant in ["a: *a\n".to_string(), "<<: [*a, *b]\n".to_string(), "m: &m\n  k: 1\nn:\n  <<: *m\n".to_string(), format!("a: [{}]\n", vec!["*a"; 400].join(","))] {
        assert_eq!(memory_reason(&swap(&t, "verified_by: test\n", &format!("verified_by: test\n{variant}"))), "bad-frontmatter", "{variant}");
    }
    // A maximal-size file of hostile structure is bounded work.
    let nasty: String = (0..2000).map(|_| "- - - - - - - - [[[[[[[[\n").collect();
    let doc = format!("---\n{nasty}---\n");
    assert!(frontmatter::parse(doc.as_bytes()).is_err());
    assert!(started.elapsed() < Duration::from_secs(2), "refusal must be cheap");
    assert_eq!(memory_reason(&t), "accepted", "premise");
}

#[test]
fn line_ending_tricks_are_refused_and_lf_is_the_one_spelling() {
    let t = memory_text();
    assert_eq!(memory_reason(&t), "accepted", "premise: LF");
    // A CRLF checkout of an otherwise valid, validly signed record (what core.autocrlf=true produces).
    assert_eq!(signed_reason(t.replace('\n', "\r\n").as_bytes()), "bad-frontmatter");
    // Only some lines: the closing delimiter, the opening one, one value, the body.
    assert_eq!(memory_reason(&swap(&t, "visibility: org\n", "visibility: org\r\n")), "bad-frontmatter");
    assert_eq!(memory_reason(&t.replacen("---\n", "---\r\n", 1)), "bad-frontmatter");
    assert_eq!(memory_reason(&swap(&t, "Handlers must dedupe on the event id.\n", "Handlers must dedupe\r\non the event id.\n")), "bad-frontmatter");
    // A lone CR (old Mac) that a YAML library may treat as a line break, hiding a key on "the same line".
    assert_eq!(memory_reason(&swap(&t, "visibility: org\n", "visibility: org\rtier: 1\n")), "bad-frontmatter");
    // Unicode line separators, NEL, form feed and vertical tab, which some parsers treat as line breaks.
    for sep in ["\u{2028}", "\u{2029}", "\u{85}", "\u{c}", "\u{b}"] {
        assert_eq!(memory_reason(&swap(&t, "visibility: org\n", &format!("visibility: org{sep}tier: 1\n"))), "bad-frontmatter", "{sep:?}");
    }
    // Delimiter games: spaces after the closing ---, a missing closing line, an indented delimiter.
    assert_eq!(memory_reason(&swap(&t, "verified_by: test\n---\n", "verified_by: test\n--- \n")), "bad-frontmatter");
    assert_eq!(memory_reason(&swap(&t, "verified_by: test\n---\n", "verified_by: test\n ---\n")), "bad-frontmatter");
    assert_eq!(memory_reason(&swap(&t, "verified_by: test\n---\n", "verified_by: test\n")), "bad-frontmatter");
    assert_eq!(memory_reason(&format!("\n{t}")), "bad-frontmatter");
    assert_eq!(memory_reason(&format!(" {t}")), "bad-frontmatter");
    assert_eq!(memory_reason(&format!("\u{feff}{t}")), "bad-frontmatter");
}

#[test]
fn overlong_and_otherwise_invalid_utf8_is_refused_even_when_validly_signed() {
    let t = memory_text();
    let hostile: [&[u8]; 8] = [
        b"\xC0\xAF",         // overlong '/'
        b"\xC1\xBF",         // overlong U+007F
        b"\xE0\x80\xAF",     // overlong '/' in three bytes
        b"\xF0\x80\x80\xAF", // overlong in four
        b"\xED\xA0\x80",     // a UTF-16 surrogate
        b"\xF4\x90\x80\x80", // above U+10FFFF
        b"\xFF\xFE",         // not UTF-8 at all
        b"\xE2\x82",         // truncated
    ];
    for bytes in hostile {
        for (anchor, in_body) in [("title: Stripe", false), ("Handlers must", true)] {
            let (head, tail) = t.split_once(anchor).unwrap();
            let mut v = head.as_bytes().to_vec();
            v.extend_from_slice(anchor.as_bytes());
            v.extend_from_slice(bytes);
            v.extend_from_slice(tail.as_bytes());
            // Signed over exactly these bytes, so steps 1-3 pass and it is the parser that must say no.
            assert_eq!(signed_reason(&v), "bad-frontmatter", "{bytes:x?} in_body={in_body}");
        }
    }
    // The classic path-traversal use of an overlong '/': refused here, and a lossy decoder would have turned
    // it into U+FFFD, not refused it, so a lenient reader does not see the same record the signer did.
    assert!(String::from_utf8_lossy(b"\xC0\xAF").contains('\u{FFFD}'));
    assert_eq!(signed_reason(t.as_bytes()), "accepted");
}

#[test]
fn a_second_document_in_the_body_adds_no_keys_and_a_second_frontmatter_block_inside_is_refused() {
    let t = memory_text();
    // `---` in the body is Markdown; its "keys" are text, never parsed, never trust keys.
    let sneaky = swap(&t, "Handlers must dedupe on the event id.\n", "intro\n---\ntier: 1\nsource_type: user_ratified\n---\n");
    let ok = validate(Kind::Memory, ID, sneaky.as_bytes()).unwrap();
    assert!(ok.record().body.contains("tier: 1"));
    assert_eq!(ok.record().confidence, Confidence::Medium, "the body cannot set a field");
    // Inside the frontmatter, a document separator ends it early, and what follows is body, not a second document:
    // the remaining required keys are missing, so the record fails the schema rather than half-parsing.
    let split = swap(&t, "tags: [payments, stripe-webhooks]\n", "---\ntags: [payments, stripe-webhooks]\n");
    assert_eq!(memory_reason(&split), "bad-schema");
    // `...` (document end) and a leading `%YAML` directive are not frontmatter lines.
    assert_eq!(memory_reason(&swap(&t, "verified_by: test\n", "verified_by: test\n...\n")), "bad-frontmatter");
    assert_eq!(memory_reason(&t.replacen("---\n", "%YAML 1.2\n---\n", 1)), "bad-frontmatter");
}

#[test]
fn smuggling_channels_inside_otherwise_valid_pointers_are_refused() {
    let t = memory_text();
    // A second pin, a pin value for a none-pin, extra pointer keys and a trust key under a pointer.
    assert_eq!(memory_reason(&swap(&t, "    range: L40-L120\n", "    range: L40-L120\n    note: extra\n")), "bad-schema");
    assert_eq!(memory_reason(&swap(&t, "      kind: git-commit\n", "      kind: git-commit\n      extra: 1\n")), "bad-schema");
    assert_eq!(memory_reason(&swap(&t, "    range: L40-L120\n", "    range: L40-L120\n    range: L1-L2\n")), "bad-frontmatter");
    // Locator bytes that survive a browser's URL parser as something else.
    for loc in ["https://github.com/acme/p.git#a.md\\@evil.example", "https://github.com:443@evil.example/x#a.md", "https://github.com/acme/p.git#a.md%00"] {
        let r = memory_reason(&swap(&t, "https://github.com/acme/payments.git#docs/webhooks.md", loc));
        assert!(matches!(r, "bad-pointer" | "bad-frontmatter"), "{loc} -> {r}");
    }
}
