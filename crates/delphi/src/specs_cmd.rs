//! `delphi specs show | list | report` (spec 088 section 10.3): reading the compiled specs, and counting that they were read.
//!
//! `show <spec>` prints a compiled file and **refuses a stale one**; `show <spec> --section <n>` prints that section of the source
//! spec itself, so the reader sees the spec's own words and the hash they came from. `list` names every spec and whether its
//! compiled file is current. `report` prints the read counter (see `delphi_engine::spec_lookup` for what it counts and cannot see).
//! Every `show` appends one line to the counter, only if the Delphi home exists; a lookup never creates it. A compiled file is a
//! derived view, never the authority: what it says about status is informational, and the source spec decides.

use delphi_engine::spec_lookup as lookup;
use std::path::PathBuf;

/// The result of a command: what it printed and how it ended.
pub struct Out {
    pub code: i32,
    pub out: String,
    pub err: String,
}

fn done(out: String) -> Out {
    Out { code: 0, out, err: String::new() }
}

fn refuse(code: i32, err: impl Into<String>) -> Out {
    Out { code, out: String::new(), err: err.into() }
}

/// Flags that take a value, so a value is never mistaken for the spec being named.
const VALUE_FLAGS: [&str; 6] = ["--repo", "--home", "--for", "--section", "--group", "--status"];

fn positional(args: &[String]) -> Option<String> {
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if VALUE_FLAGS.contains(&a.as_str()) {
            skip = true;
        } else if !a.starts_with("--") {
            return Some(a.clone());
        }
    }
    None
}

fn has(args: &[String], f: &str) -> bool {
    args.iter().any(|a| a == f)
}

fn repo(args: &[String]) -> PathBuf {
    crate::flag(args, "--repo").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn home(args: &[String]) -> PathBuf {
    crate::home::delphi_home(crate::flag(args, "--home").as_deref())
}

pub fn usage() -> &'static str {
    "usage: delphi specs show <spec> [--section <n>] [--json] [--for <label>] [--repo <path>]\n       delphi specs list [--group <g>] [--status <s>] [--json] [--repo <path>]\n       delphi specs report [--json]\n       delphi specs compile [--check] [--repo <path>]\n<spec> is a file name without .md, a spec number, or a unique prefix. --home <dir> names the Delphi home for the read counter."
}

/// Runs `show`, `list` or `report` (the arguments start with the subcommand) and returns what they print.
pub fn exec(args: &[String]) -> Out {
    let rest = &args[1.min(args.len())..];
    match args.first().map(String::as_str) {
        Some("show") => show(rest),
        Some("list") => list(rest),
        Some("report") => report(rest),
        _ => refuse(2, usage()),
    }
}

/// The entry point from `main`: prints and returns the exit code.
pub fn run(args: &[String]) -> i32 {
    let o = exec(args);
    if !o.out.is_empty() {
        print!("{}", o.out);
    }
    if !o.err.is_empty() {
        eprintln!("{}", o.err.trim_end());
    }
    o.code
}

fn show(args: &[String]) -> Out {
    let Some(query) = positional(args) else { return refuse(2, format!("delphi specs show: name a spec\n{}", usage())) };
    let label = match crate::flag(args, "--for") {
        Some(l) => match lookup::clean_label(&l) {
            Some(c) => Some(c),
            None => return refuse(2, "delphi specs show: --for takes a label of at most 64 letters, digits and ._/#:- (a slice or a branch name)"),
        },
        None => None,
    };
    let repo = repo(args);
    let home = home(args);
    if let Some(section) = crate::flag(args, "--section") {
        return match lookup::section(&repo, &query, &section) {
            Ok(s) => {
                lookup::record(&home, &lookup::Read { surface: "cli", op: "section", stem: &s.stem, section: Some(&section), label: label.as_deref() });
                if has(args, "--json") {
                    let v = serde_json::json!({"stem": s.stem, "path": s.path, "sha256": s.sha256, "section": section, "heading": s.heading, "line": s.line, "end_line": s.end_line, "text": s.text});
                    return done(format!("{}\n", serde_json::to_string_pretty(&v).unwrap_or_default()));
                }
                let mut out = format!("{} section {}: {} (source sha256 {})\n\n", s.stem, section, s.heading, &s.sha256[..12]);
                out.push_str(&lookup::fenced_excerpt(&s.path, s.line as u64, s.end_line as u64, &s.text));
                done(out)
            }
            Err(e) => refuse(1, format!("delphi specs show: {}", e)),
        };
    }
    match lookup::show(&repo, &query) {
        Ok(v) => {
            let stem = v["spec"]["stem"].as_str().unwrap_or("");
            lookup::record(&home, &lookup::Read { surface: "cli", op: "show", stem, section: None, label: label.as_deref() });
            if has(args, "--json") {
                done(format!("{}\n", serde_json::to_string_pretty(&v).unwrap_or_default()))
            } else {
                done(lookup::describe(&v))
            }
        }
        Err(e) => refuse(1, format!("delphi specs show: {}", e)),
    }
}

fn list(args: &[String]) -> Out {
    let rows = match lookup::list(&repo(args)) {
        Ok(r) => r,
        Err(e) => return refuse(2, format!("delphi specs list: {}", e)),
    };
    let (group, status) = (crate::flag(args, "--group"), crate::flag(args, "--status"));
    let rows: Vec<&lookup::Row> = rows.iter().filter(|r| group.as_deref().is_none_or(|g| r.group == g)).filter(|r| status.as_deref().is_none_or(|s| r.status == s)).collect();
    if has(args, "--json") {
        let v: Vec<serde_json::Value> = rows.iter().map(|r| serde_json::json!({"stem": r.stem, "group": r.group, "status": r.status, "tier": r.tier, "has_summary": r.has_summary, "state": r.state})).collect();
        return done(format!("{}\n", serde_json::to_string_pretty(&v).unwrap_or_default()));
    }
    let width = rows.iter().map(|r| r.stem.len()).max().unwrap_or(4).max(4);
    let mut out = format!("{:<w$}  {:<12}  {:<9}  {:<28}  SUMMARY\n", "SPEC", "STATE", "STATUS", "GROUP", w = width);
    for r in &rows {
        out.push_str(&format!("{:<w$}  {:<12}  {:<9}  {:<28}  {}\n", r.stem, r.state, r.status, r.group, if r.has_summary { "yes" } else { "no" }, w = width));
    }
    let bad = rows.iter().filter(|r| r.state != "compiled").count();
    out.push_str(&format!("\n{} spec(s).{}\n", rows.len(), if bad > 0 { format!(" {} not current: run `delphi specs compile`.", bad) } else { String::new() }));
    out.push_str("A status here is what the source said when it was compiled; the source spec decides.\n");
    done(out)
}

fn report(args: &[String]) -> Out {
    match lookup::report(&home(args)) {
        Ok(v) if has(args, "--json") => done(format!("{}\n", serde_json::to_string_pretty(&v).unwrap_or_default())),
        Ok(v) => done(lookup::report_text(&v)),
        Err(e) => refuse(2, format!("delphi specs report: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const GROUPS: &str = "[[group]]\nid = \"g1\"\ntitle = \"One\"\norder = 1\n[[group]]\nid = \"g2\"\ntitle = \"Two\"\norder = 2\n[specs]\n\"001-first\" = \"g1\"\n\"002-second\" = \"g2\"\n\"003-third\" = \"g2\"\n";

    fn spec(status: &str, body: &str) -> String {
        format!("---\nid: spec-x\ntitle: \"X: a title\"\nstatus: {}\n---\n\n# Spec X\n\n{}", status, body)
    }

    const BODY: &str = "## 1. Scope\n\nThe summary.\n\n### 1.1 Detail\n\ndetail text\n\n## 2. Next\n\nnext text\n";

    /// A repository with three compiled specs and an empty Delphi home.
    fn fixture() -> (tempfile::TempDir, tempfile::TempDir) {
        let repo = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(repo.path().join("specs")).unwrap();
        fs::create_dir_all(repo.path().join("compiled")).unwrap();
        // Built, not written out, so this file needs no exception from the guard in tests/compiled_specs.rs.
        fs::write(repo.path().join("compiled").join("groups.toml"), GROUPS).unwrap();
        fs::write(repo.path().join("specs/001-first.md"), spec("approved   # by the operator", BODY)).unwrap();
        fs::write(repo.path().join("specs/002-second.md"), spec("draft", BODY)).unwrap();
        fs::write(repo.path().join("specs/003-third.md"), spec("approved", "no scope here\n")).unwrap();
        assert_eq!(crate::spec_compile::cli(&["compile".into(), "--repo".into(), repo.path().to_string_lossy().to_string()]), 0);
        (repo, home)
    }

    fn a(repo: &tempfile::TempDir, home: &tempfile::TempDir, words: &[&str]) -> Out {
        let mut v: Vec<String> = words.iter().map(|s| s.to_string()).collect();
        v.extend(["--repo".to_string(), repo.path().to_string_lossy().to_string(), "--home".to_string(), home.path().to_string_lossy().to_string()]);
        exec(&v)
    }

    fn counter(home: &tempfile::TempDir) -> Vec<serde_json::Value> {
        fs::read_to_string(home.path().join(lookup::COUNTER_FILE)).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    #[test]
    fn show_prints_a_current_compiled_spec_and_counts_the_read() {
        let (repo, home) = fixture();
        let o = a(&repo, &home, &["show", "001", "--for", "S3b"]);
        assert_eq!(o.code, 0, "{}", o.err);
        assert!(o.out.starts_with("001-first\n") && o.out.contains("status: approved (per source at `") && o.out.contains("The summary."), "{}", o.out);
        let c = counter(&home);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0]["op"].as_str(), c[0]["stem"].as_str(), c[0]["label"].as_str(), c[0]["surface"].as_str()), (Some("show"), Some("001-first"), Some("S3b"), Some("cli")));
    }

    #[test]
    fn show_json_is_the_compiled_file() {
        let (repo, home) = fixture();
        let o = a(&repo, &home, &["show", "002-second", "--json"]);
        let v: serde_json::Value = serde_json::from_str(&o.out).unwrap();
        let on_disk: serde_json::Value = serde_json::from_str(&fs::read_to_string(repo.path().join("compiled").join("specs").join("data").join("002-second.json")).unwrap()).unwrap();
        assert_eq!(v, on_disk);
        assert_eq!(v["authoritative"], false);
    }

    #[test]
    fn a_stale_compiled_file_is_refused_not_served_and_not_counted() {
        let (repo, home) = fixture();
        fs::write(repo.path().join("specs/001-first.md"), spec("approved", "## 1. Scope\n\nChanged after the compile.\n")).unwrap();
        let o = a(&repo, &home, &["show", "001-first"]);
        assert_eq!(o.code, 1);
        assert!(o.out.is_empty(), "nothing of the stale file is printed");
        assert!(o.err.contains("is stale") && o.err.contains("delphi specs compile"), "{}", o.err);
        assert!(counter(&home).is_empty(), "a refusal is not a read");
        // The same spec can still be read from its source.
        let s = a(&repo, &home, &["show", "001-first", "--section", "1"]);
        assert_eq!(s.code, 0, "{}", s.err);
        assert!(s.out.contains("Changed after the compile."));
    }

    #[test]
    fn a_section_is_the_sources_own_text_in_a_labelled_fence_and_is_counted_as_a_section_read() {
        let (repo, home) = fixture();
        let o = a(&repo, &home, &["show", "001-first", "--section", "2", "--for", "S3b"]);
        assert_eq!(o.code, 0, "{}", o.err);
        assert!(o.out.starts_with("001-first section 2: Next (source sha256 "), "{}", o.out);
        assert!(o.out.contains("**source excerpt, untrusted** · `specs/001-first.md` lines ") && o.out.contains("```text\n## 2. Next\n\nnext text\n```\n"), "{}", o.out);
        let c = counter(&home);
        assert_eq!((c[0]["op"].as_str(), c[0]["section"].as_str()), (Some("section"), Some("2")));
        let j: serde_json::Value = serde_json::from_str(&a(&repo, &home, &["show", "001-first", "--section", "1.1", "--json"]).out).unwrap();
        assert_eq!(j["text"], "### 1.1 Detail\n\ndetail text");
        let bad = a(&repo, &home, &["show", "001-first", "--section", "99"]);
        assert_eq!(bad.code, 1);
        assert!(bad.err.contains("has no section 99"));
    }

    #[test]
    fn an_unknown_or_ambiguous_or_missing_name_is_refused_with_the_candidates() {
        let (repo, home) = fixture();
        let none = a(&repo, &home, &["show", "nope"]);
        assert_eq!((none.code, none.err.contains("no spec matches")), (1, true));
        let amb = a(&repo, &home, &["show", "00"]);
        assert_eq!(amb.code, 1);
        assert!(amb.err.contains("001-first") && amb.err.contains("003-third"), "{}", amb.err);
        assert_eq!(a(&repo, &home, &["show"]).code, 2);
        assert_eq!(exec(&["frobnicate".to_string()]).code, 2);
        assert_eq!(exec(&[]).code, 2);
        assert!(counter(&home).is_empty());
    }

    #[test]
    fn a_bad_label_is_a_usage_error_before_anything_is_read() {
        let (repo, home) = fixture();
        let o = a(&repo, &home, &["show", "001", "--for", "has `ticks`"]);
        assert_eq!(o.code, 2);
        assert!(counter(&home).is_empty());
    }

    #[test]
    fn a_lookup_never_creates_the_delphi_home() {
        let (repo, _home) = fixture();
        let gone = repo.path().join("no-such-home");
        let v = vec!["show".to_string(), "001".to_string(), "--repo".to_string(), repo.path().to_string_lossy().to_string(), "--home".to_string(), gone.to_string_lossy().to_string()];
        assert_eq!(exec(&v).code, 0);
        assert!(!gone.exists());
    }

    #[test]
    fn list_names_every_spec_filters_by_group_and_status_and_is_not_counted() {
        let (repo, home) = fixture();
        let all = a(&repo, &home, &["list"]);
        assert_eq!(all.code, 0);
        for s in ["001-first", "002-second", "003-third"] {
            assert!(all.out.contains(s), "{}", all.out);
        }
        assert!(all.out.contains("3 spec(s).") && all.out.contains("the source spec decides"), "{}", all.out);
        let g2 = a(&repo, &home, &["list", "--group", "g2", "--json"]);
        let v: serde_json::Value = serde_json::from_str(&g2.out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);
        let draft = a(&repo, &home, &["list", "--status", "draft"]);
        assert!(draft.out.contains("002-second") && !draft.out.contains("001-first"));
        assert!(counter(&home).is_empty(), "listing is not reading a spec");
        // A changed spec and a new one are flagged.
        fs::write(repo.path().join("specs/001-first.md"), spec("approved", "changed\n")).unwrap();
        fs::write(repo.path().join("specs/004-new.md"), spec("draft", "x\n")).unwrap();
        let after = a(&repo, &home, &["list"]);
        assert!(after.out.contains("stale") && after.out.contains("not compiled") && after.out.contains("2 not current"), "{}", after.out);
    }

    #[test]
    fn report_counts_the_reads_and_names_a_compiled_file_read_without_its_source_section() {
        let (repo, home) = fixture();
        assert!(a(&repo, &home, &["report"]).out.contains("No reads have been recorded"));
        a(&repo, &home, &["show", "001", "--for", "S3b"]);
        a(&repo, &home, &["show", "001", "--section", "1", "--for", "S3b"]);
        a(&repo, &home, &["show", "002", "--for", "S3b"]);
        let r = a(&repo, &home, &["report"]);
        assert!(r.out.contains("3 read(s) of 2 spec(s)") && r.out.contains("no source section read for: 002-second") && r.out.contains("Coverage:"), "{}", r.out);
        let j: serde_json::Value = serde_json::from_str(&a(&repo, &home, &["report", "--json"]).out).unwrap();
        assert_eq!(j["records"], 3);
    }
}
