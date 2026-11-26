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

// ── the run (§2.2) ──────────────────────────────────────────────────────────

/// The depth a run reads: twenty results (§2.2 item 4), through `SearchOpts::cap`.
pub const RUN_CAP: usize = 20;

/// Where one returned hit falls: its path and lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Got {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
}

impl From<&crate::kb::Hit> for Got {
    fn from(h: &crate::kb::Hit) -> Got {
        Got { path: h.rel_path.clone(), start_line: h.start_line, end_line: h.end_line }
    }
}

/// The grade of each returned hit, in order: the best grade among the case's passages that it overlaps (by path and lines, never by
/// heading), each passage counted once; 0 for a hit that matches none.
pub fn grades(relevant: &[Relevant], got: &[Got]) -> Vec<u8> {
    let mut used = vec![false; relevant.len()];
    got.iter()
        .map(|g| {
            let best = relevant.iter().enumerate().filter(|(i, r)| !used[*i] && r.path == g.path && r.start_line >= g.start_line && r.start_line <= g.end_line.max(g.start_line)).max_by_key(|(_, r)| r.grade).map(|(i, r)| (i, r.grade));
            match best {
                Some((i, grade)) => {
                    used[i] = true;
                    grade
                }
                None => 0,
            }
        })
        .collect()
}

/// recall@k: the share of the case's passages in the first `k` results, out of as many as `k` results could hold (so a perfect
/// ranking scores 1 even for a document longer than `k` passages).
pub fn recall_at(g: &[u8], relevant: usize, k: usize) -> f64 {
    if relevant == 0 {
        return 0.0;
    }
    g.iter().take(k).filter(|x| **x > 0).count() as f64 / relevant.min(k) as f64
}

/// nDCG@k with graded gains and the standard log2 discount: Σ grade / log2(rank + 1), over the ideal ordering of the case's grades.
pub fn ndcg_at(g: &[u8], relevant: &[Relevant], k: usize) -> f64 {
    let dcg = |gs: &mut dyn Iterator<Item = u8>| gs.take(k).enumerate().map(|(i, x)| x as f64 / ((i + 2) as f64).log2()).sum::<f64>();
    let mut ideal: Vec<u8> = relevant.iter().map(|r| r.grade).collect();
    ideal.sort_unstable_by(|a, b| b.cmp(a));
    let idcg = dcg(&mut ideal.into_iter());
    if idcg == 0.0 {
        return 0.0;
    }
    dcg(&mut g.iter().copied()) / idcg
}

/// The rank (from 1) of the first relevant result, if any.
pub fn first_rank(g: &[u8]) -> Option<usize> {
    g.iter().position(|x| *x > 0).map(|i| i + 1)
}

/// The metrics of one path over a set (§2.2 item 2).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub cases: usize,
    pub recall10: f64,
    pub recall20: f64,
    pub ndcg10: f64,
    pub mrr: f64,
    /// `none` cases, and how many of them still returned a result.
    pub none_cases: usize,
    pub none_answered: usize,
    /// Cases a passage of which is no longer in the index: listed, not scored (§2.1 item 7).
    pub stale: Vec<String>,
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// Score a path: `results` holds, for each case, what the path returned (at most [`RUN_CAP`]). `present` says whether a passage is
/// still in the index. Returns the metrics and, per case, the rank of its first relevant passage.
pub fn score(cases: &[Case], results: &[Vec<Got>], present: &dyn Fn(&Relevant) -> bool) -> (Metrics, BTreeMap<String, Option<usize>>) {
    let mut m = Metrics::default();
    let mut ranks = BTreeMap::new();
    let (mut r10, mut r20, mut nd, mut rr) = (0.0, 0.0, 0.0, 0.0);
    for (c, got) in cases.iter().zip(results) {
        if c.none {
            m.none_cases += 1;
            m.none_answered += (!got.is_empty()) as usize;
            continue;
        }
        if !c.relevant.iter().all(present) {
            m.stale.push(c.id.clone());
            continue;
        }
        let g = grades(&c.relevant, got);
        m.cases += 1;
        r10 += recall_at(&g, c.relevant.len(), 10);
        r20 += recall_at(&g, c.relevant.len(), 20);
        nd += ndcg_at(&g, &c.relevant, 10);
        let first = first_rank(&g);
        rr += first.map_or(0.0, |r| 1.0 / r as f64);
        ranks.insert(c.id.clone(), first);
    }
    let n = m.cases.max(1) as f64;
    (m.recall10, m.recall20, m.ndcg10, m.mrr) = (round4(r10 / n), round4(r20 / n), round4(nd / n), round4(rr / n));
    (m, ranks)
}

/// A path of §2.2 item 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPath {
    Service,
    Hybrid,
    Lexical,
}

impl RunPath {
    pub fn as_str(self) -> &'static str {
        match self {
            RunPath::Service => "service",
            RunPath::Hybrid => "hybrid",
            RunPath::Lexical => "lexical",
        }
    }

    pub fn parse(s: &str) -> Option<Vec<RunPath>> {
        match s {
            "service" => Some(vec![RunPath::Service]),
            "hybrid" => Some(vec![RunPath::Hybrid]),
            "lexical" => Some(vec![RunPath::Lexical]),
            "all" => Some(vec![RunPath::Service, RunPath::Hybrid, RunPath::Lexical]),
            _ => None,
        }
    }
}

/// The search options every path of a run uses: the operator's view (§3: untrusted passages and every registered project's
/// research included, as `kb search --all-projects` reads them), twenty results, the reranker off (the service never reranks), and
/// the workspace's own ranking terms.
pub fn run_opts(ws: &Path) -> crate::kb::SearchOpts {
    let projects: Vec<(String, String)> = crate::scope::project_entries(ws).into_iter().map(|p| (p.id.as_str().to_string(), p.name)).collect();
    crate::kb::SearchOpts {
        limit: RUN_CAP,
        cap: RUN_CAP,
        include_untrusted: true,
        rerank_weight: 0.0,
        shared: (!projects.is_empty()).then(|| crate::kb::SharedResearch { projects, ..Default::default() }),
        ranking: crate::use_signals::ranking(ws),
        ..Default::default()
    }
}

/// The options as a run file records them.
fn opts_json(o: &crate::kb::SearchOpts) -> Value {
    json!({"limit": o.limit, "cap": o.cap, "include_untrusted": o.include_untrusted, "vec_weight": o.vec_weight, "natural_lex_weight": o.natural_lex_weight, "loose_weight": o.loose_weight, "rerank_weight": o.rerank_weight, "shared_projects": o.shared.as_ref().map_or(0, |s| s.projects.len()), "ranking": {"use_boost": o.ranking.use_boost, "dispute_penalty": o.ranking.dispute_penalty, "confidence_weight": o.ranking.confidence_weight}})
}

fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    Some((sorted[i] * 10.0).round() / 10.0)
}

/// One path's part of a run.
fn path_run(home: &Path, ws: &Path, db: &Path, path: RunPath, cases: &[Case], present: &dyn Fn(&Relevant) -> bool) -> Result<Value, String> {
    let opts = run_opts(ws);
    let mut results: Vec<Vec<Got>> = Vec::with_capacity(cases.len());
    let mut modes: BTreeMap<&'static str, usize> = BTreeMap::new();
    let (mut cold, mut warm): (Option<f64>, Vec<f64>) = (None, Vec::new());
    let mut stages: Vec<Value> = Vec::new();
    match path {
        RunPath::Service => {
            // The reader is forgotten first, so the first query opens it: that one is the cold one, whether or not it answered in
            // time (a late answer carries no timing of its own to say so).
            crate::retrieval::forget_reader(db);
            for (i, c) in cases.iter().enumerate() {
                let r = crate::retrieval::search(home, db, &c.q, opts.clone()).map_err(|e| e.to_string())?;
                *modes.entry(r.mode.as_str()).or_default() += 1;
                let ms = r.ms as f64;
                if i == 0 {
                    cold = Some(ms);
                } else {
                    warm.push(ms);
                }
                stages.push(json!({"id": c.id, "mode": r.mode.as_str(), "ms": ms}));
                results.push(r.hits.iter().map(Got::from).collect());
            }
        }
        RunPath::Hybrid | RunPath::Lexical => {
            let mut kb = Kb::open_readonly(db).map_err(|e| e.to_string())?;
            let opts = if path == RunPath::Hybrid {
                if !crate::kb::attach_default_readonly(home, &mut kb) {
                    return Err("the hybrid path needs the embedding model the index's vectors were made with".into());
                }
                opts
            } else {
                crate::kb::SearchOpts { legs: crate::kb::Legs::LexicalOnly, ..opts }
            };
            for c in cases {
                let t = std::time::Instant::now();
                let hits = kb.search(&c.q, &opts).map_err(|e| e.to_string())?;
                warm.push(t.elapsed().as_secs_f64() * 1000.0);
                results.push(hits.iter().map(Got::from).collect());
            }
        }
    }
    let (m, ranks) = score(cases, &results, present);
    warm.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = cases.len().max(1) as f64;
    let mut v = json!({"path": path.as_str(), "metrics": m, "ranks": ranks, "p50_ms": percentile(&warm, 0.5), "p95_ms": percentile(&warm, 0.95)});
    if path == RunPath::Service {
        v["cold_ms"] = json!(cold);
        v["modes"] = json!(modes.iter().map(|(k, c)| (k.to_string(), json!({"count": c, "share": round4(*c as f64 / n)}))).collect::<serde_json::Map<String, Value>>());
        v["vector_leg"] = json!(round4(modes.get("hybrid").copied().unwrap_or(0) as f64 / n));
        v["queries"] = json!(stages);
    }
    Ok(v)
}

/// A run's stamp: the UTC time to the second, `YYYYMMDDTHHMMSSZ`.
fn stamp() -> String {
    let now = delphi_core::time::now_rfc3339();
    let digits: String = now.chars().take(19).filter(|c| c.is_ascii_digit()).collect();
    format!("{}T{}Z", &digits[..8.min(digits.len())], &digits[8.min(digits.len())..])
}

pub fn runs_dir(ws: &Path) -> PathBuf {
    eval_dir(ws).join("runs")
}

/// Score the accepted set (§2.2) on `paths` and write the run file. The index is opened read-only; nothing else is written.
pub fn run(home: &Path, ws: &Path, paths: &[RunPath], label: Option<&str>) -> Result<(PathBuf, Value), String> {
    let (h, cases) = load_accepted(ws, None)?;
    let db = ws.join("local/state.db");
    let kb = Kb::open_readonly(&db).map_err(|e| e.to_string())?;
    let spans: BTreeSet<(String, usize)> = cases.iter().flat_map(|c| c.relevant.iter()).map(|r| (r.path.clone(), r.start_line)).filter(|(p, s)| kb.chunk_spans(p).iter().any(|(a, _, _)| a == s)).collect();
    let present = |r: &Relevant| spans.contains(&(r.path.clone(), r.start_line));
    let (documents, chunks) = kb.counts().map_err(|e| e.to_string())?;
    let generation = kb.index_generation();
    drop(kb);
    let mut out = Vec::new();
    for p in paths {
        out.push(path_run(home, ws, &db, *p, &cases, &present)?);
    }
    let stamp = stamp();
    let v = json!({
        "v": 1, "run": stamp, "label": label,
        "set": {"version": h.version, "sha256": h.sha256, "counts": h.counts},
        "corpus": {"documents": documents, "chunks": chunks, "index_generation": generation},
        "models": {"embedder": crate::embed::installed_model_id(Some(home)), "reranker": crate::embed::installed_reranker_id(Some(home))},
        "build": {"version": env!("CARGO_PKG_VERSION"), "commit": option_env!("DELPHI_GIT_COMMIT").unwrap_or("unknown")},
        "opts": opts_json(&run_opts(ws)),
        "paths": out,
    });
    std::fs::create_dir_all(runs_dir(ws)).map_err(|e| e.to_string())?;
    let file = runs_dir(ws).join(format!("{stamp}.json"));
    if file.exists() {
        return Err(format!("a run named {stamp} already exists; run again in a second"));
    }
    std::fs::write(&file, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    Ok((file, v))
}

/// The summary a terminal or a pull request shows: numbers and case ids, never a question or a passage (§2.2 item 5).
pub fn summary(v: &Value) -> String {
    let mut s = format!("run {} on golden-v{} ({} chunk(s), index generation {})\n", v["run"].as_str().unwrap_or("?"), v["set"]["version"], v["corpus"]["chunks"], v["corpus"]["index_generation"]);
    s.push_str(&format!("  {:<8} {:>6} {:>9} {:>9} {:>8} {:>6} {:>8} {:>8}\n", "path", "cases", "recall@10", "recall@20", "nDCG@10", "MRR", "p50 ms", "p95 ms"));
    for p in v["paths"].as_array().into_iter().flatten() {
        let m = &p["metrics"];
        let f = |x: &Value| x.as_f64().map_or("-".to_string(), |x| format!("{x:.4}"));
        let ms = |x: &Value| x.as_f64().map_or("-".to_string(), |x| format!("{x:.0}"));
        s.push_str(&format!("  {:<8} {:>6} {:>9} {:>9} {:>8} {:>6} {:>8} {:>8}\n", p["path"].as_str().unwrap_or("?"), m["cases"], f(&m["recall10"]), f(&m["recall20"]), f(&m["ndcg10"]), f(&m["mrr"]), ms(&p["p50_ms"]), ms(&p["p95_ms"])));
        if m["none_cases"].as_u64().unwrap_or(0) > 0 {
            s.push_str(&format!("           none cases answered: {} of {}\n", m["none_answered"], m["none_cases"]));
        }
        if let Some(st) = m["stale"].as_array().filter(|a| !a.is_empty()) {
            s.push_str(&format!("           stale (a passage is gone; not scored): {}\n", st.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")));
        }
        if p["path"] == "service" {
            let modes: Vec<String> = p["modes"].as_object().into_iter().flatten().map(|(k, c)| format!("{k} {}", c["share"])).collect();
            s.push_str(&format!("           modes: {}; cold {} ms\n", modes.join(", "), ms(&p["cold_ms"])));
        }
    }
    s
}

pub fn read_run(ws: &Path, name: &str) -> Result<Value, String> {
    let p = if name.ends_with(".json") && Path::new(name).is_file() { PathBuf::from(name) } else { runs_dir(ws).join(format!("{}.json", name.trim_end_matches(".json"))) };
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))
}

/// Two runs side by side (§2.2 item 6): the change in each metric, and the case ids whose first relevant rank moved by more than
/// three. Refused for runs on different set versions. No question text: a run file holds none.
pub fn compare(a: &Value, b: &Value) -> Result<String, String> {
    if a["set"]["version"] != b["set"]["version"] || a["set"]["sha256"] != b["set"]["sha256"] {
        return Err(format!("the runs are on different golden sets (v{} and v{}); runs are compared only on the same set", a["set"]["version"], b["set"]["version"]));
    }
    let mut s = format!("{} -> {} on golden-v{}\n", a["run"].as_str().unwrap_or("?"), b["run"].as_str().unwrap_or("?"), a["set"]["version"]);
    if a["corpus"]["index_generation"] != b["corpus"]["index_generation"] {
        s.push_str(&format!("  warning: the index changed between the runs (generation {} -> {}); the corpus, not the code, may have moved the numbers\n", a["corpus"]["index_generation"], b["corpus"]["index_generation"]));
    }
    for pa in a["paths"].as_array().into_iter().flatten() {
        let Some(pb) = b["paths"].as_array().into_iter().flatten().find(|p| p["path"] == pa["path"]) else { continue };
        s.push_str(&format!("  {}:\n", pa["path"].as_str().unwrap_or("?")));
        for k in ["recall10", "recall20", "ndcg10", "mrr"] {
            let (x, y) = (pa["metrics"][k].as_f64().unwrap_or(0.0), pb["metrics"][k].as_f64().unwrap_or(0.0));
            s.push_str(&format!("    {:<9} {:.4} -> {:.4} ({:+.4})\n", k, x, y, y - x));
        }
        for k in ["p50_ms", "p95_ms", "vector_leg"] {
            if let (Some(x), Some(y)) = (pa[k].as_f64(), pb[k].as_f64()) {
                s.push_str(&format!("    {:<9} {} -> {}\n", k, x, y));
            }
        }
        let rank = |p: &Value, id: &str| p["ranks"][id].as_u64().map(|r| r as i64).unwrap_or(RUN_CAP as i64 + 1);
        let moved: Vec<String> = pa["ranks"].as_object().into_iter().flatten().map(|(id, _)| id).filter(|id| (rank(pa, id) - rank(pb, id)).abs() > 3).map(|id| format!("{id} ({} -> {})", rank(pa, id), rank(pb, id))).collect();
        s.push_str(&format!("    moved more than three ranks: {}\n", if moved.is_empty() { "none".into() } else { moved.join(", ") }));
    }
    Ok(s)
}

/// The runs on file, oldest first: `(stamp, set version, label)`.
pub fn list_runs(ws: &Path) -> Vec<Value> {
    let mut names: Vec<String> = std::fs::read_dir(runs_dir(ws)).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(String::from)).collect();
    names.sort();
    names.into_iter().filter_map(|n| read_run(ws, &n).ok()).map(|v| json!({"run": v["run"], "set": v["set"]["version"], "label": v["label"], "paths": v["paths"].as_array().into_iter().flatten().map(|p| json!({"path": p["path"], "recall10": p["metrics"]["recall10"], "ndcg10": p["metrics"]["ndcg10"]})).collect::<Vec<_>>()})).collect()
}


// ── the history (§2.6) ──────────────────────────────────────────────────────

/// The ledger's format version. A reader accepts every older one.
pub const HISTORY_V: u32 = 1;
/// The view of the ledger that a search finds (zone `knowledge`), in the workspace.
pub const VIEW: &str = "local/knowledge/retrieval-quality.md";
/// Thresholds of a noticeable change (§2.6 item 7, OD-5).
pub const NOTICE_METRIC: f64 = 0.03;
pub const NOTICE_VECTOR_LEG: f64 = 0.10;
pub const NOTICE_P95: f64 = 1.5;
/// How many earlier runs of a series a run is set against: the median of the five before it.
pub const NOTICE_WINDOW: usize = 5;
/// How many runs of a series the view's table shows.
const VIEW_ROWS: usize = 20;

/// The ledger: `local/eval/history.jsonl`. Append-only: only [`append_history`] writes it, and never truncates it.
pub fn history_path(ws: &Path) -> PathBuf {
    eval_dir(ws).join("history.jsonl")
}

/// What started a run (§2.6 item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Manual,
    Scheduled,
    Before,
    After,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Manual => "manual",
            Trigger::Scheduled => "scheduled",
            Trigger::Before => "before",
            Trigger::After => "after",
        }
    }

    pub fn parse(s: &str) -> Option<Trigger> {
        [Trigger::Manual, Trigger::Scheduled, Trigger::Before, Trigger::After].into_iter().find(|t| t.as_str() == s)
    }
}

/// The SHA-256 of a model package, file or compiled in; computed once per process for each.
fn model_sha(src: crate::embed::ModelSource, embedded: Option<&'static [u8]>) -> Option<String> {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
    let key = match &src {
        crate::embed::ModelSource::File(p) => p.display().to_string(),
        crate::embed::ModelSource::Embedded => format!("embedded:{:p}", embedded?.as_ptr()),
        _ => return None,
    };
    if let Some(h) = SEEN.get_or_init(Default::default).lock().unwrap().get(&key) {
        return Some(h.clone());
    }
    let h = match src {
        crate::embed::ModelSource::File(p) => sha256_hex(&std::fs::read(p).ok()?),
        _ => sha256_hex(embedded?),
    };
    SEEN.get_or_init(Default::default).lock().unwrap().insert(key, h.clone());
    Some(h)
}

/// The hash of the search options of a run and the `[retrieval]` keys in effect (§2.6 item 1, `config_sha256`).
fn config_sha(ws: &Path) -> String {
    let keys = std::fs::read_to_string(ws.join("config.toml")).ok().and_then(|t| toml::from_str::<toml::Value>(&t).ok()).and_then(|v| v.get("retrieval").cloned()).and_then(|v| serde_json::to_value(v).ok()).unwrap_or(Value::Null);
    sha256_hex(json!({"opts": opts_json(&run_opts(ws)), "retrieval": keys}).to_string().as_bytes())
}

/// The ledger lines of a run file: one per path, numbers and identifiers only (§2.6 item 1).
pub fn ledger_lines(home: &Path, ws: &Path, run: &Value, trigger: Trigger, at: &str) -> Vec<Value> {
    let vectors = Kb::open_readonly(&ws.join("local/state.db")).ok().and_then(|k| k.vector_counts().ok()).map(|(v, _)| v);
    let models = json!({
        "embedder": {"id": crate::embed::installed_model_id(Some(home)), "sha256": model_sha(crate::embed::locate(Some(home)), crate::embed::embedded_bytes())},
        "reranker": {"id": crate::embed::installed_reranker_id(Some(home)), "sha256": model_sha(crate::embed::locate_reranker(Some(home)), crate::embed::embedded_reranker_bytes())},
    });
    let config = config_sha(ws);
    run["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let m = &p["metrics"];
            json!({
                "v": HISTORY_V, "at": at, "run": run["run"], "trigger": trigger.as_str(),
                "set": {"version": run["set"]["version"], "sha256": run["set"]["sha256"], "counts": run["set"]["counts"]},
                "corpus": {"documents": run["corpus"]["documents"], "chunks": run["corpus"]["chunks"], "vectors": vectors, "index_generation": run["corpus"]["index_generation"]},
                "config_sha256": config, "models": models, "build": run["build"], "path": p["path"],
                "recall10": m["recall10"], "recall20": m["recall20"], "ndcg10": m["ndcg10"], "mrr": m["mrr"],
                "p50_ms": p["p50_ms"], "p95_ms": p["p95_ms"], "vector_leg": p.get("vector_leg").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

/// Append lines to the ledger. The file is opened to append and is never truncated or rewritten (§2.6 item 3).
pub fn append_history(ws: &Path, lines: &[Value]) -> Result<(), String> {
    let path = history_path(ws);
    std::fs::create_dir_all(eval_dir(ws)).map_err(|e| e.to_string())?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    f.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

/// The ledger, read: every line that parses, and how many did not (a torn write is skipped and reported, §2.6 item 3).
#[derive(Debug, Default)]
pub struct History {
    pub lines: Vec<Value>,
    pub unreadable: usize,
}

pub fn read_history(ws: &Path) -> History {
    let mut h = History::default();
    for l in std::fs::read_to_string(history_path(ws)).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Value>(l) {
            // Every version up to this one is read; a later one is not understood and is counted.
            Ok(v) if v["v"].as_u64().is_some_and(|n| n >= 1 && n <= HISTORY_V as u64) => h.lines.push(v),
            _ => h.unreadable += 1,
        }
    }
    h
}

/// A series: one set version on one path (§2.6 item 2), in the order the ledger holds its runs.
pub fn series(lines: &[Value]) -> BTreeMap<(u64, String), Vec<&Value>> {
    let mut out: BTreeMap<(u64, String), Vec<&Value>> = BTreeMap::new();
    for l in lines {
        out.entry((l["set"]["version"].as_u64().unwrap_or(0), l["path"].as_str().unwrap_or("?").to_string())).or_default().push(l);
    }
    out
}

fn median(mut xs: Vec<f64>) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    Some(if n % 2 == 1 { xs[n / 2] } else { (xs[n / 2 - 1] + xs[n / 2]) / 2.0 })
}

/// The median of `key` over the (at most five) runs before the last of `runs`.
pub fn median_before(runs: &[&Value], key: &str) -> Option<f64> {
    let before = &runs[..runs.len().saturating_sub(1)];
    median(before.iter().rev().take(NOTICE_WINDOW).filter_map(|l| l[key].as_f64()).collect())
}

/// What moved noticeably in the last run of a series against the median of the five before it (§2.6 item 7): one phrase per
/// metric, and whether the change is for the worse. `None` when fewer than five runs came before, or nothing moved that far.
pub fn noticeable(runs: &[&Value]) -> Option<(Vec<String>, bool)> {
    let new = runs.last()?;
    if runs.len() <= NOTICE_WINDOW {
        return None;
    }
    let (mut moved, mut worse) = (Vec::new(), false);
    for (key, name, limit) in [("recall10", "recall@10", NOTICE_METRIC), ("ndcg10", "nDCG@10", NOTICE_METRIC), ("vector_leg", "the vector leg's share", NOTICE_VECTOR_LEG)] {
        if let (Some(x), Some(m)) = (new[key].as_f64(), median_before(runs, key)) {
            if (x - m).abs() >= limit - 1e-9 {
                moved.push(format!("{name} {} from {m:.4} to {x:.4}", if x < m { "fell" } else { "rose" }));
                worse |= x < m;
            }
        }
    }
    if let (Some(x), Some(m)) = (new["p95_ms"].as_f64(), median_before(runs, "p95_ms")) {
        if m > 0.0 && x >= m * NOTICE_P95 {
            moved.push(format!("p95 latency rose from {m:.0} ms to {x:.0} ms"));
            worse = true;
        }
    }
    (!moved.is_empty()).then_some((moved, worse))
}

/// What differed between two runs' lines: the build, the models, the config hash and the corpus size.
pub fn differed(a: &Value, b: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if a["build"] != b["build"] {
        out.push(format!("the build ({} {} -> {} {})", a["build"]["version"].as_str().unwrap_or("?"), a["build"]["commit"].as_str().unwrap_or("?"), b["build"]["version"].as_str().unwrap_or("?"), b["build"]["commit"].as_str().unwrap_or("?")));
    }
    if a["models"] != b["models"] {
        out.push(format!("the models ({} -> {})", a["models"]["embedder"]["id"].as_str().unwrap_or("none"), b["models"]["embedder"]["id"].as_str().unwrap_or("none")));
    }
    if a["config_sha256"] != b["config_sha256"] {
        out.push("the search options or [retrieval] keys".into());
    }
    if a["corpus"]["chunks"] != b["corpus"]["chunks"] {
        out.push(format!("the corpus ({} -> {} chunks)", a["corpus"]["chunks"], b["corpus"]["chunks"]));
    }
    out
}

/// The one-line trend of a series: the last run against the median of the five before it.
pub fn trend(runs: &[&Value]) -> String {
    let Some(last) = runs.last() else { return String::new() };
    let x = last["ndcg10"].as_f64().unwrap_or(0.0);
    match median_before(runs, "ndcg10") {
        None => format!("nDCG@10 {x:.4}, one run so far"),
        Some(m) if (x - m).abs() < NOTICE_METRIC => format!("nDCG@10 {x:.4}, steady over {} runs", runs.len()),
        Some(m) => format!("nDCG@10 {x:.4}, {} {:.4} from the median of the {} runs before it", if x < m { "down" } else { "up" }, (x - m).abs(), (runs.len() - 1).min(NOTICE_WINDOW)),
    }
}

fn series_name(set: u64, path: &str) -> String {
    format!("set v{set}, {path} path")
}

/// The rows of a series' table: the run, its trigger, the metrics, and the change in nDCG@10 from the run before.
fn table(runs: &[&Value]) -> String {
    let f = |x: &Value| x.as_f64().map_or("-".to_string(), |x| format!("{x:.4}"));
    let ms = |x: &Value| x.as_f64().map_or("-".to_string(), |x| format!("{x:.0}"));
    let mut s = String::from("| Run | Trigger | recall@10 | recall@20 | nDCG@10 | change | MRR | p95 ms | vector leg |\n| :-- | :-- | --: | --: | --: | --: | --: | --: | --: |\n");
    let from = runs.len().saturating_sub(VIEW_ROWS);
    for (i, l) in runs.iter().enumerate().skip(from) {
        let change = match (i.checked_sub(1).and_then(|j| runs[j]["ndcg10"].as_f64()), l["ndcg10"].as_f64()) {
            (Some(a), Some(b)) => format!("{:+.4}", b - a),
            _ => "-".into(),
        };
        s.push_str(&format!("| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n", l["run"].as_str().unwrap_or("?"), l["trigger"].as_str().unwrap_or("?"), f(&l["recall10"]), f(&l["recall20"]), f(&l["ndcg10"]), change, f(&l["mrr"]), ms(&l["p95_ms"]), f(&l["vector_leg"])));
    }
    s
}

/// The view (§2.6 item 5): one sentence per series and a table of its last twenty runs, rebuilt from the ledger. Numbers and run
/// stamps only. Written only inside a workspace whose `local/` exists.
pub fn write_view(ws: &Path) -> Result<bool, String> {
    if !ws.join("local").is_dir() {
        return Ok(false);
    }
    let h = read_history(ws);
    let mut s = String::from("# Retrieval quality\n\nHow well the knowledge base's search finds what the operator's golden set expects (specs/140), as measured by `delphi kb eval run` and the daemon's scheduled run. This page is a view of the ledger `local/eval/history.jsonl`, rebuilt after every run; the ledger is the record.\n");
    for ((set, path), runs) in series(&h.lines) {
        s.push_str(&format!("\n## Retrieval quality: {}\n\nSet v{set}, {path} path: {}.\n\n{}", series_name(set, &path), trend(&runs), table(&runs)));
    }
    if h.lines.is_empty() {
        s.push_str("\nNo run yet.\n");
    }
    let path = ws.join(VIEW);
    std::fs::create_dir_all(path.parent().unwrap_or(ws)).map_err(|e| e.to_string())?;
    std::fs::write(&path, s).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

/// Where the note state lives: which series already has a note, and for which direction (§2.6 item 7: one note per series and
/// direction until a run is back inside the thresholds).
fn noted_path(ws: &Path) -> PathBuf {
    eval_dir(ws).join("noticed.json")
}

/// After a run: for each series the run added to, a provisional note when the run moved noticeably and the series has no note for
/// that direction yet; a run back inside the thresholds clears the series. Returns the notes written (their logical paths).
pub fn note_changes(ws: &Path, now: &str) -> Vec<String> {
    let h = read_history(ws);
    let mut noted: BTreeMap<String, String> = std::fs::read_to_string(noted_path(ws)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut written = Vec::new();
    for ((set, path), runs) in series(&h.lines) {
        let key = format!("v{set}/{path}");
        match noticeable(&runs) {
            None => {
                noted.remove(&key);
            }
            Some((moved, worse)) => {
                let dir = if worse { "worse" } else { "better" };
                if noted.get(&key).map(String::as_str) == Some(dir) {
                    continue;
                }
                let (new, prev) = (runs[runs.len() - 1], runs[runs.len() - 2]);
                let diff = differed(prev, new);
                let text = format!(
                    "Retrieval quality, {}: {} in run {} against the median of the five runs before it; the run before it was {}. Between those two runs {}.",
                    series_name(set, &path),
                    moved.join("; "),
                    new["run"].as_str().unwrap_or("?"),
                    prev["run"].as_str().unwrap_or("?"),
                    if diff.is_empty() { "nothing recorded differed (not the build, the models, the config hash or the corpus size)".to_string() } else { format!("these differed: {}", diff.join(", ")) }
                );
                if let Ok(Some(p)) = crate::provisional_notes::keep_daemon_fact(ws, &key, &text, now) {
                    written.push(p);
                }
                noted.insert(key, dir.into());
            }
        }
    }
    if std::fs::create_dir_all(eval_dir(ws)).is_ok() {
        let _ = std::fs::write(noted_path(ws), serde_json::to_string(&noted).unwrap_or_default());
    }
    written
}

/// What follows every run, whoever started it: its ledger lines, the view, and a note on a noticeable change.
pub fn record(home: &Path, ws: &Path, run: &Value, trigger: Trigger) -> Result<Vec<String>, String> {
    let now = delphi_core::time::now_rfc3339();
    append_history(ws, &ledger_lines(home, ws, run, trigger, &now))?;
    let _ = write_view(ws);
    Ok(note_changes(ws, &now))
}

/// A run as the operator's command and the daemon make it: [`run`], then [`record`]. Returns the run file, the run, and the notes
/// a noticeable change left.
pub fn run_recorded(home: &Path, ws: &Path, paths: &[RunPath], label: Option<&str>, trigger: Trigger) -> Result<(PathBuf, Value, Vec<String>), String> {
    let (file, v) = run(home, ws, paths, label)?;
    let notes = record(home, ws, &v, trigger)?;
    Ok((file, v, notes))
}

/// The newest run's line against the median of the five before it in its series, for `delphi kb health`: `(series, nDCG@10, median)`.
pub fn health_numbers(ws: &Path) -> Option<(String, f64, Option<f64>)> {
    let h = read_history(ws);
    let newest = h.lines.iter().max_by(|a, b| a["at"].as_str().cmp(&b["at"].as_str()))?;
    let (set, path) = (newest["set"]["version"].as_u64().unwrap_or(0), newest["path"].as_str().unwrap_or("?").to_string());
    let all = series(&h.lines);
    let runs = all.get(&(set, path.clone()))?;
    Some((series_name(set, &path), newest["ndcg10"].as_f64()?, median_before(runs, "ndcg10")))
}

/// Is a scheduled run due (§2.6 item 4)? `every_days` 0 is off; otherwise when the newest line of the ledger is that old, or there is
/// none, and the daemon is idle.
pub fn scheduled_due(every_days: u32, now: i64, newest: Option<i64>, idle: bool) -> bool {
    every_days > 0 && idle && newest.is_none_or(|t| now - t >= every_days as i64 * 86_400)
}

/// The time of the ledger's newest line, in Unix seconds.
pub fn newest_run(ws: &Path) -> Option<i64> {
    read_history(ws).lines.iter().filter_map(|l| l["at"].as_str().and_then(delphi_core::time::parse_rfc3339)).max()
}

/// `delphi kb eval history`: each series as a table, with the change from run to run and a one-line trend.
pub fn history_text(ws: &Path, set: Option<u64>, path: Option<&str>) -> String {
    let h = read_history(ws);
    let mut s = String::new();
    for ((v, p), runs) in series(&h.lines).into_iter().filter(|((v, p), _)| set.is_none_or(|s| s == *v) && path.is_none_or(|q| q == p)) {
        s.push_str(&format!("{}: {}\n{}\n", series_name(v, &p), trend(&runs), table(&runs)));
    }
    if s.is_empty() {
        s.push_str("no runs in the history yet (`delphi kb eval run`)\n");
    }
    if h.unreadable > 0 {
        s.push_str(&format!("note: {} line(s) of {} could not be read and were skipped\n", h.unreadable, history_path(ws).display()));
    }
    s
}

// ── the command (§2.3) ──────────────────────────────────────────────────────

pub const USAGE: &str = "usage: delphi kb eval build --real-only | accept | run [--path service|hybrid|lexical|all] [--label <text>] [--trigger before|after] [--json] | compare <run> <run> [--json] | runs [--json] | history [--set <n>] [--path <p>] [--json]   [--workspace <name>] [--home <dir>]";

/// `delphi kb eval build|accept|run|compare|runs|history …` for the workspace at `ws`. The caller has refused `build` and `accept`
/// inside an agent seat; `compare`, `runs` and `history` read only. `run` writes its run file, and with `record` (the operator's
/// own process, not an agent seat's) its ledger lines, the view and a note on a noticeable change (`specs/140` §2.6 item 1).
pub fn cli(home: &Path, ws: &Path, args: &[String], record_it: bool) -> i32 {
    let json_out = args.iter().any(|a| a == "--json");
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    match args.first().map(String::as_str) {
        Some("run") => {
            let Some(paths) = RunPath::parse(flag("--path").as_deref().unwrap_or("all")) else {
                eprintln!("delphi kb eval run: --path is service, hybrid, lexical or all\n{USAGE}");
                return 2;
            };
            // `--trigger before|after` marks the runs of a retrieval change (§2.4); a label of `before` or `after` says the same.
            let label = flag("--label");
            let trigger = match flag("--trigger").or_else(|| label.clone().filter(|l| l == "before" || l == "after")) {
                None => Trigger::Manual,
                Some(t) => match Trigger::parse(&t).filter(|t| matches!(t, Trigger::Before | Trigger::After)) {
                    Some(t) => t,
                    None => {
                        eprintln!("delphi kb eval run: --trigger is before or after\n{USAGE}");
                        return 2;
                    }
                },
            };
            let done = if record_it { run_recorded(home, ws, &paths, label.as_deref(), trigger) } else { run(home, ws, &paths, label.as_deref()).map(|(f, v)| (f, v, Vec::new())) };
            match done {
                Ok((file, v, notes)) => {
                    if json_out {
                        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                    } else {
                        print!("{}", summary(&v));
                        println!("  written: {}", file.display());
                        if record_it {
                            println!("  history: {} ({}), view: {}", history_path(ws).display(), trigger.as_str(), ws.join(VIEW).display());
                        } else {
                            println!("  not added to the history: this process was started by an agent seat");
                        }
                        for n in &notes {
                            println!("  a noticeable change: provisional note {n} (delphi learn list)");
                        }
                    }
                    0
                }
                Err(e) => {
                    eprintln!("delphi kb eval run: {e}");
                    1
                }
            }
        }
        Some("compare") => {
            let names: Vec<&String> = args[1..].iter().filter(|a| !a.starts_with("--")).collect();
            let [a, b] = names[..] else {
                eprintln!("{USAGE}");
                return 2;
            };
            match (read_run(ws, a), read_run(ws, b)) {
                (Ok(a), Ok(b)) => match compare(&a, &b) {
                    Ok(text) => {
                        if json_out {
                            println!("{}", serde_json::to_string_pretty(&json!({"before": a["run"], "after": b["run"], "set": a["set"]["version"], "text": text})).unwrap_or_default());
                        } else {
                            print!("{text}");
                        }
                        0
                    }
                    Err(e) => {
                        eprintln!("delphi kb eval compare: {e}");
                        1
                    }
                },
                (Err(e), _) | (_, Err(e)) => {
                    eprintln!("delphi kb eval compare: {e}");
                    1
                }
            }
        }
        Some("history") => {
            let set = match flag("--set").map(|s| s.trim_start_matches('v').parse::<u64>()) {
                None => None,
                Some(Ok(n)) => Some(n),
                Some(Err(_)) => {
                    eprintln!("delphi kb eval history: --set is a set version, such as 1\n{USAGE}");
                    return 2;
                }
            };
            let path = flag("--path");
            if path.as_deref().is_some_and(|p| !matches!(p, "service" | "hybrid" | "lexical")) {
                eprintln!("delphi kb eval history: --path is service, hybrid or lexical\n{USAGE}");
                return 2;
            }
            if json_out {
                let h = read_history(ws);
                let lines: Vec<&Value> = h.lines.iter().filter(|l| set.is_none_or(|s| l["set"]["version"].as_u64() == Some(s)) && path.as_deref().is_none_or(|p| l["path"] == p)).collect();
                println!("{}", serde_json::to_string_pretty(&json!({"lines": lines, "unreadable": h.unreadable})).unwrap_or_default());
            } else {
                print!("{}", history_text(ws, set, path.as_deref()));
            }
            0
        }
        Some("runs") => {
            let runs = list_runs(ws);
            if json_out {
                println!("{}", serde_json::to_string_pretty(&runs).unwrap_or_default());
            } else if runs.is_empty() {
                println!("no runs yet (`delphi kb eval run`)");
            } else {
                for r in &runs {
                    let paths: Vec<String> = r["paths"].as_array().into_iter().flatten().map(|p| format!("{} recall@10 {} nDCG@10 {}", p["path"].as_str().unwrap_or("?"), p["recall10"], p["ndcg10"])).collect();
                    println!("{}  golden-v{}  {}{}", r["run"].as_str().unwrap_or("?"), r["set"], paths.join("; "), r["label"].as_str().map(|l| format!("  ({l})")).unwrap_or_default());
                }
            }
            0
        }
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

/// Tests only: three research records, two findings and four misses in the workspace at `ws`, indexed.
#[cfg(test)]
pub(crate) fn fixture_in(ws: &Path) {
    const TIER: &str = "internal_generated";
    let db = ws.join("local/state.db");
    std::fs::create_dir_all(ws.join("local")).unwrap();
    let mut kb = Kb::open(&db).unwrap();
    for q in ["How do herons hunt in shallow water?", "Where do ospreys nest along the estuary?", "Why do kingfishers dive from perches?"] {
        let id = crate::research_record::topic_id(q);
        let dir = crate::research_record::topics_dir(ws).join(&id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("record.json"), serde_json::to_vec(&json!({"topic_id": id, "question": q, "private": false, "claims": []})).unwrap()).unwrap();
        kb.upsert_from(&format!("research/topics/{id}/answer.md"), crate::research_index::ZONE, TIER, &format!("# Answer\n\n{q} The answer is long.\n\n## Detail\n\nMore about it.\n"), "t").unwrap();
    }
    for (slug, q) in [("why-do-terns-hover", "Why do terns hover?"), ("what-do-gulls-eat", "What do gulls eat?")] {
        kb.upsert_from(&format!("missions/m1/findings/{slug}.md"), crate::finding_record::ZONE, TIER, &format!("# {q}\n\nConclusion (likely): fish.\n"), "t").unwrap();
    }
    for q in ["albatross wingspan", "puffin burrows", "cormorant drying wings", "pelican pouch size"] {
        crate::kb::log_miss(ws, q, crate::kb::Searcher::Agent("scout"), 0);
    }
}

/// Tests only: [`fixture_in`], built and accepted with every sampled case kept.
#[cfg(test)]
pub(crate) fn accepted_in(ws: &Path) {
    fixture_in(ws);
    build(ws, &Kb::open_readonly(&ws.join("local/state.db")).unwrap()).unwrap();
    accept(ws, &mut std::io::Cursor::new("keep\n".repeat(SAMPLE).into_bytes()), &mut Vec::new()).unwrap();
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
        super::fixture_in(&ws);
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

    fn rel(path: &str, start_line: usize, grade: u8) -> Relevant {
        Relevant { path: path.into(), start_line, grade, tier: String::new() }
    }

    fn got(path: &str, a: usize, b: usize) -> Got {
        Got { path: path.into(), start_line: a, end_line: b }
    }

    /// AC-5: a known ranking, scored by hand.
    ///
    /// Relevant: a.md:1 (grade 3), a.md:10 (grade 2), b.md:5 (grade 1). Returned, in order: x.md:1-4 (0), a.md:8-12 (holds a.md:10,
    /// grade 2), a.md:1-6 (grade 3), y.md:1-2 (0), then 15 misses and b.md:5-6 at rank 20 (grade 1).
    /// - recall@10 = 2 found / min(3, 10) = 0.6667; recall@20 = 3 / 3 = 1.
    /// - DCG@10 = 2/log2(3) + 3/log2(4) = 1.26186 + 1.5 = 2.76186; IDCG@10 = 3/log2(2) + 2/log2(3) + 1/log2(4) = 3 + 1.26186 + 0.5
    ///   = 4.76186; nDCG@10 = 0.5800.
    /// - first relevant at rank 2: MRR 0.5.
    #[test]
    fn the_metrics_equal_values_computed_by_hand() {
        let relevant = vec![rel("a.md", 1, 3), rel("a.md", 10, 2), rel("b.md", 5, 1)];
        let mut ranking = vec![got("x.md", 1, 4), got("a.md", 8, 12), got("a.md", 1, 6), got("y.md", 1, 2)];
        ranking.extend((0..15).map(|i| got("z.md", i * 10 + 1, i * 10 + 5)));
        ranking.push(got("b.md", 5, 6));
        let g = grades(&relevant, &ranking);
        assert_eq!(&g[..4], &[0, 2, 3, 0]);
        assert_eq!(g[19], 1);
        assert_eq!(round4(recall_at(&g, relevant.len(), 10)), 0.6667);
        assert_eq!(round4(recall_at(&g, relevant.len(), 20)), 1.0);
        assert_eq!(round4(ndcg_at(&g, &relevant, 10)), 0.58);
        assert_eq!(first_rank(&g), Some(2));
        // The same passage returned twice counts once; a heading never decides a match.
        assert_eq!(grades(&relevant, &[got("a.md", 1, 6), got("a.md", 1, 6)]), vec![3, 0]);
        // Scored over cases, for each path alike: a `none` case and a stale case are counted apart, not scored.
        let cases = vec![
            Case { id: "r-1".into(), q: "q".into(), kind: "real".into(), from: "f1".into(), relevant: relevant.clone(), none: false },
            Case { id: "n-1".into(), q: "q".into(), kind: "real".into(), from: "f2".into(), relevant: vec![], none: true },
            Case { id: "r-2".into(), q: "q".into(), kind: "real".into(), from: "f3".into(), relevant: vec![rel("gone.md", 1, 3)], none: false },
        ];
        let (m, ranks) = score(&cases, &[ranking, vec![got("x.md", 1, 2)], vec![]], &|r: &Relevant| r.path != "gone.md");
        assert_eq!((m.cases, m.recall10, m.recall20, m.ndcg10, m.mrr), (1, 0.6667, 1.0, 0.58, 0.5));
        assert_eq!((m.none_cases, m.none_answered, m.stale.clone()), (1, 1, vec!["r-2".to_string()]));
        assert_eq!(ranks.get("r-1"), Some(&Some(2)));
    }

    /// An accepted set on the fixture, with every sampled case kept.
    fn accepted() -> (tempfile::TempDir, PathBuf) {
        let (d, ws) = fixture();
        build(&ws, &open(&ws)).unwrap();
        accept(&ws, &mut answers(&"keep\n".repeat(SAMPLE)), &mut Vec::new()).unwrap();
        (d, ws)
    }

    /// AC-6: a reader made to answer late gives `lexical-deadline` for the query, and what came back is scored.
    #[test]
    fn a_late_reader_is_reported_as_lexical_deadline_and_still_scored() {
        let (d, ws) = accepted();
        let db = ws.join("local/state.db");
        crate::retrieval::test_hooks::LATE.lock().unwrap().push((db.clone(), crate::retrieval::DEADLINE + std::time::Duration::from_millis(300)));
        let (_, v) = run(d.path(), &ws, &[RunPath::Service], Some("late")).unwrap();
        crate::retrieval::test_hooks::LATE.lock().unwrap().retain(|(p, _)| *p != db);
        let p = &v["paths"][0];
        assert_eq!(p["path"], "service");
        assert_eq!(p["modes"]["lexical-deadline"]["count"], 5 + 4, "every query answered late");
        assert_eq!(p["vector_leg"], 0.0);
        assert!(p["cold_ms"].as_f64().is_some_and(|ms| ms >= crate::retrieval::DEADLINE.as_millis() as f64), "a cold query that missed is still reported");
        assert!(p["metrics"]["recall10"].as_f64().unwrap() > 0.0, "what the keyword answer returned is scored");
    }

    /// AC-7: two runs on one set compare; on two versions they are refused; neither the summary nor the comparison holds a question.
    #[test]
    fn runs_compare_on_one_set_only_and_print_no_question_text() {
        let (d, ws) = accepted();
        let (_, a) = run(d.path(), &ws, &[RunPath::Lexical, RunPath::Hybrid, RunPath::Service], None).unwrap_or_else(|e| {
            // No model in a test home: the hybrid path says so, and the rest run.
            assert!(e.contains("embedding model"), "{e}");
            run(d.path(), &ws, &[RunPath::Lexical, RunPath::Service], None).unwrap()
        });
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let (_, b) = run(d.path(), &ws, &[RunPath::Lexical, RunPath::Service], Some("after")).unwrap();
        let cmp = compare(&a, &b).unwrap();
        assert!(cmp.contains("recall10") && cmp.contains("moved more than three ranks"), "{cmp}");
        let (_, cases) = load_accepted(&ws, None).unwrap();
        for text in [summary(&a), cmp.clone(), serde_json::to_string(&a).unwrap()] {
            for c in &cases {
                assert!(!text.contains(&c.q), "a question appeared: {}", c.id);
            }
        }
        assert_eq!(list_runs(&ws).len(), 2);
        let mut other = b.clone();
        other["set"]["version"] = json!(2);
        assert!(compare(&a, &other).unwrap_err().contains("different golden sets"));
    }

    /// The run opens the index read-only: the index file and the set are unchanged, and the only new file is the run's.
    #[test]
    fn a_run_writes_nothing_but_its_own_file() {
        let (d, ws) = accepted();
        let db = ws.join("local/state.db");
        let before = (std::fs::read(&db).unwrap(), std::fs::read(set_path(&ws, 1)).unwrap());
        let (file, _) = run(d.path(), &ws, &[RunPath::Lexical], None).unwrap();
        assert_eq!((std::fs::read(&db).unwrap(), std::fs::read(set_path(&ws, 1)).unwrap()), before);
        assert!(file.starts_with(runs_dir(&ws)));
    }

    /// `specs/140` AC-11: every run adds one ledger line per path with the fields of §2.6 item 1; after a reindex, a new set version
    /// and pruned run files, every earlier line is still there byte for byte; a torn last line is skipped and reported; no code
    /// path truncates or rewrites the ledger.
    #[test]
    fn every_run_adds_its_ledger_line_and_no_rebuild_new_set_or_pruning_touches_the_earlier_ones() {
        let (d, ws) = accepted();
        run_recorded(d.path(), &ws, &[RunPath::Lexical], None, Trigger::Manual).unwrap();
        let h = read_history(&ws);
        assert_eq!(h.lines.len(), 1, "one path, one line");
        let l = &h.lines[0];
        for k in ["v", "at", "run", "trigger", "set", "corpus", "config_sha256", "models", "build", "path", "recall10", "recall20", "ndcg10", "mrr", "p50_ms", "p95_ms", "vector_leg"] {
            assert!(l.get(k).is_some(), "{k}");
        }
        for (o, k) in [("set", "version"), ("set", "sha256"), ("set", "counts"), ("corpus", "documents"), ("corpus", "chunks"), ("corpus", "vectors"), ("corpus", "index_generation"), ("models", "embedder"), ("models", "reranker"), ("build", "version"), ("build", "commit")] {
            assert!(l[o].get(k).is_some(), "{o}.{k}");
        }
        assert_eq!((l["v"].as_u64(), l["trigger"].as_str(), l["path"].as_str()), (Some(HISTORY_V as u64), Some("manual"), Some("lexical")));
        assert!(l["config_sha256"].as_str().is_some_and(|s| s.len() == 64));
        let (_, cases) = load_accepted(&ws, None).unwrap();
        let text = std::fs::read_to_string(history_path(&ws)).unwrap();
        assert!(cases.iter().all(|c| !text.contains(&c.q)), "numbers and identifiers only");
        let before = std::fs::read(history_path(&ws)).unwrap();
        // A reindex, a new set version, and the run files pruned.
        let mut kb = Kb::open(&ws.join("local/state.db")).unwrap();
        kb.upsert_from("context/new.md", "context", "internal_generated", "# New\n\nA new page.\n", "t").unwrap();
        drop(kb);
        assert_eq!(build(&ws, &open(&ws)).unwrap().version, 2);
        let n = sample(&parse_jsonl(&std::fs::read_to_string(set_path(&ws, 2)).unwrap()).unwrap()).len();
        accept(&ws, &mut answers(&"keep\n".repeat(n)), &mut Vec::new()).unwrap();
        std::fs::remove_dir_all(runs_dir(&ws)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        run_recorded(d.path(), &ws, &[RunPath::Lexical, RunPath::Service], Some("after"), Trigger::After).unwrap();
        let after = std::fs::read(history_path(&ws)).unwrap();
        assert!(after.starts_with(&before), "every earlier line, byte for byte");
        let h = read_history(&ws);
        assert_eq!(h.lines.len(), 3);
        assert_eq!(series(&h.lines).keys().cloned().collect::<Vec<_>>(), vec![(1, "lexical".to_string()), (2, "lexical".to_string()), (2, "service".to_string())], "a new set version is a new series");
        assert!(h.lines[2]["vector_leg"].is_number() && h.lines[1]["vector_leg"].is_null(), "the vector leg is the service path's");
        // A torn write: skipped and said.
        let mut f = std::fs::OpenOptions::new().append(true).open(history_path(&ws)).unwrap();
        f.write_all(b"{\"v\":1,\"at\":\"2026").unwrap();
        assert_eq!((read_history(&ws).lines.len(), read_history(&ws).unreadable), (3, 1));
        assert!(history_text(&ws, None, None).contains("1 line(s)"));
        // Only `append_history` writes the ledger, and it appends.
        let src = include_str!("kb_eval.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        assert_eq!(code.matches("history_path(ws)").count(), 4, "the path is named by append_history, read_history and two messages only");
        assert!(code.contains(".create(true).append(true).open(&path)") && !code.contains("truncate(") && !code.contains("File::create("));
    }

    /// `specs/140` AC-12: after a run the view holds the series' sentence and table and no question, and an inside agent's search for
    /// `retrieval quality` returns it.
    #[test]
    fn after_a_run_the_view_holds_each_series_and_no_question_and_an_agent_search_finds_it() {
        let (d, ws) = accepted();
        let (_, v, _) = run_recorded(d.path(), &ws, &[RunPath::Lexical], None, Trigger::Manual).unwrap();
        let view = std::fs::read_to_string(ws.join(VIEW)).unwrap();
        assert!(view.contains("Set v1, lexical path: nDCG@10 ") && view.contains("one run so far"), "{view}");
        assert!(view.contains(&format!("| {} | manual |", v["run"].as_str().unwrap())), "{view}");
        let (_, cases) = load_accepted(&ws, None).unwrap();
        assert!(cases.iter().all(|c| !view.contains(&c.q)), "no question");
        // The index reads the view as zone `knowledge`; a builder's default search (nothing untrusted) finds it.
        let db = ws.join("local/state.db");
        Kb::open(&db).unwrap().sync_lexical(&crate::kb::sources(&ws), "2026-11-26T00:00:00Z").unwrap();
        let hits = crate::retrieval::search(d.path(), &db, "retrieval quality", crate::kb::SearchOpts { role: Some("builder".into()), ..Default::default() }).unwrap().hits;
        assert!(hits.iter().any(|h| h.rel_path == "knowledge/retrieval-quality.md" && h.zone == "knowledge"), "{:?}", hits.iter().map(|h| h.rel_path.clone()).collect::<Vec<_>>());
    }

    /// A ledger line of one series, made by hand: run `r`, nDCG@10 `ndcg`, on build `commit`.
    fn line(r: &str, ndcg: f64, commit: &str) -> Value {
        json!({"v": 1, "at": format!("2026-11-{:02}T00:00:00Z", 1 + r[1..].parse::<u32>().unwrap()), "run": r, "trigger": "scheduled", "set": {"version": 1, "sha256": "s"}, "corpus": {"chunks": 100}, "config_sha256": "c", "models": {"embedder": {"id": "m"}}, "build": {"version": "0.6.7", "commit": commit}, "path": "service", "recall10": 0.9, "recall20": 0.93, "ndcg10": ndcg, "mrr": 0.95, "p50_ms": 40.0, "p95_ms": 80.0, "vector_leg": 1.0})
    }

    fn notes_in(ws: &Path) -> Vec<String> {
        let dir = ws.join(crate::provisional_notes::HOME_DIR).join(crate::provisional_notes::SHARED);
        std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| std::fs::read_to_string(e.path()).unwrap()).collect()
    }

    /// `specs/140` AC-13: a run 0.05 below the median of the five before it leaves exactly one provisional note naming both runs and
    /// what differed; a fall of 0.01 leaves none; a second low run leaves no second note; a run back inside the thresholds clears it.
    #[test]
    fn a_noticeable_fall_leaves_one_note_naming_both_runs_and_a_small_fall_or_a_second_low_run_none() {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let five: Vec<Value> = (1..=5).map(|i| line(&format!("r{i}"), 0.90, "aaa")).collect();
        append_history(&ws, &five).unwrap();
        append_history(&ws, &[line("r6", 0.89, "aaa")]).unwrap();
        assert!(note_changes(&ws, "2026-11-26T10:00:00Z").is_empty() && notes_in(&ws).is_empty(), "a fall of 0.01 leaves none");
        append_history(&ws, &[line("r7", 0.85, "bbb")]).unwrap();
        let written = note_changes(&ws, "2026-11-26T11:00:00Z");
        assert_eq!(written.len(), 1);
        let notes = notes_in(&ws);
        assert_eq!(notes.len(), 1);
        let n = &notes[0];
        assert!(n.contains("nDCG@10 fell from 0.9000 to 0.8500") && n.contains("run r7") && n.contains("was r6") && n.contains("the build (0.6.7 aaa -> 0.6.7 bbb)"), "{n}");
        assert!(n.contains("kind: fact") && n.contains("trigger: retrieval_eval") && n.contains("Unreviewed note"), "{n}");
        assert_eq!(crate::provisional_notes::list(&ws).len(), 1, "a note of the tier, listed for review");
        append_history(&ws, &[line("r8", 0.84, "bbb")]).unwrap();
        assert!(note_changes(&ws, "2026-11-26T12:00:00Z").is_empty() && notes_in(&ws).len() == 1, "a second low run leaves no second note");
        // Back inside the thresholds, then down again: a new note.
        append_history(&ws, &[line("r9", 0.90, "bbb")]).unwrap();
        assert!(note_changes(&ws, "2026-11-26T13:00:00Z").is_empty());
        append_history(&ws, &[line("r10", 0.80, "bbb")]).unwrap();
        assert_eq!(note_changes(&ws, "2026-11-26T14:00:00Z").len(), 1);
        // With fewer than five runs before it, nothing is compared.
        let few: Vec<&Value> = five.iter().take(4).collect();
        assert!(noticeable(&few).is_none());
    }

    /// `specs/140` AC-14 (the command's half) and §2.6 item 6: `kb eval history` shows each series with the ledger's numbers, the
    /// change from run to run and a trend; `--set` and `--path` narrow it.
    #[test]
    fn kb_eval_history_shows_the_ledgers_series_and_numbers() {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let mut lines: Vec<Value> = (1..=6).map(|i| line(&format!("r{i}"), 0.90 + i as f64 / 1000.0, "aaa")).collect();
        let mut lexical = line("r7", 0.61, "aaa");
        lexical["path"] = json!("lexical");
        lines.push(lexical);
        append_history(&ws, &lines).unwrap();
        let text = history_text(&ws, None, None);
        assert!(text.contains("set v1, service path: nDCG@10 0.9060, steady over 6 runs"), "{text}");
        assert!(text.contains("| r6 | scheduled | 0.9000 | 0.9300 | 0.9060 | +0.0010 |"), "{text}");
        assert!(text.contains("set v1, lexical path: nDCG@10 0.6100, one run so far"), "{text}");
        let narrowed = history_text(&ws, Some(1), Some("lexical"));
        assert!(narrowed.contains("lexical path") && !narrowed.contains("service path"));
        assert!(history_text(&ws, Some(2), None).contains("no runs in the history yet"));
        assert_eq!(health_numbers(&ws).map(|(s, x, _)| (s, x)), Some(("set v1, lexical path".to_string(), 0.61)), "the newest run");
    }

    /// `specs/140` §2.6 item 4 and AC-14: a scheduled run is due every `eval_every_days` while idle; 0 is off.
    #[test]
    fn a_scheduled_run_is_due_every_eval_every_days_while_idle_and_never_at_zero() {
        let day = 86_400;
        assert!(scheduled_due(7, 100 * day, None, true), "no run yet");
        assert!(scheduled_due(7, 100 * day, Some(93 * day), true));
        assert!(!scheduled_due(7, 100 * day, Some(94 * day), true), "six days ago");
        assert!(!scheduled_due(7, 100 * day, None, false), "not while busy");
        assert!(!scheduled_due(0, 100 * day, None, true), "0 is off");
    }
}
