//! X10: deterministic fuzzing of the sidecar parser and of verify-before-parse (spec 011 §1.4 gate 6, spec 012 §6).
//!
//! The repo has no fuzz crate and no `cargo-fuzz` setup; its idiom (`record_fuzz.rs`, `serialize_fuzz.rs`) is a
//! seeded xorshift generator inside an ordinary test, with an independent oracle that shares no code with the
//! parser. This file follows it. The frontmatter parser is fuzzed in `record_fuzz.rs` (24,000 mutated records through
//! steps 4-10 plus 35,000 random byte strings and shapes into `frontmatter::parse`, and the serializer round trip in
//! `serialize_fuzz.rs`); the full budget table is in `missions/2026-10-23-foundational-design/xenia-x10-verification.md`.
//!
//! **Corpus.** Three valid, really signed sidecars (a `memory`, an `endorsement` and a `tombstone` record, signed by
//! the standard test author) and their record files, plus a dictionary of JSON and encoding hazards.
//!
//! **Budget.** 8 seeds. Sidecar parsing: 8 x 5,000 = 40,000 mutants. Pipeline (verify-before-parse): 8 x 1,500 =
//! 12,000 mutated sidecar / record / standing combinations.
//!
//! **Invariants.**
//! 1. *Total*: `Sidecar::parse`, `verify_record` and `validate_record` return a verdict for every input; none panics.
//! 2. *Never accepts malformed*: if `Sidecar::parse` accepts, an independent oracle agrees the bytes are UTF-8, at
//!    most 1 KiB, exactly the six fixed keys once each (counted by a key-recording visitor, not by the parser's own
//!    derive), `alg` is `ed25519`, `kid` a `did:key:z...`, `signed_at` the strict 20-character UTC spelling,
//!    `roster_epoch` at least 1, `hash` `blake3:` plus 64 lowercase hex digits.
//! 3. *Round trip where defined*: an accepted sidecar re-serialised with `to_json` parses back to itself.
//! 4. *No forgery*: `verify_record` accepts a mutant only if its fields equal the fields of the one signature that
//!    exists for that record (nobody holds another key), and only if the record bytes are the signed bytes.
//! 5. *YAML never runs before steps 1-3*: the instrumented frontmatter parser's call count moves by exactly one when
//!    steps 1-3 (size, sidecar, hash, signature) and the standing check all pass, and by zero otherwise.

mod support;

use delphi_xenia::frontmatter;
use delphi_xenia::hash;
use delphi_xenia::layout::Kind;
use delphi_xenia::record::validate_record;
use delphi_xenia::sidecar::{verify_record, Sidecar, MAX_RECORD_BYTES};
use serde::de::{Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use support::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
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
        "\"alg\"", ", \"alg\": \"ed25519\"", "\"extra\": 1,", "\\u0000", "\\ud800", "\\u0065d25519", "1e0", "01", "-0", "-1", "18446744073709551616", "0", "1.0", "null", "true", "[", "]", "{", "}", "\"", "\\",
        "/*c*/", "//", ",", ",}", ":", "\r\n", "\t", " ", "\u{feff}", "\u{202e}", "\"sig\": \"\"", "blake3:", "did:key:z", "\"hash\"", "\"kid\"", "ed25519", "ED25519", "2026-10-24T09:00:03", "Z", "+00:00", "A", "=",
    ];
    let mut out: Vec<Vec<u8>> = strs.iter().map(|s| s.as_bytes().to_vec()).collect();
    out.extend([b"\xC0\xAF".to_vec(), b"\xFF".to_vec(), b"\xED\xA0\x80".to_vec(), vec![b'a'; 2000], vec![0u8; 3]]);
    out
}

fn mutate(rng: &mut Rng, base: &[u8], others: &[Vec<u8>], dict: &[Vec<u8>]) -> Vec<u8> {
    let mut v = base.to_vec();
    for _ in 0..1 + rng.below(3) {
        if v.is_empty() {
            v.push(b'{');
        }
        let at = rng.below(v.len());
        match rng.below(10) {
            0 => v[at] ^= 1 << rng.below(8),
            1 => {
                let end = (at + 1 + rng.below(12)).min(v.len());
                v.drain(at..end);
            }
            2 => {
                let t = &dict[rng.below(dict.len())];
                let tail = v.split_off(at);
                v.extend_from_slice(t);
                v.extend(tail);
            }
            3 => {
                let t = dict[rng.below(dict.len())].clone();
                let end = (at + 1 + rng.below(8)).min(v.len());
                v.splice(at..end, t);
            }
            4 => {
                let end = (at + 1 + rng.below(40)).min(v.len());
                let chunk = v[at..end].to_vec();
                v.splice(at..at, chunk);
            }
            5 => v.truncate(at),
            6 => {
                let o = &others[rng.below(others.len())];
                let from = rng.below(o.len());
                let end = (from + 1 + rng.below(60)).min(o.len());
                let chunk = o[from..end].to_vec();
                v.splice(at..(at + chunk.len()).min(v.len()), chunk);
            }
            7 => v[at] = b"0123456789abcdefABCDEF:\"{}, \n"[rng.below(29)],
            8 => v.push(b"\n \t}"[rng.below(4)]),
            _ => v.insert(at, rng.next() as u8),
        }
        if v.len() > 4096 {
            v.truncate(4096);
        }
    }
    v
}

/// Records the keys of a JSON object in order, duplicates and all, without interpreting the values.
struct Keys(Vec<String>);

impl<'de> Deserialize<'de> for Keys {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Keys, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Keys;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Keys, A::Error> {
                let mut keys = Vec::new();
                while let Some(k) = m.next_key::<String>()? {
                    m.next_value::<IgnoredAny>()?;
                    keys.push(k);
                }
                Ok(Keys(keys))
            }
        }
        d.deserialize_map(V)
    }
}

/// What an accepted sidecar must look like, judged without the parser's own code. `None` if it is fine.
fn oracle_violation(bytes: &[u8], car: &Sidecar) -> Option<String> {
    if bytes.len() > 1024 {
        return Some("accepted a sidecar over 1 KiB".into());
    }
    let Ok(text) = std::str::from_utf8(bytes) else { return Some("accepted a sidecar that is not UTF-8".into()) };
    let Ok(Keys(mut keys)) = serde_json::from_str::<Keys>(text) else { return Some("accepted a sidecar that is not a JSON object".into()) };
    keys.sort();
    if keys != ["alg", "hash", "kid", "roster_epoch", "sig", "signed_at"] {
        return Some(format!("accepted keys {keys:?}"));
    }
    let v: serde_json::Value = serde_json::from_str(text).unwrap();
    if v["alg"] != "ed25519" {
        return Some("accepted another alg".into());
    }
    if !car.kid.starts_with("did:key:z") {
        return Some("accepted a kid that is not a did:key".into());
    }
    let s = car.signed_at.as_bytes();
    let shape = s.len() == 20 && s[4] == b'-' && s[7] == b'-' && s[10] == b'T' && s[13] == b':' && s[16] == b':' && s[19] == b'Z';
    if !shape || !s.iter().enumerate().all(|(i, b)| matches!(i, 4 | 7 | 10 | 13 | 16 | 19) || b.is_ascii_digit()) {
        return Some(format!("accepted signed_at {:?}", car.signed_at));
    }
    if car.roster_epoch < 1 {
        return Some("accepted roster_epoch 0".into());
    }
    let ok_hash = car.hash.strip_prefix("blake3:").is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    if !ok_hash {
        return Some(format!("accepted hash {:?}", car.hash));
    }
    // The fields are the JSON values, not something the parser made up.
    if v["kid"] != car.kid.as_str() || v["hash"] != car.hash.as_str() || v["sig"] != car.sig.as_str() || v["signed_at"] != car.signed_at.as_str() || v["roster_epoch"].as_u64() != Some(car.roster_epoch) {
        return Some("accepted fields that differ from the JSON".into());
    }
    None
}

struct Case {
    kind: Kind,
    id: &'static str,
    record: Vec<u8>,
    sidecar: Vec<u8>,
}

fn corpus() -> Vec<Case> {
    let who = author();
    [(Kind::Memory, ID, memory_text()), (Kind::Endorsement, OTHER_ID, endorsement_text()), (Kind::Tombstone, OTHER_ID, tombstone_text())]
        .into_iter()
        .map(|(kind, id, text)| {
            let record = text.into_bytes();
            let sidecar = sidecar_for(&who, kind, id, &record);
            Case { kind, id, record, sidecar }
        })
        .collect()
}

fn original(c: &Case) -> Sidecar {
    Sidecar::parse(&c.sidecar).expect("premise: the corpus sidecar parses")
}

#[test]
fn the_sidecar_parser_is_total_sound_and_round_trips_over_forty_thousand_mutants() {
    let cases = corpus();
    let sidecars: Vec<Vec<u8>> = cases.iter().map(|c| c.sidecar.clone()).collect();
    let dict = dictionary();
    for c in &cases {
        assert!(verify_record(ORG, STORE, &path(c.kind, c.id), &c.record, &c.sidecar).is_ok(), "premise: the unmutated pair verifies");
    }
    let (mut accepted, mut rejected, mut forged) = (0usize, 0usize, 0usize);
    for seed in 1..=8u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for iteration in 0..5000 {
            let c = &cases[rng.below(cases.len())];
            let bytes = mutate(&mut rng, &c.sidecar, &sidecars, &dict);
            let verdict = Sidecar::parse(&bytes);
            assert_eq!(verdict, Sidecar::parse(&bytes), "deterministic");
            match verdict {
                Ok(car) => {
                    accepted += 1;
                    if let Some(why) = oracle_violation(&bytes, &car) {
                        panic!("seed {seed} iteration {iteration}: {why}\ninput: {:?}", String::from_utf8_lossy(&bytes));
                    }
                    assert_eq!(Sidecar::parse(car.to_json().as_bytes()), Ok(car.clone()), "seed {seed} iteration {iteration}: round trip");
                    // No forgery: it verifies only as the one signature that exists.
                    if verify_record(ORG, STORE, &path(c.kind, c.id), &c.record, &bytes).is_ok() {
                        forged += 1;
                        assert_eq!(car, original(c), "seed {seed} iteration {iteration}: verified fields that nobody signed");
                    }
                }
                Err(_) => rejected += 1,
            }
        }
    }
    eprintln!("sidecar fuzz: accepted {accepted}, rejected {rejected}, of the accepted verified (all equal to the signed fields) {forged}");
    assert!(accepted > 1500, "the oracle is only exercised if mutants are sometimes accepted (got {accepted})");
    assert!(rejected > 10_000);
    assert!(forged > 100, "the no-forgery check needs mutants that still verify (got {forged})");
}

#[test]
fn a_mutated_record_or_sidecar_never_reaches_the_yaml_parser_unless_steps_one_to_three_pass() {
    let cases = corpus();
    let sidecars: Vec<Vec<u8>> = cases.iter().map(|c| c.sidecar.clone()).collect();
    let records: Vec<Vec<u8>> = cases.iter().map(|c| c.record.clone()).collect();
    let dict = dictionary();
    let (mut reached, mut stopped_early, mut unsigned_but_parseable) = (0usize, 0usize, 0usize);
    for seed in 1..=8u64 {
        let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
        for iteration in 0..1500 {
            let c = &cases[rng.below(cases.len())];
            let (rec, car) = match rng.below(5) {
                0 => (c.record.clone(), mutate(&mut rng, &c.sidecar, &sidecars, &dict)),
                1 => (mutate(&mut rng, &c.record, &records, &dict), c.sidecar.clone()),
                2 => (c.record.clone(), c.sidecar.clone()),
                3 => {
                    // Oversize, validly signed: the size check comes before everything.
                    let mut big = c.record.clone();
                    big.resize(MAX_RECORD_BYTES + 1 + rng.below(64), b' ');
                    let car = sidecar_for(&author(), c.kind, c.id, &big);
                    (big, car)
                }
                _ => (mutate(&mut rng, &c.record, &records, &dict), mutate(&mut rng, &c.sidecar, &sidecars, &dict)),
            };
            let active = rng.below(4) != 0;
            let standing = move |_: &Sidecar| if active { Ok(()) } else { Err("not-active") };
            let steps_1_to_3 = verify_record(ORG, STORE, &path(c.kind, c.id), &rec, &car).is_ok() && active;
            let before = frontmatter::parse_count();
            let verdict = validate_record(pinned(), &path(c.kind, c.id), &rec, &car, &standing);
            let ran = frontmatter::parse_count() - before;
            if steps_1_to_3 {
                reached += 1;
                assert_eq!(ran, 1, "seed {seed} iteration {iteration}: the parser runs exactly once after steps 1-3");
            } else {
                stopped_early += 1;
                assert_eq!(ran, 0, "seed {seed} iteration {iteration}: the YAML parser ran although steps 1-3 failed ({verdict:?})");
                assert!(verdict.is_err());
                if std::str::from_utf8(&rec).is_ok_and(|t| t.starts_with("---\n")) && hash::of(&rec) != hash::of(&c.record) {
                    unsigned_but_parseable += 1;
                }
            }
        }
    }
    eprintln!("verify-before-parse fuzz: reached the parser {reached}, stopped before it {stopped_early} (of those, record text that would have parsed: {unsigned_but_parseable})");
    assert!(reached > 500 && stopped_early > 5000);
    assert!(unsigned_but_parseable > 100, "the test must include inputs a lenient pipeline would have parsed");
}
