//! The gate-changing class (spec 094 §3.4): paths that decide what is allowed to merge. A pull request that touches one is never
//! merged by `--all` or `--auto`, is refused a `chat-quote`, and needs a human at a terminal with the full diff.
//!
//! The list lives in `merge-gates.toml` at the repository root and is **read from the base branch, never from the pull
//! request**, so a change cannot weaken the list that judges it. The file is on its own list. When the base branch has no such
//! file yet, the built-in list below is used and the verdict says so.

pub const GATES_FILE: &str = "merge-gates.toml";

/// The starting list (spec 094 OD-5). `merge-gates.toml` in the repository root carries the same entries.
pub const DEFAULT_GATES: &[&str] = &[
    ".github/**",
    "deny.toml",
    "GOVERNANCE.md",
    "REVIEW.md",
    "AGENTS.md",
    "context/heuristics.md",
    "merge-gates.toml",
    "crates/delphi/tests/spawn_sites.*",
    "crates/delphi/tests/must_run.rs",
    "crates/delphi/tests/home_isolation.rs",
    "crates/delphi/tests/deny_policy.rs",
    "crates/delphi-engine/src/service_gates_tests.rs",
    "crates/delphi/src/merge/**",
];

/// Where a list came from, so the verdict can say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    BaseBranch,
    BuiltIn,
}

/// Parse `merge-gates.toml`: `paths = ["...", ...]`. An unreadable file, an empty list or an entry that could escape the
/// repository is an error, so a damaged list refuses merges instead of waving them through.
pub fn parse(text: &str) -> Result<Vec<String>, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("{GATES_FILE} is not valid TOML: {e}"))?;
    let list = v.get("paths").and_then(|p| p.as_array()).ok_or_else(|| format!("{GATES_FILE} needs a `paths = [...]` list"))?;
    let mut out = Vec::new();
    for item in list {
        let p = item.as_str().ok_or_else(|| format!("{GATES_FILE}: every entry in `paths` must be a string"))?.trim();
        if p.is_empty() || p.starts_with('/') || p.split('/').any(|seg| seg == "..") || p.contains('\\') {
            return Err(format!("{GATES_FILE}: `{p}` is not a repository-relative path pattern"));
        }
        out.push(p.to_string());
    }
    if out.is_empty() {
        return Err(format!("{GATES_FILE}: `paths` is empty, which would make every pull request ordinary; refusing to use it"));
    }
    Ok(out)
}

/// A segment pattern with `*` standing for any run of characters inside the segment.
fn seg_match(pat: &[u8], seg: &[u8]) -> bool {
    match pat.split_first() {
        None => seg.is_empty(),
        Some((b'*', rest)) => (0..=seg.len()).any(|i| seg_match(rest, &seg[i..])),
        Some((c, rest)) => seg.first() == Some(c) && seg_match(rest, &seg[1..]),
    }
}

fn parts_match(pat: &[&str], path: &[&str]) -> bool {
    match pat.split_first() {
        None => path.is_empty(),
        // `**` is any number of whole segments, including none after a directory name.
        Some((&"**", rest)) => (0..=path.len()).any(|i| parts_match(rest, &path[i..])),
        Some((p, rest)) => path.split_first().is_some_and(|(seg, tail)| seg_match(p.as_bytes(), seg.as_bytes()) && parts_match(rest, tail)),
    }
}

/// Does `path` match `pattern`? Case-insensitive on purpose: matching more is harmless, and a path that differs only in case
/// must not slip past a gate.
pub fn matches(pattern: &str, path: &str) -> bool {
    let (pat, path) = (pattern.to_lowercase(), path.to_lowercase());
    let pat: Vec<&str> = pat.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    parts_match(&pat, &path)
}

/// The changed paths that fall in the gate class, in the order given.
pub fn hits(patterns: &[String], changed: &[String]) -> Vec<String> {
    changed.iter().filter(|c| patterns.iter().any(|p| matches(p, c))).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_directories_names_and_segment_wildcards() {
        assert!(matches(".github/**", ".github/workflows/ci.yml"));
        assert!(matches(".github/**", ".github/x"));
        // `**` may match no segments, so the bare name matches too: stricter on purpose (a file named `.github` replacing the directory).
        assert!(matches(".github/**", ".github"));
        assert!(!matches(".github/**", "docs/.github/x"));
        assert!(matches("deny.toml", "deny.toml") && !matches("deny.toml", "sub/deny.toml") && !matches("deny.toml", "deny.toml.bak"));
        assert!(matches("crates/delphi/tests/spawn_sites.*", "crates/delphi/tests/spawn_sites.rs"));
        assert!(matches("crates/delphi/tests/spawn_sites.*", "crates/delphi/tests/spawn_sites.toml"));
        assert!(!matches("crates/delphi/tests/spawn_sites.*", "crates/delphi/tests/spawn_sites_other.rs/x"));
        assert!(matches("crates/delphi/src/merge/**", "crates/delphi/src/merge/forge.rs"));
        assert!(matches("crates/delphi/src/merge/**", "crates/delphi/src/merge/deep/er/x.rs"));
        assert!(!matches("crates/delphi/src/merge/**", "crates/delphi/src/merged.rs"));
        assert!(matches("**/Cargo.lock", "Cargo.lock") && matches("**/Cargo.lock", "a/b/Cargo.lock"));
    }

    #[test]
    fn a_path_that_differs_only_in_case_does_not_slip_past() {
        assert!(matches(".github/**", ".GitHub/Workflows/CI.yml"));
        assert!(matches("GOVERNANCE.md", "governance.md"));
    }

    #[test]
    fn the_built_in_list_covers_the_documented_gate_paths() {
        let all = DEFAULT_GATES.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for p in [
            ".github/workflows/ci.yml",
            "deny.toml",
            "GOVERNANCE.md",
            "REVIEW.md",
            "AGENTS.md",
            "context/heuristics.md",
            "merge-gates.toml",
            "crates/delphi/tests/spawn_sites.toml",
            "crates/delphi/tests/must_run.rs",
            "crates/delphi/tests/home_isolation.rs",
            "crates/delphi/tests/deny_policy.rs",
            "crates/delphi-engine/src/service_gates_tests.rs",
            "crates/delphi/src/merge/mod.rs",
        ] {
            assert_eq!(hits(&all, &[p.to_string()]).len(), 1, "{p}");
        }
        for p in ["README.md", "specs/094-core-repo-merge-flow.md", "crates/delphi/src/main.rs", "docs/merging-core-changes.md", "skills/x/SKILL.md"] {
            assert!(hits(&all, &[p.to_string()]).is_empty(), "{p} is not gate-changing");
        }
    }

    #[test]
    fn the_list_file_parses_and_a_damaged_one_is_an_error_not_an_empty_list() {
        assert_eq!(parse("paths = [\".github/**\", \"deny.toml\"]").unwrap(), vec![".github/**".to_string(), "deny.toml".to_string()]);
        assert!(parse("paths = []").unwrap_err().contains("empty"));
        assert!(parse("nothing = 1").unwrap_err().contains("paths"));
        assert!(parse("paths = [1]").unwrap_err().contains("string"));
        assert!(parse("paths = [\"../x\"]").unwrap_err().contains("repository-relative"));
        assert!(parse("paths = [\"/etc/x\"]").unwrap_err().contains("repository-relative"));
        assert!(parse("not toml [[").unwrap_err().contains("TOML"));
    }

    #[test]
    fn the_committed_file_is_the_built_in_list() {
        // The file at the repository root and the built-in fallback must not drift apart.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(root.join(GATES_FILE)).expect("merge-gates.toml at the repository root");
        assert_eq!(parse(&text).unwrap(), DEFAULT_GATES.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }
}
