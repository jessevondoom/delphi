//! The forge: GitHub, reached only through the `gh` command (spec 094 §3.2).
//!
//! `delphi merge` decides from facts it reads here, never from what a label or a caller claims. The trait exists so the whole
//! flow can be tested against a fake forge with no network. `Gh` is the one real implementation; every process it starts goes
//! through `Gh::run` (one `Command::new`, listed in `crates/delphi/tests/spawn_sites.toml` as class `git`).

use serde_json::Value;
use std::process::Command;

/// What `delphi merge` needs to know about a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrInfo {
    pub number: u64,
    pub title: String,
    /// `OPEN`, `MERGED` or `CLOSED`.
    pub state: String,
    pub is_draft: bool,
    /// `MERGEABLE`, `CONFLICTING` or `UNKNOWN` (GitHub has not finished computing it).
    pub mergeable: String,
    pub head_sha: String,
    pub head_ref: String,
    pub base_ref: String,
    pub base_sha: String,
    pub labels: Vec<String>,
    pub additions: u64,
    pub deletions: u64,
    pub body: String,
    /// Every commit's full message on the pull request, for the `Implements-Spec:` trailer.
    pub commit_messages: Vec<String>,
    pub created_at: String,
}

/// One check run on the head commit. `bucket` is `gh`'s own classification: `pass`, `fail`, `pending`, `skipping` or `cancel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub bucket: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeError {
    /// `gh` is not installed or could not be started.
    Missing(String),
    /// `gh` ran and said no.
    Failed { what: String, detail: String },
    /// `gh` answered with something this build cannot read.
    Parse(String),
}

impl std::fmt::Display for ForgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ForgeError::Missing(m) => write!(f, "cannot run `gh`: {m} (install the GitHub CLI and run `gh auth login`)"),
            ForgeError::Failed { what, detail } => write!(f, "`gh` could not {what}: {detail}"),
            ForgeError::Parse(m) => write!(f, "could not read what `gh` returned: {m}"),
        }
    }
}

impl std::error::Error for ForgeError {}

pub trait Forge {
    fn pr(&self, n: u64) -> Result<PrInfo, ForgeError>;
    /// Every path the pull request touches, including the old name of a renamed file.
    fn changed_paths(&self, n: u64) -> Result<Vec<String>, ForgeError>;
    fn checks(&self, n: u64) -> Result<Vec<Check>, ForgeError>;
    /// A file's content at a branch or commit; `None` when it does not exist there.
    fn file_at(&self, git_ref: &str, path: &str) -> Result<Option<String>, ForgeError>;
    /// Open pull requests carrying `label`, oldest first.
    fn queued(&self, label: &str) -> Result<Vec<u64>, ForgeError>;
    /// The whole diff, for a human to read before a gate-changing merge.
    fn diff(&self, n: u64) -> Result<String, ForgeError>;
    /// Merge with a merge commit, and only if the head is still `expect_head` (GitHub refuses otherwise).
    fn merge(&self, n: u64, expect_head: &str) -> Result<(), ForgeError>;
    fn comment(&self, n: u64, body: &str) -> Result<(), ForgeError>;
    fn set_label(&self, n: u64, label: &str, on: bool) -> Result<(), ForgeError>;
}

/// Characters allowed in a ref or a path that goes into an API URL. Anything else is refused, not escaped.
fn url_safe(s: &str) -> bool {
    !s.is_empty() && s.len() <= 300 && !s.contains("..") && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

pub struct Gh;

impl Gh {
    /// Run `gh` with these arguments. `ok_codes` are exit codes that still carry a usable answer (`gh pr checks` exits 8 while
    /// checks are pending and 1 when one failed, and still prints its JSON).
    fn run(&self, what: &str, args: &[&str], ok_codes: &[i32]) -> Result<String, ForgeError> {
        let out = Command::new("gh").args(args).stdin(std::process::Stdio::null()).output().map_err(|e| ForgeError::Missing(e.to_string()))?;
        let code = out.status.code().unwrap_or(-1);
        if code == 0 || ok_codes.contains(&code) {
            return Ok(String::from_utf8_lossy(&out.stdout).to_string());
        }
        let detail = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(ForgeError::Failed { what: what.to_string(), detail: if detail.is_empty() { format!("exit code {code}") } else { detail } })
    }

    fn json(&self, what: &str, args: &[&str], ok_codes: &[i32]) -> Result<Value, ForgeError> {
        serde_json::from_str(&self.run(what, args, ok_codes)?).map_err(|e| ForgeError::Parse(format!("{what}: {e}")))
    }
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

impl Forge for Gh {
    fn pr(&self, n: u64) -> Result<PrInfo, ForgeError> {
        let num = n.to_string();
        let v = self.json(
            "read the pull request",
            &["pr", "view", &num, "--json", "number,title,state,isDraft,mergeable,headRefOid,headRefName,baseRefName,baseRefOid,labels,additions,deletions,body,commits,createdAt"],
            &[],
        )?;
        Ok(PrInfo {
            number: v["number"].as_u64().unwrap_or(n),
            title: s(&v, "title"),
            state: s(&v, "state"),
            is_draft: v["isDraft"].as_bool().unwrap_or(false),
            mergeable: s(&v, "mergeable"),
            head_sha: s(&v, "headRefOid"),
            head_ref: s(&v, "headRefName"),
            base_ref: s(&v, "baseRefName"),
            base_sha: s(&v, "baseRefOid"),
            labels: v["labels"].as_array().into_iter().flatten().map(|l| s(l, "name")).collect(),
            additions: v["additions"].as_u64().unwrap_or(0),
            deletions: v["deletions"].as_u64().unwrap_or(0),
            body: s(&v, "body"),
            commit_messages: v["commits"].as_array().into_iter().flatten().map(|c| format!("{}\n\n{}", s(c, "messageHeadline"), s(c, "messageBody"))).collect(),
            created_at: s(&v, "createdAt"),
        })
    }

    fn changed_paths(&self, n: u64) -> Result<Vec<String>, ForgeError> {
        let path = format!("repos/{{owner}}/{{repo}}/pulls/{n}/files");
        let out = self.run("list the changed files", &["api", &path, "--paginate", "--jq", ".[] | .filename, (.previous_filename // empty)"], &[])?;
        Ok(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
    }

    fn checks(&self, n: u64) -> Result<Vec<Check>, ForgeError> {
        let num = n.to_string();
        // "no checks reported" is an error exit with no JSON; that is an empty list, and the caller says what it means.
        let out = match self.run("read the checks", &["pr", "checks", &num, "--json", "name,bucket"], &[1, 8]) {
            Ok(o) => o,
            Err(ForgeError::Failed { detail, .. }) if detail.contains("no checks reported") => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let v: Value = serde_json::from_str(&out).map_err(|e| ForgeError::Parse(format!("checks: {e}")))?;
        Ok(v.as_array().into_iter().flatten().map(|c| Check { name: s(c, "name"), bucket: s(c, "bucket") }).collect())
    }

    fn file_at(&self, git_ref: &str, path: &str) -> Result<Option<String>, ForgeError> {
        if !url_safe(git_ref) || !url_safe(path) {
            return Err(ForgeError::Failed { what: "read a file".into(), detail: format!("`{git_ref}:{path}` has characters that are not allowed in a ref or path here") });
        }
        let url = format!("repos/{{owner}}/{{repo}}/contents/{path}?ref={git_ref}");
        match self.run("read a file", &["api", &url, "-H", "Accept: application/vnd.github.raw+json"], &[]) {
            Ok(text) => Ok(Some(text)),
            Err(ForgeError::Failed { detail, .. }) if detail.contains("404") || detail.contains("Not Found") => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn queued(&self, label: &str) -> Result<Vec<u64>, ForgeError> {
        let v = self.json("list queued pull requests", &["pr", "list", "--label", label, "--state", "open", "--limit", "100", "--json", "number,createdAt"], &[])?;
        let mut rows: Vec<(String, u64)> = v.as_array().into_iter().flatten().filter_map(|p| Some((s(p, "createdAt"), p["number"].as_u64()?))).collect();
        rows.sort();
        Ok(rows.into_iter().map(|(_, n)| n).collect())
    }

    fn diff(&self, n: u64) -> Result<String, ForgeError> {
        self.run("read the diff", &["pr", "diff", &n.to_string()], &[])
    }

    fn merge(&self, n: u64, expect_head: &str) -> Result<(), ForgeError> {
        self.run("merge", &["pr", "merge", &n.to_string(), "--merge", "--match-head-commit", expect_head], &[]).map(|_| ())
    }

    fn comment(&self, n: u64, body: &str) -> Result<(), ForgeError> {
        self.run("comment", &["pr", "comment", &n.to_string(), "--body", body], &[]).map(|_| ())
    }

    fn set_label(&self, n: u64, label: &str, on: bool) -> Result<(), ForgeError> {
        let flag = if on { "--add-label" } else { "--remove-label" };
        self.run("change a label", &["pr", "edit", &n.to_string(), flag, label], &[]).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_refs_and_paths_reach_a_url() {
        assert!(url_safe("main") && url_safe("worktree-spec-094/x") && url_safe("specs/094-core-repo-merge-flow.md"));
        for bad in ["", "a..b", "a b", "a?b=c", "a#b", "a%2e", "$(x)", "a;b", "\u{0}"] {
            assert!(!url_safe(bad), "{bad:?}");
        }
        assert!(!url_safe(&"a".repeat(301)));
    }

    #[test]
    fn file_at_refuses_an_unsafe_ref_before_running_anything() {
        let r = Gh.file_at("main?x=1", "specs/094.md");
        assert!(matches!(r, Err(ForgeError::Failed { .. })), "{r:?}");
    }
}
