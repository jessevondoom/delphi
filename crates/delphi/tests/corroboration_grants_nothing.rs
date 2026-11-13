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
    ("crates/delphi-engine/src/fact_background.rs", "the background worker: reads the fast lane's order and the rungs a claim may wait at, only to choose WHICH claim the validator checks next at low priority; the check is the same for every claim, runs at most advisory, and the worker sets no rung and ratifies nothing"),
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
        // the background worker takes the next claim in the lane's order (`specs/040` §18.3); it orders spending, never a claim's standing
        "crates/delphi-engine/src/fact_background.rs",
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
fn an_archived_copy_is_a_locator_that_only_the_library_the_command_and_the_settings_name() {
    // `specs/101` §5 and §12 item 6: a copy taken from a web archive adds no origin and no trusted source, and nothing reads the mark,
    // the lookup or its outcome as a criterion for promotion, trust, tool access or approval. The library stores and skips it
    // (`library_archive.rs`, `library_claims.rs`), the registry lists its channel (`egress.rs`, `library.rs`), the operator's command and the
    // settings carry it (`sources_cmd.rs`, `engine.rs`, `boot.rs`). Any other file naming it fails here and a reviewer has to ask why.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let uses = Regex::new(r#"archived_copy|ArchiveOutcome|ArchiveConfig|add_archived_copy|add_archived_url|ArchiveLookup|\["archived"\]"#).unwrap();
    let allowed = [
        "crates/delphi-engine/src/library_archive.rs",
        "crates/delphi-engine/src/library_claims.rs",
        "crates/delphi-engine/src/library.rs",
        "crates/delphi-engine/src/egress.rs",
        "crates/delphi-engine/src/engine.rs",
        "crates/delphi-engine/src/boot.rs",
        "crates/delphi/src/sources_cmd.rs",
    ];
    let mut stray = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if uses.is_match(&code) && !allowed.contains(&rel.as_str()) {
            stray.push(rel);
        }
    }
    assert!(stray.is_empty(), "an archived copy, its lookup or its outcome is named outside the files that may: {stray:?}");
}

#[test]
fn a_chain_result_is_a_lead_that_only_the_run_the_live_side_and_the_settings_name() {
    // `specs/101` §6 and §12 item 6: what citation chaining finds is a `noted` lead; it adds no origin, moves no status, and nothing reads
    // it, the index's answer or the chain's bounds as a criterion for promotion, trust, tool access or approval. The module parses the
    // answer (`research_chain.rs`), the run keeps leads (`research_run.rs`), the live side asks the index and guards the seed
    // (`research_host.rs`), the channel is registered (`library.rs`, `egress.rs`), the log names its purpose (`search_log.rs`), and the
    // settings carry the bounds (`engine.rs`, `boot.rs`, `service.rs`). Any other file naming it fails here and a reviewer has to ask why.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let uses = Regex::new(r#"research_chain|ChainPlan|ChainConfig|CitationLookup|chainable_doi|Purpose::Chain|chain_pass|LiveCites|DoiResolve\b|resolve_url|parse_handle|read_chained|chain_max_pages"#).unwrap();
    let allowed = [
        "crates/delphi-engine/src/research_chain.rs",
        "crates/delphi-engine/src/research_run.rs",
        "crates/delphi-engine/src/research_host.rs",
        "crates/delphi-engine/src/library.rs",
        "crates/delphi-engine/src/egress.rs",
        "crates/delphi-engine/src/search_log.rs",
        "crates/delphi-engine/src/engine.rs",
        "crates/delphi-engine/src/boot.rs",
        "crates/delphi-engine/src/service.rs",
        "crates/delphi-engine/src/lib.rs",
    ];
    let mut stray = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if uses.is_match(&code) && !allowed.contains(&rel.as_str()) {
            stray.push(rel);
        }
    }
    assert!(stray.is_empty(), "citation chaining, its channel or its bounds are named outside the files that may: {stray:?}");
}

#[test]
fn a_contradiction_mark_is_made_only_by_the_library_and_nothing_reads_a_sources_standing() {
    // `specs/100` §9.1. `contradicts_validated` lowers only the contradicting claim, never the validated fact, and only the library names it.
    // The validator's contradiction step offers the knowledge base's validated facts to its judge (`ValidatedFacts`: the trait lives in
    // `fact_run.rs`, the library implements it) and the validator's own loop (`fact_serve.rs`) hands the ids the judge read as saying the opposite
    // to `Library::record_contradictions`, the only route to the mark. No tool, host field or model reaches either. `standing()` is the only
    // history that may ever count toward a source's standing (claims the operator accepted one at a time); nothing reads it, because a source
    // becomes a trusted source only by the operator's own edit of the list (`specs/066` §5). A caller of any of these has to be added to this
    // test on purpose.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let marker = Regex::new(r"contradicts_validated\(").unwrap();
    let recorder = Regex::new(r"record_contradictions\(").unwrap();
    let source_trait = Regex::new(r"\bValidatedFacts\b").unwrap();
    let standing = Regex::new(r"\.standing\(\)").unwrap();
    let (mut mark_callers, mut recorders, mut trait_users, mut standing_readers) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if marker.is_match(&code) && rel != "crates/delphi-engine/src/library_claims.rs" {
            mark_callers.push(rel.clone());
        }
        if recorder.is_match(&code) && !["crates/delphi-engine/src/library_claims.rs", "crates/delphi-engine/src/fact_serve.rs"].contains(&rel.as_str()) {
            recorders.push(rel.clone());
        }
        if source_trait.is_match(&code) && !["crates/delphi-engine/src/fact_run.rs", "crates/delphi-engine/src/library_claims.rs", "crates/delphi-engine/src/fact_serve.rs"].contains(&rel.as_str()) {
            trait_users.push(rel.clone());
        }
        if standing.is_match(&code) {
            standing_readers.push(rel);
        }
    }
    assert!(mark_callers.is_empty(), "something other than the library names the contradiction mark: {mark_callers:?}");
    assert!(recorders.is_empty(), "something other than the library and the validator's loop records a contradiction: {recorders:?}");
    assert!(trait_users.is_empty(), "the validated-fact source is named outside the validator, its loop and the library: {trait_users:?}");
    assert!(standing_readers.is_empty(), "something reads a source's standing, which no path may do yet: {standing_readers:?}");
}

#[test]
fn a_normalised_identifier_is_a_matching_key_that_only_the_matching_files_name() {
    // `specs/101` §11: a normalised DOI, ORCID or ROR id is a key for matching and for merging origins, never a trust escalator. Only the
    // normaliser, the retraction match, the chain parser and the library (which hands a source's DOI to the origin count) name it; nothing
    // that decides a status, a tier, a queue order, an approval or a tool does. The ORCID and ROR normalisers have no caller yet (the library
    // records no author or publisher identifier): a caller has to be added to this test on purpose.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let uses = Regex::new(r"crate::doi::|delphi_engine::doi::|\bdoi::(normalize|from_url|orcid|ror)\b|\bdoi_of\(").unwrap();
    let people_and_orgs = Regex::new(r"\bdoi::(orcid|ror)\b|crate::doi::\{[^}]*\b(orcid|ror)\b").unwrap();
    let allowed = [
        "crates/delphi-engine/src/doi.rs",
        "crates/delphi-engine/src/retractions.rs",
        "crates/delphi-engine/src/library_retraction.rs",
        "crates/delphi-engine/src/research_chain.rs",
        "crates/delphi-engine/src/library_claims.rs",
        // slice D's seed: `chainable_doi` reads the DOI of a public, daemon-fetched source to start a chain
        "crates/delphi-engine/src/research_host.rs",
    ];
    let (mut stray, mut orcid_ror_callers) = (Vec::new(), Vec::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if uses.is_match(&code) && !allowed.contains(&rel.as_str()) {
            stray.push(rel.clone());
        }
        if people_and_orgs.is_match(&code) && rel != "crates/delphi-engine/src/doi.rs" {
            orcid_ror_callers.push(rel);
        }
    }
    assert!(stray.is_empty(), "a normalised identifier is named outside the files that may: {stray:?}");
    assert!(orcid_ror_callers.is_empty(), "something calls the ORCID or ROR normaliser, which nothing may yet: {orcid_ror_callers:?}");
    // The origin count may merge on the identifier (`Cite::work`); the files that turn counts into a label must not read it.
    for f in ["crates/delphi-core/src/knowledge_status.rs", "crates/delphi-engine/src/accept_all.rs"] {
        if let Ok(src) = std::fs::read_to_string(root.join(f)) {
            assert!(!production(&src).contains("doi"), "{f} names an identifier");
        }
    }
}

#[test]
fn the_accept_all_cards_actions_and_the_rejection_are_the_operators_and_reach_no_grant() {
    // `specs/100` §6 and §10 item 4: the decisions-pane card offers to leave the batch or to reject one claim of it (marking it `disproved`), never to
    // accept; both go through the console, which only the operator holds, and the terminal path asks for a typed word. Only the card's own module,
    // the service that answers it and the operator's command call them, and nothing reads the dismissal or the rejection as a criterion for anything.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let callers = Regex::new(r"accept_all::act\(|accept_all::reject_one\(|\bac::reject_one\(|accept_all_reject\(").unwrap();
    let ok = ["crates/delphi-engine/src/accept_all.rs", "crates/delphi-engine/src/service.rs", "crates/delphi/src/validation_tier_cmd.rs", "crates/delphi/src/main.rs"];
    let events = Regex::new(r#"accept_all\.card_dismissed|accept_all\.claim_rejected|accept_all_reject""#).unwrap();
    let events_ok = ["crates/delphi-engine/src/accept_all.rs"];
    let (mut stray, mut stray_events) = (Vec::new(), Vec::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if callers.is_match(&code) && !ok.contains(&rel.as_str()) {
            stray.push(rel.clone());
        }
        if events.is_match(&code) && !events_ok.contains(&rel.as_str()) {
            stray_events.push(rel);
        }
    }
    assert!(stray.is_empty(), "the card's actions or the rejection are called from outside the module, the service and the operator's command: {stray:?}");
    assert!(stray_events.is_empty(), "the dismissal or rejection events are named outside accept_all.rs: {stray_events:?}");
    // the terminal is checked before the `--reject` branch can run
    let main = std::fs::read_to_string(root.join("crates/delphi/src/main.rs")).unwrap();
    let at = main.find("\"accept-all\" => {").expect("the accept-all arm");
    let arm = &main[at..at + 4000];
    let (term, reject) = (arm.find("is_terminal").expect("a terminal check"), arm.find("validation_tier_cmd::accept_all_reject(").expect("the reject call"));
    assert!(term < reject, "the terminal is checked before a claim can be rejected");
    // the service answers the card only for a console client
    let svc = std::fs::read_to_string(root.join("crates/delphi-engine/src/service.rs")).unwrap();
    let at = svc.find("async fn digest_act").expect("digest_act");
    let body = &svc[at..at + 3000];
    let (console, act) = (body.find("need_console(&ctx)?").expect("the console check"), body.find("accept_all::act(").expect("the card action"));
    assert!(console < act, "only a console client reaches the card's actions");
}

#[test]
fn a_hard_failure_mark_only_lowers_what_is_shown_and_is_named_by_the_library_the_latch_and_the_validators_loop() {
    // `specs/100` §7.2 item 2 and §10: a recorded hard failure (a protected-class claim that reached vetted, a poisoned page that passed) can only trip
    // the vetted tier through the audit's own roll-back, which lowers a label and deletes nothing. It is written by the library, read by the one latch
    // function, and that function is called from the validator's own loop and the operator's accept: no tool, host field or model reaches it, and
    // nothing reads it as a criterion for promotion, trust, tool access or approval.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let names = Regex::new(r#"\.hard_failures\(|\["hard_failure"\]|hard_failure_of\(|claim_hard_failure"#).unwrap();
    let names_ok = ["crates/delphi-engine/src/library_claims.rs", "crates/delphi-engine/src/accept_all.rs"];
    let latch = Regex::new(r"latch_on_hard_failures\(").unwrap();
    let latch_ok = ["crates/delphi-engine/src/accept_all.rs", "crates/delphi-engine/src/fact_serve.rs"];
    let (mut stray_names, mut stray_latch) = (Vec::new(), Vec::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if names.is_match(&code) && !names_ok.contains(&rel.as_str()) {
            stray_names.push(rel.clone());
        }
        if latch.is_match(&code) && !latch_ok.contains(&rel.as_str()) {
            stray_latch.push(rel);
        }
    }
    assert!(stray_names.is_empty(), "the hard-failure record is named outside the library and the latch function: {stray_names:?}");
    assert!(stray_latch.is_empty(), "the hard-failure latch is called from outside the validator's loop and the operator's accept: {stray_latch:?}");
}

#[test]
fn a_history_pointer_and_the_project_timeline_are_context_that_nothing_reads_as_a_criterion_and_that_reach_nothing() {
    // `specs/102` §11 item 1 (slices S1 and S2): a pointer, a timeline entry and the report are read by no promotion, trust, tool-access or approval path,
    // and the two modules reach no network, no process and no model. So (1) only the two modules (and the line that declares them) name each other's
    // items today: the exchange record that will hold a pointer is S3, and when it lands a reviewer adds its file to the list on purpose; (2) every `use`
    // in them comes from a short list of inert crates; (3) their non-test code names no socket, process or model call.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let names = Regex::new(r"history_pointer|history_report").unwrap();
    // The exchange record (slice S3) and its redactor hold a pointer, so they name it on purpose; the test below holds them to the same limits.
    let readers_ok = ["crates/delphi-engine/src/lib.rs", "crates/delphi-engine/src/history_pointer.rs", "crates/delphi-engine/src/history_report.rs", "crates/delphi-engine/src/history_record.rs", "crates/delphi-engine/src/history_redact.rs", "crates/delphi-engine/src/history_manage.rs", "crates/delphi-engine/src/history_view.rs", "crates/delphi-engine/src/history_citation.rs"];
    let mut stray = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if names.is_match(&code) && !readers_ok.contains(&rel.as_str()) {
            stray.push(rel);
        }
    }
    assert!(stray.is_empty(), "a history pointer or the timeline is named outside its own modules; read it and add it on purpose: {stray:?}");
    let inert = ["crate::transcript", "delphi_core::event", "delphi_core::server", "hmac::", "serde::", "serde_json::", "sha2::", "std::"];
    let reach = Regex::new(r"std::net|std::process|Command::new|TcpStream|reqwest|tiny_http|tokio::|delphi_adapter|complete\(|model_call|\.send\(").unwrap();
    for rel in ["crates/delphi-engine/src/history_pointer.rs", "crates/delphi-engine/src/history_report.rs"] {
        let src = std::fs::read_to_string(root.join(rel)).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for l in code.lines().filter(|l| l.starts_with("use ")) {
            assert!(inert.iter().any(|p| l.trim_start_matches("use ").starts_with(p)), "{rel} imports from a path that is not on the inert list: {l}");
        }
        assert!(!reach.is_match(&code), "{rel} names a socket, a process or a model call");
    }
}

#[test]
fn an_exchange_record_is_context_that_nothing_reads_as_a_criterion_and_that_reaches_nothing() {
    // `specs/102` §11 items 1, 2, 3 and 5 (slice S3): the exchange record and its redactor are named only where they are wired (the engine's
    // task start and artifact log, the mission's event path, the daemon's mission open, the settings at boot, the egress enumeration and
    // the module list), they import only inert crates, they name no socket, process, model call, promotion, trust, tool-access or
    // approval path, and the record's `outcome` is read by nothing but the record itself (no count, ratio or trend of it exists).
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let names = Regex::new(r"history_record|history_redact").unwrap();
    let wired = [
        "crates/delphi-engine/src/lib.rs",
        "crates/delphi-engine/src/history_record.rs",
        "crates/delphi-engine/src/history_redact.rs",
        "crates/delphi-engine/src/history_view.rs",
        "crates/delphi-engine/src/engine.rs",
        "crates/delphi-engine/src/mission.rs",
        "crates/delphi-engine/src/service.rs",
        "crates/delphi-engine/src/boot.rs",
        "crates/delphi-engine/src/egress.rs",
        // Slice S4: the operator's controls read the store's records to count, verify and erase them (`history_manage`), the install
        // question gates the append (`history_consent` is named by the record, not the reverse), and the command and `doctor` call them.
        "crates/delphi-engine/src/history_manage.rs",
        "crates/delphi-engine/src/history_consent.rs",
        "crates/delphi/src/history_cmd.rs",
        "crates/delphi/src/doctor.rs",
    ];
    let mut stray = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if names.is_match(&code) && !wired.contains(&rel.as_str()) {
            stray.push(rel);
        }
    }
    assert!(stray.is_empty(), "the exchange record is named outside the files that wire it; read it and add it on purpose: {stray:?}");
    let inert = ["crate::history_pointer", "crate::history_redact", "crate::history_report", "crate::library::Channel", "delphi_core::event", "delphi_core::redact", "regex::", "serde::", "serde_json::", "std::"];
    let reach = Regex::new(r"std::net|std::process|Command::new|TcpStream|reqwest|tiny_http|tokio::|delphi_adapter|complete\(|model_call|\.send\(").unwrap();
    let grants = Regex::new(r"\b(?:promote\w*|ratif\w*|approv\w*|grant\w*|trust\w*|taint\w*|tier\w*|authoriz\w*|library_claims|knowledge_status|fact_\w*|validator\w*|score\w*|rank\w*)\b|council::|research_run").unwrap();
    for rel in ["crates/delphi-engine/src/history_record.rs", "crates/delphi-engine/src/history_redact.rs"] {
        let src = std::fs::read_to_string(root.join(rel)).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for l in code.lines().filter(|l| l.starts_with("use ")) {
            assert!(inert.iter().any(|p| l.trim_start_matches("use ").starts_with(p)), "{rel} imports from a path that is not on the inert list: {l}");
        }
        assert!(!reach.is_match(&code), "{rel} names a socket, a process or a model call");
        assert!(!grants.is_match(&code), "{rel} names a promotion, trust, tool-access or approval path: {:?}", grants.find(&code).map(|m| m.as_str().to_string()));
    }
    // The outcome is a label: only the record module reads or builds one.
    let outcome = Regex::new(r"OutcomeValue|history_record::Provenance").unwrap();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        // The operator's read-only view (S5) draws the label as a word; the test after this one holds it to drawing and counting nothing.
        if rel.ends_with("history_record.rs") || rel.ends_with("history_record_tests.rs") || rel.ends_with("history_view.rs") {
            continue;
        }
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        assert!(!outcome.is_match(&code), "{rel} names the record's outcome type: it is a label that nothing else reads");
    }
}

#[test]
fn the_merge_trigger_and_the_hosts_exchange_fields_reach_the_store_only_through_the_daemon_and_grant_nothing() {
    // `specs/102` §3.2 item 1, §3.3, §10 items 1 and 2, §11 item 5 (the S3 follow-ups). (1) The merge ledger reader is named only by the module
    // list and the daemon's own timer; (2) `delphi merge` (the CLI's merge flow) names no history module at all, so it cannot write the daemon-only store;
    // (3) the reader imports only `std` and `serde_json`, names no socket, process or model call and no promotion, trust, tool-access or approval
    // path, and writes nothing; (4) the host's exchange entry points are called only by the daemon's host path, the mission that holds the tap and the
    // tap itself, so a record can be completed by a trigger the daemon saw and made by no one else.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let strip = |src: &str| -> String { production(src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n") };
    let reader = Regex::new(r"history_merge::|mod history_merge").unwrap();
    let reader_ok = ["crates/delphi-engine/src/lib.rs", "crates/delphi-engine/src/history_merge.rs", "crates/delphi-engine/src/service.rs"];
    let exchange = Regex::new(r"on_host_exchange|history_host_exchange").unwrap();
    let exchange_ok = ["crates/delphi-engine/src/history_record.rs", "crates/delphi-engine/src/history_record_tests.rs", "crates/delphi-engine/src/mission.rs", "crates/delphi-engine/src/service.rs"];
    let (mut stray_reader, mut stray_exchange, mut merge_names_history) = (Vec::new(), Vec::new(), Vec::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let code = strip(&std::fs::read_to_string(f).unwrap());
        if reader.is_match(&code) && !reader_ok.contains(&rel.as_str()) {
            stray_reader.push(rel.clone());
        }
        if exchange.is_match(&code) && !exchange_ok.contains(&rel.as_str()) {
            stray_exchange.push(rel.clone());
        }
        if rel.starts_with("crates/delphi/src/merge") && code.contains("history_") {
            merge_names_history.push(rel);
        }
    }
    assert!(stray_reader.is_empty(), "the merge ledger reader is named outside the daemon's timer: {stray_reader:?}");
    assert!(stray_exchange.is_empty(), "the host's exchange entry points are named outside the daemon's host path: {stray_exchange:?}");
    assert!(merge_names_history.is_empty(), "the merge command must not name the history store: {merge_names_history:?}");
    let src = std::fs::read_to_string(root.join("crates/delphi-engine/src/history_merge.rs")).unwrap();
    let code = strip(&src);
    for l in code.lines().filter(|l| l.starts_with("use ")) {
        let path = l.trim_start_matches("use ");
        assert!(["std::", "serde_json::"].iter().any(|p| path.starts_with(p)), "history_merge imports from a path that is not on the inert list: {l}");
    }
    let reach = Regex::new(r"std::net|std::process|Command::new|TcpStream|reqwest|tiny_http|tokio::|delphi_adapter|complete\(|model_call|\.send\(").unwrap();
    let grants = Regex::new(r"\b(?:promote\w*|ratif\w*|approv\w*|grant\w*|trust\w*|taint\w*|tier\w*|authoriz\w*|library_claims|knowledge_status|fact_\w*|validator\w*|score\w*|rank\w*)\b|council::|research_run").unwrap();
    let writes = Regex::new(r"fs::write|File::create|OpenOptions|create_dir|remove_|rename|set_permissions|write_all").unwrap();
    assert!(!reach.is_match(&code), "history_merge names a socket, a process or a model call");
    assert!(!grants.is_match(&code), "history_merge names a promotion, trust, tool-access or approval path: {:?}", grants.find(&code).map(|m| m.as_str().to_string()));
    assert!(!writes.is_match(&code), "history_merge writes or changes a file: it only reads the ledger");
    // The daemon's timer reads the ledger only when history is on, and hands the tap only the reduced row.
    let svc = std::fs::read_to_string(root.join("crates/delphi-engine/src/service.rs")).unwrap();
    let tick = svc.split("async fn merge_ledger_tick").nth(1).and_then(|t| t.split("\n    }\n").next()).expect("the timer is in service.rs");
    assert!(tick.contains("settings.history.enabled"), "the merge ledger is read only while history is on");
    // The host's exchange fields never name what the daemon computes.
    let rec = std::fs::read_to_string(root.join("crates/delphi-engine/src/history_record.rs")).unwrap();
    let keys = rec.split("pub const HOST_EXCHANGE_KEYS").nth(1).and_then(|t| t.split("= [").nth(1)).and_then(|t| t.split(']').next()).expect("the key list is in history_record.rs");
    assert!(keys.contains("\"prompt\"") && keys.contains("\"did\""), "the key list was not read: {keys}");
    for daemon_only in ["kind", "observed", "private", "chain", "hash", "pointer", "\"id\"", "\"at\"", "project", "mission", "trigger", "outcome"] {
        assert!(!keys.contains(daemon_only), "the host's exchange keys include a field the daemon computes: {daemon_only}");
    }
}

#[test]
fn the_citation_check_is_derived_context_that_no_promotion_trust_tool_or_approval_path_calls_and_that_it_never_calls() {
    // `specs/102` §6 and §11 item 8 (slice S6): whether a decision cites a span that resolves is worked out when it is asked and is shown. (1) Only the
    // module, the module list and the daemon's `decisions.list` answer name it; (2) it imports only inert crates and the two history modules it reads;
    // (3) it names no socket, process or model call and no promotion, trust, tool-access or approval path, so a `cited` decision is exactly as
    // approvable, ratifiable and trusted as an `unsourced` one; (4) it writes nothing; (5) the `citation` field is read only to print it, in the one file
    // that renders decisions.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let names = Regex::new(r"history_citation").unwrap();
    let wired = ["crates/delphi-engine/src/lib.rs", "crates/delphi-engine/src/history_citation.rs", "crates/delphi-engine/src/service.rs"];
    let mut stray = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if names.is_match(&code) && !wired.contains(&rel.as_str()) {
            stray.push(rel);
        }
    }
    assert!(stray.is_empty(), "the citation check is named outside the files that wire it; read it and add it on purpose: {stray:?}");
    let rel = "crates/delphi-engine/src/history_citation.rs";
    let src = std::fs::read_to_string(root.join(rel)).unwrap();
    let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    let inert = ["crate::history_pointer", "crate::history_report", "delphi_core::event", "serde_json::", "std::"];
    for l in code.lines().filter(|l| l.starts_with("use ")) {
        assert!(inert.iter().any(|p| l.trim_start_matches("use ").starts_with(p)), "{rel} imports from a path that is not on the inert list: {l}");
    }
    let reach = Regex::new(r"std::net|std::process|Command::new|TcpStream|reqwest|tiny_http|tokio::|delphi_adapter|complete\(|model_call|\.send\(").unwrap();
    assert!(!reach.is_match(&code), "{rel} names a socket, a process or a model call");
    let grants = Regex::new(r"\b(?:promote\w*|ratif\w*|approv\w*|grant\w*|trust\w*|taint\w*|tier\w*|authoriz\w*|library_claims|knowledge_status|fact_\w*|validator\w*|score\w*|rank\w*)\b|council::|research_run").unwrap();
    assert!(!grants.is_match(&code), "{rel} names a promotion, trust, tool-access or approval path: {:?}", grants.find(&code).map(|m| m.as_str().to_string()));
    let writes = Regex::new(r"fs::write|File::create|OpenOptions|create_dir|remove_file|rename\(").unwrap();
    assert!(!writes.is_match(&code), "{rel} writes a file: a citation is derived, never stored");
    let field = Regex::new(r#"\["citation"\]"#).unwrap();
    let readers_ok = ["crates/delphi-engine/src/history_citation.rs", "crates/delphi/src/decisions.rs"];
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if field.is_match(&code) && !readers_ok.contains(&rel.as_str()) {
            stray.push(rel);
        }
    }
    assert!(stray.is_empty(), "a decision's `citation` field is read outside the module that makes it and the console view that prints it: {stray:?}");
}

#[test]
fn the_operators_history_view_only_reads_draws_and_counts_nothing_that_grants() {
    // `specs/102` §5.1 and §11 items 1, 2 and 11 (slice S5): the console's view of the records has no model and no agent in it. So (1) its
    // non-test code names no socket, process or model call; (2) it writes nothing (no file is created, written, renamed or removed, and no key
    // is made: it reads `rec::read`, which only reads); (3) it names no promotion, trust, tool-access or approval path; (4) the only counts it
    // makes are how many records verified, did not, or are hidden by the list's length; the outcome is drawn as a word and never counted;
    // (5) the daemon method that serves it is for the console scope alone.
    let root = root();
    let rel = "crates/delphi-engine/src/history_view.rs";
    let src = std::fs::read_to_string(root.join(rel)).unwrap();
    let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    let inert = ["crate::history_record", "crate::history_report", "serde_json::", "std::path"];
    for l in code.lines().filter(|l| l.starts_with("use ")) {
        assert!(inert.iter().any(|p| l.trim_start_matches("use ").starts_with(p)), "{rel} imports from a path that is not on the inert list: {l}");
    }
    let reach = Regex::new(r"std::net|std::process|Command::new|TcpStream|reqwest|tiny_http|tokio::|delphi_adapter|complete\(|model_call|\.send\(").unwrap();
    assert!(!reach.is_match(&code), "{rel} names a socket, a process or a model call");
    let writes = Regex::new(r"fs::write|fs::create|create_dir|OpenOptions|File::create|fs::remove|remove_file|remove_dir|fs::rename|fs::copy|set_permissions|load_or_create_key|append\(|Tap::new|\.on_events|on_merge_row|note_operator_prompt").unwrap();
    assert!(!writes.is_match(&code), "{rel} writes, makes a key, or feeds the recorder: {:?}", writes.find(&code).map(|m| m.as_str().to_string()));
    let grants = Regex::new(r"\b(?:promote\w*|ratif\w*|approv\w*|grant\w*|trust\w*|taint\w*|tier\w*|authoriz\w*|library_claims|knowledge_status|fact_\w*|validator\w*|score\w*|rank\w*)\b|council::|research_run").unwrap();
    assert!(!grants.is_match(&code), "{rel} names a promotion, trust, tool-access or approval path: {:?}", grants.find(&code).map(|m| m.as_str().to_string()));
    // The outcome is read once, to draw it. No line that touches it counts, filters, sums or compares.
    let doing = Regex::new(r"count|filter|sum\(|len\(\)|max|min|sort|==|!=").unwrap();
    for l in code.lines().filter(|l| l.contains("outcome")) {
        assert!(!doing.is_match(l), "{rel} does something with the outcome besides draw it: {l}");
    }
    // Only the daemon method for the console scope reaches the view.
    let service = std::fs::read_to_string(root.join("crates/delphi-engine/src/service.rs")).unwrap();
    let at = service.find("async fn history_view(").expect("the method exists");
    let method: String = service[at..].lines().take(8).collect::<Vec<_>>().join("\n");
    assert!(method.contains("need_console(&ctx)?;"), "history.view is for the console scope alone");
    let callers = service.matches("history_view::view(").count();
    assert_eq!(callers, 1, "the view is reached by the one daemon method: {callers}");
}

#[test]
fn the_background_worker_picks_a_claim_to_check_and_grants_nothing_and_only_the_daemon_asks_for_it() {
    // `specs/040` §18.3 and `specs/100` §4: the worker chooses which claim the validator checks next and runs the same advisory check. It cannot
    // ratify (it runs at most `advisory`), it writes no status and raises no tier, and a request a span wrote cannot switch it on: the `background`
    // flag is set only by the daemon's own call. Nothing reads the worker's module to decide trust, a tier, tool access or an approval; the files
    // that name it are listed on purpose.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let module = "crates/delphi-engine/src/fact_background.rs";
    let code_of = |s: &str| -> String { production(s).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n") };
    // 1. the worker's own code names no way to grant: no ratify, no validate, no tier raise, no operator-only act
    let code = code_of(&std::fs::read_to_string(root.join(module)).unwrap());
    let grants = Regex::new(r"ratify_fact\(|validate_claim\(|Validator::Operator|restore\(|raise_latch|raise_share|unquarantine\(|record_would_ratify\(|\.accept\(|write_fact").unwrap();
    assert!(!grants.is_match(&code), "the background worker names a way to grant: {:?}", grants.find(&code).map(|m| m.as_str()));
    // 2. the module is named only by the daemon's service, the validator's loop, the two commands that show its status, and the engine's list
    let named = Regex::new(r"fact_background").unwrap();
    let named_ok = [module, "crates/delphi-engine/src/lib.rs", "crates/delphi-engine/src/service.rs", "crates/delphi-engine/src/fact_serve.rs", "crates/delphi/src/doctor.rs", "crates/delphi/src/validation_audit_cmd.rs"];
    // 3. the request flag is built in one place, by the daemon, and the span-facing entry clears it
    let flag = Regex::new(r#""background"\s*:\s*true"#).unwrap();
    let flag_ok = ["crates/delphi-engine/src/fact_serve.rs"];
    let (mut stray_named, mut stray_flag) = (Vec::new(), Vec::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let c = code_of(&std::fs::read_to_string(f).unwrap());
        if named.is_match(&c) && !named_ok.contains(&rel.as_str()) {
            stray_named.push(rel.clone());
        }
        if flag.is_match(&c) && !flag_ok.contains(&rel.as_str()) {
            stray_flag.push(rel);
        }
    }
    assert!(stray_named.is_empty(), "something outside the daemon, the validator's loop and the status commands names the background worker: {stray_named:?}");
    assert!(stray_flag.is_empty(), "a request is built with the daemon-only background flag outside the validator's loop: {stray_flag:?}");
    let serve = std::fs::read_to_string(root.join("crates/delphi-engine/src/fact_serve.rs")).unwrap();
    assert!(production(&serve).contains(r#"req["background"] = json!(false);"#), "the entry a span's request takes must clear the daemon-only background flag");
    assert!(production(&serve).contains("never ratifies"), "a background check is capped at advisory");
}

#[test]
fn the_private_read_mark_is_named_only_where_it_is_set_observed_cleared_and_shown_and_it_reaches_nothing() {
    // `specs/103` §8 and §4.3 (slices M1 and M2): a mark only ever raises a card, degrades or refuses; it never lowers a tier, approves or grants. So
    // (1) only the files below name the module: the readers that set it (`service.rs`), the two senders that act on it (M2), the daemon's console
    // methods, `doctor`, the operator's command and the module's own declaration; no promotion, trust, tool-access or approval file does
    // (`merge_class.rs` names the two held card kinds by string, so a relay reply can deny them and never approve); (2) `mark(` is called only by
    // the daemon's marker, and `send_record(` and `note_held(` only by the two senders (`clear(` is checked by the module's own test); (3) the module
    // names no socket, process or model call, and its `use` lines come from a short list of inert crates.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let names = Regex::new(r"private_read").unwrap();
    let named_ok = [
        "crates/delphi-engine/src/lib.rs",
        "crates/delphi-engine/src/private_read.rs",
        // slice M3: the daemon's boot installs the relay filter that withholds agent-written text (`specs/103` §4.4), and so does the test
        // fixture that stands in for the boot
        "crates/delphi-engine/src/boot.rs",
        "crates/delphi-engine/src/testkit.rs",
        // the egress table names the file that decides the Relay and Mail rows (`PATHS`)
        "crates/delphi-engine/src/egress.rs",
        "crates/delphi-engine/src/service.rs",
        "crates/delphi-engine/src/harness_run.rs",
        "crates/delphi-engine/src/connector_run.rs",
        // slice M4: the engine, when it raises a card, notes the card's mission (an id pair) so the relay's card list finds a mark kept on the mission
        "crates/delphi-engine/src/engine.rs",
        // the approval-refusal list names a payload key (`private_read_mark`) so a relay reply cannot approve a held card; it only widens what is refused
        "crates/delphi-engine/src/merge_class.rs",
        "crates/delphi/src/doctor.rs",
        "crates/delphi/src/private_read_cmd.rs",
        "crates/delphi/src/main.rs",
    ];
    let (mut stray, mut mark_callers, mut record_callers, mut held_callers) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut link_callers, mut timer_note_callers, mut woken_link_callers): (Vec<String>, Vec<String>, Vec<String>) = (Vec::new(), Vec::new(), Vec::new());
    // production code runs to the LAST `mod tests` (a file such as `service.rs` has an earlier `#[cfg(test)]` item, which `production()` would cut at)
    let production_last = |src: &str| -> String {
        let cut = src.rfind("\n#[cfg(test)]\nmod tests").or_else(|| src.rfind("\nmod tests")).unwrap_or(src.len());
        src[..cut].to_string()
    };
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production_last(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if names.is_match(&code) && !named_ok.contains(&rel.as_str()) {
            stray.push(rel.clone());
        }
        if rel != "crates/delphi-engine/src/private_read.rs" {
            if code.contains("private_read::mark(") {
                mark_callers.push(rel.clone());
            }
            if code.contains("private_read::scope_record(") {
                record_callers.push(rel.clone());
            }
            // OD-5: a parent link is written by the daemon from its own context and by nothing else
            if code.contains("private_read::link_task(") {
                link_callers.push(rel.clone());
            }
            if code.contains("private_read::note_timer_setter(") {
                timer_note_callers.push(rel.clone());
            }
            if code.contains("private_read::link_woken_task(") {
                woken_link_callers.push(rel.clone());
            }
            if code.contains("private_read::note_held(") {
                held_callers.push(rel.clone());
            }
        }
    }
    record_callers.sort();
    held_callers.sort();
    assert!(link_callers.is_empty(), "a parent link is made only inside the module, by `link_woken_task`: {link_callers:?}");
    assert_eq!(timer_note_callers, vec!["crates/delphi-engine/src/engine.rs".to_string()], "the setter of a timer is noted only where the engine sets a role's timer (its own context, never a tool argument)");
    assert_eq!(woken_link_callers, vec!["crates/delphi-engine/src/service.rs".to_string()], "a woken task is linked to its setter only where the daemon starts it");
    assert!(stray.is_empty(), "the private-read mark is named outside the files that set, observe, clear and show it; read the new file and add it on purpose: {stray:?}");
    assert_eq!(mark_callers, vec!["crates/delphi-engine/src/service.rs".to_string()], "a mark is set only by the daemon's marker for a reader in READS");
    // `merge_class.rs` is the one approval-side file that names the mark, by a string, in one place, and only to refuse: the line returns `Some(..)`
    // (a reason a relay may not approve), never a decision to allow.
    let mc = std::fs::read_to_string(root.join("crates/delphi-engine/src/merge_class.rs")).unwrap();
    let mc_code: Vec<&str> = production_last(&mc).lines().filter(|l| !l.trim_start().starts_with("//")).filter(|l| l.contains("private_read")).map(|_| "x").collect();
    assert_eq!(mc_code.len(), 1, "merge_class.rs names the mark in exactly one line");
    let mc_prod = production_last(&mc);
    let line = mc_prod.lines().find(|l| !l.trim_start().starts_with("//") && l.contains("private_read")).unwrap();
    assert!(line.contains("=> Some("), "that line only refuses: {line}");
    let senders = vec!["crates/delphi-engine/src/connector_run.rs".to_string(), "crates/delphi-engine/src/harness_run.rs".to_string()];
    assert_eq!(record_callers, senders, "the mark is read for a send by the two senders only");
    assert_eq!(held_callers, senders, "a held send is counted by the two senders only");
    // slice M4: the engine's one mention of the module is the call that notes a raised card's mission; it reads no mark, sets none, and decides nothing
    let engine = std::fs::read_to_string(root.join("crates/delphi-engine/src/engine.rs")).unwrap();
    let engine_lines: Vec<&str> = production_last(&engine).lines().filter(|l| !l.trim_start().starts_with("//")).filter(|l| l.contains("private_read")).map(|_| "x").collect();
    assert_eq!(engine_lines.len(), 2, "engine.rs names the mark module in exactly two lines");
    assert!(production_last(&engine).contains("crate::private_read::note_card_raised(&id, &who.m.id);"), "one only notes the card's mission");
    assert!(production_last(&engine).contains("crate::private_read::note_timer_setter("), "the other only notes which task set a timer (OD-5)");
    let inert = ["delphi_core::", "serde_json::", "std::"];
    let reach = Regex::new(r"std::net|std::process|Command::new|TcpStream|reqwest|tiny_http|tokio::|delphi_adapter|complete\(|model_call|\.send\(").unwrap();
    let rel = "crates/delphi-engine/src/private_read.rs";
    let src = std::fs::read_to_string(root.join(rel)).unwrap();
    let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    for l in code.lines().filter(|l| l.starts_with("use ")) {
        assert!(inert.iter().any(|p| l.trim_start_matches("use ").starts_with(p)), "{rel} imports from a path that is not on the inert list: {l}");
    }
    assert!(!reach.is_match(&code), "{rel} names a socket, a process or a model call");
}

#[test]
fn the_relay_filter_only_withholds_text_and_a_card_it_withheld_is_approved_only_at_the_console() {
    // `specs/103` §4.2 and §4.3 (slice M3): the mark now degrades one thing, the text an agent wrote on the relay stream, and refuses one thing, a
    // relay reply that allows a card whose details the relay was not shown. It lowers no tier, grants no tool and approves nothing. So (1) the
    // functions that withhold are called from one place each (the boot installs the stream filter; the daemon's card list and relay answer use the
    // other two); (2) the daemon's relay answer asks `card_withheld` BEFORE it asks the broker to resolve anything, so a withheld card cannot be
    // allowed by any reply; (3) the hub applies the filter to relay clients only, so no other scope is sent anything different.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let production_last = |src: &str| -> String {
        let cut = src.rfind("\n#[cfg(test)]\nmod tests").or_else(|| src.rfind("\nmod tests")).unwrap_or(src.len());
        src[..cut].to_string()
    };
    let (mut install, mut stream, mut list, mut asked) = (Vec::<String>::new(), Vec::<String>::new(), Vec::new(), Vec::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if rel == "crates/delphi-engine/src/private_read.rs" {
            continue;
        }
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production_last(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if code.contains("private_read::install_relay_filter(") {
            install.push(rel.clone());
        }
        if code.contains("private_read::mask_event_for_relay(") {
            stream.push(rel.clone());
        }
        if code.contains("private_read::mask_pending_for_relay(") {
            list.push(rel.clone());
        }
        if code.contains("private_read::card_withheld(") {
            asked.push(rel.clone());
        }
    }
    install.sort();
    assert_eq!(install, vec!["crates/delphi-engine/src/boot.rs".to_string(), "crates/delphi-engine/src/testkit.rs".to_string()], "the stream filter is installed by the boot and by the test fixture that stands in for it, and by nothing else");
    assert!(stream.is_empty(), "nothing outside the module calls the stream mask directly; it runs only through the installed filter: {stream:?}");
    assert_eq!(list, vec!["crates/delphi-engine/src/service.rs".to_string()], "the card list is masked by the daemon's list method only");
    assert_eq!(asked, vec!["crates/delphi-engine/src/service.rs".to_string()], "the withheld-card question is asked by the daemon's relay answer only");
    let svc = std::fs::read_to_string(root.join("crates/delphi-engine/src/service.rs")).unwrap();
    let at = svc.find("fn approval_relay_resolve").expect("the relay answer exists");
    let body = &svc[at..svc[at..].find("\n    }\n").map(|n| at + n).unwrap_or(svc.len())];
    let (asks, resolves) = (body.find("private_read::card_withheld(").expect("the relay answer asks whether the card was withheld"), body.find("broker.resolve(").expect("the relay answer resolves"));
    assert!(asks < resolves, "the withheld-card question comes before anything is resolved");
    assert!(body[..asks].rfind("if allow {").is_some_and(|i| asks - i < 200), "only an allow is refused (the question sits inside the nearest `if allow`); a deny stays open");
    let hub = std::fs::read_to_string(root.join("crates/delphi-core/src/hub.rs")).unwrap();
    let publish = &hub[hub.find("pub fn publish_event").unwrap()..hub.find("pub fn set_relay_filter").unwrap()];
    assert!(publish.contains("c.scope == Scope::Relay") && publish.matches("relay_filter").count() == 1, "the filter runs for relay clients only, once per event");
}

#[test]
fn the_history_controls_and_the_install_question_are_the_operators_and_grant_and_reach_nothing() {
    // `specs/102` §11 items 7, 10 and 15 (slice S4): `off`, `purge`, `delete`, `status` and the install question live in two engine modules
    // that read and write local files only. (1) Only the engine's module list, the record store (which checks the switches and the
    // question), the operator's command, the command table, `delphi init` and `doctor` name them: no daemon method, host tool or seat
    // path does, so nothing a host sends can reach them. (2) The one function that records an answer, and the one that proves a human
    // is answering, are called from the consent module and the operator's command and nowhere else. (3) The two modules import only
    // inert crates and name no socket, process, model call, promotion, trust, tool-access or approval path.
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let names = Regex::new(r"history_manage|history_consent").unwrap();
    let readers_ok = [
        "crates/delphi-engine/src/lib.rs",
        "crates/delphi-engine/src/history_manage.rs",
        "crates/delphi-engine/src/history_consent.rs",
        "crates/delphi-engine/src/history_record.rs",
        "crates/delphi/src/history_cmd.rs",
        "crates/delphi/src/main.rs",
        "crates/delphi/src/doctor.rs",
    ];
    let answers = Regex::new(r"record_answer\(|Human::at_terminal\(").unwrap();
    let answer_ok = ["crates/delphi-engine/src/history_consent.rs", "crates/delphi/src/history_cmd.rs"];
    let (mut stray, mut stray_answer) = (Vec::new(), Vec::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if names.is_match(&code) && !readers_ok.contains(&rel.as_str()) {
            stray.push(rel.clone());
        }
        if answers.is_match(&code) && !answer_ok.contains(&rel.as_str()) {
            stray_answer.push(rel);
        }
    }
    assert!(stray.is_empty(), "the history controls or the install question are named outside the files that wire them; read it and add it on purpose: {stray:?}");
    assert!(stray_answer.is_empty(), "the install question is answered from outside the consent module and the operator's command: {stray_answer:?}");
    // No daemon method or host tool is named for them.
    let method = Regex::new(r#""(?:host\.)?history\.(?:off|purge|delete|status|answer|consent)""#).unwrap();
    for rel in ["crates/delphi-engine/src/service.rs", "crates/delphi-engine/src/host_rpc.rs", "crates/delphi-engine/src/mcp.rs"] {
        let Ok(src) = std::fs::read_to_string(root.join(rel)) else { continue };
        assert!(!method.is_match(production(&src)), "{rel} exposes a history control as a method");
    }
    let inert = ["crate::history_pointer", "crate::history_record", "crate::history_report", "crate::history_manage", "crate::history_consent", "delphi_core::time", "serde::", "serde_json::", "std::"];
    let reach = Regex::new(r"std::net|std::process|Command::new|TcpStream|reqwest|tiny_http|tokio::|delphi_adapter|complete\(|model_call|\.send\(").unwrap();
    let grants = Regex::new(r"\b(?:promote\w*|ratif\w*|approv\w*|grant\w*|trust\w*|taint\w*|tier\w*|authoriz\w*|library_claims|knowledge_status|fact_\w*|validator\w*|score\w*|rank\w*)\b|council::|research_run").unwrap();
    for rel in ["crates/delphi-engine/src/history_manage.rs", "crates/delphi-engine/src/history_consent.rs"] {
        let src = std::fs::read_to_string(root.join(rel)).unwrap();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for l in code.lines().filter(|l| l.starts_with("use ")) {
            assert!(inert.iter().any(|p| l.trim_start_matches("use ").starts_with(p)), "{rel} imports from a path that is not on the inert list: {l}");
        }
        assert!(!reach.is_match(&code), "{rel} names a socket, a process or a model call");
        assert!(!grants.is_match(&code), "{rel} names a promotion, trust, tool-access or approval path: {:?}", grants.find(&code).map(|m| m.as_str().to_string()));
    }
}
