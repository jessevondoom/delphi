//! `delphi research migrate` (`specs/112` §3.4): the claims a workspace's `research/host-claims.jsonl` lists, written as library
//! claims.
//!
//! Before claims reached the library, a host research run kept its claims only as lines of that file, where nothing but the
//! duplicate check read them. This reads the file once and writes the library claims it implies, by the same call a run now makes
//! (`Library::add_research_claim`): the status is left to be derived (a migrated claim reads *noted*), a claim whose fingerprint the
//! library already carries gains anchors instead of a second claim, and running it again changes nothing more.
//!
//! **Dry run by default.** [`plan`] only reads: the file and the library. Nothing is written until [`apply`] is called, which the
//! command does only with `--apply`. The file itself is never changed: it stays the append-only log of what each run extracted.
//!
//! **What a line gives.** Its fingerprint, its statement (subject, predicate, object) and one quote, which the run found in the
//! text of the line's first source. So the first source gets an anchor with that quote; a further source gets one only when the
//! library holds its text and the quote is in it letter for letter. A source the library no longer holds gets no anchor, and the
//! plan says how many. A line is data: nothing in it is read as a status, a trust level or an instruction, and its `status`,
//! `trust` and `tags` fields are not copied.

use crate::library::claims::{AnchorSpec, ResearchClaim};
use crate::library::Library;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `<workspace>/research/host-claims.jsonl`.
pub fn ledger_path(ws: &Path) -> PathBuf {
    ws.join("research").join("host-claims.jsonl")
}

/// One library claim the ledger implies.
#[derive(Debug, Clone)]
pub struct Planned {
    pub fingerprint: String,
    pub text: String,
    pub anchors: Vec<AnchorSpec>,
    /// The library claim that already carries the fingerprint: this one is merged into it.
    pub merge_into: Option<String>,
    /// Sources the line names that the library does not hold, or holds without a way to place the quote: no anchor for them.
    pub unanchored: usize,
    /// The research topic the line names, when it names one (lines written since `specs/112`).
    pub topic: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// Whether the workspace has a ledger at all.
    pub found: bool,
    pub lines: usize,
    pub claims: Vec<Planned>,
    /// Lines that give no claim, by a fixed reason.
    pub skipped: BTreeMap<&'static str, usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub written: usize,
    pub merged: usize,
    pub failed: usize,
}

fn field(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

/// Read the ledger and the library and say what would be written. It writes nothing.
pub fn plan(ws: &Path, lib: &Library) -> Plan {
    let mut plan = Plan::default();
    let Ok(text) = std::fs::read_to_string(ledger_path(ws)) else { return plan };
    plan.found = true;
    let mut by_fp: BTreeMap<String, Planned> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        plan.lines += 1;
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            *plan.skipped.entry("not_json").or_default() += 1;
            continue;
        };
        let fp = field(&v, "fingerprint");
        if fp.len() < 12 || fp.len() > 64 || !fp.bytes().all(|b| b.is_ascii_hexdigit()) {
            *plan.skipped.entry("no_fingerprint").or_default() += 1;
            continue;
        }
        let statement = [field(&v, "subject"), field(&v, "predicate"), field(&v, "object")].join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
        if field(&v, "subject").is_empty() || field(&v, "predicate").is_empty() || field(&v, "object").is_empty() {
            *plan.skipped.entry("no_statement").or_default() += 1;
            continue;
        }
        let quote = field(&v, "quote");
        let sources: Vec<String> = v["source_ids"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(String::from)).collect();
        let urls: Vec<String> = v["source_urls"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(String::from)).collect();
        let topic = v["topic"].as_str().filter(|t| crate::research_record::valid_topic_id(t)).map(String::from);
        let p = by_fp.entry(fp.clone()).or_insert_with(|| {
            order.push(fp.clone());
            Planned { fingerprint: fp.clone(), text: statement.clone(), anchors: Vec::new(), merge_into: lib.claim_by_fingerprint(&fp).and_then(|c| c["id"].as_str().map(String::from)), unanchored: 0, topic: topic.clone() }
        });
        for (i, source_id) in sources.iter().enumerate() {
            if p.anchors.iter().any(|a| &a.source_id == source_id) {
                continue;
            }
            let held = lib.read_meta(source_id).filter(|m| m["purged"].is_null()).is_some();
            let in_text = held && lib.holds_quote(source_id, &quote);
            if held && !quote.is_empty() && (i == 0 || in_text) {
                p.anchors.push(AnchorSpec { source_id: source_id.clone(), locator: urls.get(i).cloned().filter(|u| !u.is_empty()), quote: quote.clone(), prefix: None, suffix: None });
            } else {
                p.unanchored += 1;
            }
        }
    }
    plan.claims = order.into_iter().filter_map(|fp| by_fp.remove(&fp)).collect();
    plan
}

/// Write the planned claims to the library. A claim that cannot be written is counted and the rest go on.
pub fn apply(plan: &Plan, lib: &Library) -> Applied {
    let mut out = Applied::default();
    for p in &plan.claims {
        match lib.add_research_claim(&ResearchClaim { text: &p.text, fingerprint: &p.fingerprint, project: None, topic: p.topic.as_deref(), anchors: &p.anchors }) {
            Ok((_, true)) => out.merged += 1,
            Ok((_, false)) => out.written += 1,
            Err(_) => out.failed += 1,
        }
    }
    out
}

pub fn to_json(plan: &Plan, applied: Option<&Applied>) -> Value {
    json!({
        "ledger": plan.found, "lines": plan.lines, "applied": applied.is_some(),
        "claims": plan.claims.iter().map(|p| json!({"fingerprint": p.fingerprint, "text": p.text, "anchors": p.anchors.len(), "unanchored_sources": p.unanchored, "merge_into": p.merge_into, "topic": p.topic})).collect::<Vec<_>>(),
        "skipped": plan.skipped,
        "result": applied.map(|a| json!({"written": a.written, "merged": a.merged, "failed": a.failed})),
    })
}

/// What the command prints: first what it would do, and with `applied`, what it did.
pub fn render(plan: &Plan, applied: Option<&Applied>) -> String {
    if !plan.found {
        return "no research/host-claims.jsonl in this workspace: there is nothing to migrate\n".into();
    }
    let new = plan.claims.iter().filter(|p| p.merge_into.is_none()).count();
    let anchors: usize = plan.claims.iter().map(|p| p.anchors.len()).sum();
    let unanchored: usize = plan.claims.iter().map(|p| p.unanchored).sum();
    let mut out = format!("research/host-claims.jsonl: {} line(s), {} claim(s)\n", plan.lines, plan.claims.len());
    out.push_str(&format!("  {} would be written as new library claims (each reads noted)\n", new));
    out.push_str(&format!("  {} would be merged into a claim the library already holds\n", plan.claims.len() - new));
    out.push_str(&format!("  {} anchor(s) into sources the library holds; {} source(s) it does not hold get none\n", anchors, unanchored));
    for (why, n) in &plan.skipped {
        out.push_str(&format!("  {} line(s) skipped: {}\n", n, why));
    }
    for p in plan.claims.iter().take(20) {
        let text: String = p.text.chars().take(100).collect();
        out.push_str(&format!("    {} {}  ({} anchor(s))\n", if p.merge_into.is_some() { "merge" } else { "new  " }, text, p.anchors.len()));
    }
    if plan.claims.len() > 20 {
        out.push_str(&format!("    ... and {} more\n", plan.claims.len() - 20));
    }
    match applied {
        None => out.push_str("nothing was changed: this was a dry run. Run it again with --apply to write the claims.\n"),
        Some(a) => out.push_str(&format!("applied: {} claim(s) written, {} merged, {} failed. The file itself is unchanged.\n", a.written, a.merged, a.failed)),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{AddOpts, Budgets, Level};

    /// Every file under `dir` with its bytes, in a stable order: what "writes nothing" is checked against.
    fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push((p.strip_prefix(dir).unwrap().to_string_lossy().to_string(), std::fs::read(&p).unwrap()));
                }
            }
        }
        out.sort();
        out
    }

    /// A source the library holds whole (an open licence), with `text`.
    fn source(lib: &Library, files: &Path, name: &str, text: &str) -> String {
        let f = files.join(format!("{name}.md"));
        std::fs::write(&f, text).unwrap();
        lib.add_file(&f, &AddOpts { licence: Some("CC0-1.0".into()), level: Some(Level::Full), ..Default::default() }, &Budgets::default()).unwrap().id
    }

    fn line(fp: &str, object: &str, quote: &str, sources: &[&str]) -> String {
        json!({"fingerprint": fp, "id": fp.get(..12).unwrap_or(fp), "subject": "aspirin", "predicate": "reduces", "object": object, "scope": "adults", "time": "", "quote": quote,
            "source_ids": sources, "source_urls": sources.iter().map(|s| format!("https://example.org/{s}")).collect::<Vec<_>>(), "status": "validated", "trust": "user_ratified", "retrieved": "2026-11-01", "requested_by": "host:x/y",
            "tags": {"origins": 9, "trusted_sources": 9, "contradicted": false}})
        .to_string()
    }

    const FP_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const FP_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    struct Fx {
        home: tempfile::TempDir,
        ws: tempfile::TempDir,
        _files: tempfile::TempDir,
        s1: String,
        s2: String,
    }

    fn fx() -> Fx {
        let (home, ws, files) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let lib = Library::in_home(home.path());
        let s1 = source(&lib, files.path(), "one", "Aspirin lowers fever in adults, the trial found.");
        let s2 = source(&lib, files.path(), "two", "A second report: aspirin lowers fever in adults and eases pain.");
        std::fs::create_dir_all(ws.path().join("research")).unwrap();
        let ledger = [
            line(FP_A, "fever", "lowers fever in adults", &[&s1, &s2, "s-gone"]),
            "this line is not json".to_string(),
            line("nothex", "x", "q", &[&s1]),
            json!({"fingerprint": FP_B, "subject": "", "predicate": "p", "object": "o", "quote": "q", "source_ids": [s1]}).to_string(),
            line(FP_B, "pain", "eases pain", &[&s2]),
            line(FP_A, "fever", "lowers fever in adults", &[&s1]),
            String::new(),
        ];
        std::fs::write(ledger_path(ws.path()), ledger.join("\n")).unwrap();
        Fx { home, ws, _files: files, s1, s2 }
    }

    /// AC-8: without `--apply` nothing is written, in the library or in the workspace; the plan says what would be.
    #[test]
    fn the_dry_run_reads_the_ledger_and_the_library_and_writes_nothing() {
        let f = fx();
        let lib = Library::in_home(f.home.path());
        let (home_before, ws_before) = (tree(f.home.path()), tree(f.ws.path()));
        let p = plan(f.ws.path(), &lib);
        let text = render(&p, None);
        let _ = to_json(&p, None);
        assert_eq!(tree(f.home.path()), home_before, "the dry run changed the library");
        assert_eq!(tree(f.ws.path()), ws_before, "the dry run changed the workspace");
        assert!(lib.claims().is_empty());
        assert_eq!((p.found, p.lines, p.claims.len()), (true, 6, 2));
        assert_eq!(p.skipped, BTreeMap::from([("not_json", 1), ("no_fingerprint", 1), ("no_statement", 1)]));
        let a = &p.claims[0];
        assert_eq!((a.fingerprint.as_str(), a.text.as_str(), a.merge_into.is_none()), (FP_A, "aspirin reduces fever", true));
        assert_eq!(a.anchors.iter().map(|x| x.source_id.as_str()).collect::<Vec<_>>(), [f.s1.as_str(), f.s2.as_str()], "the first source, and a further one whose held text has the quote; the same line again adds nothing");
        assert_eq!(a.unanchored, 1, "a source the library does not hold gets no anchor");
        assert_eq!(p.claims[1].anchors.len(), 1);
        for needle in ["6 line(s), 2 claim(s)", "2 would be written as new library claims", "0 would be merged", "3 anchor(s)", "1 source(s) it does not hold", "skipped: not_json", "nothing was changed: this was a dry run", "--apply"] {
            assert!(text.contains(needle), "{needle}\n{text}");
        }
        // A workspace with no ledger has nothing to do, and says so.
        let empty = tempfile::tempdir().unwrap();
        let none = plan(empty.path(), &lib);
        assert!(!none.found && none.claims.is_empty());
        assert!(render(&none, None).contains("nothing to migrate"));
        assert!(tree(empty.path()).is_empty());
    }

    #[test]
    fn applying_writes_noted_library_claims_with_anchors_once_and_leaves_the_ledger_alone() {
        let f = fx();
        let lib = Library::in_home(f.home.path());
        let ledger_before = std::fs::read(ledger_path(f.ws.path())).unwrap();
        let p = plan(f.ws.path(), &lib);
        let done = apply(&p, &lib);
        assert_eq!(done, Applied { written: 2, merged: 0, failed: 0 });
        assert!(render(&p, Some(&done)).contains("applied: 2 claim(s) written, 0 merged, 0 failed"));
        assert_eq!(std::fs::read(ledger_path(f.ws.path())).unwrap(), ledger_before, "the ledger is a log: it is never rewritten");
        let claims = lib.claims();
        assert_eq!(claims.len(), 2);
        let fever = lib.claim_by_fingerprint(FP_A).unwrap();
        assert_eq!((fever["text"].as_str(), fever["scope"].as_str(), fever["kind"].as_str()), (Some("aspirin reduces fever"), Some("team"), Some("fact")));
        assert_eq!(fever["anchors"].as_array().unwrap().len(), 2);
        assert!(fever["anchors"].as_array().unwrap().iter().all(|a| a["quote"] == "lowers fever in adults" && a["state"] == "active" && a["start"].is_u64()), "a held source places the quote: {fever}");
        // What the line said about itself is not copied: the status is derived, and a migrated claim reads noted.
        assert!(fever.get("validation").is_none() && fever.get("trust").is_none() && fever.get("status").is_none(), "{fever}");
        assert_eq!(crate::library::claims::status(&fever).as_str(), "noted");
        assert_ne!(fever["support"]["origins"], 9, "the tags are counted by the library, not read from the line");
        // The sources know which claims cite them.
        assert!(lib.read_meta(&f.s1).unwrap()["cited_by"].as_array().unwrap().iter().any(|c| c == &fever["id"]));
        // Again: everything merges, nothing is added.
        let again = plan(f.ws.path(), &lib);
        assert!(again.claims.iter().all(|c| c.merge_into.is_some()));
        assert_eq!(apply(&again, &lib), Applied { written: 0, merged: 2, failed: 0 });
        assert_eq!(lib.claims().len(), 2);
        assert_eq!(lib.claim_by_fingerprint(FP_A).unwrap()["anchors"].as_array().unwrap().len(), 2, "an anchor is kept once");
    }
}
