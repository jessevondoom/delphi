//! `delphi specs compile` (spec 088, stage 1, tier 0): one JSON file per spec under `compiled/specs/data/`, copied out of the
//! source specs byte for byte, for agents to read.
//!
//! The compile *extracts*; it never rewrites or summarizes, and no model runs in it. Every piece of a spec's text it writes is a
//! quotation (`kind: "quote"`) with the byte offsets and lines it was cut from, so a test can cut the same slice from the file and
//! compare. The status is written only as what the source said at a hash (`source_status_at_hash`), and no approval field is
//! copied. A compiled file is never the authority, and nothing in the program decides anything from one: the guard test at the
//! bottom fails when any Rust file other than this one names the output directory.
//!
//! The groups of the human views come from `compiled/groups.toml`, the only place a spec's group is declared. A spec is never
//! edited for the compile (a ratified spec's hash would change).
//!
//! A compile is all or nothing: every spec compiles in memory first, and only when none fails is the output directory replaced (a
//! temporary directory renamed over it), so a failure leaves the committed files as they were, and a deleted or renamed spec's
//! file disappears in the same run.

use crate::lint;
use delphi_core::specdoc::{self, Quote};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The compiler's format revision, written into every file. It is not the program's version, so a release does not change a
/// hundred committed files.
pub const COMPILER: &str = "delphi-spec-compile/1";
const SCHEMA_VERSION: u64 = 1;
/// Where the hand-written groups are, relative to the repository root.
pub const GROUPS_FILE: &str = "compiled/groups.toml";
/// Where the compiled files go, relative to the repository root.
pub const DATA_DIR: &str = "compiled/specs/data";

/// A group of the human views. `title` and `order` are read by the views (S3); the compile reads only which ids exist.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct Group {
    pub id: String,
    pub title: String,
    pub order: i64,
}

/// The closed list of groups, and which group each spec (by file stem) belongs to.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct Groups {
    pub groups: Vec<Group>,
    pub specs: BTreeMap<String, String>,
}

/// One source spec: its file stem (the identity, never a bare number), its repository path, and its bytes.
#[derive(Debug, Clone)]
pub struct Source {
    pub stem: String,
    pub path: String,
    pub bytes: Vec<u8>,
}

/// Reads `compiled/groups.toml`. A key other than `group` and `specs` is refused, so a typo cannot silently drop a table.
pub fn parse_groups(text: &str) -> Result<Groups, String> {
    let table: toml::Table = text.parse().map_err(|e| format!("{} is not valid TOML: {}", GROUPS_FILE, e))?;
    if let Some(k) = table.keys().find(|k| !matches!(k.as_str(), "group" | "specs")) {
        return Err(format!("{} has an unknown top-level key `{}` (only `group` and `specs` are read)", GROUPS_FILE, k));
    }
    let mut groups = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(list) = table.get("group") {
        let list = list.as_array().ok_or_else(|| format!("`group` in {} must be an array of tables ([[group]])", GROUPS_FILE))?;
        for g in list {
            let t = g.as_table().ok_or_else(|| format!("a `group` entry in {} is not a table", GROUPS_FILE))?;
            let id = t.get("id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).ok_or_else(|| format!("a [[group]] in {} has no `id`", GROUPS_FILE))?;
            let title = t.get("title").and_then(|v| v.as_str()).ok_or_else(|| format!("group `{}` has no `title`", id))?;
            let order = t.get("order").and_then(|v| v.as_integer()).ok_or_else(|| format!("group `{}` has no integer `order`", id))?;
            if !seen.insert(id.to_string()) {
                return Err(format!("group `{}` is listed twice in {}", id, GROUPS_FILE));
            }
            groups.push(Group { id: id.to_string(), title: title.to_string(), order });
        }
    }
    let mut specs = BTreeMap::new();
    if let Some(t) = table.get("specs") {
        let t = t.as_table().ok_or_else(|| format!("`specs` in {} must be a table of stem = \"group\"", GROUPS_FILE))?;
        for (stem, v) in t {
            let g = v.as_str().ok_or_else(|| format!("`{}` in {} must be assigned to a group name", stem, GROUPS_FILE))?;
            if !seen.contains(g) {
                return Err(format!("`{}` is assigned to group `{}`, which is not in the group list of {}", stem, g, GROUPS_FILE));
            }
            specs.insert(stem.clone(), g.to_string());
        }
    }
    Ok(Groups { groups, specs })
}

/// Every Markdown file directly under `specs/`, by stem.
pub fn read_sources(repo: &Path) -> Result<Vec<Source>, String> {
    let dir = repo.join("specs");
    let rd = fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {}", dir.display(), e))?;
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.ends_with(".md") || !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let bytes = fs::read(e.path()).map_err(|err| format!("cannot read specs/{}: {}", name, err))?;
        out.push(Source { stem: name.trim_end_matches(".md").to_string(), path: format!("specs/{}", name), bytes });
    }
    out.sort_by(|a, b| a.stem.cmp(&b.stem));
    Ok(out)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|x| format!("{:02x}", x)).collect()
}

fn quote_json(q: &Quote) -> Value {
    json!({"kind": "quote", "text": q.text, "span": {"start": q.start, "end": q.end, "line": q.line, "end_line": q.end_line}})
}

fn compile_one(s: &Source, groups: &Groups) -> Result<String, String> {
    let group = groups.specs.get(&s.stem).ok_or_else(|| format!("`{}` is not in {}; give it a group", s.stem, GROUPS_FILE))?;
    let text = std::str::from_utf8(&s.bytes).map_err(|e| format!("is not valid UTF-8 ({})", e))?;
    let fm = lint::frontmatter_of(text).ok_or_else(|| "has no frontmatter".to_string())?;
    let status_line = specdoc::frontmatter_line(text, "status").ok_or_else(|| "has no `status:` line in its frontmatter".to_string())?;
    let word = fm
        .get("status")
        .and_then(|v| v.as_str())
        .and_then(|v| v.split_whitespace().next())
        .ok_or_else(|| "its `status:` has no word".to_string())?
        .to_string();
    let yaml = |key: &str| fm.get(key).and_then(|v| serde_json::to_value(v).ok()).unwrap_or(Value::Null);
    let hash = sha256_hex(&s.bytes);
    let summary = specdoc::scope_summary(text).map(|q| quote_json(&q)).unwrap_or(Value::Null);
    let outline: Vec<Value> = specdoc::outline(text).into_iter().map(|e| json!({"level": e.level, "number": e.number, "title": e.title, "line": e.line})).collect();
    let doc = json!({
        "schema_version": SCHEMA_VERSION,
        "authoritative": false,
        "generated_by": COMPILER,
        "source": {"path": s.path, "sha256": hash},
        "spec": {
            "stem": s.stem,
            "number": specdoc::spec_number(&format!("{}.md", s.stem)),
            "title": fm.get("title").and_then(|v| v.as_str()),
            "group": group,
            "supersedes": yaml("supersedes"),
            "superseded_by": yaml("superseded_by"),
            "tier": 0,
        },
        "source_status_at_hash": {"word": word, "line": quote_json(&status_line), "sha256": hash},
        "summary": summary,
        "outline": outline,
    });
    Ok(serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n")
}

/// Compiles every source in memory. All or nothing: any failure returns every problem found and no output.
pub fn compile(sources: &[Source], groups: &Groups) -> Result<BTreeMap<String, String>, Vec<String>> {
    let mut errors = Vec::new();
    let mut out = BTreeMap::new();
    let stems: BTreeSet<&str> = sources.iter().map(|s| s.stem.as_str()).collect();
    for stem in groups.specs.keys() {
        if !stems.contains(stem.as_str()) {
            errors.push(format!("{} names `{}`, but specs/{}.md does not exist", GROUPS_FILE, stem, stem));
        }
    }
    for s in sources {
        match compile_one(s, groups) {
            Ok(json) => {
                out.insert(s.stem.clone(), json);
            }
            Err(e) => errors.push(format!("{}: {}", s.path, e)),
        }
    }
    if errors.is_empty() { Ok(out) } else { Err(errors) }
}

/// Replaces `compiled/specs/data/` with `files` (stem to file text): written to a sibling directory first, then renamed over.
pub fn write_all(repo: &Path, files: &BTreeMap<String, String>) -> Result<(), String> {
    let base = repo.join("compiled/specs");
    let (data, tmp, old) = (base.join("data"), base.join(".data.new"), base.join(".data.old"));
    fs::create_dir_all(&base).map_err(|e| format!("cannot create {}: {}", base.display(), e))?;
    let _ = fs::remove_dir_all(&tmp);
    let _ = fs::remove_dir_all(&old);
    fs::create_dir(&tmp).map_err(|e| format!("cannot create {}: {}", tmp.display(), e))?;
    for (stem, text) in files {
        if let Err(e) = fs::write(tmp.join(format!("{}.json", stem)), text) {
            let _ = fs::remove_dir_all(&tmp);
            return Err(format!("cannot write {}.json: {}", stem, e));
        }
    }
    if data.exists() {
        fs::rename(&data, &old).map_err(|e| format!("cannot move the old {} aside: {}", DATA_DIR, e))?;
    }
    if let Err(e) = fs::rename(&tmp, &data) {
        if old.exists() {
            let _ = fs::rename(&old, &data);
        }
        return Err(format!("cannot put the new {} in place: {}", DATA_DIR, e));
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

/// The differences between a fresh compile and what is on disk: a file that differs, a file that is missing, and a file with no
/// source (an orphan). Empty means `compiled/specs/data/` is current.
pub fn check(repo: &Path, files: &BTreeMap<String, String>) -> Vec<String> {
    let data = repo.join(DATA_DIR);
    let mut diffs = Vec::new();
    for (stem, text) in files {
        match fs::read_to_string(data.join(format!("{}.json", stem))) {
            Ok(cur) if cur == *text => {}
            Ok(_) => diffs.push(format!("{}/{}.json differs from a fresh compile", DATA_DIR, stem)),
            Err(_) => diffs.push(format!("{}/{}.json is missing", DATA_DIR, stem)),
        }
    }
    if let Ok(rd) = fs::read_dir(&data) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.strip_suffix(".json").is_none_or(|stem| !files.contains_key(stem)) {
                diffs.push(format!("{}/{} has no source spec (an orphan)", DATA_DIR, name));
            }
        }
    }
    diffs.sort();
    diffs
}

fn usage() -> &'static str {
    "usage: delphi specs compile [--check] [--repo <path>]"
}

/// `delphi specs ...` (only `compile` exists in stage 1). Exit 0 current or written, 1 a spec or the groups failed or the files
/// are stale, 2 the command was misused or a file could not be read.
pub fn cli(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("compile") => compile_cmd(&args[1..]),
        _ => {
            eprintln!("{}", usage());
            2
        }
    }
}

fn compile_cmd(args: &[String]) -> i32 {
    let check_only = args.iter().any(|a| a == "--check");
    let repo = crate::flag(args, "--repo").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let groups = match fs::read_to_string(repo.join(GROUPS_FILE)) {
        Ok(t) => match parse_groups(&t) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("delphi specs compile: {}", e);
                return 1;
            }
        },
        Err(e) => {
            eprintln!("delphi specs compile: cannot read {} under {}: {}", GROUPS_FILE, repo.display(), e);
            return 2;
        }
    };
    let sources = match read_sources(&repo) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("delphi specs compile: {}", e);
            return 2;
        }
    };
    let files = match compile(&sources, &groups) {
        Ok(f) => f,
        Err(errors) => {
            for e in &errors {
                eprintln!("delphi specs compile: {}", e);
            }
            eprintln!("delphi specs compile: {} problem(s); nothing was written and {} is as it was", errors.len(), DATA_DIR);
            return 1;
        }
    };
    if check_only {
        let diffs = check(&repo, &files);
        if diffs.is_empty() {
            println!("delphi specs compile --check: {} spec(s), {} is current", files.len(), DATA_DIR);
            return 0;
        }
        for d in &diffs {
            eprintln!("delphi specs compile --check: {}", d);
        }
        eprintln!("delphi specs compile --check: {} difference(s); run `delphi specs compile` and commit the result", diffs.len());
        return 1;
    }
    match write_all(&repo, &files) {
        Ok(()) => {
            println!("delphi specs compile: wrote {} file(s) to {}", files.len(), DATA_DIR);
            0
        }
        Err(e) => {
            eprintln!("delphi specs compile: {}", e);
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUPS: &str = "[[group]]\nid = \"a\"\ntitle = \"A\"\norder = 1\n\n[[group]]\nid = \"b\"\ntitle = \"B\"\norder = 2\n\n[specs]\n\"001-first\" = \"a\"\n\"002-second\" = \"b\"\n";

    fn spec(status: &str, body: &str) -> String {
        format!("---\nid: spec-x\ntitle: \"X: a title\"\nstatus: {}\n---\n\n# Spec X\n\n{}", status, body)
    }

    const BODY: &str = "## 1. Scope and objectives\n\nThe summary, in two\nlines.\n\n## 2. Next\n\ntext\n";

    fn repo(files: &[(&str, &[u8])], groups: &str) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("specs")).unwrap();
        fs::create_dir_all(t.path().join("compiled")).unwrap();
        fs::write(t.path().join(GROUPS_FILE), groups).unwrap();
        for (name, bytes) in files {
            fs::write(t.path().join("specs").join(name), bytes).unwrap();
        }
        t
    }

    fn two() -> tempfile::TempDir {
        repo(&[("001-first.md", spec("approved   # by the operator, 2026-11-07", BODY).as_bytes()), ("002-second.md", spec("draft", "no scope here\n").as_bytes())], GROUPS)
    }

    fn compiled(t: &tempfile::TempDir) -> BTreeMap<String, String> {
        compile(&read_sources(t.path()).unwrap(), &parse_groups(GROUPS).unwrap()).unwrap()
    }

    fn val(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    // ── groups ────────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn groups_are_a_closed_list_and_each_spec_names_one() {
        let g = parse_groups(GROUPS).unwrap();
        assert_eq!(g.groups.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(g.specs["001-first"], "a");
        for (bad, why) in [
            ("[[group]]\nid = \"a\"\ntitle = \"A\"\norder = 1\n[specs]\n\"x\" = \"nope\"\n", "not in the group list"),
            ("[[group]]\nid = \"a\"\ntitle = \"A\"\norder = 1\n[[group]]\nid = \"a\"\ntitle = \"A\"\norder = 2\n", "listed twice"),
            ("[[group]]\nid = \"a\"\ntitle = \"A\"\n", "no integer `order`"),
            ("[[group]]\nid = \"a\"\norder = 1\n", "no `title`"),
            ("[spec]\n\"x\" = \"a\"\n", "unknown top-level key `spec`"),
            ("not toml [", "not valid TOML"),
        ] {
            let e = parse_groups(bad).unwrap_err();
            assert!(e.contains(why), "{} -> {}", why, e);
        }
    }

    // ── what is written ───────────────────────────────────────────────────────────────────────────

    #[test]
    fn a_compiled_file_quotes_the_source_and_carries_no_approval_field() {
        let t = two();
        let out = compiled(&t);
        assert_eq!(out.keys().map(String::as_str).collect::<Vec<_>>(), vec!["001-first", "002-second"]);
        let v = val(&out["001-first"]);
        assert_eq!(v["authoritative"], false);
        assert_eq!(v["generated_by"], COMPILER);
        assert_eq!(v["spec"]["stem"], "001-first");
        assert_eq!(v["spec"]["number"], "001");
        assert_eq!(v["spec"]["group"], "a");
        assert_eq!(v["spec"]["tier"], 0);
        assert_eq!(v["spec"]["title"], "X: a title");
        assert_eq!(v["source_status_at_hash"]["word"], "approved");
        assert_eq!(v["source_status_at_hash"]["line"]["text"], "status: approved   # by the operator, 2026-11-07");
        assert_eq!(v["source_status_at_hash"]["sha256"], v["source"]["sha256"]);
        assert_eq!(v["summary"]["kind"], "quote");
        assert_eq!(v["summary"]["text"], "The summary, in two\nlines.");
        // A draft appears labelled draft, with no summary where the spec has none, and still no approval field.
        let d = val(&out["002-second"]);
        assert_eq!(d["source_status_at_hash"]["word"], "draft");
        assert!(d["summary"].is_null());
        for f in out.values() {
            for key in ["approved_by", "approved_on", "as_of", "\"status\":"] {
                assert!(!f.contains(key), "{} must not appear in a compiled file", key);
            }
        }
    }

    #[test]
    fn every_quote_is_the_source_cut_at_its_span_and_the_spans_and_gaps_tile_the_file() {
        let t = two();
        let out = compiled(&t);
        for (stem, text) in &out {
            let src = fs::read_to_string(t.path().join(format!("specs/{}.md", stem))).unwrap();
            let v = val(text);
            let mut spans: Vec<(usize, usize)> = Vec::new();
            for q in [&v["source_status_at_hash"]["line"], &v["summary"]] {
                if q.is_null() {
                    continue;
                }
                let (s, e) = (q["span"]["start"].as_u64().unwrap() as usize, q["span"]["end"].as_u64().unwrap() as usize);
                assert_eq!(&src[s..e], q["text"].as_str().unwrap(), "{}: a quote is the slice at its span", stem);
                spans.push((s, e));
            }
            // Spans in order, and the gaps between them: pieces and gaps concatenate to the file, byte for byte.
            spans.sort();
            let mut rebuilt = String::new();
            let mut at = 0;
            for (s, e) in spans {
                assert!(s >= at, "{}: spans overlap", stem);
                rebuilt.push_str(&src[at..s]);
                rebuilt.push_str(&src[s..e]);
                at = e;
            }
            rebuilt.push_str(&src[at..]);
            assert_eq!(rebuilt, src, "{}: spans and gaps reproduce the file", stem);
            assert_eq!(v["source"]["sha256"], sha256_hex(src.as_bytes()).as_str());
        }
    }

    #[test]
    fn compiling_twice_changes_nothing() {
        let t = two();
        assert_eq!(compiled(&t), compiled(&t));
    }

    #[test]
    fn the_group_comes_from_the_groups_file_whatever_the_spec_says() {
        let src = "---\nid: spec-x\ntitle: T\nstatus: approved\ngroup: b\n---\n\n## Scope\n\ntext\n";
        let t = repo(&[("001-first.md", src.as_bytes()), ("002-second.md", spec("draft", "x\n").as_bytes())], GROUPS);
        assert_eq!(val(&compiled(&t)["001-first"])["spec"]["group"], "a");
    }

    #[test]
    fn supersession_is_copied_as_written_where_a_spec_has_it() {
        let src = "---\nid: spec-x\ntitle: T\nstatus: approved\nsupersedes: [\"010\"]\nsuperseded_by: \"099\"\n---\n\n## Scope\n\ntext\n";
        let t = repo(&[("001-first.md", src.as_bytes()), ("002-second.md", spec("draft", "x\n").as_bytes())], GROUPS);
        let v = val(&compiled(&t)["001-first"]);
        assert_eq!(v["spec"]["supersedes"], json!(["010"]));
        assert_eq!(v["spec"]["superseded_by"], "099");
        assert!(val(&compiled(&t)["002-second"])["spec"]["supersedes"].is_null());
    }

    #[test]
    fn two_specs_with_one_number_are_two_files_named_by_stem() {
        let groups = "[[group]]\nid = \"a\"\ntitle = \"A\"\norder = 1\n[specs]\n\"014-one\" = \"a\"\n\"014-two\" = \"a\"\n";
        let t = repo(&[("014-one.md", spec("approved", BODY).as_bytes()), ("014-two.md", spec("approved", BODY).as_bytes())], groups);
        let out = compile(&read_sources(t.path()).unwrap(), &parse_groups(groups).unwrap()).unwrap();
        assert_eq!(out.keys().map(String::as_str).collect::<Vec<_>>(), vec!["014-one", "014-two"]);
        assert_eq!(val(&out["014-one"])["spec"]["number"], "014");
        assert_eq!(val(&out["014-two"])["spec"]["number"], "014");
    }

    #[test]
    fn a_spec_without_a_number_in_its_name_has_a_null_number() {
        let groups = "[[group]]\nid = \"a\"\ntitle = \"A\"\norder = 1\n[specs]\n\"blueprints\" = \"a\"\n";
        let t = repo(&[("blueprints.md", spec("approved", BODY).as_bytes())], groups);
        let out = compile(&read_sources(t.path()).unwrap(), &parse_groups(groups).unwrap()).unwrap();
        assert!(val(&out["blueprints"])["spec"]["number"].is_null());
    }

    // ── refusals: each one names the spec by stem and path ───────────────────────────────────────────

    fn refusal(files: &[(&str, &[u8])], groups: &str) -> Vec<String> {
        let t = repo(files, groups);
        compile(&read_sources(t.path()).unwrap(), &parse_groups(groups).unwrap()).unwrap_err()
    }

    #[test]
    fn a_source_that_is_not_utf8_is_refused() {
        let mut bad = spec("approved", BODY).into_bytes();
        bad.extend_from_slice(&[0xff, 0xfe, 0xfd]);
        let e = refusal(&[("001-first.md", &bad), ("002-second.md", spec("draft", "x\n").as_bytes())], GROUPS);
        assert!(e.iter().any(|m| m.contains("specs/001-first.md") && m.contains("not valid UTF-8")), "{:?}", e);
    }

    #[test]
    fn a_source_without_frontmatter_or_a_status_line_is_refused() {
        let e = refusal(&[("001-first.md", b"# no frontmatter\n"), ("002-second.md", b"---\nid: x\ntitle: T\n---\n\nbody\n")], GROUPS);
        assert!(e.iter().any(|m| m.contains("specs/001-first.md") && m.contains("no frontmatter")), "{:?}", e);
        assert!(e.iter().any(|m| m.contains("specs/002-second.md") && m.contains("no `status:` line")), "{:?}", e);
    }

    #[test]
    fn a_spec_missing_from_the_groups_and_a_group_entry_with_no_spec_are_both_refused() {
        let groups = "[[group]]\nid = \"a\"\ntitle = \"A\"\norder = 1\n[specs]\n\"001-first\" = \"a\"\n\"003-gone\" = \"a\"\n";
        let e = refusal(&[("001-first.md", spec("approved", BODY).as_bytes()), ("002-second.md", spec("draft", "x\n").as_bytes())], groups);
        assert!(e.iter().any(|m| m.contains("`002-second` is not in compiled/groups.toml")), "{:?}", e);
        assert!(e.iter().any(|m| m.contains("names `003-gone`, but specs/003-gone.md does not exist")), "{:?}", e);
    }

    #[test]
    fn every_problem_is_reported_not_just_the_first() {
        let e = refusal(&[("001-first.md", b"# nothing\n"), ("002-second.md", b"# nothing\n")], GROUPS);
        assert_eq!(e.len(), 2, "{:?}", e);
    }

    // ── writing and checking ──────────────────────────────────────────────────────────────────────

    fn run(t: &tempfile::TempDir, extra: &[&str]) -> i32 {
        let mut a: Vec<String> = vec!["compile".into(), "--repo".into(), t.path().to_string_lossy().to_string()];
        a.extend(extra.iter().map(|s| s.to_string()));
        cli(&a)
    }

    fn listing(t: &tempfile::TempDir) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(t.path().join(DATA_DIR)).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
        v.sort();
        v
    }

    #[test]
    fn compile_writes_the_files_and_check_then_passes_until_something_drifts() {
        let t = two();
        assert_eq!(run(&t, &["--check"]), 1, "nothing is written yet, so the check fails");
        assert_eq!(run(&t, &[]), 0);
        assert_eq!(listing(&t), vec!["001-first.json", "002-second.json"]);
        assert_eq!(run(&t, &["--check"]), 0);
        // Edit a source: the check names the stale file.
        fs::write(t.path().join("specs/001-first.md"), spec("approved", "## Scope\n\nchanged\n")).unwrap();
        assert_eq!(run(&t, &["--check"]), 1);
        assert_eq!(check(t.path(), &compiled(&t)), vec![format!("{}/001-first.json differs from a fresh compile", DATA_DIR)]);
        assert_eq!(run(&t, &[]), 0);
        assert_eq!(run(&t, &["--check"]), 0);
        // A file with no source spec is an orphan, and a missing file is reported.
        fs::write(t.path().join(DATA_DIR).join("009-ghost.json"), "{}").unwrap();
        fs::remove_file(t.path().join(DATA_DIR).join("002-second.json")).unwrap();
        let d = check(t.path(), &compiled(&t));
        assert_eq!(d, vec![format!("{}/002-second.json is missing", DATA_DIR), format!("{}/009-ghost.json has no source spec (an orphan)", DATA_DIR)]);
    }

    #[test]
    fn a_failed_compile_leaves_the_committed_files_exactly_as_they_were() {
        let t = two();
        assert_eq!(run(&t, &[]), 0);
        let before: Vec<(String, String)> = listing(&t).into_iter().map(|n| (n.clone(), fs::read_to_string(t.path().join(DATA_DIR).join(&n)).unwrap())).collect();
        // One spec becomes invalid: the run fails, names it, and writes nothing.
        fs::write(t.path().join("specs/002-second.md"), b"# no frontmatter\n").unwrap();
        fs::write(t.path().join("specs/001-first.md"), spec("approved", "## Scope\n\nthis would change the first file\n")).unwrap();
        assert_eq!(run(&t, &[]), 1);
        let after: Vec<(String, String)> = listing(&t).into_iter().map(|n| (n.clone(), fs::read_to_string(t.path().join(DATA_DIR).join(&n)).unwrap())).collect();
        assert_eq!(before, after, "a failed compile changes nothing, not even the file of a spec that compiled");
        assert!(!t.path().join("compiled/specs/.data.new").exists(), "no temporary directory is left");
    }

    #[test]
    fn deleting_a_spec_removes_its_compiled_file_in_the_same_run() {
        let t = two();
        assert_eq!(run(&t, &[]), 0);
        fs::remove_file(t.path().join("specs/002-second.md")).unwrap();
        let groups = "[[group]]\nid = \"a\"\ntitle = \"A\"\norder = 1\n[specs]\n\"001-first\" = \"a\"\n";
        fs::write(t.path().join(GROUPS_FILE), groups).unwrap();
        assert_eq!(run(&t, &[]), 0);
        assert_eq!(listing(&t), vec!["001-first.json"]);
    }

    #[test]
    fn missing_groups_and_unknown_commands_are_usage_errors() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("specs")).unwrap();
        assert_eq!(run(&t, &[]), 2, "no groups file");
        assert_eq!(cli(&["frobnicate".to_string()]), 2);
        assert_eq!(cli(&[]), 2);
    }
}
