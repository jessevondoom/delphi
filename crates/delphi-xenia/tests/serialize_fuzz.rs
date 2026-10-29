//! The record serializer against X2's parser (milestone X7).
//!
//! Three properties, over seeded pseudo-random records (no crates, no clock):
//!
//! 1. **Everything emitted validates.** A record built from strings the spec allows is accepted by
//!    `serialize`, and the bytes pass the whole of `validate_record` (signature included), parse back to the
//!    same `Record`, and serialize to the same bytes again (one spelling).
//! 2. **Everything refused is refused for a stated reason.** A record with exactly one defect is refused by
//!    `serialize` with the reason the defect deserves, never accepted and never a panic.
//! 3. **The check can fail.** Writing the bytes by hand with the same defect gets the same verdict from the
//!    verifier, so the serializer's refusals are not stricter or looser than the verifier's.

mod support;

use delphi_xenia::layout::Kind;
use delphi_xenia::limits::*;
use delphi_xenia::pointer::{ContentType, Pin, PinKind, Pointer, Scheme};
use delphi_xenia::record::*;
use delphi_xenia::serialize::{self, SerializeError};
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
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

/// Pieces that are legal in a one-line plain-text field but awkward for a serializer: the characters the
/// frontmatter syntax cares about, the empty-looking words, and non-ASCII text that is already NFC.
const AWKWARD: [&str; 26] = [
    "plain words", "a \"quoted\" word", "back\\slash", "ends with a backslash\\", "colon: space", "hash # sign", "[brackets]", "a, b, c", "&ampersand", "*star", "!bang", "|pipe", ">gt", "%percent",
    "@at", "`tick`", "'single'", "null", "true", "123", "- dash", "caf\u{e9}", "\u{65e5}\u{672c}\u{8a9e}", "emoji \u{1f600}", "{braces}", "tilde ~",
];

fn text(rng: &mut Rng, max_chars: usize) -> String {
    let mut s = String::new();
    for _ in 0..1 + rng.below(4) {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(rng.pick(&AWKWARD));
    }
    let s: String = s.chars().take(max_chars).collect();
    // Plain-text fields may not start or end with a space, and must not read as a Markdown link or HTML.
    let s = s.trim().to_string();
    if s.is_empty() { "x".into() } else { s }
}

fn slug(rng: &mut Rng) -> String {
    let n = 1 + rng.below(12);
    (0..n).map(|_| *rng.pick(&['a', 'b', 'c', 'x', 'y', '0', '9', '-'])).collect::<String>().trim_matches('-').to_string() + "k"
}

fn tags(rng: &mut Rng) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for _ in 0..rng.below(MAX_TAGS + 1) {
        let t = if rng.below(8) == 0 { "null".to_string() } else { slug(rng) };
        if !v.contains(&t) {
            v.push(t);
        }
    }
    v
}

fn pointer(rng: &mut Rng, i: usize) -> Pointer {
    let label = text(rng, MAX_LABEL_CHARS);
    let access_hint = (rng.below(2) == 0).then(|| text(rng, MAX_ACCESS_HINT_CHARS));
    match rng.below(3) {
        0 => Pointer {
            scheme: Scheme::Git,
            locator: format!("https://github.com/acme/repo{i}.git#docs/notes{i}.md"),
            pin: Pin { kind: PinKind::GitCommit, value: Some(SHA.into()) },
            hash: None,
            range: (rng.below(2) == 0).then(|| (3, 40 + rng.below(50) as u64)),
            label,
            content_type: ContentType::Markdown,
            bytes: (rng.below(2) == 0).then(|| rng.below(100_000) as u64),
            access_hint,
        },
        1 => Pointer {
            scheme: Scheme::Https,
            locator: format!("https://docs.acme.example/page{i}"),
            pin: Pin { kind: PinKind::None, value: None },
            hash: None,
            range: None,
            label,
            content_type: ContentType::Pdf,
            bytes: None,
            access_hint,
        },
        _ => Pointer {
            scheme: Scheme::Xenia,
            locator: format!("xenia://memory/01JBX3Q8T2M9Z4K7WYH5N6D0R{}", ["B", "C", "D", "E"][i % 4]),
            pin: Pin { kind: PinKind::None, value: None },
            hash: None,
            range: None,
            label,
            content_type: ContentType::Other,
            bytes: None,
            access_hint,
        },
    }
}

fn pointers(rng: &mut Rng, max: usize) -> Vec<Pointer> {
    (0..rng.below(max + 1)).map(|i| pointer(rng, i)).collect()
}

fn common(rng: &mut Rng, kind_fields: KindFields, body: String) -> Record {
    let none = body.is_empty();
    Record {
        id: ID.into(),
        rev: 1 + rng.below(50) as u64,
        org: ORG.into(),
        store: STORE.into(),
        author: author_did().into(),
        author_handle: ["jlee", "m.smith", "a_b-c"][rng.below(3)].into(),
        author_role: Role::Builder,
        title: text(rng, MAX_TITLE_CHARS),
        summary: text(rng, MAX_SUMMARY_CHARS),
        tags: tags(rng),
        created: "2026-10-24T09:00:00Z".into(),
        updated: "2026-10-25T09:00:00Z".into(),
        source_ceiling: SourceCeiling::InternalEmpirical,
        confidence: Confidence::Medium,
        status: AuthorStatus::Active,
        supersedes: (rng.below(3) == 0).then(|| (OTHER_ID.into(), 1 + rng.below(5) as u64)),
        review_by: Some("2027-01-24".into()),
        expires: (rng.below(2) == 0).then(|| "2027-06-24".into()),
        pointers: pointers(rng, MAX_POINTERS),
        evidence: pointers(rng, MAX_EVIDENCE),
        related: (0..rng.below(3)).map(|i| format!("01JBX3Q8T2M9Z4K7WYH5N6D0R{}", ["F", "G", "H"][i])).collect(),
        redaction: Redaction { scanner: "rev-sec-05".into(), scanner_version: "1".into(), content_hash: delphi_xenia::hash::of(b"x") },
        body_kind: if none { BodyKind::None } else { BodyKind::Markdown },
        kind_fields,
        body,
    }
}

fn memory(rng: &mut Rng) -> Record {
    let item = |rng: &mut Rng| -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for _ in 0..rng.below(4) {
            let s = slug(rng);
            if !v.contains(&s) {
                v.push(s);
            }
        }
        v
    };
    let mut repos = item(rng);
    if repos.is_empty() {
        repos.push("payments".into());
    }
    let mut tech = item(rng);
    if tech.is_empty() {
        tech.push("stripe".into());
    }
    let paths = if rng.below(2) == 0 { vec![] } else { vec!["src/webhooks/*".to_string()] };
    let fields = KindFields::Memory(MemoryFields { memory_type: MemoryType::Gotcha, applies_to: AppliesTo { repos, tech, paths }, verified_by: VerifiedBy::Test });
    let body = if rng.below(4) == 0 { String::new() } else { format!("{}\n{}\n", text(rng, 200), text(rng, 200)) };
    let mut r = common(rng, fields, body);
    r.review_by = Some("2027-01-24".into());
    r
}

fn endorsement(rng: &mut Rng) -> Record {
    let verdict = *rng.pick(&[Verdict::Endorse, Verdict::Dispute, Verdict::Withdraw]);
    let scope = if verdict == Verdict::Endorse { vec![slug(rng)] } else { vec![] };
    let note = (verdict == Verdict::Dispute || rng.below(2) == 0).then(|| text(rng, MAX_ENDORSEMENT_NOTE_CHARS));
    let fields = KindFields::Endorsement(EndorsementFields {
        target_id: OTHER_ID.into(),
        target_rev: 1 + rng.below(9) as u64,
        target_hash: delphi_xenia::hash::of(b"target"),
        verdict,
        scope,
        note,
        checked: vec![Check::TargetOpen, Check::SummaryMatches],
    });
    let mut r = common(rng, fields, String::new());
    r.id = "01JBX3Q8T2M9Z4K7WYH5N6D0RC".into();
    r.supersedes = None;
    r.review_by = None;
    r.pointers.clear();
    r.evidence.clear();
    r
}

fn tombstone(rng: &mut Rng) -> Record {
    let fields = KindFields::Tombstone(TombstoneFields {
        target: OTHER_ID.into(),
        action: *rng.pick(&[TombstoneAction::Withdraw, TombstoneAction::Restore]),
        reason: *rng.pick(&[TombstoneReason::Mistake, TombstoneReason::Sensitive, TombstoneReason::Other]),
        note: (rng.below(2) == 0).then(|| text(rng, MAX_TOMBSTONE_NOTE_CHARS)),
    });
    let mut r = common(rng, fields, String::new());
    r.id = "01JBX3Q8T2M9Z4K7WYH5N6D0RD".into();
    r.supersedes = None;
    r.review_by = None;
    r
}

/// The whole verifier on the serialized bytes, signed by the standard author.
fn verify(r: &Record, bytes: &[u8]) -> Result<Validated, Reject> {
    validate_raw(&author(), r.kind(), &r.id, bytes)
}

#[test]
fn everything_the_serializer_emits_validates_round_trips_and_has_one_spelling() {
    let mut rng = Rng(0x5eed_0007);
    let mut by_kind = [0usize; 3];
    for n in 0..1500 {
        let r = match n % 3 {
            0 => memory(&mut rng),
            1 => endorsement(&mut rng),
            _ => tombstone(&mut rng),
        };
        let (r, bytes) = match serialize::seal(&r) {
            Ok(b) => b,
            // A generated record can exceed the 16 KiB file limit with many long pointers; that is a stated refusal.
            Err(SerializeError::TooLarge) => {
                assert!(r.pointers.len() + r.evidence.len() > 6, "a small record must not be too large: {n}");
                continue;
            }
            Err(e) => panic!("record {n} was refused: {e}\n{r:#?}"),
        };
        let v = verify(&r, &bytes).unwrap_or_else(|e| panic!("record {n}: the verifier refused the serializer's bytes: {}\n{}", e.reason(), String::from_utf8_lossy(&bytes)));
        assert_eq!(v.record(), &r, "record {n} did not round-trip");
        assert_eq!(serialize::serialize(v.record()).unwrap(), bytes, "record {n}: the spelling is not unique");
        by_kind[match r.kind() {
            Kind::Memory => 0,
            Kind::Endorsement => 1,
            Kind::Tombstone => 2,
            Kind::Bundle => 2,
        }] += 1;
    }
    assert!(by_kind.iter().all(|n| *n > 300), "every kind is exercised: {by_kind:?}");
}

/// One defect, the reason class `serialize` must give, and (where the defect can be written by hand) the
/// reason the verifier gives for the same bytes.
struct Defect {
    name: &'static str,
    apply: fn(&mut Record),
    expect: &'static str,
    verifier: Option<&'static str>,
}

fn base() -> Record {
    serialize::seal(&memory(&mut Rng(0xb45e))).unwrap().0
}

#[test]
fn a_record_with_one_defect_is_refused_for_a_stated_reason() {
    let defects: Vec<Defect> = vec![
        Defect { name: "newline in title", apply: |r| r.title = "two\nlines".into(), expect: "unrepresentable", verifier: None },
        Defect { name: "control in summary", apply: |r| r.summary = "bell\u{7}".into(), expect: "unrepresentable", verifier: None },
        Defect { name: "carriage return in a label", apply: |r| { if let Some(p) = r.pointers.first_mut() { p.label = "a\rb".into() } else { r.title = "a\rb".into() } }, expect: "unrepresentable", verifier: None },
        Defect { name: "bidi override in title", apply: |r| r.title = "evil \u{202e}text".into(), expect: "invalid", verifier: Some("bad-frontmatter") },
        Defect { name: "zero width space in summary", apply: |r| r.summary = "zero\u{200b}width".into(), expect: "invalid", verifier: Some("bad-frontmatter") },
        Defect { name: "tag character in title", apply: |r| r.title = "tag\u{e0041}".into(), expect: "invalid", verifier: Some("bad-frontmatter") },
        Defect { name: "not NFC", apply: |r| r.title = "cafe\u{301}".into(), expect: "invalid", verifier: Some("bad-frontmatter") },
        Defect { name: "leading space", apply: |r| r.title = " lead".into(), expect: "invalid", verifier: Some("bad-text") },
        Defect { name: "markdown link in summary", apply: |r| r.summary = "see [x](http://y)".into(), expect: "invalid", verifier: Some("bad-text") },
        Defect { name: "html in title", apply: |r| r.title = "a <b>bold</b> claim".into(), expect: "invalid", verifier: Some("bad-text") },
        Defect { name: "title too long", apply: |r| r.title = "t".repeat(MAX_TITLE_CHARS + 1), expect: "invalid", verifier: Some("over-limit") },
        Defect { name: "summary too long", apply: |r| r.summary = "s".repeat(MAX_SUMMARY_CHARS + 1), expect: "invalid", verifier: Some("over-limit") },
        Defect { name: "too many tags", apply: |r| r.tags = (0..=MAX_TAGS).map(|i| format!("t{i}")).collect(), expect: "invalid", verifier: Some("over-limit") },
        Defect { name: "duplicate tag", apply: |r| r.tags = vec!["a".into(), "a".into()], expect: "invalid", verifier: Some("bad-schema") },
        Defect { name: "tag with an underscore", apply: |r| r.tags = vec!["Bad_Tag".into()], expect: "invalid", verifier: Some("bad-schema") },
        Defect { name: "empty title", apply: |r| r.title = String::new(), expect: "invalid", verifier: Some("bad-schema") },
        Defect { name: "rev zero", apply: |r| r.rev = 0, expect: "invalid", verifier: Some("bad-schema") },
        Defect { name: "id is not a ULID", apply: |r| r.id = "not-a-ulid".into(), expect: "invalid", verifier: None },
        Defect { name: "author is not a did", apply: |r| r.author = "someone".into(), expect: "invalid", verifier: None },
        Defect { name: "handle with a space", apply: |r| r.author_handle = "j lee".into(), expect: "invalid", verifier: None },
        Defect { name: "updated before created", apply: |r| r.updated = "2026-10-23T09:00:00Z".into(), expect: "invalid", verifier: Some("bad-date") },
        Defect { name: "February 30th", apply: |r| r.created = "2026-02-30T09:00:00Z".into(), expect: "invalid", verifier: Some("bad-date") },
        Defect { name: "review_by missing on a memory", apply: |r| r.review_by = None, expect: "invalid", verifier: Some("bad-schema") },
        Defect { name: "body_kind none with a body", apply: |r| { r.body = "x\n".into(); r.body_kind = BodyKind::None }, expect: "invalid", verifier: Some("bad-schema") },
        Defect { name: "body_kind markdown with no body", apply: |r| { r.body = String::new(); r.body_kind = BodyKind::Markdown }, expect: "invalid", verifier: Some("bad-schema") },
        Defect { name: "body over 4 KiB", apply: |r| { r.body = "b".repeat(MAX_BODY_BYTES + 1); r.body_kind = BodyKind::Markdown }, expect: "invalid", verifier: Some("oversize") },
        Defect { name: "body with a bidi control", apply: |r| { r.body = "ok \u{202e} not ok\n".into(); r.body_kind = BodyKind::Markdown }, expect: "invalid", verifier: Some("bad-frontmatter") },
        Defect { name: "body with a carriage return", apply: |r| { r.body = "a\r\nb\n".into(); r.body_kind = BodyKind::Markdown }, expect: "invalid", verifier: Some("bad-frontmatter") },
        Defect { name: "pointer to a local file", apply: |r| r.pointers = vec![bad_pointer(Scheme::Https, "file:///etc/passwd")], expect: "invalid", verifier: Some("bad-pointer") },
        Defect { name: "pointer with userinfo", apply: |r| r.pointers = vec![bad_pointer(Scheme::Https, "https://user:pw@host.example/x")], expect: "invalid", verifier: Some("bad-pointer") },
        Defect { name: "pointer with a signed-URL query", apply: |r| r.pointers = vec![bad_pointer(Scheme::Https, "https://h.example/x?X-Amz-Signature=abc")], expect: "invalid", verifier: Some("bad-pointer") },
        Defect { name: "pointer with a delphi URI", apply: |r| r.pointers = vec![bad_pointer(Scheme::Https, "delphi://cap/abc")], expect: "invalid", verifier: Some("bad-pointer") },
        Defect { name: "drive pointer", apply: |r| { let mut p = bad_pointer(Scheme::Drive, "1AbCdEfGhIjKlMnOpQrStUv"); p.pin = Pin { kind: PinKind::DriveRevision, value: Some("12345".into()) }; r.pointers = vec![p] }, expect: "invalid", verifier: Some("bad-pointer") },
        Defect { name: "nine pointers", apply: |r| r.pointers = (0..=MAX_POINTERS).map(good_pointer).collect(), expect: "invalid", verifier: Some("over-limit") },
        Defect { name: "five evidence pointers", apply: |r| r.evidence = (0..=MAX_EVIDENCE).map(good_pointer).collect(), expect: "invalid", verifier: Some("over-limit") },
        Defect { name: "refers to itself", apply: |r| r.related = vec![r.id.clone()], expect: "invalid", verifier: Some("mismatch") },
        Defect { name: "related is not a ULID", apply: |r| r.related = vec!["nope".into()], expect: "invalid", verifier: Some("bad-schema") },
        Defect { name: "scanner name with a space", apply: |r| r.redaction.scanner = "rev sec".into(), expect: "invalid", verifier: Some("bad-schema") },
        Defect { name: "content hash is not a hash", apply: |r| r.redaction.content_hash = "blake3:zz".into(), expect: "invalid", verifier: Some("bad-schema") },
        Defect { name: "file over 16 KiB", apply: |r| { r.pointers = (0..MAX_POINTERS).map(fat_pointer).collect(); r.evidence = (0..MAX_EVIDENCE).map(fat_pointer).collect(); r.summary = "é".repeat(MAX_SUMMARY_CHARS); r.body = "b".repeat(MAX_BODY_BYTES); r.body_kind = BodyKind::Markdown }, expect: "too-large", verifier: Some("oversize") },
    ];
    // The control: the undamaged base record is accepted, so each refusal below is the defect's doing.
    let ok = serialize::serialize(&base()).expect("the base record serializes");
    assert!(verify(&base(), &ok).is_ok());
    for d in &defects {
        let mut r = base();
        (d.apply)(&mut r);
        match serialize::serialize(&r) {
            Ok(_) => panic!("{}: the serializer accepted it", d.name),
            Err(e) => {
                assert_eq!(e.reason(), d.expect, "{}: wrong reason ({e})", d.name);
                // An `invalid` refusal carries the verifier's own reason for the rule that was broken.
                if let (SerializeError::Invalid(rej), Some(want)) = (&e, d.verifier) {
                    assert_eq!(rej.reason(), want, "{}: the verifier's reason", d.name);
                }
            }
        }
    }
}

fn bad_pointer(scheme: Scheme, locator: &str) -> Pointer {
    Pointer { scheme, locator: locator.into(), pin: Pin { kind: PinKind::None, value: None }, hash: None, range: None, label: "L".into(), content_type: ContentType::Other, bytes: None, access_hint: None }
}

fn good_pointer(i: usize) -> Pointer {
    bad_pointer(Scheme::Https, &format!("https://docs.acme.example/p{i}"))
}

fn fat_pointer(i: usize) -> Pointer {
    let mut p = bad_pointer(Scheme::Https, &format!("https://docs.acme.example/{}{i}", "p".repeat(480)));
    p.label = "\u{e9}".repeat(MAX_LABEL_CHARS);
    p.access_hint = Some("\u{e9}".repeat(MAX_ACCESS_HINT_CHARS));
    p
}

#[test]
fn the_verifier_agrees_with_every_refusal_that_can_be_written_by_hand() {
    // For each defect that has a hand-writable form, take the serializer's bytes for the *undamaged* record
    // and apply the same edit as text: the verifier must refuse with the stated reason. This is what shows the
    // serializer is not simply stricter than the verifier.
    let r = base();
    let good = String::from_utf8(serialize::serialize(&r).unwrap()).unwrap();
    let swap = |from: &str, to: &str| -> String {
        assert!(good.contains(from), "fixture lost {from:?}");
        good.replacen(from, to, 1)
    };
    let title_line = format!("title: {}\n", q(&r.title));
    let cases: Vec<(&str, String, &str)> = vec![
        ("bidi in the title", swap(&title_line, "title: \"evil \u{202e}text\"\n"), "bad-frontmatter"),
        ("leading space in the title", swap(&title_line, "title: \" lead\"\n"), "bad-text"),
        ("rev zero", swap(&format!("rev: {}\n", r.rev), "rev: 0\n"), "bad-schema"),
        ("a trust key", swap("visibility: org\n", "visibility: org\ntier: L2\n"), "forbidden-key"),
        ("updated before created", swap("updated: \"2026-10-25T09:00:00Z\"", "updated: \"2026-10-23T09:00:00Z\""), "bad-date"),
        ("an unquoted tab", swap(&title_line, "title: a\tb\n"), "bad-frontmatter"),
    ];
    for (name, text, reason) in cases {
        let got = verify(&r, text.as_bytes()).err().map(|e| e.reason());
        assert_eq!(got, Some(reason), "{name}");
    }
}

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[test]
fn a_sealed_record_carries_the_recipe_hash_of_its_own_bytes() {
    let mut rng = Rng(0xfeed);
    for _ in 0..200 {
        let r = memory(&mut rng);
        let Ok((sealed, bytes)) = serialize::seal(&r) else { continue };
        assert_eq!(delphi_xenia::redaction::content_hash(&bytes).unwrap(), sealed.redaction.content_hash);
        assert!(verify(&sealed, &bytes).is_ok());
        // Sealing twice changes nothing.
        let (again, bytes2) = serialize::seal(&sealed).unwrap();
        assert_eq!(again, sealed);
        assert_eq!(bytes2, bytes);
    }
}
