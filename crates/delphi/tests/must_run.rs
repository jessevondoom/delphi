//! The must-run gate (spec 093 K2): a test family that can skip when its dependency is missing is also *required* by a CI job,
//! so a missing dependency there is a failure. This checks the three ways that goes wrong: a family no job requires, a job
//! requiring a family nothing consumes, and a family whose tests do not look at the gate at all.

#[allow(dead_code)] // this gate uses the shared module's must-run helpers, not its daemon reaper
mod common;

use regex::Regex;
use std::path::{Path, PathBuf};

/// `DELPHI_REQUIRE_*` switches that are not test families.
const NOT_FAMILIES: &[&str] = &["SIGNED"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// The families a workflow text requires: `DELPHI_REQUIRE_UI: "1"` names `ui`; `DELPHI_REQUIRE: "ui,sandbox"` names both.
fn required_by(workflow: &str) -> Vec<String> {
    let re = Regex::new(r#"(?m)^\s*DELPHI_REQUIRE(?:_([A-Z]+))?:\s*"?([^"\n#]*)"?"#).unwrap();
    let mut out = vec![];
    for c in re.captures_iter(workflow) {
        let value = c[2].trim();
        match c.get(1) {
            Some(suffix) if NOT_FAMILIES.contains(&suffix.as_str()) => {}
            Some(suffix) if !value.is_empty() && value != "0" => out.push(suffix.as_str().to_lowercase()),
            Some(_) => {}
            None => out.extend(value.split(',').map(|f| f.trim().to_string()).filter(|f| !f.is_empty())),
        }
    }
    out.sort();
    out.dedup();
    out
}

fn workflows(root: &Path) -> Vec<(String, String)> {
    let dir = root.join(".github/workflows");
    let mut out = vec![];
    for e in std::fs::read_dir(&dir).expect(".github/workflows exists").flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "yml" || x == "yaml") {
            out.push((p.file_name().unwrap().to_string_lossy().to_string(), std::fs::read_to_string(&p).unwrap()));
        }
    }
    out
}

/// Whether some Rust source other than this gate reads the family's switch.
fn consumed(root: &Path, family: &str) -> bool {
    let (call, legacy) = (format!("skip_or_fail(\"{family}\""), format!("DELPHI_REQUIRE_{}", family.to_uppercase()));
    let mut stack = vec![root.join("crates")];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if name != "target" && !name.starts_with('.') {
                    stack.push(p);
                }
            } else if name.ends_with(".rs") && name != "must_run.rs" && name != "mod.rs" {
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                if text.contains(&call) || text.contains(&format!("\"{legacy}\"")) {
                    return true;
                }
            }
        }
    }
    false
}

#[test]
fn every_must_run_family_is_required_by_a_ci_job_and_consumed_by_a_test() {
    let root = root();
    let mut required: Vec<String> = workflows(&root).iter().flat_map(|(_, w)| required_by(w)).collect();
    required.sort();
    required.dedup();
    let mut problems = vec![];
    for family in common::MUST_RUN_FAMILIES {
        if !required.iter().any(|r| r == family) {
            problems.push(format!("the `{family}` family can skip, but no CI job requires it: set DELPHI_REQUIRE={family} (or DELPHI_REQUIRE_{}=1) in the job that provisions it", family.to_uppercase()));
        }
        if !consumed(&root, family) {
            problems.push(format!("the `{family}` family is registered but no test reads its switch: call common::skip_or_fail(\"{family}\", ..) where it can skip"));
        }
    }
    for r in &required {
        if !common::MUST_RUN_FAMILIES.contains(&r.as_str()) {
            problems.push(format!("CI requires `{r}` but it is not in MUST_RUN_FAMILIES (crates/delphi/tests/common/mod.rs): register it, or remove the switch"));
        }
    }
    assert!(problems.is_empty(), "the must-run gate and CI disagree:\n  {}", problems.join("\n  "));
}

#[test]
fn a_required_family_fails_instead_of_skipping() {
    // Spec 093 AC-K2b, for the three outcomes the gate has.
    // Required through the list: a missing dependency must fail.
    assert!(common::required_in("ui", Some("ui,sandbox"), None));
    assert!(common::required_in("sandbox", Some(" ui , sandbox "), None));
    // Required through the older single switch.
    assert!(common::required_in("ui", None, Some("1")));
    // Not required: the test skips and says so.
    assert!(!common::required_in("ui", None, None));
    assert!(!common::required_in("ui", Some("sandbox"), None));
    assert!(!common::required_in("ui", None, Some("0")));
    assert!(!common::required_in("ui", None, Some("")));
}

#[test]
fn the_ci_scan_reads_both_forms_and_ignores_other_switches() {
    let wf = "env:\n  DELPHI_REQUIRE_UI: \"1\"\n  DELPHI_REQUIRE: \"sandbox, pty\"\n  DELPHI_REQUIRE_SIGNED: \"1\"\n  DELPHI_REQUIRE_OFF: \"0\"\n";
    assert_eq!(required_by(wf), vec!["pty".to_string(), "sandbox".to_string(), "ui".to_string()]);
    assert!(required_by("run: cargo test\n").is_empty());
}
