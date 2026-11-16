//! `specs/115` §11 items 5 to 7 (slice X6): an internal or private source confirms the claim it states directly, and nothing else.
//!
//! The confirmation is `Library::internal_confirmation`, a label for that one claim (`confirmed by an internal source (<project>)`). These
//! tests show each "never" of §11 item 6: it is never an origin in the count of another claim, never corroboration for a claim the source
//! does not state, never weight in the status of any other claim or of the claim itself, never in the validator's check, and (by the scan
//! at the end) never read by a written answer, a page, a decision record or a Council.

use crate::library::claims::{status, AnchorSpec, ClaimOpts, Validator};
use crate::library::{AddOpts, Budgets, Level, Library};
use delphi_core::knowledge_status::Status;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn lib() -> (tempfile::TempDir, Library) {
    let d = tempfile::tempdir().unwrap();
    let l = Library::in_home(d.path());
    (d, l)
}

fn source(d: &Path, l: &Library, name: &str, text: &str, private: bool) -> String {
    let p: PathBuf = d.join(name);
    std::fs::write(&p, text).unwrap();
    l.add_file(&p, &AddOpts { licence: Some("CC0-1.0".into()), level: Some(Level::Full), private, ..Default::default() }, &Budgets::default()).unwrap().id
}

/// A source as the fetcher leaves it (a canonical URL, an origin, a content hash, a class), so it would count as an origin.
fn fetched(l: &Library, id: &str, host: &str) {
    let mut m = l.read_meta(id).unwrap();
    m["canonical_url"] = json!(format!("https://{host}/page"));
    m["origin"] = json!(host);
    m["content_hash"] = json!(format!("sha256:{host}"));
    m["source_class"] = json!("reputable_secondary");
    m["source_unfamiliar"] = json!(false);
    std::fs::write(l.source_dir(id).join("meta.json"), serde_json::to_vec(&m).unwrap()).unwrap();
}

fn spec(source: &str, quote: &str) -> AnchorSpec {
    AnchorSpec { source_id: source.into(), locator: Some("section 2".into()), quote: quote.into(), prefix: None, suffix: None }
}

fn of_alpha() -> ClaimOpts {
    ClaimOpts { scope: Some("project".into()), project: Some("alpha".into()), ..Default::default() }
}

fn claim(l: &Library, text: &str, anchors: &[AnchorSpec]) -> String {
    l.add_claim(text, &of_alpha(), anchors).unwrap()["id"].as_str().unwrap().to_string()
}

const MEMO: &str = "# Platform memo\n\nThe wallaby platform ships its second release in March 2027, the internal plan says. It runs on the kestrel engine.\n";

/// §11 item 5: a claim the internal source states (its quote found in the source's text) is confirmed internally, labelled with its
/// project's name, and its derived status is exactly what it was: the label lifts nothing. A claim of no project names this install.
#[test]
fn a_claim_its_internal_source_states_is_confirmed_and_labelled_and_its_status_does_not_move() {
    let (d, l) = lib();
    let memo = source(d.path(), &l, "memo.md", MEMO, true);
    let id = claim(&l, "The wallaby platform ships its second release in March 2027.", &[spec(&memo, "ships its second release in March 2027")]);
    let c = l.claim(&id).unwrap();
    assert_eq!(l.internal_confirmation(&c).as_deref(), Some("alpha"));
    assert_eq!((status(&c), status(&c).label()), (Status::Noted, "not validated"), "the confirmation is a label; the derived status is unchanged");
    assert!(l.rests_on_internal(&c) && l.cites_of(&c).is_empty(), "and its source is still no origin, even for its own claim");
    let team = l.add_claim("The wallaby platform runs on the kestrel engine.", &ClaimOpts::default(), &[spec(&memo, "runs on the kestrel engine")]).unwrap();
    assert_eq!(l.internal_confirmation(&team).as_deref(), Some("this install"));
    // The index's text carries the label as a field it reads for internal claims only; the status word is still the derived one.
    let text = crate::research_index::text_of(&l.claims_dir().join(format!("{id}.json"))).unwrap();
    assert!(text.contains("claim_status: noted\n") && text.contains("internal_confirmation: \"alpha\"\n"), "{text}");
    assert!(text.contains("- confirmed by an internal source (alpha); it lends no weight to any other claim\n"), "{text}");
    // The label as a session reads it, own and shared.
    assert_eq!(crate::shared_research::confirmed_label(crate::shared_research::CONFIRMED_LABEL, "alpha"), "another project's research, confirmed by an internal source (alpha)");
    assert_eq!(crate::shared_research::confirmed_label(crate::shared_research::OWN_CONFIRMED_LABEL, "al\u{7}pha"), "this project's research, confirmed by an internal source (al pha)");
}

/// §11 item 5, what is not a confirmation: a public source; an attributed claim (an opinion); a claim retired or past its date; a host's
/// claim with no anchor (its word alone); and an anchor into an internal source that holds text in which the quote was not found.
#[test]
fn only_a_direct_anchor_into_an_internal_source_confirms() {
    let (d, l) = lib();
    let memo = source(d.path(), &l, "memo.md", MEMO, true);
    let open = source(d.path(), &l, "open.md", "The wallaby platform ships its second release in March 2027, the press release says.\n", false);
    let public = claim(&l, "The wallaby platform ships in March 2027 (press).", &[spec(&open, "ships its second release in March 2027")]);
    assert_eq!(l.internal_confirmation(&l.claim(&public).unwrap()), None, "a public source confirms nothing internally");
    let opinion = l.add_claim("The wallaby release date is ambitious.", &ClaimOpts { kind: Some("attributed".into()), ..of_alpha() }, &[spec(&memo, "ships its second release in March 2027")]).unwrap();
    assert_eq!(l.internal_confirmation(&opinion), None, "an attributed claim is an opinion");
    let mut retired = l.claim(&claim(&l, "The wallaby platform runs on the kestrel engine.", &[spec(&memo, "runs on the kestrel engine")])).unwrap();
    assert!(l.internal_confirmation(&retired).is_some());
    retired["lifecycle"] = json!("retired");
    assert_eq!(l.internal_confirmation(&retired), None);
    retired["lifecycle"] = Value::Null;
    retired["valid_until"] = json!("2001-01-01");
    assert_eq!(l.internal_confirmation(&retired), None, "past its date");
    let bare = json!({"id": "c-000000000001", "kind": "fact", "project": "alpha", "anchors": [], "host_reported": ["host-reported"]});
    assert!(l.rests_on_internal(&bare) && l.internal_confirmation(&bare).is_none(), "a host's word with no anchor is internal and confirms nothing");
    // An anchor written into a held internal source whose quote is not in its text (a research anchor keeps it, without a place).
    let mut unfound = l.claim(retired["id"].as_str().unwrap()).unwrap();
    unfound["valid_until"] = Value::Null;
    unfound["anchors"][0]["start"] = Value::Null;
    unfound["anchors"][0]["end"] = Value::Null;
    assert_eq!(l.internal_confirmation(&unfound), None, "the source holds text and the quote was not found in it");
    unfound["anchors"][0]["state"] = json!("lost");
    assert_eq!(l.internal_confirmation(&unfound), None);
    // A private pointer holds no text: its anchor's line and quote are what tie the claim to it.
    let pointer = l.add_private_pointer("internal", "planning wiki, page releases", "Release plan", "2026-11-15T00:00:00Z", None).unwrap();
    let anchors = [spec(&pointer, "second release in March 2027")];
    let hosted = l.add_host_claim(&crate::library::claims::ResearchClaim { text: "The wallaby second release is in March 2027.", fingerprint: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeee1", project: Some("alpha"), topic: None, anchors: &anchors }, crate::host_research::HOST_REPORTED).unwrap();
    assert_eq!(l.internal_confirmation(&hosted).as_deref(), Some("alpha"), "{hosted}");
}

/// §11 item 5: a public source that disagrees does not take the confirmation away; the disagreement is kept beside the claim, where the
/// operator sees both (the claim's own record, the index's text, `delphi claims show`).
#[test]
fn a_disagreeing_public_source_does_not_lower_the_confirmation_and_the_disagreement_is_shown() {
    let (d, l) = lib();
    let memo = source(d.path(), &l, "memo.md", MEMO, true);
    let open = source(d.path(), &l, "open.md", "The wallaby platform ships its second release in June 2027, the analyst says.\n", false);
    let id = claim(&l, "The wallaby platform ships its second release in March 2027.", &[spec(&memo, "ships its second release in March 2027")]);
    let other = claim(&l, "The wallaby platform ships its second release in June 2027.", &[spec(&open, "ships its second release in June 2027")]);
    l.validate_claim(&other, Validator::Operator).unwrap();
    l.contradicts_validated(&id, &other).unwrap();
    let c = l.claim(&id).unwrap();
    assert_eq!(status(&c), Status::Contested, "the disagreement is recorded as it always was");
    assert_eq!(l.internal_confirmation(&c).as_deref(), Some("alpha"), "and does not take the confirmation away");
    let text = crate::research_index::text_of(&l.claims_dir().join(format!("{id}.json"))).unwrap();
    assert!(text.contains("internal_confirmation: \"alpha\"") && text.contains("- a public source disagrees; the confirmation stands for this claim, and the operator sees both\n"), "{text}");
    // ...and the validated public claim is untouched by the confirmed one that disagrees with it.
    assert_eq!(status(&l.claim(&other).unwrap()), Status::Validated);
}

/// §11 item 6, "never an origin in the count of another claim" and "never corroboration for a claim the internal source does not itself
/// state": a claim on two fetched public pages has two origins, and adding the internal memo to it (even dressed as a fetched page) and
/// confirming another claim on that memo leaves its count, its support record and its status exactly as they were; it does not reach
/// the three origins of `corroborated`. A claim that only shares words with the confirmed one is not confirmed.
#[test]
fn the_confirmation_is_never_an_origin_and_never_corroborates_another_claim() {
    let (d, l) = lib();
    let memo = source(d.path(), &l, "memo.md", MEMO, true);
    fetched(&l, &memo, "intranet.example");
    let mut pages = Vec::new();
    let bodies = [
        ("a.example", "# Harbour systems review\n\nOur engineers toured the yard in spring and tested the loaders twice. It runs on the kestrel engine, which surprised the crane crew.\n"),
        ("b.example", "# Notes from the trade fair\n\nA vendor booth showed dashboards, pricing sheets and a long demo reel. It runs on the kestrel engine according to the brochure handed out on day two.\n"),
    ];
    for (host, body) in bodies {
        let id = source(d.path(), &l, &format!("{host}.md"), body, false);
        fetched(&l, &id, host);
        pages.push(id);
    }
    let other = claim(&l, "The wallaby platform runs on the kestrel engine.", &[spec(&pages[0], "It runs on the kestrel engine"), spec(&pages[1], "It runs on the kestrel engine")]);
    let before = l.refresh_support(&other).unwrap();
    assert_eq!((before["support"]["origins"].clone(), status(&before)), (json!(2), Status::Noted));
    // The confirmed claim on the memo, which states something else.
    let confirmed = claim(&l, "The wallaby platform ships its second release in March 2027.", &[spec(&memo, "ships its second release in March 2027")]);
    assert!(l.internal_confirmation(&l.claim(&confirmed).unwrap()).is_some());
    let after = l.refresh_support(&other).unwrap();
    assert_eq!((after["support"].clone(), status(&after)), (before["support"].clone(), Status::Noted), "nothing about another claim moved");
    // The memo states the kestrel engine too; anchored to it, the other claim gains no origin and is not corroborated.
    l.add_anchor(&other, &spec(&memo, "It runs on the kestrel engine")).unwrap();
    let c = l.claim(&other).unwrap();
    assert_eq!(c["anchors"].as_array().map(Vec::len), Some(3), "the memo is anchored: {c}");
    assert!(l.cites_of(&c).iter().all(|x| x.source_id != memo), "the internal source is never a cite");
    let anchored = l.refresh_support(&other).unwrap();
    assert_eq!(anchored["support"]["origins"], json!(2), "two public origins; the memo adds none");
    assert!(!matches!(status(&anchored), Status::Corroborated | Status::Substantiated | Status::Vetted), "{:?}", status(&anchored));
    // A claim that merely shares the confirmed claim's words, with no anchor into the memo, is not confirmed.
    let near = claim(&l, "The wallaby platform second release is in March.", &[spec(&pages[0], "Our engineers toured the yard")]);
    assert_eq!(l.internal_confirmation(&l.claim(&near).unwrap()), None);
}

/// §11 item 6, "never weight in the validator's check": the validator's contradiction step reads validated claims only
/// (`ValidatedFacts::about`), and a confirmed claim is not one, so it is never offered against another claim; and the claims the
/// validator could be offered next carry the same status and counts with or without it.
#[test]
fn the_validators_check_is_not_given_the_confirmation() {
    use crate::fact_run::ValidatedFacts;
    let (d, l) = lib();
    let memo = source(d.path(), &l, "memo.md", MEMO, true);
    let open = source(d.path(), &l, "open.md", "The wallaby platform ships its second release in June 2027, the analyst says.\n", false);
    let other = claim(&l, "The wallaby platform ships its second release in June 2027.", &[spec(&open, "ships its second release in June 2027")]);
    let before = l.validation_candidates(true);
    let confirmed = claim(&l, "The wallaby platform ships its second release in March 2027.", &[spec(&memo, "ships its second release in March 2027")]);
    assert!(l.internal_confirmation(&l.claim(&confirmed).unwrap()).is_some());
    assert!(l.about("The wallaby platform ships its second release in June 2027.", 10).is_empty(), "a confirmed claim is not a validated fact to check against");
    let after: Vec<_> = l.validation_candidates(true).into_iter().filter(|c| c.id != confirmed).collect();
    assert_eq!(after, before, "the other claims' place, status and counts are unchanged");
    assert!(before.iter().any(|c| c.id == other));
}

/// §11 item 6, the rest of the list: the confirmation is named only where it is computed, indexed and shown (the library, the index,
/// the retrieval labels, the operator's `claims show`). No written answer's check (`research_run.rs`), page (`library_page*`,
/// `library_pages.rs`, and the topic pages of `specs/117`, `library_topics.rs` and `topic_gather.rs`, which also leave every internal
/// hit and every claim on a private source out), decision record (`decision_*`), Council (`council*`),
/// validator or fact check (`fact_*`, `accept_all.rs`, `tier_audit.rs`), corroboration count or status ladder reads it, so none of them
/// can give it weight. A new reader fails here and a reviewer must ask whether it lends the confirmation to something else.
#[test]
fn the_confirmation_is_named_only_where_it_is_computed_indexed_and_shown() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let allowed = [
        "crates/delphi-engine/src/library_claims.rs",
        "crates/delphi-engine/src/research_index.rs",
        "crates/delphi-engine/src/kb.rs",
        "crates/delphi-engine/src/shared_research.rs",
        "crates/delphi/src/claims_cmd.rs",
    ];
    let pat = regex::Regex::new(r#"internal_confirmation\(|"internal_confirmation"|CONFIRMED_LABEL|confirmed_label\(|\.confirmed_by\b|\bconfirmed_by:|claim_text_with\("#).unwrap();
    let mut files = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                if !matches!(p.file_name().and_then(|n| n.to_str()), Some("target" | "tests" | "node_modules")) {
                    walk(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs") && !p.file_stem().and_then(|n| n.to_str()).is_some_and(|n| n == "tests" || n.ends_with("_tests")) {
                out.push(p);
            }
        }
    }
    walk(&root.join("crates"), &mut files);
    assert!(files.len() > 100, "the scan found {} files", files.len());
    let mut seen = Vec::new();
    let mut stray = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let cut = src.rfind("\n#[cfg(test)]\nmod tests").or_else(|| src.rfind("\nmod tests")).unwrap_or(src.len());
        let code: String = src[..cut].lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if pat.is_match(&code) {
            seen.push(rel.clone());
            if !allowed.contains(&rel.as_str()) {
                stray.push(rel);
            }
        }
    }
    assert!(stray.is_empty(), "the internal confirmation is named outside the files that compute, index and show it: {stray:?}");
    let gone: Vec<&str> = allowed.iter().copied().filter(|p| !seen.iter().any(|s| s == p)).collect();
    assert!(gone.is_empty(), "on the list but no longer naming it; take them off: {gone:?}");
    // And the status ladder, the count and the answer and page checks never read it by any other name either: `status` is computed
    // from the claim's own record, which the confirmation is not written into.
    let lc = std::fs::read_to_string(root.join("crates/delphi-engine/src/library_claims.rs")).unwrap();
    // a free function ends at `}` in column zero, a method at four spaces
    let body = |name: &str, end: &str| -> String {
        let at = lc.find(name).unwrap_or_else(|| panic!("{name}"));
        let stop = lc[at..].find(end).map(|e| at + e).unwrap_or_else(|| panic!("the end of {name}"));
        lc[at..stop].to_string()
    };
    for (f, end) in [("pub fn status_with(", "\n}\n"), ("pub fn tags_of(", "\n}\n"), ("pub fn cites_of(", "\n    }\n"), ("pub fn refresh_support(", "\n    }\n")] {
        let b = body(f, end);
        assert!(b.lines().count() > 2, "{f}: the scan found its body ({} lines)", b.lines().count());
        assert!(!b.contains("internal_confirmation") && !b.contains("confirmed"), "{f} reads the confirmation");
    }
}
