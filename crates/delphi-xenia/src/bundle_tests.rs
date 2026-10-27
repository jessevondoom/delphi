//! The `bundle` record kind (spec 027 §4.3), played by the attacker: a publisher's record is hostile text that names a
//! place to fetch a tree from. Every record here is really signed and goes through the verifier's own
//! `validate_record`; the defects are written as bytes, because a hostile client writes bytes.

use crate::hash;
use crate::identity::Identity;
use crate::layout::Kind;
use crate::record::{is_bundle_version, validate_record, KindFields, Pinned, Reject};
use crate::serialize;
use crate::testkit::{ORG, STORE};
use crate::testkit_records::{self as rk, tree_block, BundleSpec, SignedRecord};

const ID: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RA";
const TREE: &str = "0123456789abcdef0123456789abcdef01234567";

fn who() -> Identity {
    Identity::from_seed([7; 32])
}

fn spec() -> BundleSpec {
    BundleSpec::new("acme-platform", "1.4.0", &hash::of(b"files.json"))
}

fn check(r: &SignedRecord) -> Result<crate::record::Validated, Reject> {
    validate_record(Pinned { org: ORG, store: STORE }, &r.path, &r.md, &r.sidecar, &|_| Ok(()))
}

/// The record as text with `from` replaced by `to`, signed again with the content hash made true.
fn edited(from: &str, to: &str) -> SignedRecord {
    let r = rk::bundle(&who(), ID, 1, &spec());
    let text = String::from_utf8(r.md).unwrap();
    assert!(text.contains(from), "the fixture has no {from:?}");
    rk::sign_record(&who(), Kind::Bundle, ID, text.replacen(from, to, 1).into_bytes())
}

fn refused(r: &SignedRecord) -> &'static str {
    match check(r) {
        Ok(_) => panic!("accepted:\n{}", String::from_utf8_lossy(&r.md)),
        Err(e) => e.reason(),
    }
}

fn with_tree(tree: String) -> SignedRecord {
    let mut s = spec();
    s.tree = tree;
    rk::bundle(&who(), ID, 1, &s)
}

#[test]
fn a_good_bundle_record_validates_and_round_trips_through_the_serializer() {
    let r = rk::bundle(&who(), ID, 1, &spec());
    let v = check(&r).unwrap_or_else(|e| panic!("{e:?}"));
    let KindFields::Bundle(b) = &v.record().kind_fields else { panic!("not a bundle") };
    assert_eq!((b.bundle_name.as_str(), b.bundle_version.as_str(), b.compat_schema), ("acme-platform", "1.4.0", 1));
    assert_eq!(b.tree.pin.value.as_deref(), Some(TREE));
    assert_eq!(b.touches_connectors, vec!["slack".to_string()]);
    assert_eq!(v.record().kind(), Kind::Bundle);
    // The publisher's writer: sealed bytes validate, parse back to the same record, and have one spelling.
    let (sealed, bytes) = serialize::seal(v.record()).unwrap();
    let again = rk::sign_record_raw(&who(), Kind::Bundle, ID, bytes.clone());
    assert_eq!(check(&again).unwrap().record(), &sealed);
    assert_eq!(serialize::serialize(&sealed).unwrap(), bytes);
    assert!(String::from_utf8(bytes).unwrap().contains("tree:\n  scheme: git\n"));
}

#[test]
fn a_bundle_record_lives_under_records_bundle_and_nowhere_else() {
    let r = rk::bundle(&who(), ID, 1, &spec());
    // The same bytes presented as a memory (a different directory) are a kind mismatch, whatever the signature says.
    let as_memory = rk::sign_record_raw(&who(), Kind::Memory, ID, r.md.clone());
    assert_eq!(refused(&as_memory), "mismatch");
    // The old provisional `skill` directory is not a kind.
    assert!(crate::layout::RecordPath::parse(&format!("records/skill/{ID}.md")).is_err());
    assert_eq!(crate::layout::RecordPath::parse(&r.path).unwrap().kind, Kind::Bundle);
}

#[test]
fn a_bundle_cannot_state_its_own_standing() {
    for key in ["tier: L2\n", "endorsed: true\n", "verified_by_org: true\n", "trust: high\n"] {
        let r = edited("bundle_name:", &format!("{key}bundle_name:"));
        assert_eq!(refused(&r), "forbidden-key", "{key}");
    }
    // Nor inside the tree pointer.
    let t = format!("{}  endorsed: true\n", tree_block("https://git.acme.example/p/b.git#.", "git-tree", TREE));
    assert_eq!(refused(&with_tree(t)), "forbidden-key");
}

#[test]
fn the_tree_pointer_is_an_https_git_pointer_pinned_by_a_full_object_id() {
    let ok = |loc: &str, kind: &str, pin: &str| check(&with_tree(tree_block(loc, kind, pin))).map(|_| ()).map_err(|e| e.reason());
    ok("https://git.acme.example/platform/bundle.git#.", "git-tree", TREE).unwrap();
    ok("https://git.acme.example/platform/bundle.git#bundles/acme", "git-commit", TREE).unwrap();
    ok("https://git.acme.example:8443/platform/bundle.git#.", "git-tree", &"a".repeat(64)).unwrap();
    let upper = TREE.to_uppercase();
    let content = hash::of(b"x");
    for (loc, kind, pin) in [
        // Not https: ssh, file, plain git, http, a bare path, a scp-style address.
        ("ssh://git.acme.example/platform/bundle.git#.", "git-tree", TREE),
        ("file:///etc#.", "git-tree", TREE),
        ("git://git.acme.example/p.git#.", "git-tree", TREE),
        ("http://git.acme.example/p.git#.", "git-tree", TREE),
        ("/srv/git/bundle#.", "git-tree", TREE),
        ("git@git.acme.example:p/b.git#.", "git-tree", TREE),
        // Credentials, queries, ports that are not ports, look-alike hosts, upper-case hosts.
        ("https://user:pw@git.acme.example/p.git#.", "git-tree", TREE),
        ("https://git.acme.example/p.git?token=abc#.", "git-tree", TREE),
        ("https://git.acme.example:0/p.git#.", "git-tree", TREE),
        ("https://git.acme.example:99999/p.git#.", "git-tree", TREE),
        ("https://g\u{131}t.acme.example/p.git#.", "git-tree", TREE),
        ("https://GIT.acme.example/p.git#.", "git-tree", TREE),
        ("https://git.acme.example/p.git#ghp_abcdefghijklmnopqrstuvwxyz0123456789", "git-tree", TREE),
        // Traversal in the repository path or the subdirectory, encoded or not, and an absolute subdirectory.
        ("https://git.acme.example/../p.git#.", "git-tree", TREE),
        ("https://git.acme.example/p.git#..", "git-tree", TREE),
        ("https://git.acme.example/p.git#a/../b", "git-tree", TREE),
        ("https://git.acme.example/p.git#/etc", "git-tree", TREE),
        ("https://git.acme.example/p.git#a/%2e%2e/b", "git-tree", TREE),
        ("https://git.acme.example/p.git#./a", "git-tree", TREE),
        // No fragment at all, or a second one.
        ("https://git.acme.example/p.git", "git-tree", TREE),
        ("https://git.acme.example/p.git#a#b", "git-tree", TREE),
        // A pin that does not pin: a branch name, a short or upper-case id, a content hash.
        ("https://git.acme.example/p.git#.", "git-tree", "main"),
        ("https://git.acme.example/p.git#.", "git-tree", "0123456"),
        ("https://git.acme.example/p.git#.", "git-tree", upper.as_str()),
        ("https://git.acme.example/p.git#.", "content-hash", content.as_str()),
    ] {
        assert_eq!(ok(loc, kind, pin), Err("bad-pointer"), "{loc} {kind} {pin}");
    }
    // A pin of kind `none` has no value at all.
    let none = "  scheme: git\n  locator: \"https://git.acme.example/p.git#.\"\n  pin:\n    kind: none\n  label: L\n  content_type: other\n".to_string();
    assert_eq!(refused(&with_tree(none)), "bad-pointer");
    // The scheme must be git: an https pointer cannot be a tree.
    let https = "  scheme: https\n  locator: \"https://git.acme.example/p.tar\"\n  pin:\n    kind: none\n  label: L\n  content_type: other\n".to_string();
    assert_eq!(refused(&with_tree(https)), "bad-pointer");
    // A hash on a tree has no meaning and is refused rather than ignored.
    let extra = format!("{}  hash: \"{}\"\n", tree_block("https://git.acme.example/p.git#.", "git-tree", TREE), hash::of(b"x"));
    assert_eq!(refused(&with_tree(extra)), "bad-pointer");
    // An unknown key in the pointer is an error (no ignored smuggling channel).
    let unknown = format!("{}  mirror: \"https://evil.example\"\n", tree_block("https://git.acme.example/p.git#.", "git-tree", TREE));
    assert_eq!(refused(&with_tree(unknown)), "bad-schema");
}

#[test]
fn the_names_hashes_versions_and_claims_are_closed_sets() {
    for (from, to) in [
        ("\"acme-platform\"", "\"Acme-Platform\""),
        ("\"acme-platform\"", "\"acme/platform\""),
        ("\"acme-platform\"", "\"../acme\""),
        ("\"acme-platform\"", "\"\""),
        ("\"1.4.0\"", "\"v1.4.0\""),
        ("\"1.4.0\"", "\"1.4\""),
        ("\"1.4.0\"", "\"01.4.0\""),
        ("\"1.4.0\"", "\"1.4.0+build\""),
        ("\"strategist\"", "\"overlord\""),
        ("\"slack\"", "\"Slack Admin\""),
        ("\"slack\"", "\"../slack\""),
        ("bundle_schema: 1", "bundle_schema: 2"),
        ("bundle_schema: 1", "bundle_schema: 0"),
        ("\">=0.3.0\"", "\"; rm -rf\""),
    ] {
        let r = edited(from, to);
        assert_eq!(refused(&r), "bad-schema", "{from} -> {to}");
    }
    // A hash that is not a blake3 digest.
    assert_eq!(refused(&edited(&hash::of(b"files.json"), "sha256:00")), "bad-schema");
    // Duplicate roles and connectors are refused.
    assert_eq!(refused(&edited("[\"strategist\", \"builder\"]", "[\"builder\", \"builder\"]")), "bad-schema");
    assert_eq!(refused(&edited("connectors: [\"slack\"]", "connectors: [\"slack\", \"slack\"]")), "bad-schema");
    // A missing required field.
    assert_eq!(refused(&edited("bundle_hash:", "bundle_hash_typo:")), "bad-schema");
    // Over the touches limit.
    let many: Vec<String> = (0..17).map(|i| format!("\"c{i}\"")).collect();
    assert_eq!(refused(&edited("connectors: [\"slack\"]", &format!("connectors: [{}]", many.join(", ")))), "over-limit");
    assert!(is_bundle_version("1.4.0") && is_bundle_version("0.0.1-rc.1") && !is_bundle_version("1.4.0-") && !is_bundle_version(&format!("1.0.{}", "9".repeat(40))));
}

#[test]
fn memory_fields_on_a_bundle_and_bundle_fields_on_a_memory_are_unknown_keys() {
    assert_eq!(refused(&edited("compat:", "memory_type: gotcha\ncompat:")), "bad-schema");
    let m = rk::memory(&who(), ID, 1, "active", "v");
    let text = String::from_utf8(m.md).unwrap().replacen("verified_by: none\n", "verified_by: none\nbundle_name: \"x\"\n", 1);
    assert_eq!(refused(&rk::sign_record(&who(), Kind::Memory, ID, text.into_bytes())), "bad-schema");
}

#[test]
fn the_redaction_hash_covers_the_tree_pointer_so_a_swapped_locator_cannot_keep_a_passed_scan() {
    // Sign the genuine record, then change only the locator and keep the old (now false) content hash.
    let r = rk::bundle(&who(), ID, 1, &spec());
    let text = String::from_utf8(r.md).unwrap().replacen("git.acme.example", "evil.example", 1);
    assert_eq!(refused(&rk::sign_record_raw(&who(), Kind::Bundle, ID, text.into_bytes())), "mismatch");
    // The pin is covered as well.
    let r = rk::bundle(&who(), ID, 1, &spec());
    let text = String::from_utf8(r.md).unwrap().replacen(TREE, &"b".repeat(40), 1);
    assert_eq!(refused(&rk::sign_record_raw(&who(), Kind::Bundle, ID, text.into_bytes())), "mismatch");
    // A record that is not a bundle hashes exactly as before: the tree region is empty for every other kind.
    let m = rk::memory(&who(), ID, 1, "active", "v");
    assert!(crate::redaction::regions(&m.md).unwrap().tree.is_empty());
    assert!(check(&m).is_ok());
}

#[test]
fn a_bundle_is_a_record_like_any_other_for_signatures() {
    let r = rk::bundle(&who(), ID, 1, &spec());
    // One flipped byte in the record fails at the hash, before anything is parsed.
    let mut md = r.md.clone();
    let at = md.iter().position(|b| *b == b'4').unwrap();
    md[at] = b'5';
    let flipped = SignedRecord { md, ..r.clone() };
    assert_eq!(refused(&flipped), "hash-mismatch");
    // Another key's signature over the same bytes does not verify.
    let other = Identity::from_seed([8; 32]);
    let forged = SignedRecord { sidecar: rk::sign_record_raw(&other, Kind::Bundle, ID, r.md.clone()).sidecar, ..r };
    let why = refused(&forged);
    assert!(matches!(why, "bad-sidecar" | "bad-signature" | "mismatch"), "{why}");
}
