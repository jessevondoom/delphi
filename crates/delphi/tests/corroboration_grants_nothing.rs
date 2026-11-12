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
