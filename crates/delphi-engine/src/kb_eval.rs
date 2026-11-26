//! The golden set for retrieval (`specs/140`): a fixed, private set of questions with known answers in the operator's own corpus.
//!
//! A set is `local/eval/golden-v<n>.jsonl` in the workspace, one case a line, with a header `golden-v<n>.json` (§2.1). It is built
//! by code from pairs that already exist (a research record and its answer, a finding, a project's research file, a search that
//! found nothing), read from the index opened read-only and from the records on disk; nothing is written but the set itself. A
//! built set is a draft until the operator accepts it (`accept`), which writes its SHA-256; an accepted set is never edited, and a
//! set whose bytes no longer match its hash is refused.
//!
//! Nothing here leaves the workspace: `local/` is the daemon's and the operator's, and no agent tool reads it.

use crate::kb::Kb;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// The grades of §2.1 item 2.
pub const GRADE_ANSWER: u8 = 3;
pub const GRADE_PART: u8 = 2;
pub const GRADE_SUBJECT: u8 = 1;

/// The fewest real cases the spec asks for (§2.1 item 5); a smaller set is built with what there is, and says so.
pub const MIN_REAL: usize = 50;
pub const MIN_NONE: usize = 10;

/// How many cases `accept` shows: ten of each kind where there are ten.
pub const SAMPLE: usize = 20;

/// One relevant passage: the chunk that starts at `start_line` of `path`, and how well it answers. `tier` is the tier the index
/// holds for its document (§2.1 item 6: results can be split by it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relevant {
    pub path: String,
    pub start_line: usize,
    pub grade: u8,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tier: String,
}

/// One case of a set (§2.1 item 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Case {
    pub id: String,
    pub q: String,
    /// `real` or `synthetic`.
    pub kind: String,
    /// Where the case came from: `research:<topic>`, `research:<project>/<topic>`, `finding:<path>`, `file:<path>`, `miss:<hash>`.
    pub from: String,
    pub relevant: Vec<Relevant>,
    /// A question the corpus should not answer.
    #[serde(default)]
    pub none: bool,
}

/// The set's header file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub version: u32,
    pub built: String,
    pub index_generation: i64,
    /// Cases by kind (`real`, `synthetic`, `none`) and by origin (`research`, `finding`, `file`, `miss`).
    pub counts: BTreeMap<String, usize>,
    /// Pairs found on disk whose passages are not in the index, so the case could never be found: left out, and counted.
    #[serde(default)]
    pub skipped: usize,
    /// Fewer real or `none` cases than the spec asks for (§2.1 item 5).
    #[serde(default)]
    pub short: Vec<String>,
    /// The SHA-256 of the `.jsonl`, written when the operator accepts the set; `None` while it is a draft.
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub accepted: Option<Value>,
}

pub fn eval_dir(ws: &Path) -> PathBuf {
    ws.join("local/eval")
}

fn set_path(ws: &Path, v: u32) -> PathBuf {
    eval_dir(ws).join(format!("golden-v{v}.jsonl"))
}

fn header_path(ws: &Path, v: u32) -> PathBuf {
    eval_dir(ws).join(format!("golden-v{v}.json"))
}

fn short_hash(s: &str) -> String {
    let d = Sha256::digest(s.as_bytes());
    d.iter().take(5).map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A question as one line: control characters out, spaces closed up.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn words_key(s: &str) -> String {
    one_line(s).to_lowercase()
}

/// The passages of `rel_path` in the index, each at `grade`.
fn passages(kb: &Kb, rel_path: &str, grade: u8) -> Vec<Relevant> {
    let tier = kb.document_tier(rel_path).unwrap_or_default();
    kb.chunk_spans(rel_path).into_iter().map(|(start, _, _)| Relevant { path: rel_path.to_string(), start_line: start, grade, tier: tier.clone() }).collect()
}

/// The title a document's first chunk carries: the first part of its heading path, or the file's name in words.
fn title_of(kb: &Kb, rel_path: &str) -> String {
    let heading = kb.chunk_spans(rel_path).into_iter().map(|(_, _, h)| h).find(|h| !h.trim().is_empty()).and_then(|h| h.split(" > ").next().map(str::trim).map(String::from)).filter(|h| !h.is_empty());
    heading.unwrap_or_else(|| {
        let stem = rel_path.rsplit('/').next().unwrap_or(rel_path).trim_end_matches(".md");
        stem.replace(['-', '_'], " ")
    })
}

fn case(kind: &str, from: String, q: &str, mut relevant: Vec<Relevant>, none: bool) -> Case {
    relevant.sort_by(|a, b| (&a.path, a.start_line).cmp(&(&b.path, b.start_line)));
    relevant.dedup_by(|a, b| a.path == b.path && a.start_line == b.start_line);
    let prefix = if none { "n" } else if kind == "synthetic" { "s" } else { "r" };
    Case { id: format!("{prefix}-{}", short_hash(&from)), q: one_line(q), kind: kind.into(), from, relevant, none }
}

/// The claims a record kept, as the index holds them: any document of zone `research` named `<claim>.json` under the claims of
/// the workspace or of a project.
fn claim_passages(kb: &Kb, docs: &[(String, String)], rec: &Value) -> Vec<Relevant> {
    let ids: BTreeSet<String> = rec["claims"].as_array().into_iter().flatten().filter_map(|c| c.as_str().or_else(|| c["id"].as_str())).map(String::from).collect();
    let mut out = Vec::new();
    for (rel, zone) in docs {
        if zone != crate::research_index::ZONE || !(rel.starts_with("research/claims/") || rel.starts_with("research/projects/")) {
            continue;
        }
        let name = rel.rsplit('/').next().unwrap_or("").trim_end_matches(".json");
        if ids.contains(name) {
            out.extend(passages(kb, rel, GRADE_PART));
        }
    }
    out
}

/// What `build --real-only` found: the cases, and the pairs left out because their passages are not in the index.
pub struct Built {
    pub cases: Vec<Case>,
    pub skipped: usize,
}

/// The real cases of §2.1 item 3, from the workspace at `ws` and its index `kb` (opened read-only by the caller). Deterministic:
/// the same records and index give the same cases in the same order.
pub fn real_cases(ws: &Path, kb: &Kb) -> Built {
    let docs = kb.document_zones();
    let have: BTreeSet<&str> = docs.iter().map(|(r, _)| r.as_str()).collect();
    let mut cases: Vec<Case> = Vec::new();
    let mut skipped = 0usize;
    let mut asked: BTreeSet<String> = BTreeSet::new();
    let push = |c: Case, cases: &mut Vec<Case>, skipped: &mut usize| {
        if !c.none && c.relevant.is_empty() {
            *skipped += 1;
        } else {
            cases.push(c);
        }
    };
    // A research record: its question; its answer's passages (3) and its claims' documents (2). The workspace's topics, then each
    // registered project's, whose answers the index holds as the project's research files.
    let mut roots: Vec<(Option<String>, PathBuf)> = vec![(None, ws.to_path_buf())];
    roots.extend(crate::scope::project_entries(ws).into_iter().map(|p| (Some(p.id.as_str().to_string()), p.root)));
    for (project, root) in roots {
        let Ok(rd) = std::fs::read_dir(crate::research_record::topics_dir(&root)) else { continue };
        let mut ids: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        ids.sort();
        for id in ids {
            let Some(topic) = crate::research_record::Topic::at(&root, &id) else { continue };
            let Ok(rec) = topic.read() else { continue };
            let Some(q) = rec["question"].as_str().filter(|q| !q.trim().is_empty()) else { continue };
            let answer = match &project {
                None => format!("research/topics/{id}/answer.md"),
                Some(p) => format!("projects/{p}/files/research/topics/{id}/answer.md"),
            };
            let mut rel = if have.contains(answer.as_str()) { passages(kb, &answer, GRADE_ANSWER) } else { Vec::new() };
            rel.extend(claim_passages(kb, &docs, &rec));
            asked.insert(words_key(q));
            let from = match &project {
                None => format!("research:{id}"),
                Some(p) => format!("research:{p}/{id}"),
            };
            push(case("real", from, q, rel, false), &mut cases, &mut skipped);
        }
    }
    for (rel, zone) in &docs {
        // A finding: its question (the file's title) and the finding's file (3).
        if zone == crate::finding_record::ZONE {
            push(case("real", format!("finding:{rel}"), &title_of(kb, rel), passages(kb, rel, GRADE_ANSWER), false), &mut cases, &mut skipped);
        }
        // A project's research file: its title and its passages (3). A topic's own files are the record's, above.
        if zone == "project" && rel.split('/').nth(2) == Some("files") && rel.split('/').nth(3) == Some("research") && rel.split('/').nth(4) != Some("topics") {
            push(case("real", format!("file:{rel}"), &title_of(kb, rel), passages(kb, rel, GRADE_ANSWER), false), &mut cases, &mut skipped);
        }
    }
    // A search that found nothing: a `none` case, unless later research asked the same question (then the record's case stands).
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (q, ..) in crate::kb::unanswered(ws) {
        let key = words_key(&q);
        if key.is_empty() || asked.contains(&key) || !seen.insert(key.clone()) {
            continue;
        }
        push(case("real", format!("miss:{}", short_hash(&key)), &q, Vec::new(), true), &mut cases, &mut skipped);
    }
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    cases.dedup_by(|a, b| a.id == b.id);
    Built { cases, skipped }
}

/// The set's lines: one case a line, in id order.
pub fn to_jsonl(cases: &[Case]) -> String {
    cases.iter().map(|c| serde_json::to_string(c).unwrap_or_default() + "\n").collect()
}

pub fn parse_jsonl(text: &str) -> Result<Vec<Case>, String> {
    text.lines().filter(|l| !l.trim().is_empty()).enumerate().map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 1))).collect()
}

fn counts(cases: &[Case]) -> BTreeMap<String, usize> {
    let mut m: BTreeMap<String, usize> = BTreeMap::new();
    for c in cases {
        *m.entry(if c.none { "none".into() } else { c.kind.clone() }).or_default() += 1;
        *m.entry(c.from.split(':').next().unwrap_or("").to_string()).or_default() += 1;
    }
    m
}

/// The versions present, newest last.
pub fn versions(ws: &Path) -> Vec<u32> {
    let mut v: Vec<u32> = std::fs::read_dir(eval_dir(ws)).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_prefix("golden-v")).and_then(|n| n.strip_suffix(".json")).and_then(|n| n.parse().ok())).collect();
    v.sort_unstable();
    v
}

pub fn read_header(ws: &Path, v: u32) -> Result<Header, String> {
    let p = header_path(ws, v);
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))
}

fn write_header(ws: &Path, h: &Header) -> Result<(), String> {
    let p = header_path(ws, h.version);
    std::fs::write(&p, serde_json::to_string_pretty(h).map_err(|e| e.to_string())? + "\n").map_err(|e| format!("{}: {e}", p.display()))
}

/// Build the real cases into a draft set: the newest version if it is still a draft, else a new one. Returns the header written.
pub fn build(ws: &Path, kb: &Kb) -> Result<Header, String> {
    let built = real_cases(ws, kb);
    let version = match versions(ws).last() {
        Some(&v) if read_header(ws, v).map(|h| h.sha256.is_none()).unwrap_or(true) => v,
        Some(&v) => v + 1,
        None => 1,
    };
    std::fs::create_dir_all(eval_dir(ws)).map_err(|e| e.to_string())?;
    let c = counts(&built.cases);
    let mut short = Vec::new();
    if built.cases.iter().filter(|c| c.kind == "real" && !c.none).count() < MIN_REAL {
        short.push(format!("fewer than {MIN_REAL} real cases with an answer"));
    }
    if c.get("none").copied().unwrap_or(0) < MIN_NONE {
        short.push(format!("fewer than {MIN_NONE} none cases"));
    }
    let h = Header { version, built: delphi_core::time::now_rfc3339(), index_generation: kb.index_generation(), counts: c, skipped: built.skipped, short, sha256: None, accepted: None };
    let p = set_path(ws, version);
    std::fs::write(&p, to_jsonl(&built.cases)).map_err(|e| format!("{}: {e}", p.display()))?;
    write_header(ws, &h)?;
    Ok(h)
}

/// The cases `accept` shows: up to ten of each kind, the rest from the other kind, chosen by the hash of their ids (fixed for a set).
pub fn sample(cases: &[Case]) -> Vec<usize> {
    let mut by: Vec<(String, usize)> = cases.iter().enumerate().map(|(i, c)| (short_hash(&c.id), i)).collect();
    by.sort();
    let real: Vec<usize> = by.iter().filter(|(_, i)| cases[*i].kind != "synthetic").map(|(_, i)| *i).collect();
    let synth: Vec<usize> = by.iter().filter(|(_, i)| cases[*i].kind == "synthetic").map(|(_, i)| *i).collect();
    let half = SAMPLE / 2;
    let mut out: Vec<usize> = real.iter().take(half).chain(synth.iter().take(half)).copied().collect();
    let rest: Vec<usize> = real.iter().skip(half).chain(synth.iter().skip(half)).copied().collect();
    out.extend(rest.into_iter().take(SAMPLE.saturating_sub(out.len())));
    out
}

/// The operator's answer for one sampled case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Keep,
    Drop,
    Fix(String),
}

fn ask(input: &mut dyn BufRead, out: &mut dyn Write, prompt: &str) -> Option<String> {
    let _ = write!(out, "{prompt}");
    let _ = out.flush();
    let mut line = String::new();
    match input.read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim().to_string()),
    }
}

/// Accept the newest draft set: show the sample, take `keep`, `drop` or `fix` for each, then apply the answers and write the hash.
/// Nothing is written unless every sampled case was answered.
pub fn accept(ws: &Path, input: &mut dyn BufRead, out: &mut dyn Write) -> Result<Header, String> {
    let v = *versions(ws).last().ok_or("no golden set has been built (`delphi kb eval build --real-only`)")?;
    let mut h = read_header(ws, v)?;
    if h.sha256.is_some() {
        return Err(format!("golden-v{v} is already accepted; build a new version to change it"));
    }
    let mut cases = parse_jsonl(&std::fs::read_to_string(set_path(ws, v)).map_err(|e| e.to_string())?)?;
    let picked = sample(&cases);
    let _ = writeln!(out, "golden-v{v}: {} case(s); {} to review. Answer keep, drop or fix for each.", cases.len(), picked.len());
    let mut verdicts: Vec<(usize, Verdict)> = Vec::new();
    for (n, &i) in picked.iter().enumerate() {
        let c = &cases[i];
        let _ = writeln!(out, "\n[{}/{}] {} ({}{}, from {})\n  question: {}", n + 1, picked.len(), c.id, c.kind, if c.none { ", should find nothing" } else { "" }, c.from, c.q);
        for r in c.relevant.iter().take(5) {
            let _ = writeln!(out, "  relevant (grade {}): {}:{}", r.grade, r.path, r.start_line);
        }
        if c.relevant.len() > 5 {
            let _ = writeln!(out, "  … and {} more passage(s)", c.relevant.len() - 5);
        }
        let verdict = loop {
            let Some(a) = ask(input, out, "keep, drop or fix? ") else {
                return Err(format!("not accepted: {} sampled case(s) were not answered; nothing was written", picked.len() - n));
            };
            match a.as_str() {
                "keep" | "k" => break Verdict::Keep,
                "drop" | "d" => break Verdict::Drop,
                "fix" | "f" => {
                    let Some(q) = ask(input, out, "the question as it should read: ") else {
                        return Err(format!("not accepted: {} sampled case(s) were not answered; nothing was written", picked.len() - n));
                    };
                    break if q.trim().is_empty() { Verdict::Keep } else { Verdict::Fix(q) };
                }
                _ => continue,
            }
        };
        verdicts.push((i, verdict));
    }
    let (mut kept, mut dropped, mut fixed) = (0, 0, 0);
    let mut gone: BTreeSet<usize> = BTreeSet::new();
    for (i, v) in verdicts {
        match v {
            Verdict::Keep => kept += 1,
            Verdict::Drop => {
                dropped += 1;
                gone.insert(i);
            }
            Verdict::Fix(q) => {
                fixed += 1;
                cases[i].q = one_line(&q);
            }
        }
    }
    let cases: Vec<Case> = cases.into_iter().enumerate().filter(|(i, _)| !gone.contains(i)).map(|(_, c)| c).collect();
    let text = to_jsonl(&cases);
    std::fs::write(set_path(ws, v), &text).map_err(|e| e.to_string())?;
    h.counts = counts(&cases);
    h.sha256 = Some(sha256_hex(text.as_bytes()));
    h.accepted = Some(json!({"at": delphi_core::time::now_rfc3339(), "sampled": picked.len(), "kept": kept, "dropped": dropped, "fixed": fixed}));
    write_header(ws, &h)?;
    Ok(h)
}

/// The newest accepted set (or version `v`), checked against its hash. A set edited after it was accepted is refused.
pub fn load_accepted(ws: &Path, v: Option<u32>) -> Result<(Header, Vec<Case>), String> {
    let v = match v {
        Some(v) => v,
        None => *versions(ws).iter().rev().find(|&&v| read_header(ws, v).is_ok_and(|h| h.sha256.is_some())).ok_or("no accepted golden set (`delphi kb eval build --real-only`, then `delphi kb eval accept`)")?,
    };
    let h = read_header(ws, v)?;
    let want = h.sha256.clone().ok_or_else(|| format!("golden-v{v} is a draft; the operator accepts it with `delphi kb eval accept`"))?;
    let bytes = std::fs::read(set_path(ws, v)).map_err(|e| e.to_string())?;
    if sha256_hex(&bytes) != want {
        return Err(format!("golden-v{v} does not match the hash it was accepted with: it was edited, so it is refused (a changed set is a new version)"));
    }
    Ok((h, parse_jsonl(&String::from_utf8_lossy(&bytes))?))
}

// ── the command (§2.3) ──────────────────────────────────────────────────────

pub const USAGE: &str = "usage: delphi kb eval build --real-only | accept [--workspace <name>] [--home <dir>]";

/// `delphi kb eval build|accept …` for the workspace at `ws`. The caller has refused these inside an agent seat.
pub fn cli(_home: &Path, ws: &Path, args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("build") => {
            if !args.iter().any(|a| a == "--real-only") {
                eprintln!("delphi kb eval build: only --real-only can be built yet (synthetic cases need the single-turn bridge, specs/143)\n{USAGE}");
                return 2;
            }
            let kb = match Kb::open_readonly(&ws.join("local/state.db")) {
                Ok(k) => k,
                Err(e) => {
                    eprintln!("delphi kb eval build: the index cannot be read ({e}); run `delphi kb index` first");
                    return 1;
                }
            };
            match build(ws, &kb) {
                Ok(h) => {
                    println!("built golden-v{} (a draft): {}; {} pair(s) left out (their passages are not in the index)", h.version, h.counts.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "), h.skipped);
                    for s in &h.short {
                        println!("  note: {s}; the set is built with what there is");
                    }
                    println!("  next: `delphi kb eval accept` (about twenty minutes; you answer keep, drop or fix for {SAMPLE} cases)");
                    0
                }
                Err(e) => {
                    eprintln!("delphi kb eval build: {e}");
                    1
                }
            }
        }
        Some("accept") => {
            let stdin = std::io::stdin();
            match accept(ws, &mut stdin.lock(), &mut std::io::stdout()) {
                Ok(h) => {
                    println!("\naccepted golden-v{}: sha256 {}", h.version, h.sha256.as_deref().unwrap_or("?"));
                    0
                }
                Err(e) => {
                    eprintln!("delphi kb eval accept: {e}");
                    1
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIER: &str = "internal_generated";

    /// A workspace with three research records (each with an answer in the index), two findings and four searches that found
    /// nothing (§AC-2).
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        let db = ws.join("local/state.db");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let mut kb = Kb::open(&db).unwrap();
        for q in ["How do herons hunt in shallow water?", "Where do ospreys nest along the estuary?", "Why do kingfishers dive from perches?"] {
            let id = crate::research_record::topic_id(q);
            let dir = crate::research_record::topics_dir(&ws).join(&id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("record.json"), serde_json::to_vec(&json!({"topic_id": id, "question": q, "private": false, "claims": []})).unwrap()).unwrap();
            kb.upsert_from(&format!("research/topics/{id}/answer.md"), crate::research_index::ZONE, TIER, &format!("# Answer\n\n{q} The answer is long.\n\n## Detail\n\nMore about it.\n"), "t").unwrap();
        }
        for (slug, q) in [("why-do-terns-hover", "Why do terns hover?"), ("what-do-gulls-eat", "What do gulls eat?")] {
            kb.upsert_from(&format!("missions/m1/findings/{slug}.md"), crate::finding_record::ZONE, TIER, &format!("# {q}\n\nConclusion (likely): fish.\n"), "t").unwrap();
        }
        for q in ["albatross wingspan", "puffin burrows", "cormorant drying wings", "pelican pouch size"] {
            crate::kb::log_miss(&ws, q, crate::kb::Searcher::Agent("scout"), 0);
        }
        (d, ws)
    }

    fn open(ws: &Path) -> Kb {
        Kb::open_readonly(&ws.join("local/state.db")).unwrap()
    }

    #[test]
    fn build_real_only_writes_one_case_per_pair_with_the_grades_of_the_spec_and_is_deterministic() {
        let (_d, ws) = fixture();
        let h = build(&ws, &open(&ws)).unwrap();
        assert_eq!((h.counts.get("research"), h.counts.get("finding"), h.counts.get("miss")), (Some(&3), Some(&2), Some(&4)));
        assert_eq!((h.counts.get("real"), h.counts.get("none")), (Some(&5), Some(&4)));
        assert!(h.sha256.is_none() && h.short.len() == 2, "a draft, and short of 50 real and 10 none cases");
        let text = std::fs::read_to_string(set_path(&ws, 1)).unwrap();
        let cases = parse_jsonl(&text).unwrap();
        for c in cases.iter().filter(|c| c.from.starts_with("research:")) {
            assert_eq!(c.relevant.len(), 2, "both passages of the answer");
            assert!(c.relevant.iter().all(|r| r.grade == GRADE_ANSWER && r.path.ends_with("/answer.md") && r.tier == TIER));
            assert!(!c.none && c.kind == "real" && c.id.starts_with("r-"));
        }
        let finding = cases.iter().find(|c| c.from == "finding:missions/m1/findings/why-do-terns-hover.md").unwrap();
        assert_eq!((finding.q.as_str(), finding.relevant[0].grade), ("Why do terns hover?", GRADE_ANSWER));
        assert!(cases.iter().filter(|c| c.none).all(|c| c.relevant.is_empty() && c.id.starts_with("n-")));
        // Twice: the same file (the header's build time aside), and still version 1 while it is a draft.
        let again = build(&ws, &open(&ws)).unwrap();
        assert_eq!(again.version, 1);
        assert_eq!(std::fs::read_to_string(set_path(&ws, 1)).unwrap(), text);
    }

    #[test]
    fn a_miss_that_later_research_asked_is_not_a_none_case() {
        let (_d, ws) = fixture();
        crate::kb::log_miss(&ws, "how do HERONS hunt in shallow water?", crate::kb::Searcher::Agent("scout"), 0);
        let built = real_cases(&ws, &open(&ws));
        assert_eq!(built.cases.iter().filter(|c| c.none).count(), 4);
    }

    fn answers(s: &str) -> std::io::Cursor<Vec<u8>> {
        std::io::Cursor::new(s.as_bytes().to_vec())
    }

    #[test]
    fn accept_writes_the_hash_only_when_every_sampled_case_is_answered_and_an_edited_set_is_refused() {
        let (_d, ws) = fixture();
        build(&ws, &open(&ws)).unwrap();
        let before = std::fs::read_to_string(set_path(&ws, 1)).unwrap();
        // Eight of nine answered: nothing is written.
        let mut out = Vec::new();
        let e = accept(&ws, &mut answers(&"keep\n".repeat(8)), &mut out).unwrap_err();
        assert!(e.contains("not accepted"), "{e}");
        assert!(read_header(&ws, 1).unwrap().sha256.is_none());
        assert_eq!(std::fs::read_to_string(set_path(&ws, 1)).unwrap(), before);
        assert!(load_accepted(&ws, None).is_err(), "a draft is not run");
        // All nine: one dropped, one fixed, an unknown word asked again.
        let script = format!("maybe\ndrop\nfix\nHow do herons fish?\n{}", "keep\n".repeat(7));
        let h = accept(&ws, &mut answers(&script), &mut Vec::new()).unwrap();
        assert_eq!(h.accepted.as_ref().unwrap()["dropped"], 1);
        assert_eq!(h.accepted.as_ref().unwrap()["fixed"], 1);
        let (_, cases) = load_accepted(&ws, None).unwrap();
        assert_eq!(cases.len(), 8);
        assert!(cases.iter().any(|c| c.q == "How do herons fish?"));
        assert!(accept(&ws, &mut answers("keep\n"), &mut Vec::new()).unwrap_err().contains("already accepted"));
        // An accepted set that is edited fails its hash.
        let p = set_path(&ws, 1);
        let edited = std::fs::read_to_string(&p).unwrap().replacen("How do herons fish?", "How do herons feed?", 1);
        std::fs::write(&p, edited).unwrap();
        assert!(load_accepted(&ws, None).unwrap_err().contains("edited"));
        // A new build after an accepted set is a new version.
        assert_eq!(build(&ws, &open(&ws)).unwrap().version, 2);
    }

    #[test]
    fn the_sample_takes_ten_of_each_kind_and_fills_from_the_other() {
        let mk = |i: usize, kind: &str| Case { id: format!("x{i}"), q: String::new(), kind: kind.into(), from: format!("f{i}"), relevant: vec![], none: false };
        let mut cases: Vec<Case> = (0..30).map(|i| mk(i, "real")).collect();
        cases.extend((30..34).map(|i| mk(i, "synthetic")));
        let s = sample(&cases);
        assert_eq!(s.len(), SAMPLE);
        assert_eq!(s.iter().filter(|&&i| cases[i].kind == "synthetic").count(), 4);
        assert_eq!(sample(&cases[..5]).len(), 5);
    }
}
