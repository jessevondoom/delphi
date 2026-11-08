//! Looking up a compiled spec (spec 088 section 10.3): the one implementation behind `delphi specs show`, `list` and `report`
//! (and, later, the MCP tools), so every surface behaves the same.
//!
//! A compiled file is a derived view and never the authority. Two rules make that real here. First, `show` **refuses** a compiled
//! file whose recorded source hash differs from the spec as it is now: a stale view is never served, it is named and the reader is
//! told to recompile or read the source. Second, `section` reads the **source** spec itself, so "open the cited source section"
//! is one command and the reader sees the spec's own words with the hash they came from.
//!
//! The read counter (spec 088 OD-5) is one JSON line per read, appended under the Delphi home's `logs/` directory **only if the
//! home already exists**: a lookup never creates `~/.delphi`. A line holds the time, the surface, the operation, the spec's stem,
//! the section if there was one, and an optional label the caller chose (`--for`); never any text of a spec. It counts reads made
//! through Delphi's own surfaces; a host that opens the files directly is invisible to it, and the report says so.

use delphi_core::specdoc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;

/// Where the compiled files are, relative to the repository root.
pub const DATA_DIR: &str = "compiled/specs/data";
/// The read counter, relative to the Delphi home.
pub const COUNTER_FILE: &str = "logs/spec-reads.jsonl";
/// What the counter can and cannot see; printed with every report.
pub const COVERAGE: &str = "Counts reads made through `delphi specs show` and the Delphi tools. A host or agent that opens the compiled files or the specs directly is not counted, so a low count can mean that agents read around Delphi, not that they did not read.";

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|x| format!("{:02x}", x)).collect()
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(12)]
}

/// The stems of the specs directly under `specs/`, sorted.
pub fn spec_stems(repo: &Path) -> Result<Vec<String>, String> {
    let dir = repo.join("specs");
    let rd = fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {}", dir.display(), e))?;
    let mut stems: Vec<String> = rd.flatten().filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".md").map(str::to_string)).collect();
    stems.sort();
    Ok(stems)
}

/// Which spec a query names: a full stem, a spec number, or a prefix of one stem. A number or prefix that fits more than one spec is
/// refused with the full stems listed, because a bare number has been ambiguous in this repository before and a guess is how a
/// builder opens the wrong spec.
pub fn resolve(stems: &[String], query: &str) -> Result<String, String> {
    let q = query.trim().trim_end_matches(".md");
    let q = q.strip_prefix("specs/").unwrap_or(q);
    if q.is_empty() {
        return Err("name a spec: its file name without `.md`, or its number".to_string());
    }
    if stems.iter().any(|s| s == q) {
        return Ok(q.to_string());
    }
    let list = |v: &[&String]| v.iter().take(8).map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
    let numbered: Vec<&String> = stems.iter().filter(|s| specdoc::spec_number(&format!("{}.md", s)).as_deref() == Some(q)).collect();
    match numbered.len() {
        1 => return Ok(numbered[0].clone()),
        n if n > 1 => return Err(format!("`{}` names {} specs ({}); name one by its full file name", q, n, list(&numbered))),
        _ => {}
    }
    let prefixed: Vec<&String> = stems.iter().filter(|s| s.starts_with(q)).collect();
    match prefixed.len() {
        1 => Ok(prefixed[0].clone()),
        0 => Err(format!("no spec matches `{}`; `delphi specs list` shows them", q)),
        n => Err(format!("`{}` fits {} specs ({}); name one by its full file name", q, n, list(&prefixed))),
    }
}

/// A compiled file read from disk, with whether its source has changed since.
pub struct Loaded {
    pub json: Value,
    /// `Some((recorded, current))` when the file is stale; `current` is `None` when the source spec is gone.
    pub stale: Option<(String, Option<String>)>,
}

/// Reads `compiled/specs/data/<stem>.json` and compares its recorded source hash with the spec now. The spec read is `specs/<stem>.md`
/// built from the stem, never a path taken from the compiled file.
pub fn load(repo: &Path, stem: &str) -> Result<Loaded, String> {
    let text = fs::read_to_string(repo.join(DATA_DIR).join(format!("{}.json", stem))).map_err(|_| format!("no compiled file for `{}`; run `delphi specs compile`", stem))?;
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("the compiled file for `{}` is not JSON: {}", stem, e))?;
    let recorded = json["source"]["sha256"].as_str().unwrap_or("").to_string();
    let current = fs::read(repo.join("specs").join(format!("{}.md", stem))).ok().map(|b| sha256_hex(&b));
    let stale = if current.as_deref() == Some(recorded.as_str()) { None } else { Some((recorded, current)) };
    Ok(Loaded { json, stale })
}

fn stale_message(stem: &str, recorded: &str, current: &Option<String>) -> String {
    match current {
        Some(c) => format!("the compiled file for `{}` is stale: it records source hash {}, the spec is now {}. Run `delphi specs compile`, or read the source spec itself (`delphi specs show {} --section <n>`).", stem, short(recorded), short(c), stem),
        None => format!("the compiled file for `{}` has no source spec any more. Run `delphi specs compile`.", stem),
    }
}

/// The compiled file for a spec, refusing a stale one. Returns the file's JSON unchanged.
pub fn show(repo: &Path, query: &str) -> Result<Value, String> {
    let stem = resolve(&spec_stems(repo)?, query)?;
    let l = load(repo, &stem)?;
    match l.stale {
        Some((rec, cur)) => Err(stale_message(&stem, &rec, &cur)),
        None => Ok(l.json),
    }
}

/// A quotation as a labelled fenced block: the label first, then the text exactly, in a fence one backtick longer than any run of
/// backticks inside it, so nothing in the text is active or can end the block.
pub fn fenced_excerpt(src: &str, from: u64, to: u64, body: &str) -> String {
    let lines = if from == to { format!("line {}", from) } else { format!("lines {}-{}", from, to) };
    let body = body.replace('\r', "");
    let (mut longest, mut run) = (2, 0);
    for c in body.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    format!("**source excerpt, untrusted** · `{}` {}\n\n{}text\n{}\n{}\n", src, lines, fence, body, fence)
}

/// A compiled file as text for a person or an agent: identity, status as a label, and the summary and outline.
pub fn describe(v: &Value) -> String {
    let s = |p: &[&str]| p.iter().fold(v, |c, k| &c[*k]).as_str().unwrap_or("").to_string();
    let hash = s(&["source", "sha256"]);
    let mut out = format!("{}\n", s(&["spec", "stem"]));
    out.push_str(&format!("title: {}\ngroup: {}\n", s(&["spec", "title"]), s(&["spec", "group"])));
    out.push_str(&format!("status: {} (per source at `{}`; informational, the source spec decides)\n", s(&["source_status_at_hash", "word"]), short(&hash)));
    out.push_str(&format!("source: {} (sha256 {}, matches the spec now)\n", s(&["source", "path"]), short(&hash)));
    for (label, key) in [("supersedes", "supersedes"), ("superseded by", "superseded_by")] {
        if !v["spec"][key].is_null() {
            out.push_str(&format!("{}: {}\n", label, v["spec"][key]));
        }
    }
    out.push('\n');
    if v["summary"].is_null() {
        out.push_str("No standard scope section: open the spec.\n");
    } else {
        let q = &v["summary"];
        out.push_str(&fenced_excerpt(&s(&["source", "path"]), q["span"]["line"].as_u64().unwrap_or(0), q["span"]["end_line"].as_u64().unwrap_or(0), q["text"].as_str().unwrap_or("")));
    }
    out.push_str("\noutline:\n");
    for e in v["outline"].as_array().into_iter().flatten() {
        let number = e["number"].as_str().map(|n| format!("{} ", n)).unwrap_or_default();
        out.push_str(&format!("{}{}{} (line {})\n", "  ".repeat((e["level"].as_u64().unwrap_or(1) as usize).saturating_sub(1)), number, e["title"].as_str().unwrap_or(""), e["line"]));
    }
    out
}

/// One row of the listing.
pub struct Row {
    pub stem: String,
    pub group: String,
    pub status: String,
    pub tier: u64,
    pub has_summary: bool,
    /// `compiled`, `stale` (the source changed since) or `not compiled`.
    pub state: &'static str,
}

/// Every spec with its compiled state. Reading the list is not a read of a spec and is not counted.
pub fn list(repo: &Path) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for stem in spec_stems(repo)? {
        match load(repo, &stem) {
            Ok(l) => rows.push(Row {
                group: l.json["spec"]["group"].as_str().unwrap_or("").to_string(),
                status: l.json["source_status_at_hash"]["word"].as_str().unwrap_or("").to_string(),
                tier: l.json["spec"]["tier"].as_u64().unwrap_or(0),
                has_summary: !l.json["summary"].is_null(),
                state: if l.stale.is_some() { "stale" } else { "compiled" },
                stem,
            }),
            Err(_) => rows.push(Row { stem, group: String::new(), status: String::new(), tier: 0, has_summary: false, state: "not compiled" }),
        }
    }
    Ok(rows)
}

/// A section of a source spec, read from the spec itself.
#[derive(Debug)]
pub struct Section {
    pub stem: String,
    pub path: String,
    pub sha256: String,
    pub heading: String,
    pub line: usize,
    pub end_line: usize,
    pub text: String,
}

/// The text of section `number` of a spec, from the spec file: from its heading to just before the next heading of the same or a
/// higher level. `3.3.2` names an item in the list under heading `3.3`, so it resolves to that heading, as the reference gate does.
pub fn section(repo: &Path, query: &str, number: &str) -> Result<Section, String> {
    let stem = resolve(&spec_stems(repo)?, query)?;
    let path = repo.join("specs").join(format!("{}.md", stem));
    let bytes = fs::read(&path).map_err(|e| format!("cannot read specs/{}.md: {}", stem, e))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| format!("specs/{}.md is not valid UTF-8", stem))?;
    let outline = specdoc::outline(text);
    let want = number.trim().trim_start_matches('§');
    let at = outline.iter().position(|e| e.number.as_deref() == Some(want)).or_else(|| outline.iter().position(|e| e.number.as_deref() == Some(specdoc::heading_of(want))));
    let Some(i) = at else {
        let have: Vec<&str> = outline.iter().filter(|e| e.level == 2).filter_map(|e| e.number.as_deref()).take(24).collect();
        return Err(format!("specs/{}.md has no section {}; its numbered sections include: {}", stem, want, have.join(", ")));
    };
    let lines: Vec<&str> = text.split('\n').collect();
    let start = outline[i].line;
    let mut end = outline[i + 1..].iter().find(|e| e.level <= outline[i].level).map(|e| e.line - 1).unwrap_or(lines.len());
    while end > start && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    Ok(Section {
        heading: outline[i].title.clone(),
        line: start,
        end_line: end,
        text: lines[start - 1..end].join("\n"),
        path: format!("specs/{}.md", stem),
        sha256: sha256_hex(&bytes),
        stem,
    })
}

/// One read, as the counter records it.
pub struct Read<'a> {
    pub surface: &'a str,
    pub op: &'a str,
    pub stem: &'a str,
    pub section: Option<&'a str>,
    pub label: Option<&'a str>,
}

/// A label the caller chose to group reads (a slice or a branch): at most 64 characters of letters, digits and `._/#:- `.
pub fn clean_label(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > 64 || !t.chars().all(|c| c.is_ascii_alphanumeric() || "._/#:- ".contains(c)) {
        return None;
    }
    Some(t.to_string())
}

/// Appends one line to the counter. Only if the Delphi home exists (a lookup never creates it), and never fails the read it counts.
pub fn record(home: &Path, r: &Read<'_>) -> bool {
    if !home.is_dir() {
        return false;
    }
    let line = json!({"ts": delphi_core::time::now_rfc3339(), "surface": r.surface, "op": r.op, "stem": r.stem, "section": r.section, "label": r.label});
    let dir = home.join("logs");
    if fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(home.join(COUNTER_FILE)) else { return false };
    f.write_all(format!("{}\n", line).as_bytes()).is_ok()
}

/// What the counter holds, as the figures the measurement of spec 088 OD-5 needs. Lines that do not parse are skipped.
pub fn report(home: &Path) -> Result<Value, String> {
    let text = match fs::read_to_string(home.join(COUNTER_FILE)) {
        Ok(t) => t,
        Err(_) => return Ok(json!({"records": 0, "coverage": COVERAGE, "note": "No reads have been recorded yet. The counter writes only when the Delphi home exists, and starts with the first `delphi specs show`."})),
    };
    let recs: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect();
    let (mut by_surface, mut by_op): (BTreeMap<String, u64>, BTreeMap<String, u64>) = (BTreeMap::new(), BTreeMap::new());
    let mut stems = BTreeSet::new();
    // label -> (stems shown, stems whose source section was read, show count, section count)
    let mut labels: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>, u64, u64)> = BTreeMap::new();
    for r in &recs {
        let (surface, op, stem) = (r["surface"].as_str().unwrap_or("?"), r["op"].as_str().unwrap_or("?"), r["stem"].as_str().unwrap_or("?"));
        *by_surface.entry(surface.to_string()).or_default() += 1;
        *by_op.entry(op.to_string()).or_default() += 1;
        stems.insert(stem.to_string());
        let e = labels.entry(r["label"].as_str().unwrap_or("(unlabelled)").to_string()).or_default();
        match op {
            "show" => {
                e.0.insert(stem.to_string());
                e.2 += 1;
            }
            "section" => {
                e.1.insert(stem.to_string());
                e.3 += 1;
            }
            _ => {}
        }
    }
    let label_rows: Vec<Value> = labels
        .iter()
        .map(|(l, (shown, sectioned, nshow, nsec))| json!({"label": l, "show": nshow, "section": nsec, "shown_without_a_section_read": shown.difference(sectioned).collect::<Vec<_>>()}))
        .collect();
    Ok(json!({
        "records": recs.len(),
        "first": recs.first().and_then(|r| r["ts"].as_str()),
        "last": recs.last().and_then(|r| r["ts"].as_str()),
        "by_surface": by_surface,
        "by_op": by_op,
        "distinct_specs": stems.len(),
        "labels": label_rows,
        "coverage": COVERAGE,
    }))
}

/// The report as text.
pub fn report_text(v: &Value) -> String {
    let mut out = String::new();
    if v["records"].as_u64() == Some(0) {
        out.push_str(v["note"].as_str().unwrap_or("No reads recorded.\n"));
        out.push_str("\n\n");
    } else {
        out.push_str(&format!("{} read(s) of {} spec(s), {} to {}\n", v["records"], v["distinct_specs"], v["first"].as_str().unwrap_or("?"), v["last"].as_str().unwrap_or("?")));
        out.push_str(&format!("by surface: {}; by operation: {}\n\n", v["by_surface"], v["by_op"]));
        out.push_str("by label (a label is what the reader chose with --for, usually a slice or a branch):\n");
        for l in v["labels"].as_array().into_iter().flatten() {
            let missing = l["shown_without_a_section_read"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            out.push_str(&format!("  {}: {} show, {} section{}\n", l["label"].as_str().unwrap_or(""), l["show"], l["section"], if missing.is_empty() { String::new() } else { format!("; compiled file read but no source section read for: {}", missing) }));
        }
        out.push('\n');
    }
    out.push_str(&format!("Coverage: {}\n", v["coverage"].as_str().unwrap_or(COVERAGE)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_A: &str = "---\nid: spec-001-a\ntitle: A\nstatus: approved\n---\n\n# Spec A\n\n## 1. Scope\n\nSummary.\n\n### 1.1 Detail\n\ndetail text\n\n## 2. Next\n\nnext text\n\n### 2.1 Sub\n\nsub text\n\n## 3. Last\n\nlast\n";

    fn repo() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("specs")).unwrap();
        fs::create_dir_all(t.path().join(DATA_DIR)).unwrap();
        t
    }

    fn put_spec(t: &tempfile::TempDir, stem: &str, text: &str) {
        fs::write(t.path().join("specs").join(format!("{}.md", stem)), text).unwrap();
    }

    /// A compiled file for a spec, with the hash of the text it was made from (no compiler in this crate, so the fields the lookup reads).
    fn put_compiled(t: &tempfile::TempDir, stem: &str, text: &str, group: &str) {
        let hash = sha256_hex(text.as_bytes());
        let v = json!({
            "source": {"path": format!("specs/{}.md", stem), "sha256": hash},
            "spec": {"stem": stem, "number": null, "title": "A", "group": group, "supersedes": null, "superseded_by": null, "tier": 0},
            "source_status_at_hash": {"word": "approved", "sha256": hash, "line": {"kind": "quote", "text": "status: approved", "span": {"start": 0, "end": 1, "line": 4, "end_line": 4}}},
            "summary": {"kind": "quote", "text": "Summary.", "span": {"start": 0, "end": 1, "line": 11, "end_line": 11}},
            "outline": [{"level": 2, "number": "1", "title": "Scope", "line": 9}],
        });
        fs::write(t.path().join(DATA_DIR).join(format!("{}.json", stem)), v.to_string()).unwrap();
    }

    #[test]
    fn a_query_names_a_spec_by_stem_number_or_unique_prefix_and_an_ambiguous_one_is_refused() {
        let stems: Vec<String> = ["001-first", "002-second", "002a-sub", "014-relay", "095-docs"].iter().map(|s| s.to_string()).collect();
        assert_eq!(resolve(&stems, "014-relay").unwrap(), "014-relay");
        assert_eq!(resolve(&stems, "specs/014-relay.md").unwrap(), "014-relay");
        assert_eq!(resolve(&stems, "014").unwrap(), "014-relay");
        assert_eq!(resolve(&stems, "002a").unwrap(), "002a-sub");
        assert_eq!(resolve(&stems, "095-d").unwrap(), "095-docs");
        let e = resolve(&stems, "00").unwrap_err();
        assert!(e.contains("fits 3 specs") && e.contains("001-first") && e.contains("002-second"), "{}", e);
        assert!(resolve(&stems, "nope").unwrap_err().contains("no spec matches"));
        assert!(resolve(&stems, "  ").unwrap_err().contains("name a spec"));
        // A number two specs share is refused, not guessed.
        let dup: Vec<String> = ["014-a", "014-b"].iter().map(|s| s.to_string()).collect();
        let e = resolve(&dup, "014").unwrap_err();
        assert!(e.contains("names 2 specs") && e.contains("014-a, 014-b"), "{}", e);
    }

    #[test]
    fn show_serves_a_current_compiled_file_and_refuses_a_stale_one() {
        let t = repo();
        put_spec(&t, "001-a", SPEC_A);
        put_compiled(&t, "001-a", SPEC_A, "g");
        let v = show(t.path(), "001-a").unwrap();
        assert_eq!(v["spec"]["stem"], "001-a");
        // The spec changes after it was compiled: the compiled file is named stale and not served.
        put_spec(&t, "001-a", &SPEC_A.replace("Summary.", "Summary, changed."));
        let e = show(t.path(), "001-a").unwrap_err();
        assert!(e.contains("is stale") && e.contains("delphi specs compile") && e.contains("--section"), "{}", e);
        // The source is gone: also refused.
        fs::remove_file(t.path().join("specs/001-a.md")).unwrap();
        put_spec(&t, "002-b", SPEC_A);
        assert!(show(t.path(), "002-b").unwrap_err().contains("no compiled file"), "a spec that was never compiled");
    }

    #[test]
    fn a_path_in_a_tampered_compiled_file_is_never_followed() {
        let t = repo();
        put_spec(&t, "001-a", SPEC_A);
        put_compiled(&t, "001-a", SPEC_A, "g");
        fs::write(t.path().join("secret.md"), "do not read").unwrap();
        // The compiled file points its source at another file; the lookup reads specs/<stem>.md regardless.
        let p = t.path().join(DATA_DIR).join("001-a.json");
        let mut v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        v["source"]["path"] = json!("../secret.md");
        fs::write(&p, v.to_string()).unwrap();
        assert!(show(t.path(), "001-a").is_ok(), "the hash still matches the spec named by the stem");
    }

    #[test]
    fn a_section_is_read_from_the_source_from_its_heading_to_the_next_of_its_level() {
        let t = repo();
        put_spec(&t, "001-a", SPEC_A);
        let s = section(t.path(), "001-a", "2").unwrap();
        assert_eq!((s.line, s.end_line), (17, 23), "from `## 2. Next` to the last line of `### 2.1 Sub`'s text");
        assert!(s.text.starts_with("## 2. Next") && s.text.contains("### 2.1 Sub") && s.text.contains("sub text") && !s.text.contains("last"), "{}", s.text);
        assert_eq!(s.heading, "Next", "the title, without its number");
        assert_eq!(s.sha256, sha256_hex(SPEC_A.as_bytes()));
        // A sub-heading ends at the next heading of its level or higher.
        let s = section(t.path(), "001-a", "1.1").unwrap();
        assert_eq!(s.text, "### 1.1 Detail\n\ndetail text");
        // `2.1.3` names an item under heading 2.1, so it resolves to that heading, as the reference gate does.
        assert!(section(t.path(), "001-a", "2.1.3").unwrap().text.starts_with("### 2.1 Sub"));
        assert!(section(t.path(), "001-a", "§3").unwrap().text.ends_with("last"));
        let e = section(t.path(), "001-a", "9").unwrap_err();
        assert!(e.contains("has no section 9") && e.contains("1, 2, 3"), "{}", e);
    }

    #[test]
    fn a_section_read_works_when_the_compiled_file_is_stale_or_missing() {
        let t = repo();
        put_spec(&t, "001-a", SPEC_A);
        assert!(section(t.path(), "001-a", "1").is_ok(), "the source is read, not the compiled file");
    }

    #[test]
    fn list_names_every_spec_with_its_compiled_state() {
        let t = repo();
        put_spec(&t, "001-a", SPEC_A);
        put_compiled(&t, "001-a", SPEC_A, "g1");
        put_spec(&t, "002-b", SPEC_A);
        put_compiled(&t, "002-b", "older text", "g2");
        put_spec(&t, "003-c", SPEC_A);
        let rows = list(t.path()).unwrap();
        let got: Vec<(&str, &str, &str)> = rows.iter().map(|r| (r.stem.as_str(), r.group.as_str(), r.state)).collect();
        assert_eq!(got, vec![("001-a", "g1", "compiled"), ("002-b", "g2", "stale"), ("003-c", "", "not compiled")]);
        assert!(rows[0].has_summary && rows[0].status == "approved" && rows[0].tier == 0);
    }

    #[test]
    fn describe_shows_the_status_as_a_label_and_the_summary_as_a_labelled_fenced_block() {
        let t = repo();
        put_spec(&t, "001-a", SPEC_A);
        put_compiled(&t, "001-a", SPEC_A, "g");
        let d = describe(&show(t.path(), "001-a").unwrap());
        assert!(d.contains("status: approved (per source at `") && d.contains("the source spec decides"));
        assert!(d.contains("**source excerpt, untrusted** · `specs/001-a.md` line 11\n\n```text\nSummary.\n```\n"), "{}", d);
        assert!(d.contains("outline:\n  1 Scope (line 9)"));
    }

    #[test]
    fn a_fence_is_longer_than_any_run_of_backticks_in_the_text() {
        let e = fenced_excerpt("specs/x.md", 3, 5, "a\n```\nb\n````\nc");
        assert!(e.contains("\n`````text\n") && e.ends_with("\n`````\n"), "{}", e);
        assert!(fenced_excerpt("s", 1, 1, "plain").contains("```text\nplain\n```\n"));
    }

    #[test]
    fn labels_are_short_and_plain() {
        assert_eq!(clean_label("  S3b-1 / branch#2  ").as_deref(), Some("S3b-1 / branch#2"));
        assert_eq!(clean_label("build-088-s3b"), Some("build-088-s3b".to_string()));
        for bad in ["", "   ", "with `tick`", "semi;colon", "new\nline", &"x".repeat(65)] {
            assert_eq!(clean_label(bad), None, "{:?}", bad);
        }
    }

    #[test]
    fn the_counter_writes_only_when_the_home_exists_and_never_holds_spec_text() {
        let t = tempfile::tempdir().unwrap();
        let missing = t.path().join("no-such-home");
        assert!(!record(&missing, &Read { surface: "cli", op: "show", stem: "001-a", section: None, label: None }));
        assert!(!missing.exists(), "a lookup never creates the Delphi home");
        let home = t.path().join("home");
        fs::create_dir_all(&home).unwrap();
        assert!(record(&home, &Read { surface: "cli", op: "show", stem: "001-a", section: None, label: Some("S3b") }));
        assert!(record(&home, &Read { surface: "mcp", op: "section", stem: "001-a", section: Some("2"), label: Some("S3b") }));
        let text = fs::read_to_string(home.join(COUNTER_FILE)).unwrap();
        assert_eq!(text.lines().count(), 2);
        let first: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(first["stem"], "001-a");
        assert_eq!(first["op"], "show");
        assert!(first["ts"].as_str().unwrap().ends_with('Z'));
        assert_eq!(first.as_object().unwrap().len(), 6, "time, surface, op, stem, section and label only: {}", first);
    }

    #[test]
    fn the_report_counts_by_surface_operation_and_label_and_says_what_it_cannot_see() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        let r = |surface: &'static str, op: &'static str, stem: &'static str, label: Option<&'static str>| {
            assert!(record(home, &Read { surface, op, stem, section: None, label }));
        };
        let empty = report(home).unwrap();
        assert_eq!(empty["records"], 0);
        assert!(report_text(&empty).contains("No reads have been recorded") && report_text(&empty).contains("is not counted"));
        r("cli", "show", "001-a", Some("S3b"));
        r("cli", "section", "001-a", Some("S3b"));
        r("mcp", "show", "002-b", Some("S3b"));
        r("cli", "show", "003-c", None);
        fs::OpenOptions::new().append(true).open(home.join(COUNTER_FILE)).unwrap().write_all(b"not json\n").unwrap();
        let v = report(home).unwrap();
        assert_eq!(v["records"], 4, "a damaged line is skipped");
        assert_eq!(v["distinct_specs"], 3);
        assert_eq!(v["by_surface"]["cli"], 3);
        assert_eq!(v["by_op"]["show"], 3);
        let s3b = v["labels"].as_array().unwrap().iter().find(|l| l["label"] == "S3b").unwrap();
        assert_eq!((s3b["show"].as_u64(), s3b["section"].as_u64()), (Some(2), Some(1)));
        assert_eq!(s3b["shown_without_a_section_read"], json!(["002-b"]), "a compiled file read with no source section read behind it");
        assert!(v["labels"].as_array().unwrap().iter().any(|l| l["label"] == "(unlabelled)"));
        let text = report_text(&v);
        assert!(text.contains("4 read(s) of 3 spec(s)") && text.contains("no source section read for: 002-b") && text.contains("Coverage:"), "{}", text);
    }
}
