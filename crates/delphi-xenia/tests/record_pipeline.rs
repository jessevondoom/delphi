//! Spec 012 §10 tests 1-5, 10, 12, 18 and 20 against the whole validation pipeline, plus the limit
//! boundaries (both sides of each). Every rejection test also has a positive control: the same record,
//! minus the one change, is accepted, so a test cannot pass merely because the fixture is broken.

mod support;

use delphi_xenia::frontmatter;
use delphi_xenia::layout::Kind;
use delphi_xenia::limits::*;
use delphi_xenia::record::*;
use delphi_xenia::{did, hash, Error};
use support::*;

fn assert_rejects(text: &str, want: &str) {
    assert_eq!(memory_reason(text), want, "{text}");
}

#[test]
fn one_valid_record_of_each_kind_is_accepted_with_its_fields() {
    let m = validate(Kind::Memory, ID, memory_text().as_bytes()).unwrap();
    let r = m.record();
    assert_eq!((r.id.as_str(), r.rev, r.kind()), (ID, 3, Kind::Memory));
    assert_eq!(r.pointers.len(), 1);
    assert_eq!(r.pointers[0].range, Some((40, 120)));
    assert_eq!(r.review_by.as_deref(), Some("2027-01-24"));
    assert_eq!(r.body, "Handlers must dedupe on the event id.\n");
    assert_eq!(m.sidecar().kid, author().did());
    let KindFields::Memory(f) = &r.kind_fields else { panic!() };
    assert_eq!(f.applies_to.tech, ["stripe-webhooks", "postgres"]);

    let e = validate(Kind::Endorsement, OTHER_ID, endorsement_text().as_bytes()).unwrap();
    let KindFields::Endorsement(f) = &e.record().kind_fields else { panic!() };
    assert_eq!((f.target_id.as_str(), f.target_rev, f.verdict), (ID, 3, Verdict::Endorse));
    assert_eq!(f.checked, [Check::TargetOpen, Check::SummaryMatches]);

    let t = validate(Kind::Tombstone, OTHER_ID, tombstone_text().as_bytes()).unwrap();
    let KindFields::Tombstone(f) = &t.record().kind_fields else { panic!() };
    assert_eq!((f.target.as_str(), f.action, f.reason), (ID, TombstoneAction::Withdraw, TombstoneReason::Mistake));
}

// ---- spec 012 test 2: strict schema -------------------------------------------------------------------------

#[test]
fn unknown_duplicate_and_yaml_feature_keys_are_rejected_with_specific_reasons() {
    let t = memory_text();
    assert_rejects(&swap(&t, "verified_by: test\n", "verified_by: test\nextra: 1\n"), "bad-schema");
    assert_rejects(&swap(&t, "rev: 3\n", "rev: 3\nrev: 4\n"), "bad-frontmatter");
    assert_rejects(&swap(&t, "org: acme\n", "org: &o acme\n"), "bad-frontmatter");
    assert_rejects(&swap(&t, "visibility: org\n", "visibility: *o\n"), "bad-frontmatter");
    assert_rejects(&swap(&t, "visibility: org\n", "<<: *base\nvisibility: org\n"), "bad-frontmatter");
    assert_rejects(&swap(&t, "visibility: org\n", "visibility: !!str org\n"), "bad-frontmatter");
    assert_rejects(&swap(&t, "title: Stripe", "title: \u{202E}Stripe"), "bad-frontmatter");
    // A key of another kind is unknown to this one.
    assert_rejects(&swap(&t, "verified_by: test\n", "verified_by: test\nverdict: endorse\n"), "bad-schema");
    // Required keys are required.
    assert_rejects(&swap(&t, "body_kind: markdown\n", ""), "bad-schema");
    assert_rejects(&swap(&t, "review_by: \"2027-01-24\"\n", ""), "bad-schema");
    assert_rejects(&swap(&t, "pointers:", "pointerz:"), "bad-schema");
    assert_eq!(memory_reason(&t), "accepted");
}

#[test]
fn each_trust_key_is_rejected_at_the_top_level_and_in_every_nested_map() {
    let t = memory_text();
    for key in TRUST_KEYS {
        for (anchor, with) in [
            ("verified_by: test\n", format!("verified_by: test\n{key}: 1\n")),
            ("      kind: git-commit\n", format!("      kind: git-commit\n      {key}: 1\n")),
            ("    label: Webhook handler notes\n", format!("    label: Webhook handler notes\n    {key}: 1\n")),
            ("  result: passed\n", format!("  result: passed\n  {key}: 1\n")),
            ("  repos: [payments]\n", format!("  repos: [payments]\n  {key}: [x]\n")),
        ] {
            assert_rejects(&swap(&t, anchor, &with), "forbidden-key");
        }
    }
    // The same list in the endorsement and tombstone kinds.
    for key in TRUST_KEYS {
        let e = swap(&endorsement_text(), "verdict: endorse\n", &format!("verdict: endorse\n{key}: user_ratified\n"));
        assert_eq!(reason(validate(Kind::Endorsement, OTHER_ID, e.as_bytes())), "forbidden-key");
        let t = swap(&tombstone_text(), "action: withdraw\n", &format!("action: withdraw\n{key}: 1\n"));
        assert_eq!(reason(validate(Kind::Tombstone, OTHER_ID, t.as_bytes())), "forbidden-key");
    }
    // A trust word as a VALUE (a tag) is just text: only keys state a tier.
    assert_eq!(memory_reason(&swap(&t, "tags: [payments, stripe-webhooks]", "tags: [tier, trust]")), "accepted");
}

// ---- spec 012 test 3: detached signature --------------------------------------------------------------------

#[test]
fn a_changed_byte_a_changed_sidecar_hash_or_a_foreign_kid_each_fail() {
    let text = memory_text();
    let bytes = text.as_bytes();
    let who = author();
    let car = sidecar_for(&who, Kind::Memory, ID, bytes);
    let p = path(Kind::Memory, ID);
    let run = |b: &[u8], c: &[u8]| validate_record(pinned(), &p, b, c, &|_| Ok(())).map(|_| ()).map_err(|e| e.reason());
    assert_eq!(run(bytes, &car), Ok(()));
    let mut flipped = bytes.to_vec();
    flipped[200] ^= 1;
    assert_eq!(run(&flipped, &car), Err("hash-mismatch"));
    let sidecar_text = String::from_utf8(car.clone()).unwrap();
    let h = hash::of(bytes);
    let other_hash = hash::of(b"other bytes");
    assert_eq!(run(bytes, swap(&sidecar_text, &h, &other_hash).as_bytes()), Err("hash-mismatch"));
    // The hash matches the new bytes but the signature was made over the old hash.
    let edited = text.replace("rev: 3", "rev: 4");
    let forged = swap(&sidecar_text, &h, &hash::of(edited.as_bytes()));
    assert_eq!(run(edited.as_bytes(), forged.as_bytes()), Err("bad-signature"));
    // A different key signs correctly for itself, but `author` is not that key.
    let mallory = delphi_xenia::identity::Identity::from_seed([1u8; 32]);
    assert_eq!(reason(validate_signed_by(&mallory, Kind::Memory, ID, bytes)), "mismatch");
}

// ---- spec 012 test 4: domain binding ------------------------------------------------------------------------

#[test]
fn a_record_copied_renamed_moved_or_presented_under_another_key_fails_signature_verification() {
    let text = memory_text();
    let bytes = text.as_bytes();
    let car = sidecar_for(&author(), Kind::Memory, ID, bytes);
    let go = |pin: Pinned<'_>, p: &str, c: &[u8]| validate_record(pin, p, bytes, c, &|_| Ok(())).map(|_| ()).map_err(|e| e.reason());
    assert_eq!(go(pinned(), &path(Kind::Memory, ID), &car), Ok(()));
    assert_eq!(go(Pinned { org: ORG, store: "acme-other" }, &path(Kind::Memory, ID), &car), Err("bad-signature"));
    assert_eq!(go(Pinned { org: "other", store: STORE }, &path(Kind::Memory, ID), &car), Err("bad-signature"));
    assert_eq!(go(pinned(), &path(Kind::Memory, OTHER_ID), &car), Err("bad-signature"));
    assert_eq!(go(pinned(), &path(Kind::Tombstone, ID), &car), Err("bad-signature"));
    // Presented under another kid: swap the kid in the sidecar and the signature no longer verifies.
    let other = delphi_xenia::identity::Identity::from_seed([2u8; 32]).did();
    let swapped = swap(&String::from_utf8(car.clone()).unwrap(), &author().did(), &other);
    assert_eq!(go(pinned(), &path(Kind::Memory, ID), swapped.as_bytes()), Err("bad-signature"));
}

// ---- spec 012 test 5: path/id -------------------------------------------------------------------------------

#[test]
fn a_file_at_b_whose_frontmatter_id_is_a_is_rejected_even_with_a_valid_signature_for_b() {
    let a_text = memory_text();
    assert_eq!(reason(validate(Kind::Memory, ID, a_text.as_bytes())), "accepted");
    // The same bytes, signed correctly for B's path: signature fine, id disagrees with the file name.
    assert_eq!(validate(Kind::Memory, OTHER_ID, a_text.as_bytes()), Err(Reject::Mismatch("id differs from the file name")));
    // kind: memory presented from the tombstone directory.
    assert_eq!(validate(Kind::Tombstone, ID, a_text.as_bytes()), Err(Reject::Mismatch("kind differs from the directory")));
    // org and store claims are checked against the pinned values.
    for (from, to, why) in [("org: acme\n", "org: evil\n", "org differs from the pinned org"), ("store: acme-main\n", "store: evil\n", "store differs from the pinned store")] {
        assert_eq!(validate(Kind::Memory, ID, swap(&a_text, from, to).as_bytes()), Err(Reject::Mismatch(why)));
    }
    // A record that supersedes, relates to, or targets itself.
    assert_eq!(validate(Kind::Memory, ID, swap(&a_text, "supersedes: null", &format!("supersedes: \"{ID}@2\"")).as_bytes()), Err(Reject::Mismatch("a record refers to itself")));
    assert_eq!(validate(Kind::Memory, ID, swap(&a_text, "related: []", &format!("related: [{ID}]")).as_bytes()), Err(Reject::Mismatch("a record refers to itself")));
    let e = swap(&endorsement_text(), &format!("target: \"{ID}@3\""), &format!("target: \"{OTHER_ID}@3\""));
    assert_eq!(validate(Kind::Endorsement, OTHER_ID, e.as_bytes()), Err(Reject::Mismatch("a record refers to itself")));
}

// ---- spec 012 test 10: pointer rules ------------------------------------------------------------------------

#[test]
fn unpublishable_pointers_are_rejected_at_ingest_by_the_whole_pipeline() {
    let t = memory_text();
    let loc = "locator: \"https://github.com/acme/payments.git#docs/webhooks.md\"\n";
    for bad_locator in [
        "file:///etc/passwd#x", "/etc/passwd", "../../etc/passwd", "docs/notes.md", "delphi://cap/AbCd", "https://user:pw@github.com/acme/p.git#a.md",
        "https://github.com/acme/p.git?token=abc#a.md", "ssh://git@github.com/acme/p.git#a.md",
    ] {
        assert_rejects(&swap(&t, loc, &format!("locator: \"{bad_locator}\"\n")), "bad-pointer");
    }
    assert_rejects(&swap(&t, "locator: \"https://github.com/acme/payments.git#docs/webhooks.md\"\n    pin:\n      kind: git-commit", "locator: \"https://docs.acme.example/x?X-Amz-Signature=abc\"\n    pin:\n      kind: git-commit").replace("scheme: git", "scheme: https"), "bad-pointer");
    // A scheme outside the allowlist, and `drive` (accepted by the schema, refused in Phase 1).
    assert_rejects(&swap(&t, "scheme: git\n", "scheme: file\n"), "bad-schema");
    let drive = swap(&swap(&t, "scheme: git\n", "scheme: drive\n"), loc.trim_end(), "locator: 1AbCdEfGhIjKlMnOpQrStUv").replace("kind: git-commit", "kind: drive-revision").replace(&format!("\"{SHA}\""), "\"12345\"");
    assert_eq!(validate(Kind::Memory, ID, drive.as_bytes()), Err(Reject::Pointer(frontmatter::bad("scheme", "drive pointers are not publishable in Phase 1"))));
    // Evidence is held to the same rules as pointers.
    let evidence = "evidence:\n  - scheme: https\n    locator: \"http://insecure.example/x\"\n    pin:\n      kind: none\n    label: L\n    content_type: text/plain\n";
    assert_rejects(&swap(&t, "evidence: []\n", evidence), "bad-pointer");
    assert_rejects(&swap(&t, "evidence: []\n", &evidence.replace("http://", "https://")), "accepted");
}

// ---- spec 012 test 12: verify before parse ------------------------------------------------------------------

#[test]
fn the_yaml_parser_never_runs_for_an_oversized_file_a_bad_signature_or_a_non_active_signer() {
    let text = memory_text();
    let bytes = text.as_bytes();
    let who = author();
    let p = path(Kind::Memory, ID);
    let car = sidecar_for(&who, Kind::Memory, ID, bytes);
    let before = frontmatter::parse_count();
    let run = |b: &[u8], c: &[u8], standing: &dyn Fn(&delphi_xenia::sidecar::Sidecar) -> Result<(), &'static str>| validate_record(pinned(), &p, b, c, standing);

    // Oversized: 16 KiB + 1 byte of otherwise well-formed record.
    let big = format!("{text}{}", "x".repeat(MAX_RECORD_BYTES + 1 - text.len()));
    assert_eq!(big.len(), MAX_RECORD_BYTES + 1);
    assert!(matches!(run(big.as_bytes(), &sidecar_for(&who, Kind::Memory, ID, big.as_bytes()), &|_| Ok(())), Err(Reject::Verify(Error::Oversize(_)))));
    // A bad signature, a bad hash, a bad sidecar, a bad path.
    let mut sig_bad = String::from_utf8(car.clone()).unwrap();
    let at = sig_bad.find("\"sig\": \"").unwrap() + 8;
    let flipped = if &sig_bad[at..at + 1] == "A" { "B" } else { "A" };
    sig_bad.replace_range(at..at + 1, flipped);
    assert_eq!(run(bytes, sig_bad.as_bytes(), &|_| Ok(())).unwrap_err().reason(), "bad-signature");
    assert_eq!(run(b"tampered", &car, &|_| Ok(())).unwrap_err().reason(), "hash-mismatch");
    assert_eq!(run(bytes, b"{}", &|_| Ok(())).unwrap_err().reason(), "bad-sidecar");
    assert_eq!(validate_record(pinned(), "records/memory/not-an-id.md", bytes, &car, &|_| Ok(())).unwrap_err().reason(), "bad-path");
    // A signer who is not active: valid bytes, valid signature, the roster says no.
    assert_eq!(run(bytes, &car, &|_| Err("retired")), Err(Reject::Standing("retired")));
    assert_eq!(frontmatter::parse_count(), before, "no failure at steps 1-3 may reach the parser");

    // Passing steps 1-3 reaches the parser exactly once; a schema failure after that is the schema's reason.
    let broken = swap(&text, "visibility: org\n", "visibility: world\n");
    assert_eq!(reason(validate(Kind::Memory, ID, broken.as_bytes())), "bad-schema");
    assert_eq!(frontmatter::parse_count(), before + 1, "the instrument counts: this is the parser's first call");
    assert!(validate(Kind::Memory, ID, bytes).is_ok());
    assert_eq!(frontmatter::parse_count(), before + 2);
}

// ---- spec 012 test 18: version ------------------------------------------------------------------------------

#[test]
fn a_record_with_xenia_2_is_unsupported_not_an_attack_and_does_not_break_other_records() {
    let v2 = swap(&memory_text(), "xenia: 1\n", "xenia: 2\n");
    assert_eq!(validate(Kind::Memory, ID, v2.as_bytes()), Err(Reject::UnsupportedSchema));
    assert_eq!(Reject::UnsupportedSchema.reason(), "unsupported-schema");
    // Even with a field this client has never heard of, which a v2 schema may add.
    let v2_new = swap(&v2, "verified_by: test\n", "verified_by: test\nnew_v2_field: 1\n");
    assert_eq!(validate(Kind::Memory, ID, v2_new.as_bytes()), Err(Reject::UnsupportedSchema));
    // A v1 neighbour still validates after it.
    assert!(validate(Kind::Memory, ID, memory_text().as_bytes()).is_ok());
    // Versions that are not a version at all are schema errors, not "unsupported".
    for v in ["0", "\"1\"", "01", "1.0", "one", "null", "[1]"] {
        assert_eq!(memory_reason(&swap(&memory_text(), "xenia: 1\n", &format!("xenia: {v}\n"))), "bad-schema", "{v}");
    }
}

// ---- spec 012 test 20: strict signatures reach the pipeline -------------------------------------------------

#[test]
fn a_small_order_key_forgery_never_reaches_the_parser() {
    // The classic identity-key forgery (see sig.rs: the lenient verifier accepts it for every message).
    let identity_point = {
        let mut k = [0u8; 32];
        k[0] = 1;
        k
    };
    let kid = did::encode(&identity_point);
    let text = memory_with_author(&kid);
    let mut sig = [0u8; 64];
    sig[0] = 1;
    use base64::Engine;
    let sig_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sig);
    let car = delphi_xenia::sidecar::Sidecar { kid, signed_at: "2026-10-24T09:00:03Z".into(), roster_epoch: 7, hash: hash::of(text.as_bytes()), sig: sig_b64 }.to_json();
    let before = frontmatter::parse_count();
    let r = validate_record(pinned(), &path(Kind::Memory, ID), text.as_bytes(), car.as_bytes(), &|_| Ok(()));
    assert_eq!(r.unwrap_err().reason(), "bad-signature");
    assert_eq!(frontmatter::parse_count(), before);
}

// ---- limits: both sides of every bound ----------------------------------------------------------------------

fn at_and_over(label: &str, ok: String, over: String, want: &str) {
    assert_eq!(memory_reason(&ok), "accepted", "{label}: at the limit");
    assert_eq!(memory_reason(&over), want, "{label}: one over");
}

#[test]
fn text_limits_count_characters_at_and_over_the_bound() {
    let t = memory_text();
    let title = |n: usize| swap(&t, "title: Stripe webhooks retry for three days", &format!("title: {}", "t".repeat(n)));
    at_and_over("title", title(MAX_TITLE_CHARS), title(MAX_TITLE_CHARS + 1), "over-limit");
    let summary_old = "summary: Deliveries are retried with backoff for up to three days; handlers must be idempotent.";
    let summary = |n: usize| swap(&t, summary_old, &format!("summary: {}", "s".repeat(n)));
    at_and_over("summary", summary(MAX_SUMMARY_CHARS), summary(MAX_SUMMARY_CHARS + 1), "over-limit");
    // Characters, not bytes: 500 two-byte characters pass and 501 do not.
    let wide = |n: usize| swap(&t, summary_old, &format!("summary: {}", "\u{e9}".repeat(n)));
    at_and_over("summary (2-byte chars)", wide(MAX_SUMMARY_CHARS), wide(MAX_SUMMARY_CHARS + 1), "over-limit");
    let label = |n: usize| swap(&t, "label: Webhook handler notes", &format!("label: {}", "l".repeat(n)));
    at_and_over("label", label(MAX_LABEL_CHARS), label(MAX_LABEL_CHARS + 1), "over-limit");
    let hint = |n: usize| swap(&t, "access_hint: payments-eng Drive group", &format!("access_hint: {}", "h".repeat(n)));
    at_and_over("access_hint", hint(MAX_ACCESS_HINT_CHARS), hint(MAX_ACCESS_HINT_CHARS + 1), "over-limit");
    let handle = |n: usize| swap(&t, "author_handle: jlee", &format!("author_handle: {}", "j".repeat(n)));
    at_and_over("author_handle", handle(MAX_HANDLE_CHARS), handle(MAX_HANDLE_CHARS + 1), "over-limit");
    for (max, what) in [(MAX_ENDORSEMENT_NOTE_CHARS, "endorsement"), (MAX_TOMBSTONE_NOTE_CHARS, "tombstone")] {
        let (kind, base, old) = if what == "endorsement" {
            (Kind::Endorsement, endorsement_text(), "checked: [target-open, summary-matches]\n")
        } else {
            (Kind::Tombstone, tombstone_text(), "note: Written against API version 2019.\n")
        };
        let with = |n: usize| {
            let line = format!("note: {}\n", "n".repeat(n));
            if what == "endorsement" { swap(&base, old, &format!("{old}{line}")) } else { swap(&base, old, &line) }
        };
        assert_eq!(reason(validate(kind, OTHER_ID, with(max).as_bytes())), "accepted", "{what} note at the limit");
        assert_eq!(reason(validate(kind, OTHER_ID, with(max + 1).as_bytes())), "over-limit", "{what} note over");
    }
}

#[test]
fn count_limits_at_and_over_the_bound() {
    let t = memory_text();
    let tags = |n: usize| swap(&t, "tags: [payments, stripe-webhooks]", &format!("tags: [{}]", (0..n).map(|i| format!("t{i}")).collect::<Vec<_>>().join(", ")));
    at_and_over("tags", tags(MAX_TAGS), tags(MAX_TAGS + 1), "over-limit");
    let related = |n: usize| {
        let ids: Vec<String> = (0..n).map(|i| format!("01JBX3Q8T2M9Z4K7WYH5N6D1{:02}", i + 10).chars().take(26).collect()).collect();
        swap(&t, "related: []", &format!("related: [{}]", ids.join(", ")))
    };
    at_and_over("related", related(MAX_RELATED), related(MAX_RELATED + 1), "over-limit");
    let pointer = |i: usize| format!("  - scheme: https\n    locator: \"https://docs.acme.example/p{i}\"\n    pin:\n      kind: none\n    label: L{i}\n    content_type: text/plain\n");
    // Rebuild the pointers block from scratch so the count is exact.
    let rebuild = |n: usize| {
        let start = t.find("pointers:\n").unwrap();
        let end = t.find("evidence:").unwrap();
        format!("{}pointers:\n{}{}", &t[..start], (0..n).map(pointer).collect::<String>(), &t[end..])
    };
    at_and_over("pointers", rebuild(MAX_POINTERS), rebuild(MAX_POINTERS + 1), "over-limit");
    let evidence = |n: usize| swap(&t, "evidence: []\n", &format!("evidence:\n{}", (0..n).map(pointer).collect::<String>()));
    at_and_over("evidence", evidence(MAX_EVIDENCE), evidence(MAX_EVIDENCE + 1), "over-limit");
    let list = |n: usize| (0..n).map(|i| format!("r{i}")).collect::<Vec<_>>().join(", ");
    at_and_over("applies_to.repos", swap(&t, "repos: [payments]", &format!("repos: [{}]", list(MAX_APPLIES_ITEMS))), swap(&t, "repos: [payments]", &format!("repos: [{}]", list(MAX_APPLIES_ITEMS + 1))), "over-limit");
    let tech = |n: usize| swap(&t, "tech: [stripe-webhooks, postgres]", &format!("tech: [{}]", list(n)));
    at_and_over("applies_to.tech", tech(MAX_APPLIES_ITEMS), tech(MAX_APPLIES_ITEMS + 1), "over-limit");
    let item = |n: usize| swap(&t, "repos: [payments]", &format!("repos: [{}]", "r".repeat(n)));
    at_and_over("applies_to item length", item(MAX_APPLIES_ITEM_CHARS), item(MAX_APPLIES_ITEM_CHARS + 1), "over-limit");
    // Endorsement scope.
    let scope = |n: usize| swap(&endorsement_text(), "scope: [payments]", &format!("scope: [{}]", (0..n).map(|i| format!("s{i}")).collect::<Vec<_>>().join(", ")));
    assert_eq!(reason(validate(Kind::Endorsement, OTHER_ID, scope(MAX_SCOPE).as_bytes())), "accepted");
    assert_eq!(reason(validate(Kind::Endorsement, OTHER_ID, scope(MAX_SCOPE + 1).as_bytes())), "over-limit");
}

#[test]
fn body_and_file_size_limits_at_and_over_the_bound() {
    let t = memory_text();
    let body = |n: usize| swap(&t, "Handlers must dedupe on the event id.\n", &format!("{}\n", "b".repeat(n - 1)));
    let m = validate(Kind::Memory, ID, body(MAX_BODY_BYTES).as_bytes()).unwrap();
    assert_eq!(m.record().body.len(), MAX_BODY_BYTES);
    assert_eq!(validate(Kind::Memory, ID, body(MAX_BODY_BYTES + 1).as_bytes()), Err(Reject::BodyTooLarge));
    // The file limit is on the raw bytes and comes first (before any parsing).
    let pad = |n: usize| format!("{}{}", t, "x".repeat(n - t.len()));
    let at = pad(MAX_RECORD_BYTES);
    assert_eq!(at.len(), MAX_RECORD_BYTES);
    // At the file limit the file is not "oversize" at step 1; it is too big only for the body bound.
    assert_eq!(validate(Kind::Memory, ID, at.as_bytes()), Err(Reject::BodyTooLarge));
    let over = pad(MAX_RECORD_BYTES + 1);
    assert!(matches!(validate(Kind::Memory, ID, over.as_bytes()), Err(Reject::Verify(Error::Oversize(_)))));
}

#[test]
fn rev_and_date_spellings_and_ranges() {
    let t = memory_text();
    for (rev, ok) in [("1", true), ("1000000000", true), ("1000000001", false), ("0", false), ("03", false), ("\"3\"", false), ("3.0", false), ("0x3", false), ("1e1", false)] {
        let want = if ok { "accepted" } else { "bad-schema" };
        assert_eq!(memory_reason(&swap(&t, "rev: 3\n", &format!("rev: {rev}\n"))), want, "rev {rev}");
    }
    for (field, old, new) in [
        ("created", "created: \"2026-10-24T09:00:00Z\"", "created: \"2026-02-30T09:00:00Z\""),
        ("created", "created: \"2026-10-24T09:00:00Z\"", "created: \"2026-10-24T09:00:00+00:00\""),
        ("review_by", "review_by: \"2027-01-24\"", "review_by: \"2027-02-29\""),
        ("review_by", "review_by: \"2027-01-24\"", "review_by: \"2027-1-24\""),
        ("expires", "expires: \"2027-06-24\"", "expires: \"2027-06-24T00:00:00Z\""),
    ] {
        assert_eq!(memory_reason(&swap(&t, old, new)), "bad-date", "{field}");
    }
    assert_eq!(memory_reason(&swap(&t, "review_by: \"2027-01-24\"", "review_by: \"2028-02-29\"")), "accepted", "2028 is a leap year");
    assert_eq!(memory_reason(&swap(&t, "updated: \"2026-10-24T09:00:00Z\"", "updated: \"2026-10-24T08:59:59Z\"")), "bad-date", "updated before created");
    assert_eq!(memory_reason(&swap(&t, "updated: \"2026-10-24T09:00:00Z\"", "updated: \"2026-10-24T09:00:00Z\"")), "accepted", "updated equal to created");
}

#[test]
fn future_dates_are_a_publish_time_check_with_an_injected_today() {
    let r = validate(Kind::Memory, ID, memory_text().as_bytes()).unwrap();
    let r = r.record();
    r.check_future_dates("2026-10-26").unwrap();
    r.check_future_dates("2027-01-23").unwrap();
    assert!(r.check_future_dates("2027-01-24").is_err(), "review_by today is not the future");
    assert!(r.check_future_dates("2028-01-01").is_err());
    assert!(r.check_future_dates("tomorrow").is_err());
    // Ingest of the same record years later still succeeds: stale is a display state, not a rejection.
    assert!(validate(Kind::Memory, ID, memory_text().as_bytes()).is_ok());
}

#[test]
fn schema_enums_endorsement_and_tombstone_rules() {
    let t = memory_text();
    for (old, new) in [
        ("author_role: builder", "author_role: Builder"),
        ("author_role: builder", "author_role: admin"),
        ("source_ceiling: internal_empirical", "source_ceiling: trusted"),
        ("confidence: medium", "confidence: certain"),
        ("status: active", "status: draft"),
        ("memory_type: gotcha", "memory_type: rumour"),
        ("verified_by: test", "verified_by: endorsed"),
        ("body_kind: markdown", "body_kind: html"),
        ("visibility: org", "visibility: public"),
        ("tags: [payments, stripe-webhooks]", "tags: [Payments]"),
        ("tags: [payments, stripe-webhooks]", "tags: [a, a]"),
        ("kind: memory", "kind: skill"),
        ("id: 01JBX3Q8T2M9Z4K7WYH5N6D0RA", "id: 01jbx3q8t2m9z4k7wyh5n6d0ra"),
        ("supersedes: null", "supersedes: \"abc\""),
        ("supersedes: null", "supersedes: \"01JBX3Q8T2M9Z4K7WYH5N6D0RB@0\""),
        ("author_handle: jlee", "author_handle: \"j lee\""),
    ] {
        assert_eq!(memory_reason(&swap(&t, old, new)), "bad-schema", "{new}");
    }
    assert_eq!(memory_reason(&swap(&t, "supersedes: null", "supersedes: \"01JBX3Q8T2M9Z4K7WYH5N6D0RB@2\"")), "accepted");
    assert_eq!(memory_reason(&swap(&t, "body_kind: markdown", "body_kind: none")), "bad-schema", "none means no body");
    // Endorsement.
    let e = endorsement_text();
    let run = |text: String| reason(validate(Kind::Endorsement, OTHER_ID, text.as_bytes()));
    assert_eq!(run(swap(&e, "target_hash: ", "target_hashx: ")), "bad-schema", "target_hash is required");
    assert_eq!(run(e.lines().filter(|l| !l.starts_with("target_hash")).collect::<Vec<_>>().join("\n") + "\n"), "bad-schema");
    assert_eq!(run(swap(&e, &hash::of(b"the target bytes"), "blake3:abc")), "bad-schema");
    assert_eq!(run(swap(&e, &format!("target: \"{ID}@3\""), &format!("target: \"{ID}\""))), "bad-schema", "an exact revision");
    assert_eq!(run(swap(&e, "verdict: endorse", "verdict: approve")), "bad-schema");
    assert_eq!(run(swap(&e, "scope: [payments]", "scope: []")), "bad-schema", "endorse needs a scope");
    assert_eq!(run(swap(&e, "verdict: endorse", "verdict: dispute")), "bad-schema", "dispute needs a note");
    assert_eq!(run(swap(&swap(&e, "verdict: endorse", "verdict: dispute"), "scope: [payments]", "scope: []\nnote: The retry window is wrong.")), "accepted");
    assert_eq!(run(swap(&e, "checked: [target-open, summary-matches]", "checked: [target-open, target-open]")), "bad-schema");
    assert_eq!(run(swap(&e, "checked: [target-open, summary-matches]", "checked: [read-it]")), "bad-schema");
    // Tombstone.
    let ts = tombstone_text();
    let run = |text: String| reason(validate(Kind::Tombstone, OTHER_ID, text.as_bytes()));
    assert_eq!(run(swap(&ts, &format!("target: {ID}"), &format!("target: \"{ID}@3\""))), "bad-schema", "the whole record, not a revision");
    assert_eq!(run(swap(&ts, "action: withdraw", "action: delete")), "bad-schema");
    assert_eq!(run(swap(&ts, "reason: mistake", "reason: whim")), "bad-schema");
    assert_eq!(run(swap(&ts, "note: Written against API version 2019.\n", "")), "accepted", "a note is optional");
}

#[test]
fn plain_text_fields_refuse_markdown_links_html_and_entities() {
    let t = memory_text();
    let sum = |s: &str| swap(&t, "summary: Deliveries are retried with backoff for up to three days; handlers must be idempotent.", &format!("summary: \"{s}\""));
    for s in [
        "see [the docs](https://x.example)", "see [the docs][1]", "[1]: https://x.example", "<script>alert(1)</script>", "a <b>bold</b> claim", "<!-- hidden -->", "<https://x.example>", "x &lt;script&gt;",
        "x &#60;b&#62;", " leading space", "trailing space ",
    ] {
        assert_eq!(memory_reason(&sum(s)), "bad-text", "{s:?}");
    }
    // Neighbours that are plain text.
    for s in ["a < b and c > d", "AT&T & friends", "array[0] is first", "use (parentheses) freely", "ratio 3<4"] {
        assert_eq!(memory_reason(&sum(s)), "accepted", "{s:?}");
    }
    assert_eq!(memory_reason(&swap(&t, "title: Stripe webhooks retry for three days", "title: \"[x](y)\"")), "bad-text");
    assert_eq!(memory_reason(&swap(&t, "label: Webhook handler notes", "label: \"<img src=x>\"")), "bad-text");
    assert_eq!(memory_reason(&swap(&t, "access_hint: payments-eng Drive group", "access_hint: \"[g](http://x)\"")), "bad-text");
}
