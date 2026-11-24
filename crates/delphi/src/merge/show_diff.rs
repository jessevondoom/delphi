//! The diff a person reads before a gate-changing merge (spec 094 §3.4, §17), built locally with git.
//!
//! GitHub refuses to serve a large pull request's diff (about 300 files or 20,000 lines: "diff too large"), which made
//! `delphi merge` fail outright on #906. So the diff is never asked of GitHub: the derived head and base commits are fetched
//! from `origin` (GitHub's `pull/<n>/head`, then the branch), and `git diff <base>...<head>` is what the operator reads, the
//! same three-dot range CI and the merge use. A `--stat` summary always comes first. Over `FOLD_ABOVE` lines, the patches of
//! generated paths (`compiled/`, the per-slice tracker files) are folded and counted, with the command that shows them; a gate
//! path is never folded.
//!
//! This module is on the gate list (`merge-gates.toml`).

use super::cleanup::Git;
use super::forge::PrInfo;
use super::gates;

/// A patch longer than this many lines has its generated paths folded.
pub const FOLD_ABOVE: usize = 5000;

/// Paths that are generated or bulk: their patch may be folded in a large diff. Never a gate path.
pub const GENERATED: &[&str] = &["compiled/", "missions/2026-10-23-foundational-design/tracker/"];

/// Fetches are read-only; a stalled transfer gives up instead of holding the operator's terminal (git's own low-speed bound).
const BOUNDED: &[&str] = &["-c", "http.lowSpeedLimit=1000", "-c", "http.lowSpeedTime=60"];

fn is_sha(s: &str) -> bool {
    (7..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn generated(path: &str) -> bool {
    GENERATED.iter().any(|g| path.starts_with(g))
}

/// A gate path: one the verdict found on the base's list, or one on the built-in list (stricter, never weaker).
fn gate(path: &str, gate_hits: &[String]) -> bool {
    gate_hits.iter().any(|h| h == path) || gates::DEFAULT_GATES.iter().any(|p| gates::matches(p, path))
}

/// Make sure `sha` is in the local repository, fetching each refspec in turn until it is.
fn have(git: &Git, sha: &str, refspecs: &[String]) -> Result<(), String> {
    let present = |g: &Git| g.read(&["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_ok();
    if present(git) {
        return Ok(());
    }
    let mut tried = Vec::new();
    for spec in refspecs {
        let mut args: Vec<&str> = BOUNDED.to_vec();
        args.extend(["fetch", "--quiet", "--no-tags", "origin", spec.as_str()]);
        match git.read(&args) {
            Ok(_) if present(git) => return Ok(()),
            Ok(_) => tried.push(format!("{spec} (fetched, but it does not contain it)")),
            Err(e) => tried.push(format!("{spec} ({e})")),
        }
    }
    Err(format!("commit {sha} is not in this repository and could not be fetched from origin: {}; the branch may have moved, so run the merge again", tried.join("; ")))
}

/// The path of one `diff --git a/P b/P` header (no renames, so both sides name the same path). `None` for a header this cannot
/// read (a quoted name): such a patch is never folded.
fn chunk_path(header: &str) -> Option<String> {
    let rest = header.strip_prefix("diff --git a/")?;
    // `P b/P`: the length is 2*len(P) + 3.
    let n = rest.len().checked_sub(3)? / 2;
    if n == 0 || 2 * n + 3 != rest.len() {
        return None;
    }
    let (a, b) = (rest.get(..n)?, rest.get(n..)?);
    (b.strip_prefix(" b/") == Some(a)).then(|| a.to_string())
}

/// What `for_operator` built: the text to show and how much was folded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    pub lines: usize,
    pub folded_files: usize,
    pub folded_lines: usize,
}

/// Fold the generated paths of a long patch. `stat` comes first, always.
pub fn fold(stat: &str, patch: &str, range: &str, gate_hits: &[String]) -> Shown {
    let lines = patch.lines().count();
    if lines <= FOLD_ABOVE {
        return Shown { text: format!("{}\n\n{patch}", stat.trim_end()), lines, folded_files: 0, folded_lines: 0 };
    }
    let mut kept = String::new();
    let (mut folded_files, mut folded_lines) = (0, 0);
    let mut chunk = String::new();
    let mut flush = |chunk: &mut String| {
        if chunk.is_empty() {
            return;
        }
        let path = chunk.lines().next().and_then(chunk_path);
        match path {
            Some(p) if generated(&p) && !gate(&p, gate_hits) => {
                folded_files += 1;
                folded_lines += chunk.lines().count();
            }
            _ => kept.push_str(chunk),
        }
        chunk.clear();
    };
    for line in patch.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            flush(&mut chunk);
        }
        chunk.push_str(line);
    }
    flush(&mut chunk);
    let mut text = format!("{}\n\n", stat.trim_end());
    if folded_files > 0 {
        text.push_str(&format!(
            "The patch has {lines} lines; the {folded_lines} lines of {folded_files} generated file(s) under {} are folded below (every other path, and every gate path, is shown in full). To read them:\n  git diff {range} -- {}\n\n",
            GENERATED.join(" and "),
            GENERATED.join(" ")
        ));
    }
    text.push_str(&kept);
    if folded_files > 0 {
        text.push_str(&format!("\n[{folded_lines} generated line(s) in {folded_files} file(s) folded: git diff {range} -- {}]\n", GENERATED.join(" ")));
    }
    Shown { text, lines, folded_files, folded_lines }
}

/// Build the diff of pull request `pr` from its derived base and head commits, locally. Nothing here asks GitHub for a diff.
pub fn for_operator(git: &Git, pr: &PrInfo, gate_hits: &[String]) -> Result<Shown, String> {
    let (base, head) = (pr.base_sha.as_str(), pr.head_sha.as_str());
    if !is_sha(base) || !is_sha(head) {
        return Err(format!("the derived base `{base}` or head `{head}` is not a commit id"));
    }
    let mut head_specs = vec![format!("pull/{}/head", pr.number)];
    if !pr.cross_repo {
        head_specs.push(format!("refs/heads/{}", pr.head_ref));
    }
    have(git, head, &head_specs)?;
    have(git, base, &[format!("refs/heads/{}", pr.base_ref)])?;
    let range = format!("{base}...{head}");
    let opts = ["-c", "core.quotePath=false", "diff", "--no-color", "--no-ext-diff", "--no-renames"];
    let with = |extra: &[&str]| -> Result<String, String> {
        let mut args: Vec<&str> = opts.to_vec();
        args.extend(extra);
        args.extend([range.as_str(), "--"]);
        git.read(&args)
    };
    let stat = with(&["--stat=160,120"])?;
    let patch = with(&[])?;
    Ok(fold(&stat, &patch, &range, gate_hits))
}
