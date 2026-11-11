//! `specs/098` §12 item 17 and `specs/100` §10: `corroborated`, `substantiated`, `vetted` and `trusted_sources` grant nothing. They are a count the daemon writes and a word it
//! shows; no promotion, no trust, no tool access, no approval and no queue order reads them. The test is a net, not a proof: it
//! reads the non-test source of every crate and fails when a file not on the short list below mentions the status, the raising
//! function, the `trusted_sources` tag or the stored `support` record. A new reader has to be added to the list by a reviewer,
//! who then has to ask the question this test exists for: does it decide anything?

use regex::Regex;
use std::path::{Path, PathBuf};

/// Where the count may be named, and why each file may. Nothing here decides a permission, a tier, a taint level or an order.
const READERS: &[(&str, &str)] = &[
    ("crates/delphi-core/src/knowledge_status.rs", "defines the status, its label and the raising rule"),
    ("crates/delphi-engine/src/corroboration.rs", "computes the count from citations; takes no policy input"),
    ("crates/delphi-engine/src/library_claims.rs", "stores the tags, derives the displayed status, ranks a pull with the same weight as noted"),
    ("crates/delphi-engine/src/research_run.rs", "prints the count in a research answer; the corroboration pass sends only public claims' words to search"),
    ("crates/delphi-engine/src/research_host.rs", "records a staged claim's tags in the ledger"),
    ("crates/delphi-engine/src/engine.rs", "holds the ladder's numbers as a setting; reads nothing"),
    ("crates/delphi-engine/src/boot.rs", "parses `[research.tiers]` and installs it once at start"),
    ("crates/delphi/src/validation_audit_cmd.rs", "prints the claims the validator could be offered next in the fast lane's order (ids, rungs, counts); an order on a list, nothing else"),
    ("crates/delphi/src/validation_tier_cmd.rs", "the operator's tier audit: shows a sampled claim, takes their answer, then prints its counts; the audit can only take a tier away"),
    ("crates/delphi-engine/src/accept_all.rs", "the operator's accept-all: reads the rung only to choose WHICH claims to show and, at the operator's typed word, to validate them as the operator; it grants no permission and sets no rung"),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            if !matches!(p.file_name().and_then(|n| n.to_str()), Some("target" | "tests" | "node_modules")) {
                rust_files(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") && !p.file_stem().and_then(|n| n.to_str()).is_some_and(|n| n == "tests" || n.ends_with("_tests")) {
            out.push(p);
        }
    }
}

/// The text before the first `#[cfg(test)]` at column zero or the first `mod tests`: test code is allowed to name the status.
fn production(src: &str) -> &str {
    let cut = src.find("\n#[cfg(test)]").or_else(|| src.find("\nmod tests")).unwrap_or(src.len());
    &src[..cut]
}

fn pattern() -> Regex {
    Regex::new(r#"Corroborated|Substantiated|Vetted|raise_for_support|raise_for_tags|support_detail|tier_detail|tier_for|tier_params|TierParams|tags_of|trusted_sources|\["support"\]|CORROBORATED_MIN"#).unwrap()
}

#[test]
fn no_file_outside_the_list_reads_the_count_and_the_list_decides_nothing() {
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(files.len() > 100, "the scan found {} files; it is looking in the wrong place", files.len());
    let pat = pattern();
    let mut stray = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let body = production(&src);
        // comments may name the count; code may not
        let code: String = body.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if pat.is_match(&code) {
            seen.push(rel.clone());
            if !READERS.iter().any(|(p, _)| *p == rel) {
                stray.push(rel);
            }
        }
    }
    assert!(stray.is_empty(), "these files name the corroboration count but are not on the list in {}: {stray:?}", file!());
    let gone: Vec<&str> = READERS.iter().map(|(p, _)| *p).filter(|p| !seen.iter().any(|s| s == p)).collect();
    assert!(gone.is_empty(), "these files are on the list but no longer name the count; remove them from the list: {gone:?}");
}

#[test]
fn the_files_that_do_read_the_count_never_put_it_beside_a_permission_a_tier_or_an_approval() {
    // Within the listed files, a line that names the count must not also name an authority word. A word list is crude, which is
    // why the list above is short and each entry says what it does; this catches the careless edit.
    let authority = Regex::new(r"(?i)\b(approve[sd]?|approval|permit|permission|grant|taint|tier\b|trust_level|allow(ed)?_tools?|sandbox|egress|elevate|promote|ratif)").unwrap();
    let pat = pattern();
    let root = root();
    let mut bad = Vec::new();
    for (rel, _) in READERS {
        let src = std::fs::read_to_string(root.join(rel)).unwrap();
        for (i, line) in production(&src).lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with("///") {
                continue;
            }
            if pat.is_match(line) && authority.is_match(line) {
                bad.push(format!("{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }
    assert!(bad.is_empty(), "a line names the count and an authority word:\n{}", bad.join("\n"));
}

#[test]
fn the_status_table_keeps_corroborated_at_the_not_validated_label_and_raises_only_noted_and_queued_claims() {
    use delphi_core::knowledge_status::{raise_for_support, Status};
    assert_eq!(Status::Corroborated.label(), "not validated", "the operator still reads one of three words");
    let from = [Status::Validated, Status::Contested, Status::Disproved, Status::Retired];
    for s in from {
        assert_eq!(raise_for_support(s, 1000), s, "{s:?}: a count never overrides another status");
    }
    // the only transitions a count makes
    for (s, want) in [(Status::Noted, Status::Corroborated), (Status::Checking, Status::Corroborated)] {
        assert_eq!(raise_for_support(s, 3), want);
        assert_eq!(raise_for_support(s, 2), s);
    }
}

#[test]
fn the_ladder_is_computed_by_one_table_and_vetted_is_the_only_new_top_level_word() {
    use delphi_core::knowledge_status::{raise_for_tags, tier_for, Status, Tags, TierParams};
    let p = TierParams::default();
    // the two rungs below vetted still read "not validated" and say so; "vetted" is the one new word
    assert_eq!((Status::Substantiated.label(), Status::Vetted.label()), ("not validated", "vetted"));
    assert_ne!(Status::Vetted.label(), "validated", "vetted never reads as validated");
    // no count and no would_ratify ever overrides another status (a contradiction wins, a validated fact stays)
    let top = Tags { origins: 50, trusted_sources: 50, would_ratify: true, protected: false };
    for s in [Status::Validated, Status::Contested, Status::Disproved, Status::Retired] {
        assert_eq!(raise_for_tags(s, &top, &p), s, "{s:?}");
    }
    // vetted needs the validator's would_ratify: without it the same counts stop at substantiated
    assert_eq!(tier_for(&Tags { would_ratify: false, ..top }, &p), Status::Substantiated);
    // a protected claim never rises
    assert_eq!(tier_for(&Tags { protected: true, ..top }, &p), Status::Noted);
}

#[test]
fn a_tier_is_never_set_by_a_tool_argument_or_a_model_field() {
    // the only writer of a status tier is the derivation; no tool schema or MCP argument may take a `tier` or `knowledge_status` to store
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let setter = Regex::new(r#"(?i)"(set_tier|tier_override|force_tier)"|Status::(Vetted|Substantiated)\s*=>\s*(true|allow)"#).unwrap();
    let mut bad = Vec::new();
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap();
        if setter.is_match(production(&src)) {
            bad.push(f.strip_prefix(&root).unwrap().to_string_lossy().to_string());
        }
    }
    assert!(bad.is_empty(), "something sets or acts on a tier: {bad:?}");
}

#[test]
fn the_fast_lane_orders_a_list_and_changes_no_claim() {
    // `specs/100` §5 and §10 item 2: the lane reads a rung to put a claim earlier in a list. The same claims come out with it on or off, each
    // with the status it went in with, and only the library and the operator's queue display call it.
    use delphi_core::knowledge_status::Status;
    use delphi_engine::library::claims::{lane_order, Candidate};
    let c = |id: &str, status, origins, trusted_sources| Candidate { id: id.into(), status, origins, trusted_sources };
    let all = vec![
        c("a-noted", Status::Noted, 1, 0),
        c("b-corroborated", Status::Corroborated, 3, 0),
        c("c-substantiated", Status::Substantiated, 3, 1),
        c("d-substantiated-more", Status::Substantiated, 6, 2),
        c("e-corroborated-more", Status::Corroborated, 4, 0),
    ];
    let (mut on, mut off) = (all.clone(), all.clone());
    lane_order(&mut on, true);
    lane_order(&mut off, false);
    let ids = |v: &[Candidate]| v.iter().map(|x| x.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&on), ["d-substantiated-more", "c-substantiated", "e-corroborated-more", "b-corroborated", "a-noted"], "a higher rung first, then more origins");
    assert_eq!(ids(&off), ["a-noted", "b-corroborated", "c-substantiated", "d-substantiated-more", "e-corroborated-more"], "off: by id");
    // the same set, and every claim keeps the status and counts it had
    let (mut a, mut b) = (on.clone(), off.clone());
    a.sort_by(|x, y| x.id.cmp(&y.id));
    b.sort_by(|x, y| x.id.cmp(&y.id));
    assert_eq!(a, b, "the lane reorders; it changes no claim");
    // who may call it: only the library's own list builder and the operator's queue listing
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let callers = Regex::new(r"lane_order\(|validation_candidates\(|fast_lane_on").unwrap();
    let allowed = [
        "crates/delphi-engine/src/library_claims.rs",
        "crates/delphi-core/src/knowledge_status.rs",
        "crates/delphi-engine/src/boot.rs",
        "crates/delphi/src/validation_audit_cmd.rs",
    ];
    let mut stray = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if callers.is_match(&code) && !allowed.contains(&rel.as_str()) {
            stray.push(rel);
        }
    }
    assert!(stray.is_empty(), "the fast lane is named outside the files that may order a list: {stray:?}");
}

#[test]
fn only_the_operators_command_raises_a_rolled_back_tier_and_only_the_audit_and_the_operator_roll_one_back() {
    // `specs/100` §10 items 5 and 7: a latched tier is raised by the operator's `delphi validation tier restore` and nothing else, and no daemon
    // path, tool or span calls the restore. A roll-back can only lower; the audit's own triggers and the operator's command are its only callers.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let restore = Regex::new(r"tier_audit::restore\(|\bta::restore\(").unwrap();
    let rollback = Regex::new(r"tier_audit::rollback\(|\bta::rollback\(").unwrap();
    let (mut restorers, mut rollers) = (Vec::new(), Vec::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if restore.is_match(&code) {
            restorers.push(rel.clone());
        }
        if rollback.is_match(&code) {
            rollers.push(rel);
        }
    }
    assert_eq!(restorers, ["crates/delphi/src/validation_tier_cmd.rs"], "the operator's command is the only caller of the restore");
    assert_eq!(rollers, ["crates/delphi/src/validation_tier_cmd.rs"], "outside the audit module itself, only the operator's command rolls a tier back");
    // and the daemon's mutating paths never write the latch file by name
    let latch_writer = Regex::new(r"LATCH_FILE|tier_latch\.json").unwrap();
    let allowed = ["crates/delphi-engine/src/tier_audit.rs", "crates/delphi-core/src/knowledge_status.rs"];
    let mut stray = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        if latch_writer.is_match(production(&src)) && !allowed.contains(&rel.as_str()) {
            stray.push(rel);
        }
    }
    assert!(stray.is_empty(), "the latch file is named outside the audit module and the reader: {stray:?}");
}

#[test]
fn only_the_validators_own_loop_records_a_dry_run() {
    // `specs/100` §4 and §10 item 3: `support.would_ratify` is written by the daemon after the validator's advisory verdict, never by a tool,
    // a host field or a model. The writer is `Library::record_would_ratify`; its one caller outside the library is the validator's loop.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let writer = Regex::new(r"record_would_ratify\(|\[.would_ratify.\]\s*=[^=]").unwrap();
    let allowed = ["crates/delphi-engine/src/library_claims.rs", "crates/delphi-engine/src/fact_serve.rs"];
    let mut stray = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if writer.is_match(&code) && !allowed.contains(&rel.as_str()) {
            stray.push(rel);
        }
    }
    assert!(stray.is_empty(), "something writes a dry-run record outside the validator's loop: {stray:?}");
    // and no tool schema takes it as an argument
    let tool_arg = Regex::new(r#""would_ratify"\s*:\s*\{\s*"type""#).unwrap();
    let mut schemas = Vec::new();
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap();
        if tool_arg.is_match(production(&src)) {
            schemas.push(f.strip_prefix(&root).unwrap().to_string_lossy().to_string());
        }
    }
    assert!(schemas.is_empty(), "a tool schema takes `would_ratify` as an argument: {schemas:?}");
}

#[test]
fn only_the_operators_command_accepts_a_batch_and_the_batch_mark_and_the_re_audit_flag_decide_nothing() {
    // `specs/100` §10 items 1 and 4: accept-all writes only through the operator's act, and a `batch-accepted` mark or a `re-audit due` flag is a label for
    // tracing, never a criterion for promotion, trust, tool access or approval. The one caller of the batch accept is the operator's command; the mark
    // and the flag are named only by the library that stores them, the module that plans the batch, the audit that journals a roll-back, the
    // operator's commands and the help text.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let callers = Regex::new(r"accept_all::accept\(|\bac::accept\(|validate_claim_in_batch\(|mark_reaudit_due\(|clear_reaudit_due\(").unwrap();
    let callers_ok = [
        "crates/delphi/src/validation_tier_cmd.rs",
        "crates/delphi-engine/src/accept_all.rs",
        "crates/delphi-engine/src/library_claims.rs",
    ];
    // the identifiers that store or read the claim mark; the phrase "batch-accepted" alone is also used by the decisions pane's own batch accept (spec 044)
    let mark = Regex::new(r#"BATCH_MARK|reaudit_due|re-audit due|\["batch"\]\["mark"\]"#).unwrap();
    let mark_ok = [
        "crates/delphi/src/main.rs",
        "crates/delphi/src/needs.rs",
        "crates/delphi/src/validation_tier_cmd.rs",
        "crates/delphi-engine/src/accept_all.rs",
        "crates/delphi-engine/src/library_claims.rs",
        "crates/delphi-engine/src/tier_audit.rs",
    ];
    let (mut stray_callers, mut stray_mark) = (Vec::new(), Vec::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if callers.is_match(&code) && !callers_ok.contains(&rel.as_str()) {
            stray_callers.push(rel.clone());
        }
        if mark.is_match(&code) && !mark_ok.contains(&rel.as_str()) {
            stray_mark.push(rel);
        }
    }
    assert!(stray_callers.is_empty(), "something other than the operator's command accepts a batch or flags a re-audit: {stray_callers:?}");
    assert!(stray_mark.is_empty(), "the batch mark or the re-audit flag is named outside the files that store and show it: {stray_mark:?}");
    // the command that accepts needs a terminal and the typed phrase, and the main dispatch checks the terminal before it calls the accept
    let main = std::fs::read_to_string(root.join("crates/delphi/src/main.rs")).unwrap();
    let at = main.find("\"accept-all\" => {").expect("the accept-all arm");
    let arm = &main[at..at + 2500];
    let (term, accept) = (arm.find("is_terminal").expect("a terminal check"), arm.find("validation_tier_cmd::accept_all(&ws.dir").expect("the accept call"));
    assert!(term < accept, "the terminal is checked before anything is accepted");
}

#[test]
fn the_reached_mark_is_written_in_one_place_and_the_calibration_only_counts_it() {
    // `specs/100` §8: `reached` records which rung a claim has read, for the calibration report. The daemon writes it in `refresh_support`;
    // `tier_outcomes` counts it; the operator's report prints the counts. Nothing else names the mark, and nothing branches on it: it is a
    // record for calibrating the defaults, never a criterion for promotion, trust, tool access or approval.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let uses = Regex::new(r#"\["reached"\]|tier_outcomes\("#).unwrap();
    let allowed = ["crates/delphi-engine/src/library_claims.rs", "crates/delphi/src/validation_tier_cmd.rs"];
    let mut stray = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if uses.is_match(&code) && !allowed.contains(&rel.as_str()) {
            stray.push(rel);
        }
    }
    assert!(stray.is_empty(), "the reached mark or the calibration count is named outside the files that may: {stray:?}");
}

#[test]
fn a_contradiction_mark_is_made_only_by_the_library_and_nothing_reads_a_sources_standing() {
    // `specs/100` §9.1. `contradicts_validated` lowers only the contradicting claim, never the validated fact; the validator's contradiction
    // search will be its caller, so today no other production file names it. `standing()` is the only history that may ever count toward a
    // source's standing (claims the operator accepted one at a time); nothing reads it, because a source becomes a trusted source only by the
    // operator's own edit of the list (`specs/066` §5). A caller of either one has to be added to this test on purpose.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let marker = Regex::new(r"contradicts_validated\(").unwrap();
    let standing = Regex::new(r"\.standing\(\)").unwrap();
    let (mut mark_callers, mut standing_readers) = (Vec::new(), Vec::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if marker.is_match(&code) && rel != "crates/delphi-engine/src/library_claims.rs" {
            mark_callers.push(rel.clone());
        }
        if standing.is_match(&code) {
            standing_readers.push(rel);
        }
    }
    assert!(mark_callers.is_empty(), "something other than the library names the contradiction mark: {mark_callers:?}");
    assert!(standing_readers.is_empty(), "something reads a source's standing, which no path may do yet: {standing_readers:?}");
}
