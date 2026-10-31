//! A verb heuristic for "is this approval card a merge?" (`specs/014` amendment 014-A1, REV-SEC-09).
//!
//! The finalize card (`merge`, `pull_request`) is exact. A merge can also be reached through a Red `harness_bash`,
//! `run_command` or connector card, so a relay may not approve one whose command looks like it merges, pushes to a
//! protected branch or rebases. This reads words, not intent: a determined command can evade it, which is why the
//! console stays the only complete boundary. It only ever widens what the relay refuses; denying is never affected.

use serde_json::Value;

const PROTECTED: [&str; 5] = ["main", "master", "trunk", "develop", "production"];

/// Why a relay may not approve this card, or `None`.
pub fn console_only(tool: &str, payload: &Value) -> Option<String> {
    match tool {
        "merge" | "pull_request" => Some(format!("a {}", tool)),
        "harness_bash" => payload["command"].as_str().and_then(command_line).map(|v| format!("a command that looks like `{}`", v)),
        "run_command" => {
            let argv: Vec<String> = payload["argv"].as_array()?.iter().filter_map(|a| a.as_str().map(str::to_string)).collect();
            scan(&argv).map(|v| format!("a command that looks like `{}`", v))
        }
        "connector" => {
            let name = payload["tool"].as_str().unwrap_or("");
            name.to_ascii_lowercase().contains("merge").then(|| format!("a connector call to `{}`", name))
        }
        _ => None,
    }
}

fn command_line(line: &str) -> Option<String> {
    // Split at anything a shell would treat as a separator, so `a && git merge x` and `bash -c 'git merge x'` both
    // expose their words; then scan every word position, not only the first.
    let words: Vec<String> = line
        .split(|c: char| c.is_whitespace() || ";|&()<>`${}\\".contains(c))
        .map(|w| w.trim_matches(|c| c == '\'' || c == '"').to_string())
        .filter(|w| !w.is_empty())
        .collect();
    scan(&words)
}

fn scan(words: &[String]) -> Option<String> {
    let base = |w: &str| w.rsplit('/').next().unwrap_or(w).to_ascii_lowercase();
    for (i, w) in words.iter().enumerate() {
        let rest: Vec<String> = words[i + 1..].iter().map(|x| x.to_ascii_lowercase()).take_while(|x| !x.is_empty()).collect();
        match base(w).as_str() {
            "git" => {
                if let Some(v) = git(&rest) {
                    return Some(v);
                }
            }
            "gh" => {
                if let Some(v) = gh(&rest) {
                    return Some(v);
                }
            }
            _ => {}
        }
    }
    None
}

fn protected(r: &str) -> bool {
    let dest = r.rsplit(':').next().unwrap_or(r).trim_start_matches('+');
    let dest = dest.strip_prefix("refs/heads/").unwrap_or(dest);
    let dest = dest.strip_prefix("origin/").unwrap_or(dest);
    PROTECTED.contains(&dest) || dest.starts_with("release") || dest == "head"
}

fn git(rest: &[String]) -> Option<String> {
    let mut i = 0;
    while i < rest.len() && rest[i].starts_with('-') {
        i += if matches!(rest[i].as_str(), "-c" | "-C" | "--git-dir" | "--work-tree") { 2 } else { 1 };
    }
    let sub = rest.get(i)?.as_str();
    let args = &rest[i + 1..];
    let positional: Vec<&String> = args.iter().take_while(|a| !a.contains("&&")).filter(|a| !a.starts_with('-')).collect();
    match sub {
        "merge" => Some("git merge".into()),
        "rebase" if positional.is_empty() || positional.iter().any(|a| protected(a)) => Some("git rebase onto a protected or unnamed branch".into()),
        "push" => {
            let broad = args.iter().any(|a| matches!(a.as_str(), "--all" | "--mirror" | "--force" | "-f" | "--force-with-lease" | "--delete" | "-d") || a.starts_with("--force"));
            // `git push`, `git push origin`: the destination is implicit, so it could be the protected branch.
            (positional.len() < 2 || broad || positional.iter().skip(1).any(|a| protected(a))).then(|| "git push to a protected or unnamed branch".into())
        }
        _ => None,
    }
}

fn gh(rest: &[String]) -> Option<String> {
    let subs: Vec<&String> = rest.iter().filter(|a| !a.starts_with('-')).collect();
    let first = subs.first().map(|s| s.as_str());
    let second = subs.get(1).map(|s| s.as_str());
    match (first, second) {
        (Some("pr"), Some("merge")) => Some("gh pr merge".into()),
        (Some("pr"), Some("create")) if rest.iter().any(|a| a.contains("merge") || a == "--auto") => Some("gh pr create with merge flags".into()),
        (Some("api"), _) if rest.iter().any(|a| a.contains("merge")) => Some("gh api merge".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bash(line: &str) -> Option<String> {
        console_only("harness_bash", &json!({"tool": "Bash", "command": line}))
    }

    #[test]
    fn merge_class_commands_are_found_through_wrappers_and_flags() {
        for line in [
            "git merge feature",
            "git -C repo merge feature",
            "cd x && /usr/bin/git merge --no-ff y",
            "bash -c 'git merge x'",
            "GH_TOKEN=1 gh pr merge 12 --squash",
            "gh pr merge",
            "gh pr create --fill --auto",
            "gh api repos/o/r/pulls/3/merge -X PUT",
            "gh api graphql -f query=mutation{mergePullRequest(input:{})}",
            "git push origin main",
            "git push origin HEAD:refs/heads/main",
            "git push --force origin topic",
            "git push",
            "git rebase main",
            "git rebase",
        ] {
            assert!(bash(line).is_some(), "{}", line);
        }
    }

    #[test]
    fn ordinary_commands_and_unprotected_pushes_are_left_alone() {
        for line in ["cargo test", "git status", "git diff main", "git push origin topic/x", "git log --merges", "gh pr view 12", "gh pr create --fill", "ls merge"] {
            assert!(bash(line).is_none(), "{}", line);
        }
    }

    #[test]
    fn the_other_card_shapes() {
        assert!(console_only("run_command", &json!({"argv": ["git", "merge", "x"]})).is_some());
        assert!(console_only("run_command", &json!({"argv": ["cargo", "test"]})).is_none());
        assert!(console_only("connector", &json!({"tool": "merge_pull_request"})).is_some());
        assert!(console_only("connector", &json!({"tool": "send"})).is_none());
        assert!(console_only("pull_request", &json!({})).is_some());
    }
}
