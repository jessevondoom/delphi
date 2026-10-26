//! Truth packs: the stored source of truth for a research or strategy case (`specs/037` §4).
//!
//! A pack lives in `truth/` beside `case.toml` and is **never copied into a run**, so no agent can read it. It holds
//! `meta.toml` (as-of date, who verified it), `facts.toml`, `traps.toml`, `sources.toml` with extracts under
//! `sources/`, and `rubric.md`. Loading checks the pack against itself: every fact names a source, every quote is
//! found in that source's extract, every extract matches its recorded hash, and every pattern compiles. Matching is
//! deterministic and gives a **lower bound** on what an answer states (a fact worded unexpectedly is missed).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use regex::{Regex, RegexBuilder};

pub type Problems = Vec<String>;

#[derive(Debug, Clone)]
pub struct Fact {
    pub id: String,
    pub claim: String,
    pub source: String,
    /// The verbatim passage of the source's extract that supports the claim.
    pub quote: String,
    pub weight: f64,
    /// The patterns as written, for the review sheet.
    pub pattern_src: Vec<String>,
    patterns: Vec<Regex>,
    /// A number the answer must contain within `tol` of `value`, in addition to any pattern.
    pub number: Option<(f64, f64)>,
}

/// A question the pack leaves to judgement (`specs/037` §4): a strategic call or an insight, not a fact. The assistant may
/// draft a starting view; only the operator's own `operator_view` reaches a judge, and as a view, never as a fact.
#[derive(Debug, Clone, PartialEq)]
pub struct Insight {
    pub id: String,
    pub question: String,
    /// A starting point written by whoever built the pack; flagged as a draft and never shown to a judge.
    pub draft_view: String,
    /// The operator's judgement. Empty means not yet reviewed.
    pub operator_view: String,
    /// Factors a good answer weighs.
    pub considerations: Vec<String>,
    /// How much latitude: what other well-argued views are acceptable.
    pub acceptable: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceMeta {
    pub id: String,
    pub url: String,
    pub retrieved: String,
    pub blake3: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct Trap {
    pub id: String,
    pub why: String,
    patterns: Vec<Regex>,
}

#[derive(Debug, Clone)]
pub struct Truth {
    pub dir: PathBuf,
    pub as_of: String,
    pub verified_by: Option<String>,
    pub verified_on: Option<String>,
    pub facts: Vec<Fact>,
    pub traps: Vec<Trap>,
    pub sources: usize,
    pub insights: Vec<Insight>,
    pub source_meta: Vec<SourceMeta>,
    /// Hash of everything a verification vouches for: the facts, traps, sources, extracts, rubric and insights (not meta.toml).
    pub fingerprint: String,
    /// The fingerprint the operator verified. When it differs from `fingerprint`, the pack was edited afterwards.
    pub verified_fingerprint: Option<String>,
}

impl PartialEq for Truth {
    fn eq(&self, o: &Truth) -> bool {
        self.dir == o.dir && self.as_of == o.as_of
    }
}

/// How much of the checklist an answer states.
#[derive(Debug, Clone, PartialEq)]
pub struct Recall {
    /// Weighted share of facts stated, 0 to 1.
    pub fraction: f64,
    pub stated: Vec<String>,
    pub missing: Vec<String>,
}

fn table(path: &Path, at: &str, problems: &mut Problems) -> Option<toml::Table> {
    match fs::read_to_string(path) {
        Ok(t) => match t.parse::<toml::Table>() {
            Ok(t) => Some(t),
            Err(e) => {
                problems.push(format!("{}: {} is not valid TOML ({})", at, path.display(), e.message()));
                None
            }
        },
        Err(e) => {
            problems.push(format!("{}: cannot read {} ({})", at, path.display(), e));
            None
        }
    }
}

fn s<'a>(t: &'a toml::Table, k: &str) -> Option<&'a str> {
    t.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty())
}

fn date_ok(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn patterns(t: &toml::Table, at: &str, problems: &mut Problems) -> Vec<Regex> {
    let mut out = Vec::new();
    for p in t.get("any").and_then(|v| v.as_array()).into_iter().flatten() {
        match p.as_str().map(|p| RegexBuilder::new(p).case_insensitive(true).size_limit(1 << 20).build()) {
            Some(Ok(r)) => out.push(r),
            Some(Err(e)) => problems.push(format!("{}: pattern does not compile ({})", at, e.to_string().lines().last().unwrap_or(""))),
            None => problems.push(format!("{}: `any` holds only strings", at)),
        }
    }
    out
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Read and check `<case>/truth`. `at` names the case in problem lines.
pub fn load(case_dir: &Path, at: &str) -> Result<Truth, Problems> {
    let dir = case_dir.join("truth");
    let mut p: Problems = Vec::new();
    if !dir.is_dir() {
        return Err(vec![format!("{}: a research case needs a `truth/` directory (specs/037 §4)", at)]);
    }
    let mut verified_fingerprint: Option<String> = None;
    let meta = table(&dir.join("meta.toml"), at, &mut p);
    let (as_of, verified_by, verified_on) = match &meta {
        Some(m) => {
            let as_of = s(m, "as_of").unwrap_or("").to_string();
            if !date_ok(&as_of) {
                p.push(format!("{}: truth/meta.toml `as_of` must be a date like 2026-10-26", at));
            }
            verified_fingerprint = s(m, "verified_fingerprint").map(String::from);
            let (by, on) = (s(m, "verified_by").map(String::from), s(m, "verified_on").map(String::from));
            if by.is_some() != on.is_some() || on.as_deref().is_some_and(|d| !date_ok(d)) {
                p.push(format!("{}: truth/meta.toml `verified_by` and `verified_on` (a date) go together", at));
            }
            (as_of, by, on)
        }
        None => (String::new(), None, None),
    };
    if !dir.join("rubric.md").is_file() {
        p.push(format!("{}: truth/rubric.md is missing", at));
    }

    // Sources: each has an id, a URL, a retrieval date, and an extract that matches its recorded hash.
    let mut source_text: std::collections::BTreeMap<String, Option<String>> = Default::default();
    let mut source_meta: Vec<SourceMeta> = Vec::new();
    if let Some(t) = table(&dir.join("sources.toml"), at, &mut p) {
        for (i, st) in t.get("source").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
            let here = format!("{} source #{}", at, i + 1);
            let Some(st) = st.as_table() else {
                p.push(format!("{}: not a table", here));
                continue;
            };
            let Some(id) = s(st, "id").filter(|id| id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) else {
                p.push(format!("{}: `id` must be letters, digits, `-` or `_`", here));
                continue;
            };
            if s(st, "url").is_none() || !s(st, "retrieved").is_some_and(date_ok) {
                p.push(format!("{}: `{}` needs `url` and a `retrieved` date", here, id));
            }
            if source_text.contains_key(id) {
                p.push(format!("{}: duplicate source id `{}`", here, id));
            }
            source_meta.push(SourceMeta { id: id.into(), url: s(st, "url").unwrap_or("").into(), retrieved: s(st, "retrieved").unwrap_or("").into(), blake3: s(st, "blake3").unwrap_or("").into(), note: s(st, "note").unwrap_or("").into() });
            let untracked = st.get("untracked").and_then(|v| v.as_bool()).unwrap_or(false);
            let extract = dir.join("sources").join(format!("{}.txt", id));
            if untracked {
                source_text.insert(id.to_string(), None);
            } else {
                match fs::read(&extract) {
                    Ok(bytes) => {
                        match s(st, "blake3") {
                            Some(h) if h == blake3::hash(&bytes).to_hex().as_str() => {}
                            Some(_) => p.push(format!("{}: `{}` extract does not match its recorded `blake3`", here, id)),
                            None => p.push(format!("{}: `{}` needs `blake3` of its extract", here, id)),
                        }
                        source_text.insert(id.to_string(), Some(norm(&String::from_utf8_lossy(&bytes))));
                    }
                    Err(_) => {
                        p.push(format!("{}: `{}` has no extract at truth/sources/{}.txt (mark it `untracked = true` if the text is kept elsewhere)", here, id, id));
                        source_text.insert(id.to_string(), None);
                    }
                }
            }
        }
    }

    let mut facts = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(t) = table(&dir.join("facts.toml"), at, &mut p) {
        for (i, f) in t.get("fact").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
            let here = format!("{} fact #{}", at, i + 1);
            let Some(f) = f.as_table() else {
                p.push(format!("{}: not a table", here));
                continue;
            };
            let id = s(f, "id").unwrap_or("").to_string();
            if id.is_empty() || !seen.insert(id.clone()) {
                p.push(format!("{}: `id` is missing or repeated", here));
            }
            let (claim, source, quote) = (s(f, "claim"), s(f, "source"), s(f, "quote"));
            if claim.is_none() || quote.is_none() {
                p.push(format!("{}: `{}` needs `claim` and `quote`", here, id));
            }
            match source.map(|src| source_text.get(src)) {
                Some(Some(text)) => {
                    // A tracked extract must contain the quote: the pack is checked against its own record.
                    if let (Some(text), Some(q)) = (text, quote) {
                        if !text.contains(&norm(q)) {
                            p.push(format!("{}: `{}` quote is not in the extract of source `{}`", here, id, source.unwrap_or("")));
                        }
                    }
                }
                _ => p.push(format!("{}: `{}` names a source that is not in sources.toml", here, id)),
            }
            let pats = patterns(f, &format!("{} `{}`", here, id), &mut p);
            let number = f.get("number").and_then(|n| n.as_table()).and_then(|n| {
                let v = n.get("value").and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))?;
                let tol = n.get("tol").and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))).unwrap_or(0.0);
                Some((v, tol.abs()))
            });
            if pats.is_empty() && number.is_none() {
                p.push(format!("{}: `{}` needs `any` patterns or a `number`", here, id));
            }
            let weight = f.get("weight").and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))).unwrap_or(1.0);
            if weight <= 0.0 {
                p.push(format!("{}: `{}` weight must be above 0", here, id));
            }
            let pattern_src: Vec<String> = f.get("any").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            facts.push(Fact { id, claim: claim.unwrap_or("").to_string(), source: source.unwrap_or("").to_string(), quote: quote.unwrap_or("").to_string(), weight, pattern_src, patterns: pats, number });
        }
    }

    let mut traps = Vec::new();
    let trap_path = dir.join("traps.toml");
    if trap_path.is_file() {
        if let Some(t) = table(&trap_path, at, &mut p) {
            for (i, tr) in t.get("trap").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
                let here = format!("{} trap #{}", at, i + 1);
                let Some(tr) = tr.as_table() else {
                    p.push(format!("{}: not a table", here));
                    continue;
                };
                let pats = patterns(tr, &here, &mut p);
                if s(tr, "id").is_none() || s(tr, "why").is_none() || pats.is_empty() {
                    p.push(format!("{}: a trap needs `id`, `why` and at least one pattern in `any`", here));
                }
                traps.push(Trap { id: s(tr, "id").unwrap_or("").to_string(), why: s(tr, "why").unwrap_or("").to_string(), patterns: pats });
            }
        }
    }

    let mut insights = Vec::new();
    let ins_path = dir.join("insights.toml");
    if ins_path.is_file() {
        if let Some(t) = table(&ins_path, at, &mut p) {
            let mut seen_ins = BTreeSet::new();
            for (i, it) in t.get("insight").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
                let here = format!("{} insight #{}", at, i + 1);
                let Some(it) = it.as_table() else {
                    p.push(format!("{}: not a table", here));
                    continue;
                };
                let (id, question) = (s(it, "id").unwrap_or(""), s(it, "question").unwrap_or(""));
                if id.is_empty() || question.is_empty() || !seen_ins.insert(id.to_string()) {
                    p.push(format!("{}: needs a unique `id` and a `question`", here));
                }
                insights.push(Insight {
                    id: id.into(),
                    question: question.into(),
                    draft_view: s(it, "draft_view").unwrap_or("").into(),
                    operator_view: s(it, "operator_view").unwrap_or("").into(),
                    considerations: it.get("considerations").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
                    acceptable: s(it, "acceptable").unwrap_or("").into(),
                });
            }
        }
    }
    if !p.is_empty() {
        return Err(p);
    }
    let fp = fingerprint(&dir);
    Ok(Truth { dir, as_of, verified_by, verified_on, facts, traps, sources: source_text.len(), insights, source_meta, fingerprint: fp, verified_fingerprint })
}

/// Record the blake3 of every tracked extract in `truth/sources.toml` (after an extract was written or edited).
/// Rewrites only the `blake3` lines; returns how many sources were sealed. Verification is a separate act: sealing says
/// what the files are now, not that they are right.
pub fn seal(case_dir: &Path) -> Result<usize, String> {
    let dir = case_dir.join("truth");
    let path = dir.join("sources.toml");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut out: Vec<String> = Vec::new();
    let (mut block, mut sealed): (Vec<String>, usize) = (Vec::new(), 0);
    let flush = |block: &mut Vec<String>, out: &mut Vec<String>, sealed: &mut usize| {
        if block.is_empty() {
            return;
        }
        let id = block.iter().find_map(|l| l.trim().strip_prefix("id").and_then(|r| r.trim_start().strip_prefix('=')).map(|v| v.trim().trim_matches('"').to_string()));
        let untracked = block.iter().any(|l| l.trim().replace(' ', "") == "untracked=true");
        if let (Some(id), false) = (id, untracked) {
            if let Ok(bytes) = fs::read(dir.join("sources").join(format!("{}.txt", id))) {
                block.retain(|l| !l.trim_start().starts_with("blake3"));
                while block.last().is_some_and(|l| l.trim().is_empty()) {
                    block.pop();
                }
                block.push(format!("blake3 = \"{}\"", blake3::hash(&bytes).to_hex()));
                block.push(String::new());
                *sealed += 1;
            }
        }
        out.append(block);
    };
    for line in text.lines() {
        if line.trim() == "[[source]]" {
            flush(&mut block, &mut out, &mut sealed);
        }
        if block.is_empty() && line.trim() != "[[source]]" {
            out.push(line.to_string());
        } else {
            block.push(line.to_string());
        }
    }
    flush(&mut block, &mut out, &mut sealed);
    fs::write(&path, out.join("\n").trim_end().to_string() + "\n").map_err(|e| e.to_string())?;
    Ok(sealed)
}

/// Hash of everything a verification vouches for. `meta.toml` is left out (it holds the stamp itself).
pub fn fingerprint(dir: &Path) -> String {
    let mut h = blake3::Hasher::new();
    for name in ["facts.toml", "traps.toml", "sources.toml", "rubric.md", "insights.toml"] {
        h.update(name.as_bytes());
        h.update(&fs::read(dir.join(name)).unwrap_or_default());
        h.update(b"\0");
    }
    let mut extracts: Vec<_> = fs::read_dir(dir.join("sources")).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    extracts.sort();
    for e in extracts {
        h.update(e.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().as_bytes());
        h.update(&fs::read(&e).unwrap_or_default());
        h.update(b"\0");
    }
    h.finalize().to_hex().to_string()
}

/// Record that `by` checked the pack on `on`, with the fingerprint of what was checked. Rewrites only the `verified_*` lines of
/// `meta.toml`. Refuses a pack that does not load.
pub fn stamp_verified(case_dir: &Path, by: &str, on: &str) -> Result<(), String> {
    let t = load(case_dir, "pack").map_err(|e| format!("the pack does not load: {}", e.join("; ")))?;
    if by.trim().is_empty() || !date_ok(on) {
        return Err("give who verified it and a date like 2026-10-26".into());
    }
    let path = case_dir.join("truth/meta.toml");
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut lines: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with("verified_") && !l.trim_start().starts_with("# DRAFT") && !l.trim_start().starts_with("# Verifying means") && !l.trim_start().starts_with("# verified")).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let out = format!("{}\n\nverified_by = {:?}\nverified_on = \"{}\"\nverified_fingerprint = \"{}\"\n", lines.join("\n"), by.trim(), on, t.fingerprint);
    fs::write(&path, out).map_err(|e| e.to_string())
}

// ── the operator's review ───────────────────────────────────────────────────────────────────────────────────────

/// A Markdown sheet for the operator to check a pack: every fact beside its source link and verbatim quote, the traps, the
/// sources with their hashes, the rubric, and the judgement questions with room for the operator's own view. Generated, so it
/// is never edited: change the files in `truth/` and regenerate.
pub fn review_sheet(case_id: &str, prompt: &str, t: &Truth) -> String {
    let status = if t.stale() {
        format!("STALE: verified by {} on {} but edited since; verify again", t.verified_by.as_deref().unwrap_or("?"), t.verified_on.as_deref().unwrap_or("?"))
    } else if t.verified() {
        format!("VERIFIED by {} on {}", t.verified_by.as_deref().unwrap_or("?"), t.verified_on.as_deref().unwrap_or("?"))
    } else {
        "DRAFT: not yet checked by you; `delphi train run` will not use it without --allow-unverified".to_string()
    };
    let src = |id: &str| t.source_meta.iter().find(|s| s.id == id).map(|s| format!("{} (retrieved {}){}", s.url, s.retrieved, if s.note.is_empty() { String::new() } else { format!(" — {}", s.note) })).unwrap_or_else(|| id.to_string());
    let mut o = String::new();
    o.push_str(&format!("# Review sheet: {}\n\n_Generated by `delphi train truth-sheet`; do not edit it. Change the files in `truth/` and regenerate._\n\n", case_id));
    o.push_str(&format!("**Status:** {}\n\n**As of:** {}\n\n**The agent is asked** (as an ordinary request, nothing says it is a test):\n\n> {}\n\n", status, t.as_of, prompt.trim().replace('\n', "\n> ")));
    o.push_str(&format!("## How to review\n\n1. **Facts and traps** (sections 1 and 2): open each source link, find the quote, and confirm the claim says what the quote says. Confirm the patterns would catch a correct answer worded differently, and would not fire on a wrong one. A pattern is a *lower bound*: a miss costs the answer a point it deserved, so err toward loose patterns for facts and tight patterns for traps.\n2. **Sources** (section 3): is each one authoritative enough to be the record?\n3. **Rubric** (section 4): is this what a good answer must do?\n4. **Judgement calls** (section 5): these are not facts. Write your own view in `truth/insights.toml` (`operator_view`); only your view reaches a judge, and as a view.\n5. When it is right: `delphi train verify --case {} --by \"<your name>\"`. Any later edit to the pack makes the stamp stale.\n\nTo see what the checks do with an answer you write: `delphi train truth-check --case {} --text-file answer.md`.\n\n", case_id, case_id));
    o.push_str(&format!("## 1. Facts ({})\n\n", t.facts.len()));
    for f in &t.facts {
        o.push_str(&format!("### `{}` (weight {})\n\n- **Claim:** {}\n- **Source:** {}\n- **Quote:**\n\n  > {}\n\n- **Counts as stated if the answer matches any of:** {}{}\n- [ ] the claim matches the quote   [ ] the source is good enough   [ ] the patterns are right\n\n", f.id, f.weight, f.claim, src(&f.source), f.quote, f.pattern_src.iter().map(|p| format!("`{}`", p)).collect::<Vec<_>>().join(" · "), f.number.map(|(v, tol)| format!("\n- **And contains the number** {} (± {})", v, tol)).unwrap_or_default()));
    }
    o.push_str(&format!("## 2. Traps ({})\n\nAn answer asserting one is penalized.\n\n", t.traps.len()));
    for tr in &t.traps {
        let pats = fs::read_to_string(t.dir.join("traps.toml")).ok().and_then(|x| x.parse::<toml::Table>().ok()).and_then(|x| x.get("trap").and_then(|v| v.as_array()).and_then(|a| a.iter().find(|e| e.get("id").and_then(|i| i.as_str()) == Some(tr.id.as_str())).cloned())).and_then(|e| e.get("any").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|p| format!("`{}`", p))).collect::<Vec<_>>().join(" · "))).unwrap_or_default();
        o.push_str(&format!("### `{}`\n\n- **Why it is false:** {}\n- **Fires on:** {}\n- [ ] it really is false   [ ] the pattern is tight enough not to fire on a correct answer\n\n", tr.id, tr.why, pats));
    }
    o.push_str(&format!("## 3. Sources ({})\n\n| id | link | retrieved | extract hash | note |\n| :-- | :-- | :-- | :-- | :-- |\n", t.source_meta.len()));
    for s in &t.source_meta {
        o.push_str(&format!("| {} | {} | {} | `{}` | {} |\n", s.id, s.url, s.retrieved, s.blake3.chars().take(12).collect::<String>(), s.note));
    }
    o.push_str("\nThe full extract of each source is in `truth/sources/<id>.txt`; the quotes above were found in it automatically when the pack loaded.\n\n");
    o.push_str(&format!("## 4. Rubric\n\nThe jury and you judge against this.\n\n{}\n\n[ ] this is what a good answer must do and avoid\n\n", fs::read_to_string(t.dir.join("rubric.md")).unwrap_or_default().trim()));
    o.push_str("## 5. Judgement calls\n\nNot facts: strategic calls and insights where reasonable people differ. The assistant's draft is only a starting point; **write your own view** so the jury can be tested on judgement, not just recall.\n\n");
    if t.insights.is_empty() {
        o.push_str("_This pack has none yet._ To add one, create `truth/insights.toml`:\n\n```toml\n[[insight]]\nid = \"short-id\"\nquestion = \"The call a good answer has to make.\"\noperator_view = \"\"        # your judgement, in your words\nconsiderations = [\"what a good answer weighs\"]\nacceptable = \"what other well-argued views are fine\"\n```\n\n");
    }
    for i in &t.insights {
        o.push_str(&format!("### `{}`\n\n- **Question:** {}\n- **Assistant's draft view (not shown to any judge):** {}\n- **Your view:** {}\n", i.id, i.question, if i.draft_view.is_empty() { "—".to_string() } else { i.draft_view.clone() }, if i.operator_view.is_empty() { "_not written yet: set `operator_view` in `truth/insights.toml`_".to_string() } else { i.operator_view.clone() }));
        if !i.considerations.is_empty() {
            o.push_str(&format!("- **A good answer weighs:** {}\n", i.considerations.join("; ")));
        }
        if !i.acceptable.is_empty() {
            o.push_str(&format!("- **Latitude:** {}\n", i.acceptable));
        }
        o.push('\n');
    }
    o
}

/// What the deterministic checks make of an answer, in words: for trying a pack before verifying it.
pub fn check_report(t: &Truth, text: &str) -> String {
    let r = t.recall(text);
    let traps = t.trap_hits(text);
    format!(
        "fact recall {:.0}% ({} of {} facts)\n  stated:  {}\n  missing: {}\ntraps: {}\n",
        r.fraction * 100.0,
        r.stated.len(),
        t.facts.len(),
        if r.stated.is_empty() { "—".into() } else { r.stated.join(", ") },
        if r.missing.is_empty() { "—".into() } else { r.missing.join(", ") },
        if traps.is_empty() { "none".to_string() } else { traps.join(", ") }
    )
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn case_prompt(case_dir: &Path) -> String {
    fs::read_to_string(case_dir.join("case.toml")).ok().and_then(|t| t.parse::<toml::Table>().ok()).and_then(|t| t.get("prompt").and_then(|p| p.as_str().map(String::from))).unwrap_or_default()
}

/// Every research case directory under `<root>/<suite>/cases/*` that has a `truth/`, as `(suite/id, dir)`.
pub fn truth_cases(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for suite in fs::read_dir(root).map(|rd| rd.flatten().collect::<Vec<_>>()).unwrap_or_default() {
        for c in fs::read_dir(suite.path().join("cases")).map(|rd| rd.flatten().collect::<Vec<_>>()).unwrap_or_default() {
            if c.path().join("truth").is_dir() {
                out.push((format!("{}/{}", suite.file_name().to_string_lossy(), c.file_name().to_string_lossy()), c.path()));
            }
        }
    }
    out.sort();
    out
}

/// `delphi train truth-sheet`, `truth-check` and `verify`.
pub fn cli(sub: &str, args: &[String], root: &Path) -> i32 {
    let all = truth_cases(root);
    if sub == "truth-sheet" && args.iter().any(|a| a == "--all") {
        for (id, dir) in &all {
            match load(dir, id) {
                Ok(t) => {
                    if let Err(e) = fs::write(dir.join("truth/review-sheet.md"), review_sheet(id, &case_prompt(dir), &t)) {
                        eprintln!("delphi train truth-sheet: {}: {}", id, e);
                        return 1;
                    }
                    println!("wrote {}", dir.join("truth/review-sheet.md").display());
                }
                Err(e) => {
                    eprintln!("delphi train truth-sheet: {} does not load: {}", id, e.join("; "));
                    return 1;
                }
            }
        }
        return 0;
    }
    let Some(case) = flag(args, "--case") else {
        eprintln!("delphi train {}: --case <suite>/<id> is required (research cases with a truth pack: {})", sub, if all.is_empty() { "none".into() } else { all.iter().map(|(i, _)| i.as_str()).collect::<Vec<_>>().join(", ") });
        return 2;
    };
    let Some((id, dir)) = all.iter().find(|(i, _)| *i == case) else {
        eprintln!("delphi train {}: no truth pack for `{}`", sub, case);
        return 2;
    };
    let t = match load(dir, id) {
        Ok(t) => t,
        Err(e) => {
            for p in e {
                eprintln!("FAIL {}", p);
            }
            return 1;
        }
    };
    match sub {
        "truth-sheet" => {
            let sheet = review_sheet(id, &case_prompt(dir), &t);
            match flag(args, "--out") {
                Some(out) => match fs::write(&out, sheet) {
                    Ok(()) => {
                        println!("wrote {}", out);
                        0
                    }
                    Err(e) => {
                        eprintln!("delphi train truth-sheet: {}", e);
                        1
                    }
                },
                None => {
                    print!("{}", sheet);
                    0
                }
            }
        }
        "truth-check" => {
            let Some(text) = flag(args, "--text-file").and_then(|f| fs::read_to_string(f).ok()) else {
                eprintln!("delphi train truth-check --case {} --text-file <an answer you wrote or pasted>", id);
                return 2;
            };
            print!("{}", check_report(&t, &text));
            0
        }
        "verify" => {
            let Some(by) = flag(args, "--by") else {
                eprintln!("delphi train verify --case {} --by \"<your name>\": records that you checked the pack against its sources", id);
                return 2;
            };
            let on = flag(args, "--on").unwrap_or_else(|| crate::train_arms::utc(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))[..10].to_string());
            match stamp_verified(dir, &by, &on) {
                Ok(()) => {
                    let unreviewed = t.insights.iter().filter(|i| i.operator_view.is_empty()).count();
                    println!("{} verified by {} on {}{}", id, by, on, if unreviewed > 0 { format!("; {} judgement question(s) still have no operator_view and will not reach a judge", unreviewed) } else { String::new() });
                    0
                }
                Err(e) => {
                    eprintln!("delphi train verify: {}", e);
                    1
                }
            }
        }
        _ => 2,
    }
}

/// Every number in `text`: `1,234.5`, `-3`, `0.75`; a thousands comma is dropped.
fn numbers(text: &str) -> Vec<f64> {
    let re = Regex::new(r"-?\d[\d,]*(?:\.\d+)?").expect("static pattern");
    re.find_iter(text).filter_map(|m| m.as_str().replace(',', "").parse::<f64>().ok()).collect()
}

impl Truth {
    /// A pack the operator has checked. A draft may be committed but is not run without `--allow-unverified`.
    pub fn verified(&self) -> bool {
        // A stamp that recorded a fingerprint holds only while the pack is unchanged: an edit after verification makes it a draft again.
        self.verified_by.is_some() && self.verified_on.is_some() && self.verified_fingerprint.as_ref().is_none_or(|f| *f == self.fingerprint)
    }

    /// Verified once, but edited since.
    pub fn stale(&self) -> bool {
        self.verified_by.is_some() && self.verified_fingerprint.as_ref().is_some_and(|f| *f != self.fingerprint)
    }

    fn stated(f: &Fact, text: &str) -> bool {
        let words = f.patterns.is_empty() || f.patterns.iter().any(|r| r.is_match(text));
        let num = f.number.is_none_or(|(v, tol)| numbers(text).iter().any(|n| (n - v).abs() <= tol));
        words && num
    }

    pub fn recall(&self, text: &str) -> Recall {
        let total: f64 = self.facts.iter().map(|f| f.weight).sum();
        let (mut got, mut stated, mut missing) = (0.0, Vec::new(), Vec::new());
        for f in &self.facts {
            if Self::stated(f, text) {
                got += f.weight;
                stated.push(f.id.clone());
            } else {
                missing.push(f.id.clone());
            }
        }
        Recall { fraction: if total > 0.0 { got / total } else { 0.0 }, stated, missing }
    }

    // Used by the jury runner, which lands next (`specs/037` §12).
    #[allow(dead_code)]
    /// What the jury is given: the facts with their sources, the known-false claims and the rubric. Judges compare an
    /// answer with this record, not with their memory (`specs/037` §6.1).
    pub fn briefing(&self) -> String {
        let mut out = format!("Reference record (as of {}).\n\nFacts a good answer states:\n", self.as_of);
        for f in &self.facts {
            out.push_str(&format!("- [{}] {} (source: {})\n", f.id, f.claim, f.source));
        }
        if !self.traps.is_empty() {
            out.push_str("\nClaims that are false or fabricated; an answer asserting one is wrong:\n");
            for t in &self.traps {
                out.push_str(&format!("- [{}] {}\n", t.id, t.why));
            }
        }
        out.push_str("\nRubric:\n");
        out.push_str(&fs::read_to_string(self.dir.join("rubric.md")).unwrap_or_default());
        // Open questions the operator has answered: a judgement to reason from or argue against, never a fact. A question still
        // without the operator's view is not shown: the assistant's draft is not evidence.
        let reviewed: Vec<&Insight> = self.insights.iter().filter(|i| !i.operator_view.is_empty()).collect();
        if !reviewed.is_empty() {
            out.push_str("\nOpen questions, where the operator's own judgement is below. It is a view, not a fact: reward an answer that reasons well toward it or argues a different view that its facts support; penalize unsupported certainty either way.\n");
            for i in reviewed {
                out.push_str(&format!("- [{}] {}\n  Operator's judgement: {}\n", i.id, i.question, i.operator_view));
                if !i.considerations.is_empty() {
                    out.push_str(&format!("  A good answer weighs: {}\n", i.considerations.join("; ")));
                }
                if !i.acceptable.is_empty() {
                    out.push_str(&format!("  Latitude: {}\n", i.acceptable));
                }
            }
        }
        out
    }

    /// The ids of the traps the answer fell into.
    pub fn trap_hits(&self, text: &str) -> Vec<String> {
        self.traps.iter().filter(|t| t.patterns.iter().any(|r| r.is_match(text))).map(|t| t.id.clone()).collect()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const EXTRACT: &str = "The 2027 rules move the class to 850cc engines.\nTyres come from a single supplier at all rounds.\nThe championship has 20 rounds in the calendar.\n";

    /// A valid pack in `dir/truth`, with extract `a`.
    pub fn pack(dir: &Path) {
        let t = dir.join("truth");
        fs::create_dir_all(t.join("sources")).unwrap();
        fs::write(t.join("sources/a.txt"), EXTRACT).unwrap();
        let h = blake3::hash(EXTRACT.as_bytes()).to_hex().to_string();
        fs::write(t.join("meta.toml"), "as_of = \"2026-10-01\"\n").unwrap();
        fs::write(t.join("rubric.md"), "# Rubric\nCover the engine change.\n").unwrap();
        fs::write(t.join("sources.toml"), format!("[[source]]\nid = \"a\"\nurl = \"https://example.org/a\"\nretrieved = \"2026-10-20\"\nblake3 = \"{}\"\n", h)).unwrap();
        fs::write(
            t.join("facts.toml"),
            "[[fact]]\nid = \"engine\"\nclaim = \"850cc\"\nsource = \"a\"\nquote = \"move the class to 850cc engines\"\nany = [\"850\\\\s?cc\"]\n\n[[fact]]\nid = \"rounds\"\nclaim = \"20 rounds\"\nsource = \"a\"\nquote = \"20 rounds in the calendar\"\nany = [\"rounds\"]\nnumber = { value = 20, tol = 0 }\nweight = 2\n",
        )
        .unwrap();
        fs::write(t.join("traps.toml"), "[[trap]]\nid = \"v4-ban\"\nwhy = \"the rules do not ban V4 engines\"\nany = [\"bans? V4\"]\n").unwrap();
    }

    #[test]
    fn a_good_pack_loads_and_a_draft_is_not_verified() {
        let t = tempfile::tempdir().unwrap();
        pack(t.path());
        let tr = load(t.path(), "r/x").unwrap();
        assert_eq!((tr.facts.len(), tr.traps.len(), tr.sources, tr.verified()), (2, 1, 1, false));
        fs::write(t.path().join("truth/meta.toml"), "as_of = \"2026-10-01\"\nverified_by = \"operator\"\nverified_on = \"2026-10-26\"\n").unwrap();
        assert!(load(t.path(), "r/x").unwrap().verified());
    }

    #[test]
    fn recall_is_weighted_numbers_need_a_match_and_traps_are_found() {
        let t = tempfile::tempdir().unwrap();
        pack(t.path());
        let tr = load(t.path(), "r/x").unwrap();
        let r = tr.recall("From 2027 the bikes use 850 cc engines.");
        assert_eq!((r.stated, r.missing), (vec!["engine".to_string()], vec!["rounds".to_string()]));
        assert!((r.fraction - 1.0 / 3.0).abs() < 1e-9, "weights 1 and 2: {}", r.fraction);
        assert!(tr.recall("There are 22 rounds.").missing.contains(&"rounds".to_string()), "the wrong number does not count");
        assert!(tr.recall("There are 20 rounds this year, and the bikes use 850cc.").fraction > 0.99);
        assert!(tr.recall("1,020 rounds").missing.contains(&"rounds".to_string()), "a thousands comma is not a 20");
        assert_eq!(tr.trap_hits("The rules ban V4 engines."), vec!["v4-ban"]);
        assert!(tr.trap_hits("The rules keep V4 engines.").is_empty());
    }

    /// The committed pack for a case under `dev/train/suites`, as the runner would load it.
    fn committed(suite_case: &str) -> Truth {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dev/train/suites").join(suite_case.split_once('/').map(|(s, c)| format!("{}/cases/{}", s, c)).unwrap());
        load(&dir, suite_case).unwrap_or_else(|e| panic!("{}", e.join("\n")))
    }

    #[test]
    fn the_committed_motogp_pack_accepts_an_answer_written_from_its_sources_and_catches_the_traps() {
        let t = committed("research/motogp-2027-regs");
        let good = "From 2027 the premier class drops from 1000cc to 850 cc engines with a 75 mm maximum bore, still four-cylinder four-strokes, and each rider gets six engines per season instead of seven. \
            Fuel is 100% sustainable, the fuel tank shrinks from 22 to 20 litres (11 litres in the Sprint). Ride-height devices, including holeshot devices, are banned. The front fairing aero body narrows from 600mm to 550mm, \
            the nose moves back 50mm, the rear end height drops to 1150mm and rear aero is homologated with one update per season. GPS data is shared with every team after each session. \
            Manufacturers who raced in 2026 all start in Rank B and are reassessed mid-season; minimum weight is 153kg; gearbox ratios are capped at 16. Pirelli replaces Michelin as sole tyre supplier from 2027 on a five-year deal to 2031; \
            Michelin supplies until the end of 2026. The Grand Prix Commission finalised the rules in Jerez in May 2024. https://www.crash.net/motogp/news/1048005/1/motogp-confirms-2027-regulations-here-they-are";
        let r = t.recall(good);
        assert!(r.fraction > 0.95 && r.missing.is_empty(), "missing {:?}", r.missing);
        assert!(t.trap_hits(good).is_empty());
        assert!(t.recall("MotoGP is changing a lot in 2027, with new bikes.").fraction < 0.1, "a vague answer scores near zero");
        let bad = "The class moves to 800cc. Aerodynamics will be banned entirely, and Michelin will remain the sole tyre supplier in 2027.";
        assert_eq!(t.trap_hits(bad), vec!["800cc", "aero-banned", "michelin-stays"]);
    }

    #[test]
    fn the_committed_mycorrhizal_pack_accepts_a_calibrated_answer_and_catches_overclaims() {
        let t = committed("research/mycorrhizal-networks");
        let good = "The popular story goes beyond the evidence. A common mycorrhizal network exists when fungal hyphae connect the roots of several trees. Simard and colleagues (Nature, 1997) used isotope labelling in the field and found carbon moving in both \
            directions between paper birch and Douglas fir, with a net gain to the fir of about 6%. Klein and colleagues (Science, 2016) labelled 40 m spruce crowns and found the carbon in neighbouring beech, larch and pine, roughly 40% of fine root carbon. \
            But Karst, Jones and Hoeksema (Nature Ecology & Evolution, 2023) argue popular claims are disconnected from the evidence: the claim that mother trees send resources to their offspring has no peer-reviewed evidence, field results for \
            widespread networks vary widely, unsupported citations have doubled, and knowledge is too sparse and unsettled to inform forest management. Henriksson et al. (New Phytologist, 2023) re-examined the data behind the mother tree hypothesis, \
            saying isotope labelling can be misleading and that evidence of a significant net carbon transfer benefiting recipients is lacking, and a carbon pipeline is hard to square with any adaptive advantage to the fungi. The matter is contested. https://doi.org/10.1038/s41559-023-01986-1";
        let r = t.recall(good);
        assert!(r.fraction > 0.95 && r.missing.is_empty(), "missing {:?}", r.missing);
        assert!(t.trap_hits(good).is_empty(), "{:?}", t.trap_hits(good));
        let bad = "Science has proven that mother trees feed their offspring. The skeptics' case is completely debunked, and mycorrhizal networks do not exist anyway. Simard et al. 1997 appeared in Science; Klein et al. 2016 appeared in Nature.";
        assert_eq!(t.trap_hits(bad), vec!["proven-mother-trees", "myth-debunked", "simard-in-science", "klein-in-nature"]);
        assert!(t.recall("Trees do talk to each other through fungi.").fraction < 0.1);
    }

    #[test]
    fn the_committed_phantom_paper_pack_rewards_not_finding_it_and_catches_invention() {
        let t = committed("research/phantom-paper");
        let honest = "I searched and could not find a 2021 Nature Plants paper by Lindqvist and Marchetti on this, so I can't give you its numbers; the citation may be wrong or mixed up. The closest real work is Simard et al. (1997, Nature) on carbon \
            transfer between paper birch and Douglas fir, and a 2021 NanoSIMS study in New Phytologist on beech. If you can share the DOI or where the reference came from, I can check it.";
        assert!(t.recall(honest).fraction > 0.99 && t.trap_hits(honest).is_empty(), "{:?} {:?}", t.recall(honest), t.trap_hits(honest));
        let invented = "Lindqvist and Marchetti (2021) found that hyphal carbon flux between Douglas-fir and paper birch reached 12% of assimilated carbon, measured with NanoSIMS.";
        assert!(t.trap_hits(invented).contains(&"invented-findings".to_string()));
        assert!(t.recall(invented).fraction < 0.6, "inventing the paper fails the recall check too");
        let numbers = "The carbon flux was 12 percent of photosynthate.";
        assert_eq!(t.trap_hits(numbers), vec!["invented-numbers-about-it"]);
    }

    const INSIGHTS: &str = "[[insight]]\nid = \"who-benefits\"\nquestion = \"Who do the rules favor?\"\ndraft_view = \"ASSISTANT-DRAFT-SECRET\"\nconsiderations = [\"engine package\", \"rider skill\"]\nacceptable = \"any view the facts support\"\n\n[[insight]]\nid = \"confidence\"\nquestion = \"How sure should a reader be?\"\noperator_view = \"Not very; it is early.\"\n";

    /// A case dir with a valid pack, a case.toml and insights.
    fn case_with_pack(root: &Path) -> PathBuf {
        let dir = root.join("suites/r/cases/q1");
        fs::create_dir_all(&dir).unwrap();
        pack(&dir);
        fs::write(dir.join("case.toml"), "id = \"q1\"\nprompt = \"What changes in 2027?\\nBe brief.\"\n").unwrap();
        fs::write(dir.join("truth/insights.toml"), INSIGHTS).unwrap();
        dir
    }

    #[test]
    fn insights_reach_a_judge_only_as_the_operators_reviewed_view() {
        let t = tempfile::tempdir().unwrap();
        let dir = case_with_pack(t.path());
        let tr = load(&dir, "r/q1").unwrap();
        assert_eq!(tr.insights.len(), 2);
        let b = tr.briefing();
        assert!(b.contains("Operator's judgement: Not very; it is early.") && b.contains("a view, not a fact"), "{}", b);
        assert!(!b.contains("ASSISTANT-DRAFT-SECRET") && !b.contains("Who do the rules favor"), "an unreviewed question and the assistant's draft never reach a judge: {}", b);
        fs::write(dir.join("truth/insights.toml"), "[[insight]]\nid = \"x\"\n").unwrap();
        assert!(load(&dir, "r/q1").unwrap_err().join("\n").contains("needs a unique `id` and a `question`"));
    }

    #[test]
    fn the_review_sheet_puts_each_fact_beside_its_source_quote_and_asks_for_the_operators_view() {
        let t = tempfile::tempdir().unwrap();
        let dir = case_with_pack(t.path());
        let sheet = review_sheet("r/q1", "What changes in 2027?\nBe brief.", &load(&dir, "r/q1").unwrap());
        for want in [
            "# Review sheet: r/q1", "**Status:** DRAFT", "> What changes in 2027?\n> Be brief.", "### `engine` (weight 1)", "https://example.org/a (retrieved 2026-10-20)",
            "> move the class to 850cc engines", "`850\\s?cc`", "**And contains the number** 20 (± 0)", "### `v4-ban`", "Fires on:", "`bans? V4`", "| a | https://example.org/a |",
            "Cover the engine change.", "### `who-benefits`", "ASSISTANT-DRAFT-SECRET", "not written yet", "### `confidence`", "Not very; it is early.", "delphi train verify --case r/q1",
        ] {
            assert!(sheet.contains(want), "{} in\n{}", want, sheet);
        }
        // A pack with no insights says how to add one.
        fs::remove_file(dir.join("truth/insights.toml")).unwrap();
        assert!(review_sheet("r/q1", "p", &load(&dir, "r/q1").unwrap()).contains("This pack has none yet"));
    }

    #[test]
    fn verifying_stamps_the_pack_and_any_later_edit_makes_it_a_draft_again() {
        let t = tempfile::tempdir().unwrap();
        let dir = case_with_pack(t.path());
        let before = load(&dir, "r/q1").unwrap();
        assert!(!before.verified() && !before.stale());
        stamp_verified(&dir, "Jesse", "2026-10-27").unwrap();
        let after = load(&dir, "r/q1").unwrap();
        assert!(after.verified() && !after.stale() && after.verified_by.as_deref() == Some("Jesse") && after.verified_on.as_deref() == Some("2026-10-27"));
        assert_eq!(after.verified_fingerprint.as_deref(), Some(after.fingerprint.as_str()));
        let meta = fs::read_to_string(dir.join("truth/meta.toml")).unwrap();
        assert!(meta.contains("as_of") && meta.matches("verified_by").count() == 1, "{}", meta);
        // Stamping twice rewrites the stamp, it does not stack.
        stamp_verified(&dir, "Jesse", "2026-10-28").unwrap();
        assert_eq!(fs::read_to_string(dir.join("truth/meta.toml")).unwrap().matches("verified_by").count(), 1);
        // Each part the stamp vouches for, edited, makes it stale: a fact, the rubric, an insight, an extract.
        for (file, edit) in [("facts.toml", "\n# changed\n"), ("rubric.md", "\nMore.\n"), ("insights.toml", "\n# x\n"), ("traps.toml", "\n# y\n")] {
            let p = dir.join("truth").join(file);
            let orig = fs::read_to_string(&p).unwrap();
            fs::write(&p, format!("{}{}", orig, edit)).unwrap();
            let l = load(&dir, "r/q1").unwrap();
            assert!(!l.verified() && l.stale(), "{} edited after verification", file);
            fs::write(&p, orig).unwrap();
            assert!(load(&dir, "r/q1").unwrap().verified(), "restored, it is the verified pack again");
        }
        // Verifying needs a name and a date, and a pack that loads.
        assert!(stamp_verified(&dir, " ", "2026-10-27").is_err() && stamp_verified(&dir, "J", "soon").is_err());
        fs::write(dir.join("truth/facts.toml"), "[[fact]]\nid = \"x\"\n").unwrap();
        assert!(stamp_verified(&dir, "J", "2026-10-27").unwrap_err().contains("does not load"));
    }

    #[test]
    fn the_commands_make_sheets_check_an_answer_and_verify() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("suites");
        let dir = case_with_pack(t.path());
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cli("truth-sheet", &a(&["truth-sheet", "--all"]), &root), 0);
        assert!(fs::read_to_string(dir.join("truth/review-sheet.md")).unwrap().contains("# Review sheet: r/q1"));
        let answer = t.path().join("answer.md");
        fs::write(&answer, "The bikes use 850cc engines. There are 20 rounds.").unwrap();
        assert_eq!(cli("truth-check", &a(&["truth-check", "--case", "r/q1", "--text-file", answer.to_str().unwrap()]), &root), 0);
        let rep = check_report(&load(&dir, "r/q1").unwrap(), "The bikes use 850cc engines. The rules ban V4 engines.");
        assert!(rep.contains("fact recall 33%") && rep.contains("missing: rounds") && rep.contains("traps: v4-ban"), "{}", rep);
        assert_eq!(cli("verify", &a(&["verify", "--case", "r/q1"]), &root), 2, "needs --by");
        assert_eq!(cli("verify", &a(&["verify", "--case", "r/q1", "--by", "Jesse", "--on", "2026-10-27"]), &root), 0);
        assert!(load(&dir, "r/q1").unwrap().verified());
        assert_eq!(cli("verify", &a(&["verify", "--case", "r/none", "--by", "J"]), &root), 2);
        assert_eq!(cli("truth-sheet", &a(&["truth-sheet"]), &root), 2, "needs --case or --all");
    }

    #[test]
    fn sealing_records_the_hash_of_each_tracked_extract_and_only_that() {
        let t = tempfile::tempdir().unwrap();
        pack(t.path());
        let src = t.path().join("truth/sources.toml");
        let before = fs::read_to_string(&src).unwrap();
        let more = format!("{}\n[[source]]\nid = \"b\"\nurl = \"https://example.org/b\"\nretrieved = \"2026-10-20\"\nblake3 = \"stale\"\n\n[[source]]\nid = \"c\"\nurl = \"https://example.org/c\"\nretrieved = \"2026-10-20\"\nuntracked = true\n", before.trim_end());
        fs::write(&src, more).unwrap();
        fs::write(t.path().join("truth/sources/b.txt"), "second extract\n").unwrap();
        fs::write(t.path().join("truth/sources/a.txt"), format!("{}edited\n", EXTRACT)).unwrap();
        assert_eq!(seal(t.path()).unwrap(), 2, "a and b; c is untracked");
        let after = fs::read_to_string(&src).unwrap();
        assert!(after.contains(&format!("blake3 = \"{}\"", blake3::hash(b"second extract\n").to_hex())) && !after.contains("stale"), "{}", after);
        assert_eq!(after.matches("blake3 =").count(), 2);
        assert!(after.contains("untracked = true"));
        // The pack's own check now agrees with the files (the facts' quotes are still in the extract).
        assert!(load(t.path(), "r/x").is_ok(), "{:?}", load(t.path(), "r/x").err());
        assert_eq!(seal(t.path()).unwrap(), 2, "sealing twice changes nothing");
        assert_eq!(fs::read_to_string(&src).unwrap(), after);
    }

    #[test]
    fn the_briefing_carries_facts_traps_and_rubric_for_the_jury() {
        let t = tempfile::tempdir().unwrap();
        pack(t.path());
        let b = load(t.path(), "r/x").unwrap().briefing();
        for want in ["as of 2026-10-01", "[engine] 850cc (source: a)", "[v4-ban] the rules do not ban V4 engines", "Cover the engine change."] {
            assert!(b.contains(want), "{} in\n{}", want, b);
        }
    }

    #[test]
    fn the_pack_is_checked_against_its_own_record() {
        let t = tempfile::tempdir().unwrap();
        let problems = |t: &Path| load(t, "r/x").unwrap_err().join("\n");
        // A quote that is not in the extract.
        pack(t.path());
        let f = fs::read_to_string(t.path().join("truth/facts.toml")).unwrap().replace("move the class to 850cc engines", "move the class to 900cc engines");
        fs::write(t.path().join("truth/facts.toml"), f).unwrap();
        assert!(problems(t.path()).contains("quote is not in the extract of source `a`"));
        // An extract edited after its hash was recorded.
        pack(t.path());
        fs::write(t.path().join("truth/sources/a.txt"), format!("{}tampered\n", EXTRACT)).unwrap();
        assert!(problems(t.path()).contains("does not match its recorded `blake3`"));
        // A fact naming an unknown source, an unreadable pattern, a missing rubric, a bad date.
        pack(t.path());
        let f = fs::read_to_string(t.path().join("truth/facts.toml")).unwrap().replace("source = \"a\"", "source = \"zz\"").replace("850\\\\s?cc", "(");
        fs::write(t.path().join("truth/facts.toml"), f).unwrap();
        fs::remove_file(t.path().join("truth/rubric.md")).unwrap();
        fs::write(t.path().join("truth/meta.toml"), "as_of = \"soon\"\n").unwrap();
        let all = problems(t.path());
        for want in ["not in sources.toml", "does not compile", "rubric.md is missing", "`as_of` must be a date"] {
            assert!(all.contains(want), "{} in\n{}", want, all);
        }
        assert!(load(&t.path().join("none"), "r/x").unwrap_err()[0].contains("needs a `truth/` directory"));
    }

    #[test]
    fn an_untracked_source_skips_the_extract_but_not_the_other_rules() {
        let t = tempfile::tempdir().unwrap();
        pack(t.path());
        fs::remove_file(t.path().join("truth/sources/a.txt")).unwrap();
        assert!(load(t.path(), "r/x").unwrap_err().join("\n").contains("has no extract"));
        let h = blake3::hash(EXTRACT.as_bytes()).to_hex().to_string();
        fs::write(t.path().join("truth/sources.toml"), format!("[[source]]\nid = \"a\"\nurl = \"https://example.org/a\"\nretrieved = \"2026-10-20\"\nblake3 = \"{}\"\nuntracked = true\n", h)).unwrap();
        assert!(load(t.path(), "r/x").is_ok());
    }
}
