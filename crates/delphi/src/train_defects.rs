//! `delphi train defects`: does a reviewing seat find REAL defects, or only plausible-sounding ones? (`specs/037` §5, M3 of the
//! build plan of 2026-11-07; `specs/048` spirit: no model grades a model.)
//!
//! A seeded generator writes a small synthetic Python module. It holds K **planted defects** of known kinds from a fixed
//! catalogue and N **decoys**: constructs that look suspicious and are correct. The answer key (each defect's line and kind,
//! each decoy's line) goes to a file outside the fixture, so the seat under test never sees it. The seat answers with lines in a
//! fixed format, `FINDING <line> <kind>: <one sentence>`, and scoring is a regular expression and arithmetic:
//!
//! - a finding **catches** a planted defect when its line is within one of the defect's line and its kind is one the key accepts
//!   for that defect (a second finding for a defect already caught is ignored);
//! - a finding within one line of a **decoy** is a **false positive**;
//! - any other finding is **unmatched** (a real line with the wrong kind, or a line that holds nothing).
//!
//! # What this is not
//!
//! These figures (recall, false positives, unmatched, unique catches) are **diagnostics in a training report**. They are not a
//! seat score. They are never an input to routing, ranking, staffing, a Council decision or any default (`specs/078`: Council
//! measurement may not score seat accuracy as a ranking or routing input; Goodhart: a number that picks seats trains seats to
//! the number). The `review_findings` check's pass bit is a threshold on one synthetic case in one experiment, nothing more.
//!
//! Nothing here calls a model.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::train_items::Rng;

/// The kinds a defect may have, in the order the prompt lists them.
pub const KINDS: [&str; 8] = ["off-by-one", "inverted-condition", "missing-error-check", "wrong-operator", "resource-leak", "swapped-arguments", "stale-constant", "injection"];

/// The kinds a finding may name for a defect of `kind`. Neighbours are accepted where two honest names exist for one fault
/// (`value < low and value > high` is a wrong operator and is also a wrong condition); nothing else is.
pub fn accepted(kind: &str) -> &'static [&'static str] {
    match kind {
        "off-by-one" => &["off-by-one", "wrong-operator"],
        "inverted-condition" => &["inverted-condition", "wrong-operator"],
        "missing-error-check" => &["missing-error-check"],
        "wrong-operator" => &["wrong-operator", "inverted-condition"],
        "resource-leak" => &["resource-leak"],
        "swapped-arguments" => &["swapped-arguments"],
        "stale-constant" => &["stale-constant"],
        "injection" => &["injection"],
        _ => &[],
    }
}

/// One construct: its lines, and which of them is the one a review should name. `{f}` is a function name, `{C}` a constant
/// prefix, `{n}` and `{m}` numbers (`{m}` differs from `{n}`).
struct Tpl {
    kind: &'static str,
    /// Decoys only: why the construct is correct.
    why: &'static str,
    target: usize,
    lines: &'static [&'static str],
}

const DEFECTS: [Tpl; 8] = [
    Tpl { kind: "off-by-one", why: "", target: 3, lines: &["def {f}(rows):", "    \"\"\"Return the sum of every value in rows.\"\"\"", "    total = 0", "    for i in range(1, len(rows)):", "        total += rows[i]", "    return total"] },
    Tpl { kind: "inverted-condition", why: "", target: 2, lines: &["def {f}(balance, amount):", "    \"\"\"Withdraw amount; refuse when the balance does not cover it.\"\"\"", "    if balance >= amount:", "        raise ValueError(\"insufficient funds\")", "    return balance - amount"] },
    Tpl { kind: "missing-error-check", why: "", target: 3, lines: &["def {f}(conn, key):", "    \"\"\"Return the cached value for key; raise KeyError when it is absent.\"\"\"", "    row = conn.execute(\"SELECT value FROM cache WHERE key = ?\", (key,)).fetchone()", "    return row[0]"] },
    Tpl { kind: "wrong-operator", why: "", target: 2, lines: &["def {f}(value, low, high):", "    \"\"\"Return True when value lies outside the range low to high.\"\"\"", "    return value < low and value > high"] },
    Tpl { kind: "resource-leak", why: "", target: 2, lines: &["def {f}(path):", "    \"\"\"Return the number of lines in the file at path.\"\"\"", "    fh = open(path)", "    count = sum(1 for _ in fh)", "    return count"] },
    Tpl { kind: "swapped-arguments", why: "", target: 2, lines: &["def {f}(src, dst):", "    \"\"\"Copy the file at src to dst.\"\"\"", "    shutil.copyfile(dst, src)"] },
    Tpl { kind: "stale-constant", why: "", target: 5, lines: &["{C}_PAGE_SIZE = {n}", "", "", "def {f}(items, page):", "    \"\"\"Return the items on a page of {C}_PAGE_SIZE items; pages count from 0.\"\"\"", "    start = page * {m}", "    return items[start:start + {C}_PAGE_SIZE]"] },
    Tpl { kind: "injection", why: "", target: 2, lines: &["def {f}(conn, name):", "    \"\"\"Return the row of the user called name.\"\"\"", "    query = \"SELECT * FROM users WHERE name = '\" + name + \"'\"", "    return conn.execute(query).fetchone()"] },
];

/// Constructs that look like the defects above and are correct. Indexed like [`DEFECTS`] by the fault they imitate.
const DECOYS: [Tpl; 8] = [
    Tpl { kind: "off-by-one", why: "pairs each value with its predecessor, so starting at 1 is right", target: 3, lines: &["def {f}(rows):", "    \"\"\"Return the differences between consecutive values.\"\"\"", "    out = []", "    for i in range(1, len(rows)):", "        out.append(rows[i] - rows[i - 1])", "    return out"] },
    Tpl { kind: "inverted-condition", why: "the negated range test rejects exactly the values outside 0 to 100", target: 2, lines: &["def {f}(pct):", "    \"\"\"Reject a percentage outside 0 to 100.\"\"\"", "    if not 0 <= pct <= 100:", "        raise ValueError(\"percentage out of range\")", "    return pct / 100"] },
    Tpl { kind: "missing-error-check", why: "the None case is handled two lines above", target: 5, lines: &["def {f}(conn, key):", "    \"\"\"Return the cached value for key; raise KeyError when it is absent.\"\"\"", "    row = conn.execute(\"SELECT value FROM cache WHERE key = ?\", (key,)).fetchone()", "    if row is None:", "        raise KeyError(key)", "    return row[0]"] },
    Tpl { kind: "wrong-operator", why: "the negated chained comparison is the outside-the-range test", target: 2, lines: &["def {f}(value, low, high):", "    \"\"\"Return True when value lies outside the range low to high.\"\"\"", "    return not (low <= value <= high)"] },
    Tpl { kind: "resource-leak", why: "the file is closed in the finally block", target: 2, lines: &["def {f}(path):", "    \"\"\"Return the number of lines in the file at path.\"\"\"", "    fh = open(path)", "    try:", "        return sum(1 for _ in fh)", "    finally:", "        fh.close()"] },
    Tpl { kind: "swapped-arguments", why: "relpath takes the target first and the start second, as written", target: 2, lines: &["def {f}(frm, to):", "    \"\"\"Return the path of `to` relative to the directory `frm`.\"\"\"", "    return os.path.relpath(to, frm)"] },
    Tpl { kind: "stale-constant", why: "the literal is the documented page size", target: 2, lines: &["def {f}(items, page):", "    \"\"\"Return the items on a page of {n} items; pages count from 0.\"\"\"", "    start = page * {n}", "    return items[start:start + {n}]"] },
    Tpl { kind: "injection", why: "the table name comes from a fixed mapping; the value is a bound parameter", target: 6, lines: &["{C}_TABLES = {\"orders\": \"orders_2024\", \"refunds\": \"refunds_2024\"}", "", "", "def {f}(conn, kind, ident):", "    \"\"\"Return the row with id ident from the table for kind (one of {C}_TABLES).\"\"\"", "    table = {C}_TABLES[kind]", "    return conn.execute(f\"SELECT * FROM {table} WHERE id = ?\", (ident,)).fetchone()"] },
];

const VERBS: [&str; 8] = ["load", "fetch", "build", "check", "apply", "merge", "read", "count"];
const NOUNS: [&str; 8] = ["order", "ledger", "invoice", "session", "token", "quota", "report", "batch"];
const HEADER: [&str; 4] = ["\"\"\"Order and account helpers.\"\"\"", "import os", "import shutil", ""];

#[derive(Debug, Clone, PartialEq)]
pub struct Defect {
    pub line: usize,
    pub kind: String,
}

impl Defect {
    /// A stable name within a case: the line and the kind. Unique catches are counted by this and the case id.
    pub fn id(&self) -> String {
        format!("{}:{}", self.line, self.kind)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decoy {
    pub line: usize,
    pub imitates: String,
    pub why: String,
}

/// The answer key. It is written outside the fixture and read only by the check.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub defects: Vec<Defect>,
    pub decoys: Vec<Decoy>,
}

pub struct Artifact {
    pub source: String,
    pub key: Key,
}

fn shuffle<T>(rng: &mut Rng, xs: &mut [T]) {
    for i in (1..xs.len()).rev() {
        xs.swap(i, rng.below(i + 1));
    }
}

/// The module for `seed` with `k` planted defects (1 to 8, distinct kinds) and `decoys` correct look-alikes (0 to 8, distinct).
/// The same arguments give the same bytes on every platform.
pub fn generate(seed: u64, k: usize, decoys: usize) -> Artifact {
    let mut rng = Rng::new(seed ^ 0x00D3_FEC7);
    let (k, decoys) = (k.clamp(1, 8), decoys.min(8));
    let mut d_idx: Vec<usize> = (0..8).collect();
    shuffle(&mut rng, &mut d_idx);
    let mut c_idx: Vec<usize> = (0..8).collect();
    shuffle(&mut rng, &mut c_idx);
    // (template, is_defect), shuffled into file order.
    let mut parts: Vec<(&Tpl, bool)> = d_idx[..k].iter().map(|i| (&DEFECTS[*i], true)).chain(c_idx[..decoys].iter().map(|i| (&DECOYS[*i], false))).collect();
    shuffle(&mut rng, &mut parts);
    let mut names: Vec<(usize, usize)> = (0..64).map(|i| (i / 8, i % 8)).collect();
    shuffle(&mut rng, &mut names);
    let mut lines: Vec<String> = HEADER.iter().map(|s| s.to_string()).collect();
    let (mut defects, mut decoy_rows) = (Vec::new(), Vec::new());
    for (i, (t, is_defect)) in parts.iter().enumerate() {
        let (v, n) = names[i];
        let num = [20usize, 25, 40, 50, 100][rng.below(5)];
        let sub = |s: &str| s.replace("{f}", &format!("{}_{}", VERBS[v], NOUNS[n])).replace("{C}", &NOUNS[n].to_uppercase()).replace("{n}", &num.to_string()).replace("{m}", &(num * 2).to_string());
        lines.push(String::new());
        lines.push(String::new());
        let at = lines.len() + t.target + 1;
        lines.extend(t.lines.iter().map(|l| sub(l)));
        if *is_defect {
            defects.push(Defect { line: at, kind: t.kind.to_string() });
        } else {
            decoy_rows.push(Decoy { line: at, imitates: t.kind.to_string(), why: t.why.to_string() });
        }
    }
    // Header lines count too: `at` was computed from the length before the block, so it is already the 1-based file line.
    let mut source = lines.join("\n");
    source.push('\n');
    Artifact { source, key: Key { defects, decoys: decoy_rows } }
}

impl Key {
    pub fn to_json(&self, seed: u64, file: &str) -> Value {
        json!({"seed": seed, "file": file,
               "defects": self.defects.iter().map(|d| json!({"id": d.id(), "line": d.line, "kind": d.kind, "accept": accepted(&d.kind)})).collect::<Vec<_>>(),
               "decoys": self.decoys.iter().map(|d| json!({"line": d.line, "imitates": d.imitates, "why": d.why})).collect::<Vec<_>>()})
    }

    pub fn from_json(text: &str) -> Result<Key, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON ({})", e))?;
        let defects: Vec<Defect> = v["defects"].as_array().ok_or("no `defects` array")?.iter().filter_map(|d| Some(Defect { line: d["line"].as_u64()? as usize, kind: d["kind"].as_str()?.to_string() })).collect();
        if defects.is_empty() {
            return Err("the key lists no defects".into());
        }
        let decoys = v["decoys"].as_array().map(|a| a.iter().filter_map(|d| Some(Decoy { line: d["line"].as_u64()? as usize, imitates: d["imitates"].as_str().unwrap_or("").to_string(), why: d["why"].as_str().unwrap_or("").to_string() })).collect()).unwrap_or_default();
        Ok(Key { defects, decoys })
    }
}

/// The findings in an answer: `FINDING <line> <kind>: <sentence>`, one per line, an optional list marker allowed. Anything else
/// in the answer is ignored. Returns `(line, kind)` in order.
pub fn parse_findings(answer: &str) -> Vec<(usize, String)> {
    let re = regex::Regex::new(r"(?im)^[ \t]*(?:[-*][ \t]+)?FINDING[ \t]+(\d+)[ \t]+([a-z][a-z-]*)[ \t]*:").expect("a fixed pattern");
    re.captures_iter(answer).filter_map(|c| Some((c[1].parse::<usize>().ok()?, c[2].to_lowercase()))).collect()
}

/// One answer's figures. Diagnostics only (see the module note).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Score {
    pub findings: usize,
    pub defects: usize,
    /// The ids of the planted defects the answer caught.
    pub caught: Vec<String>,
    pub false_positives: usize,
    pub unmatched: usize,
}

impl Score {
    pub fn recall(&self) -> f64 {
        if self.defects == 0 { 0.0 } else { self.caught.len() as f64 / self.defects as f64 }
    }

    pub fn note(&self) -> Value {
        json!({"defects": self.defects, "caught": self.caught, "false_positives": self.false_positives, "unmatched": self.unmatched, "findings": self.findings})
    }

    pub fn from_note(v: &Value) -> Option<Score> {
        Some(Score {
            findings: v["findings"].as_u64()? as usize,
            defects: v["defects"].as_u64()? as usize,
            caught: v["caught"].as_array()?.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            false_positives: v["false_positives"].as_u64()? as usize,
            unmatched: v["unmatched"].as_u64()? as usize,
        })
    }
}

fn near(a: usize, b: usize) -> bool {
    a.abs_diff(b) <= 1
}

pub fn score(key: &Key, answer: &str) -> Score {
    let found = parse_findings(answer);
    let mut s = Score { findings: found.len(), defects: key.defects.len(), ..Score::default() };
    let mut caught: BTreeSet<String> = BTreeSet::new();
    for (line, kind) in &found {
        if let Some(d) = key.defects.iter().find(|d| near(d.line, *line) && accepted(&d.kind).contains(&kind.as_str())) {
            caught.insert(d.id());
        } else if key.decoys.iter().any(|d| near(d.line, *line)) {
            s.false_positives += 1;
        } else {
            s.unmatched += 1;
        }
    }
    s.caught = caught.into_iter().collect();
    s
}

/// The check's reading of a case: the key file in the case directory against the deliverable.
pub fn grade(case_dir: &Path, file: &str, deliverable: &str) -> Result<Score, String> {
    let text = fs::read_to_string(case_dir.join(file)).map_err(|e| format!("{}: {}", file, e))?;
    let key = Key::from_json(&text).map_err(|e| format!("{}: {}", file, e))?;
    Ok(score(&key, deliverable))
}

/// `Ok` when the answer clears the case's thresholds, else what fell short, in words.
pub fn verdict(s: &Score, min_recall: f64, max_false_positives: u64) -> Result<(), String> {
    if s.findings == 0 {
        return Err("no `FINDING <line> <kind>: <sentence>` lines in the deliverable".into());
    }
    let mut why = Vec::new();
    if s.recall() + 1e-9 < min_recall {
        why.push(format!("caught {} of {} planted defects ({:.0}%), wanted {:.0}%", s.caught.len(), s.defects, s.recall() * 100.0, min_recall * 100.0));
    }
    if s.false_positives as u64 > max_false_positives {
        why.push(format!("{} decoy(s) flagged, at most {} allowed", s.false_positives, max_false_positives));
    }
    if why.is_empty() { Ok(()) } else { Err(why.join("; ")) }
}

// ── a scripted instrument check ─────────────────────────────────────────────────────────────────────────────────

/// Scripted reviewers: functions, not models. They show what the scoring can tell apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reviewer {
    /// Names every planted defect with its true kind and nothing else.
    Thorough,
    /// Flags everything that looks odd: every defect and every decoy.
    Suspicious,
    /// Names only look-alikes and invents a plausible fault on a line that holds nothing wrong.
    Plausible,
    /// Reports the right lines under the wrong kinds.
    Mislabelling,
    /// Says nothing in the format.
    Silent,
}

impl Reviewer {
    pub const ALL: [Reviewer; 5] = [Reviewer::Thorough, Reviewer::Suspicious, Reviewer::Plausible, Reviewer::Mislabelling, Reviewer::Silent];

    pub fn name(self) -> &'static str {
        match self {
            Reviewer::Thorough => "thorough",
            Reviewer::Suspicious => "suspicious",
            Reviewer::Plausible => "plausible",
            Reviewer::Mislabelling => "mislabelling",
            Reviewer::Silent => "silent",
        }
    }

    pub fn answer(self, key: &Key) -> String {
        let line = |l: usize, k: &str| format!("FINDING {} {}: scripted finding.\n", l, k);
        match self {
            Reviewer::Thorough => key.defects.iter().map(|d| line(d.line, &d.kind)).collect(),
            Reviewer::Suspicious => key.defects.iter().map(|d| line(d.line, &d.kind)).chain(key.decoys.iter().map(|d| line(d.line, &d.imitates))).collect(),
            Reviewer::Plausible => key.decoys.iter().map(|d| line(d.line, &d.imitates)).chain(std::iter::once(line(1, "injection"))).collect(),
            Reviewer::Mislabelling => key.defects.iter().map(|d| line(d.line, if d.kind == "injection" { "resource-leak" } else { "injection" })).collect(),
            Reviewer::Silent => "The code looks fine to me.\n".to_string(),
        }
    }
}

/// The table `run` prints.
pub fn table(seed: u64, key: &Key, rows: &[(Reviewer, Score)]) -> String {
    let mut out = format!("SEEDED-DEFECT REVIEW (seed {}, {} planted defects, {} decoys): scripted reviewers\n", seed, key.defects.len(), key.decoys.len());
    out.push_str(&format!("{:<14}{:>10}{:>16}{:>11}\n", "reviewer", "recall", "false positives", "unmatched"));
    for (r, s) in rows {
        out.push_str(&format!("{:<14}{:>10}{:>16}{:>11}\n", r.name(), format!("{}/{}", s.caught.len(), s.defects), s.false_positives, s.unmatched));
    }
    out.push_str(DISCLAIMER);
    out.push('\n');
    out
}

/// Printed under every figure this module produces.
pub const DISCLAIMER: &str = "Diagnostics for a training report, not a seat score: nothing here feeds routing, ranking, staffing or any Council decision (specs/078). Scripted reviewers are functions, not models.";

// ── the report section ──────────────────────────────────────────────────────────────────────────────────────────

/// One graded run's review figures, with the case and arm it belongs to.
pub struct RunReview<'a> {
    pub case: &'a str,
    pub arm: &'a str,
    pub score: &'a Score,
}

/// The `SEEDED-DEFECT REVIEW` section of `delphi train report`: recall, false positives and unique catches by arm. Empty when no
/// run carries review figures.
pub fn report_section(runs: &[RunReview]) -> String {
    if runs.is_empty() {
        return String::new();
    }
    let arms: BTreeSet<&str> = runs.iter().map(|r| r.arm).collect();
    // Per case and arm: the defects any run of that arm caught. A defect is a unique catch when exactly one arm caught it and
    // at least two arms ran the case.
    let mut caught: BTreeMap<(&str, &str), BTreeSet<&str>> = BTreeMap::new();
    let mut ran: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for r in runs {
        ran.entry(r.case).or_default().insert(r.arm);
        caught.entry((r.case, r.arm)).or_default().extend(r.score.caught.iter().map(String::as_str));
    }
    let mut unique: BTreeMap<&str, usize> = BTreeMap::new();
    for (case, with) in &ran {
        if with.len() < 2 {
            continue;
        }
        let mut owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for arm in with {
            for d in caught.get(&(*case, *arm)).into_iter().flatten() {
                owners.entry(d).or_default().push(arm);
            }
        }
        for (_, o) in owners.iter().filter(|(_, o)| o.len() == 1) {
            *unique.entry(o[0]).or_default() += 1;
        }
    }
    let mut out = String::from("\nSEEDED-DEFECT REVIEW (planted defects found, by arm; diagnostics only)\n");
    out.push_str(&format!("{:<22}{:>6}{:>9}{:>9}{:>12}{:>12}{:>9}\n", "arm", "runs", "recall", "caught", "false pos.", "unmatched", "unique"));
    for arm in &arms {
        let rs: Vec<&RunReview> = runs.iter().filter(|r| r.arm == *arm).collect();
        let (def, hit): (usize, usize) = rs.iter().fold((0, 0), |(d, h), r| (d + r.score.defects, h + r.score.caught.len()));
        let (fp, un) = rs.iter().fold((0, 0), |(f, u), r| (f + r.score.false_positives, u + r.score.unmatched));
        let pct = if def == 0 { "—".to_string() } else { format!("{:.0}%", hit as f64 / def as f64 * 100.0) };
        out.push_str(&format!("{:<22}{:>6}{:>9}{:>9}{:>12}{:>12}{:>9}\n", arm, rs.len(), pct, format!("{}/{}", hit, def), fp, un, unique.get(arm).copied().unwrap_or(0)));
    }
    out.push_str("  unique = planted defects caught by only one arm, over cases that two or more arms ran. A false positive is a flagged decoy (correct code that looks suspicious); unmatched is a finding on neither.\n");
    out.push_str(&format!("  {}\n", DISCLAIMER.replace(" Scripted reviewers are functions, not models.", "")));
    out
}

// ── a case for a real model ─────────────────────────────────────────────────────────────────────────────────────

const FILE: &str = "review.py";
const KEY_FILE: &str = "review-key.json";

fn format_rules() -> String {
    format!("Answer with one line per defect, in exactly this form and nothing else on that line:\nFINDING <line number> <kind>: <one sentence>\nwhere <kind> is one of: {}. The line number is the file's own (the first line is 1). Report only real defects, where the code does not do what its docstring says; code that looks odd but is correct is not a defect. If you find none, say so.", KINDS.join(", "))
}

fn prompt() -> String {
    format!("Review @project/{}/{}. It is a small module whose functions each have a docstring stating what they should do. {}", crate::train_run::REPO_DIR, FILE, format_rules())
}

/// What an inline case asks: the module is in the prompt, with line numbers, so a seat with no tools can take it.
fn inline_prompt(source: &str) -> String {
    let numbered: String = source.lines().enumerate().map(|(i, l)| format!("{:>3} | {}\n", i + 1, l)).collect();
    format!("Below is a small Python module with line numbers. Each function has a docstring stating what it should do. Review it. {}\n\n---\n{}---", format_rules(), numbered)
}

/// What a generated case is made of.
#[derive(Debug, Clone, Copy)]
pub struct Params<'a> {
    pub seed: u64,
    pub k: usize,
    pub decoys: usize,
    pub split: &'a str,
    pub inline: bool,
    pub min_recall: f64,
    pub max_fp: u64,
}

/// Write a case directory (`case.toml`, `fixture/review.py`, `review-key.json`) for `delphi train run`. The key sits outside the
/// fixture, so the model under test never sees it.
pub fn write_case(dir: &Path, p: &Params) -> Result<(), String> {
    let Params { seed, k, decoys, split, inline, min_recall, max_fp } = *p;
    let art = generate(seed, k, decoys);
    let id = dir.file_name().and_then(|s| s.to_str()).ok_or("the case directory has no name")?;
    fs::create_dir_all(dir.join("fixture")).map_err(|e| format!("{}: {}", dir.display(), e))?;
    fs::write(dir.join("fixture").join(FILE), &art.source).map_err(|e| e.to_string())?;
    fs::write(dir.join(KEY_FILE), serde_json::to_string_pretty(&art.key.to_json(seed, FILE)).unwrap_or_default() + "\n").map_err(|e| e.to_string())?;
    let prompt_toml = if inline {
        let p = inline_prompt(&art.source);
        if p.contains("'''") {
            return Err("the artifact contains three single quotes and cannot be written as a literal TOML string".into());
        }
        format!("'''\n{}'''", p)
    } else {
        format!("'''\n{}\n'''", prompt())
    };
    let toml = format!(
        "id = \"{id}\"\nsuite = \"review\"\nkind = \"task\"\nrole = \"red-team\"\nsplit = \"{split}\"\ntags = [\"seeded-defects\"]\nmax_cost_usd = 1.0\ntimeout_secs = 900\nprompt = {prompt}\n\n# Seed {seed}: {k} planted defects, {decoys} decoys (`delphi train defects gen` made this case). The thresholds are this case's pass bar;\n# the recall and false-positive figures are diagnostics in the report, never a seat score (specs/078).\n[[check]]\nkind = \"review_findings\"\npath = \"{key}\"\nmin_recall = {min_recall}\nmax_false_positives = {max_fp}\n",
        id = id, split = split, prompt = prompt_toml, seed = seed, k = k, decoys = decoys, key = KEY_FILE, min_recall = min_recall, max_fp = max_fp
    );
    fs::write(dir.join("case.toml"), toml).map_err(|e| e.to_string())
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

const USAGE: &str = "delphi train defects run [--seed <n>] [--defects <1-8>] [--decoys <0-8>] [--json]\n  delphi train defects gen [--seed <n>] [--defects <1-8>] [--decoys <0-8>] [--split dev|holdout] [--min-recall <0-1>] [--max-false-positives <n>] [--inline] --out <new case dir>\n  run scores scripted reviewers (no model, no cost). gen writes a case for `delphi train run` (suite `review`); --inline puts the numbered module in the prompt, for a seat with no tools. The figures are diagnostics, not a seat score.";

/// `delphi train defects run|gen ...` (`args[0]` is `defects`).
pub fn cli(args: &[String], suites: Option<&Path>) -> i32 {
    let sub = args.get(1).map(String::as_str);
    let num = |name: &str, default: u64, lo: u64, hi: u64| -> Result<u64, String> {
        let n = flag(args, name).map_or(Ok(default), |v| v.parse::<u64>().map_err(|_| format!("{} must be a whole number", name)))?;
        if (lo..=hi).contains(&n) { Ok(n) } else { Err(format!("{} must be between {} and {}", name, lo, hi)) }
    };
    let (seed, k, decoys) = match (num("--seed", 1, 0, u64::MAX), num("--defects", 5, 1, 8), num("--decoys", 4, 0, 8)) {
        (Ok(s), Ok(k), Ok(d)) => (s, k as usize, d as usize),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
            eprintln!("delphi train defects: {}", e);
            return 2;
        }
    };
    match sub {
        Some("run") => {
            let art = generate(seed, k, decoys);
            let rows: Vec<(Reviewer, Score)> = Reviewer::ALL.iter().map(|r| (*r, score(&art.key, &r.answer(&art.key)))).collect();
            if args.iter().any(|a| a == "--json") {
                let j = json!({"seed": seed, "defects": k, "decoys": decoys, "note": DISCLAIMER,
                               "reviewers": rows.iter().map(|(r, s)| json!({"reviewer": r.name(), "recall": s.recall(), "caught": s.caught.len(), "false_positives": s.false_positives, "unmatched": s.unmatched})).collect::<Vec<_>>()});
                println!("{}", serde_json::to_string_pretty(&j).unwrap_or_default());
            } else {
                print!("{}", table(seed, &art.key, &rows));
            }
            0
        }
        Some("gen") => {
            let split = flag(args, "--split").unwrap_or_else(|| "dev".into());
            if !["dev", "holdout"].contains(&split.as_str()) {
                eprintln!("delphi train defects: --split must be dev or holdout");
                return 2;
            }
            let min_recall = match flag(args, "--min-recall").map_or(Ok(0.5), |v| v.parse::<f64>()) {
                Ok(m) if m > 0.0 && m <= 1.0 => m,
                _ => {
                    eprintln!("delphi train defects: --min-recall must be above 0 and at most 1");
                    return 2;
                }
            };
            let max_fp = match num("--max-false-positives", 2, 0, 64) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("delphi train defects: {}", e);
                    return 2;
                }
            };
            let Some(out) = flag(args, "--out").map(PathBuf::from) else {
                eprintln!("delphi train defects gen needs --out <new case dir>, such as {}/review/cases/rev-{}-{}\n  {}", suites.map_or("dev/train/suites".into(), |s| s.display().to_string()), seed, k, USAGE);
                return 2;
            };
            if out.exists() {
                eprintln!("delphi train defects: {} already exists; give a new directory", out.display());
                return 2;
            }
            match write_case(&out, &Params { seed, k, decoys, split: &split, inline: args.iter().any(|a| a == "--inline"), min_recall, max_fp }) {
                Ok(()) => {
                    println!("wrote {}: {} planted defects, {} decoys (seed {}); `delphi train validate` checks it, `delphi train run` runs it on an arm", out.display(), k, decoys, seed);
                    0
                }
                Err(e) => {
                    eprintln!("delphi train defects: {}", e);
                    1
                }
            }
        }
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u64) -> (Artifact, Key) {
        let a = generate(seed, 5, 4);
        let k = a.key.clone();
        (a, k)
    }

    #[test]
    fn generation_is_deterministic_per_seed_and_the_key_points_at_the_planted_lines() {
        let (a, b, c) = (generate(7, 5, 4), generate(7, 5, 4), generate(8, 5, 4));
        assert_eq!((&a.source, &a.key), (&b.source, &b.key));
        assert_ne!(a.source, c.source, "a new seed is a new case");
        assert_eq!((a.key.defects.len(), a.key.decoys.len()), (5, 4));
        let lines: Vec<&str> = a.source.lines().collect();
        // Each recorded line holds the construct the key says it does.
        let marks = [("off-by-one", "range(1, len(rows))"), ("inverted-condition", "balance >= amount"), ("missing-error-check", "return row[0]"), ("wrong-operator", "value < low and value > high"), ("resource-leak", "open(path)"), ("swapped-arguments", "copyfile(dst, src)"), ("stale-constant", "start = page * "), ("injection", "+ name +")];
        for d in &a.key.defects {
            let mark = marks.iter().find(|m| m.0 == d.kind).unwrap().1;
            assert!(lines[d.line - 1].contains(mark), "{} at {}: {}", d.kind, d.line, lines[d.line - 1]);
        }
        for d in &a.key.decoys {
            assert!(!lines[d.line - 1].trim().is_empty(), "decoy {} at {}", d.imitates, d.line);
        }
        let distinct: BTreeSet<&str> = a.key.defects.iter().map(|d| d.kind.as_str()).collect();
        assert_eq!(distinct.len(), 5, "kinds are distinct");
        // Function names are unique, so no two constructs collide.
        let defs: Vec<&str> = lines.iter().filter(|l| l.starts_with("def ")).copied().collect();
        assert_eq!(defs.iter().collect::<BTreeSet<_>>().len(), defs.len());
        // Targets are far enough apart that a +-1 window never covers two of them.
        let mut at: Vec<usize> = a.key.defects.iter().map(|d| d.line).chain(a.key.decoys.iter().map(|d| d.line)).collect();
        at.sort();
        assert!(at.windows(2).all(|w| w[1] - w[0] > 2), "{:?}", at);
    }

    #[test]
    fn every_defect_and_decoy_template_is_valid_python_shaped_and_hits_its_own_line() {
        // The recorded line of each template is the line holding its target text.
        for t in DEFECTS.iter().chain(DECOYS.iter()) {
            assert!(t.target < t.lines.len() && !t.lines[t.target].is_empty(), "{}", t.kind);
        }
        for s in 0..40u64 {
            let a = generate(s, 8, 8);
            let n = a.source.lines().count();
            assert!(a.key.defects.iter().map(|d| d.line).chain(a.key.decoys.iter().map(|d| d.line)).all(|l| l >= 1 && l <= n));
            assert!(!a.source.contains("{f}") && !a.source.contains("{C}") && !a.source.contains("{n}") && !a.source.contains("{m}"));
            assert!(!a.source.contains("'''"), "fits a TOML literal string");
        }
    }

    #[test]
    fn a_perfect_answer_catches_everything_with_no_false_positives() {
        let (_, k) = key(3);
        let s = score(&k, &Reviewer::Thorough.answer(&k));
        assert_eq!((s.caught.len(), s.defects, s.false_positives, s.unmatched, s.recall()), (5, 5, 0, 0, 1.0));
        assert!(verdict(&s, 1.0, 0).is_ok());
    }

    #[test]
    fn a_partial_answer_scores_its_recall_and_a_line_one_off_still_counts_but_two_off_does_not() {
        let (_, k) = key(3);
        let (d0, d1, d2) = (&k.defects[0], &k.defects[1], &k.defects[2]);
        let ans = format!("FINDING {} {}: x\n- FINDING {} {}: y\n  FINDING {} {}: z\n", d0.line + 1, d0.kind, d1.line - 1, d1.kind, d2.line + 2, d2.kind);
        let s = score(&k, &ans);
        assert_eq!((s.caught.len(), s.false_positives), (2, 0));
        assert_eq!(s.unmatched, 1, "two lines off holds nothing");
        assert!((s.recall() - 0.4).abs() < 1e-9);
        assert!(verdict(&s, 0.5, 2).unwrap_err().contains("caught 2 of 5 planted defects (40%), wanted 50%"));
        // A neighbouring kind the key accepts counts; a kind it does not accept is unmatched, not a catch.
        let off = k.defects.iter().find(|d| d.kind == "off-by-one").map(|d| d.line);
        let swapped = k.defects.iter().find(|d| d.kind == "swapped-arguments").map(|d| d.line);
        if let Some(l) = off {
            assert_eq!(score(&k, &format!("FINDING {} wrong-operator: x", l)).caught.len(), 1);
        }
        if let Some(l) = swapped {
            let s = score(&k, &format!("FINDING {} resource-leak: x", l));
            assert_eq!((s.caught.len(), s.unmatched), (0, 1));
        }
        // A second finding for a defect already caught is ignored, not counted against the seat.
        let twice = format!("FINDING {} {}: a\nFINDING {} {}: b\n", d0.line, d0.kind, d0.line, d0.kind);
        let s = score(&k, &twice);
        assert_eq!((s.caught.len(), s.false_positives, s.unmatched, s.findings), (1, 0, 0, 2));
    }

    #[test]
    fn flagging_decoys_is_a_false_positive_and_does_not_help_recall() {
        let (_, k) = key(5);
        let s = score(&k, &Reviewer::Plausible.answer(&k));
        assert_eq!((s.caught.len(), s.false_positives), (0, 4));
        assert!(verdict(&s, 0.5, 4).unwrap_err().contains("caught 0 of 5"));
        // Flagging everything catches everything and pays for it in false positives.
        let all = score(&k, &Reviewer::Suspicious.answer(&k));
        assert_eq!((all.caught.len(), all.false_positives), (5, 4));
        assert!(verdict(&all, 1.0, 2).unwrap_err().contains("4 decoy(s) flagged, at most 2 allowed"));
        assert!(verdict(&all, 1.0, 4).is_ok());
    }

    #[test]
    fn garbage_is_not_a_finding() {
        let (_, k) = key(5);
        for bad in ["", "The code looks fine to me.", "FINDING at line 12: off by one", "finding: 12 off-by-one", "Line 12 off-by-one: x", "FINDING 12: no kind"] {
            let s = score(&k, bad);
            assert_eq!((s.findings, s.caught.len(), s.false_positives, s.unmatched), (0, 0, 0, 0), "{:?}", bad);
            assert!(verdict(&s, 0.1, 9).unwrap_err().contains("no `FINDING"));
        }
        // Case and list markers are tolerated; prose around the lines is ignored.
        let l = k.defects[0].line;
        let s = score(&k, &format!("Here is my review.\n* finding {} {}: ok\nDone.", l, k.defects[0].kind.to_uppercase()));
        assert_eq!(s.caught.len(), 1);
    }

    #[test]
    fn the_key_is_not_in_the_prompt_and_the_inline_prompt_holds_the_whole_module() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("suites/review/cases/rev-3-5i");
        write_case(&dir, &Params { seed: 3, k: 5, decoys: 4, split: "dev", inline: true, min_recall: 0.5, max_fp: 2 }).unwrap();
        let case = crate::train::load_case(&dir, "review").unwrap();
        let prompt = case.prompt.clone().unwrap();
        let art = generate(3, 5, 4);
        // Every line of the module is in the prompt, behind its number; nothing the key adds is.
        for (i, l) in art.source.lines().enumerate() {
            assert!(prompt.contains(&format!("{:>3} | {}\n", i + 1, l)), "line {}", i + 1);
        }
        assert!(prompt.contains("FINDING <line number> <kind>") && prompt.contains("injection") && !prompt.contains("@project/"));
        for d in &art.key.decoys {
            assert!(!d.why.is_empty() && !prompt.contains(&d.why));
        }
        assert!(!prompt.contains("planted") && !prompt.contains("decoy") && !prompt.contains(KEY_FILE));
        assert!(fs::read_to_string(dir.join(KEY_FILE)).unwrap().contains("\"decoys\""));
        for f in fs::read_dir(dir.join("fixture")).unwrap().flatten() {
            assert!(f.file_name() == FILE, "only the module is in the fixture");
        }
        assert_eq!(fs::read_to_string(dir.join("fixture").join(FILE)).unwrap(), art.source);
        // The ordinary case names the file and carries no code.
        let plain = t.path().join("suites/review/cases/rev-3-5");
        write_case(&plain, &Params { seed: 3, k: 5, decoys: 4, split: "dev", inline: false, min_recall: 0.5, max_fp: 2 }).unwrap();
        let p = crate::train::load_case(&plain, "review").unwrap().prompt.unwrap();
        assert!(p.contains("@project/repo/review.py") && !p.contains("def "), "{}", p);
    }

    #[test]
    fn the_check_parses_from_case_toml_and_refuses_bad_thresholds() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("suites/review/cases/rev-2-4");
        write_case(&dir, &Params { seed: 2, k: 4, decoys: 3, split: "holdout", inline: false, min_recall: 0.75, max_fp: 1 }).unwrap();
        let case = crate::train::load_case(&dir, "review").unwrap();
        assert!(matches!(&case.checks[0], crate::train::Check::ReviewFindings { file, min_recall, max_false_positives } if file == KEY_FILE && *min_recall == 0.75 && *max_false_positives == 1), "{:?}", case.checks);
        let good = fs::read_to_string(dir.join("case.toml")).unwrap();
        for (bad, want) in [
            (good.replace("path = \"review-key.json\"\n", ""), "needs `path`"),
            (good.replace("path = \"review-key.json\"", "path = \"../k.json\""), "must be relative"),
            (good.replace("min_recall = 0.75", "min_recall = 0.0"), "needs `min_recall` above 0"),
            (good.replace("min_recall = 0.75", "min_recall = 1.5"), "needs `min_recall` above 0"),
            (good.replace("max_false_positives = 1\n", ""), "needs `max_false_positives`"),
            (good.replace("max_false_positives = 1", "max_false_positives = -1"), "needs `max_false_positives`"),
        ] {
            fs::write(dir.join("case.toml"), &bad).unwrap();
            let e = crate::train::load_case(&dir, "review").unwrap_err().join("; ");
            assert!(e.contains(want), "{} in {}", want, e);
        }
    }

    #[test]
    fn the_report_section_prints_recall_false_positives_and_unique_catches() {
        let s = |caught: &[&str], fp: usize, un: usize| Score { findings: caught.len() + fp + un, defects: 4, caught: caught.iter().map(|c| c.to_string()).collect(), false_positives: fp, unmatched: un };
        let (a1, a2, b1, b2) = (s(&["10:x", "20:y", "30:z"], 0, 1), s(&["10:x", "40:w", "50:q"], 1, 0), s(&["10:x", "20:y"], 3, 2), s(&["20:y", "10:x"], 2, 0));
        let runs = [
            RunReview { case: "review/c1", arm: "tool", score: &a1 },
            RunReview { case: "review/c2", arm: "tool", score: &a2 },
            RunReview { case: "review/c1", arm: "toolless", score: &b1 },
            RunReview { case: "review/c2", arm: "toolless", score: &b2 },
        ];
        let out = report_section(&runs);
        let cells = |prefix: &str| -> Vec<String> { out.lines().find(|l| l.starts_with(prefix)).unwrap().split_whitespace().map(String::from).collect() };
        assert_eq!(cells("arm"), ["arm", "runs", "recall", "caught", "false", "pos.", "unmatched", "unique"]);
        // c1: 30:z only the tool arm. c2: 40:w and 50:q only the tool arm, 20:y only the toolless arm.
        assert_eq!(cells("tool "), ["tool", "2", "75%", "6/8", "1", "1", "3"], "{}", out);
        assert_eq!(cells("toolless"), ["toolless", "2", "50%", "4/8", "5", "2", "1"], "{}", out);
        assert!(out.starts_with("\nSEEDED-DEFECT REVIEW (planted defects found, by arm; diagnostics only)\n"));
        assert!(out.contains("not a seat score") && out.contains("Council decision") && out.contains("specs/078"), "{}", out);
        assert!(report_section(&[]).is_empty());
        // One arm alone has no unique catches: there is nobody to be unique against.
        let solo = report_section(&runs[..2]);
        assert!(solo.lines().any(|l| l.starts_with("tool ") && l.split_whitespace().last() == Some("0")), "{}", solo);
    }

    #[test]
    fn the_scripted_reviewers_tell_the_cases_apart_and_the_command_checks_its_arguments() {
        let art = generate(1, 5, 4);
        let by: BTreeMap<&str, Score> = Reviewer::ALL.iter().map(|r| (r.name(), score(&art.key, &r.answer(&art.key)))).collect();
        assert_eq!(by["thorough"].recall(), 1.0);
        assert_eq!((by["suspicious"].recall(), by["suspicious"].false_positives), (1.0, 4));
        assert_eq!((by["plausible"].caught.len(), by["plausible"].false_positives), (0, 4));
        assert_eq!(by["mislabelling"].caught.len(), 0);
        assert_eq!(by["silent"].findings, 0);
        let rows: Vec<(Reviewer, Score)> = Reviewer::ALL.iter().map(|r| (*r, by[r.name()].clone())).collect();
        let t = table(1, &art.key, &rows);
        for want in ["SEEDED-DEFECT REVIEW (seed 1, 5 planted defects, 4 decoys)", "thorough", "5/5", "suspicious", "silent", "not a seat score"] {
            assert!(t.contains(want), "{} in\n{}", want, t);
        }
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("new-case");
        assert_eq!(cli(&a(&["defects", "gen", "--seed", "3", "--inline", "--out", out.to_str().unwrap()]), None), 0);
        assert!(fs::read_to_string(out.join("case.toml")).unwrap().contains("Below is a small Python module"));
        assert_eq!(cli(&a(&["defects", "gen", "--out", out.to_str().unwrap()]), None), 2, "an existing directory is refused");
        assert_eq!(cli(&a(&["defects", "gen"]), None), 2);
        assert_eq!(cli(&a(&["defects", "run", "--defects", "0"]), None), 2);
        assert_eq!(cli(&a(&["defects", "run", "--decoys", "9"]), None), 2);
        assert_eq!(cli(&a(&["defects", "run", "--seed", "x"]), None), 2);
        assert_eq!(cli(&a(&["defects", "gen", "--min-recall", "0", "--out", tmp.path().join("z").to_str().unwrap()]), None), 2);
        assert_eq!(cli(&a(&["defects", "run"]), None), 0);
        assert_eq!(cli(&a(&["defects", "run", "--json"]), None), 0);
        assert_eq!(cli(&a(&["defects"]), None), 2);
    }
}
