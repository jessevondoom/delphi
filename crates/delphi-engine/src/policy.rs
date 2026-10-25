//! Does the organization's managed Claude Code policy let Delphi's tool bridge load? (`specs/020` §7)
//!
//! An organization that signs its people in through SSO can also push managed settings to the CLI. Those apply even
//! to a CLI started with `--setting-sources ""`, and `allowedMcpServers` / `deniedMcpServers` /
//! `allowManagedMcpServersOnly` can keep an MCP server out. The bridge is an MCP server named `delphi`, started with
//! the command `[<delphi executable>, "__mcp-bridge"]` (the same for every span, so it can be pinned once).
//!
//! This reads only the policy files the CLI itself reads from the machine (the same locations its own binary names:
//! `/Library/Application Support/ClaudeCode/managed-settings.json` on macOS, `/etc/claude-code/managed-settings.json`
//! elsewhere, plus a `managed-settings.d/` drop-in directory, or the file named by `CLAUDE_CODE_MANAGED_SETTINGS_PATH`).
//! It never reads a credential. A policy delivered from the organization's admin console is not on disk, so
//! "no local policy" is not proof that nothing applies: only starting the CLI shows that (`delphi endpoints test --probe`).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpPolicy {
    /// No policy file on this machine.
    NoLocalPolicy,
    /// A policy exists and does not keep the bridge out.
    Allowed,
    /// The bridge is explicitly denied.
    Denied(String),
    /// An allowlist exists and does not list the bridge.
    NotListed,
}

/// The policy files that exist on this machine, in the order they apply.
pub fn policy_files() -> Vec<PathBuf> {
    let mut main = std::env::var_os("CLAUDE_CODE_MANAGED_SETTINGS_PATH").map(PathBuf::from).unwrap_or_else(|| {
        if cfg!(target_os = "macos") {
            PathBuf::from("/Library/Application Support/ClaudeCode/managed-settings.json")
        } else {
            PathBuf::from("/etc/claude-code/managed-settings.json")
        }
    });
    if main.is_dir() {
        main = main.join("managed-settings.json");
    }
    let mut out = Vec::new();
    if main.is_file() {
        out.push(main.clone());
    }
    if let Some(dir) = main.parent().map(|p| p.join("managed-settings.d")) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            let mut drop_ins: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
            drop_ins.sort();
            out.extend(drop_ins);
        }
    }
    out
}

/// Read and evaluate the policy on this machine for the bridge started by `exe`.
pub fn local_policy(exe: &Path) -> McpPolicy {
    let docs: Vec<Value> = policy_files().iter().filter_map(|p| std::fs::read_to_string(p).ok()).filter_map(|t| serde_json::from_str(&t).ok()).collect();
    if docs.is_empty() {
        return McpPolicy::NoLocalPolicy;
    }
    evaluate(&docs, exe)
}

fn entry_matches(entry: &Value, exe: &Path) -> bool {
    if entry["serverName"].as_str() == Some(crate::bridge::SERVER_NAME) {
        return true;
    }
    match entry["serverCommand"].as_array() {
        Some(cmd) => cmd.len() == 2 && cmd[0].as_str() == Some(&exe.display().to_string()) && cmd[1].as_str() == Some("__mcp-bridge"),
        None => false,
    }
}

/// The merged policy of `docs` (later files add to earlier ones) applied to the bridge.
pub fn evaluate(docs: &[Value], exe: &Path) -> McpPolicy {
    let list = |key: &str| -> Option<Vec<Value>> {
        let mut found = None;
        for d in docs {
            if let Some(a) = d[key].as_array() {
                found.get_or_insert_with(Vec::new).extend(a.iter().cloned());
            }
        }
        found
    };
    if let Some(denied) = list("deniedMcpServers") {
        if denied.iter().any(|e| entry_matches(e, exe)) {
            return McpPolicy::Denied("deniedMcpServers lists it".into());
        }
    }
    match list("allowedMcpServers") {
        Some(allowed) if !allowed.iter().any(|e| entry_matches(e, exe)) => McpPolicy::NotListed,
        _ => McpPolicy::Allowed,
    }
}

/// What to give the administrator: the entry that allows the bridge, by name (any server named `delphi`) or by exact command.
pub fn admin_snippet(exe: &Path) -> String {
    serde_json::to_string_pretty(&json!({"allowedMcpServers": [{"serverName": crate::bridge::SERVER_NAME}, {"serverCommand": [exe.display().to_string(), "__mcp-bridge"]}]})).unwrap_or_default()
}

/// A sentence for `doctor` and the probe, or `None` when there is nothing to say.
pub fn explain(policy: &McpPolicy, exe: &Path) -> Option<String> {
    match policy {
        McpPolicy::NoLocalPolicy => None,
        McpPolicy::Allowed => None,
        McpPolicy::Denied(why) => Some(format!("the organization's managed settings deny the Delphi tool bridge ({}); roles that need tools cannot run on this seat until an administrator removes the `delphi` entry", why)),
        McpPolicy::NotListed => Some(format!("the organization's managed settings allow only listed MCP servers and the Delphi tool bridge is not listed; ask an administrator to add it to `allowedMcpServers` (either entry works; the command form pins this executable):\n{}", admin_snippet(exe))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        PathBuf::from("/opt/delphi/bin/delphi")
    }

    #[test]
    fn no_allowlist_means_the_bridge_is_not_kept_out() {
        assert_eq!(evaluate(&[json!({"permissions": {"deny": ["Bash(rm:*)"]}})], &exe()), McpPolicy::Allowed);
        assert_eq!(evaluate(&[json!({"allowManagedMcpServersOnly": true})], &exe()), McpPolicy::Allowed, "managed-only with no list of its own restricts nothing here");
    }

    #[test]
    fn an_allowlist_must_name_the_bridge_by_name_or_by_its_exact_command() {
        let other = json!({"allowedMcpServers": [{"serverName": "github"}]});
        assert_eq!(evaluate(std::slice::from_ref(&other), &exe()), McpPolicy::NotListed);
        assert_eq!(evaluate(&[json!({"allowedMcpServers": [{"serverName": "github"}, {"serverName": "delphi"}]})], &exe()), McpPolicy::Allowed);
        assert_eq!(evaluate(&[json!({"allowedMcpServers": [{"serverCommand": ["/opt/delphi/bin/delphi", "__mcp-bridge"]}]})], &exe()), McpPolicy::Allowed);
        assert_eq!(evaluate(&[json!({"allowedMcpServers": [{"serverCommand": ["/elsewhere/delphi", "__mcp-bridge"]}]})], &exe()), McpPolicy::NotListed, "another executable is not this one");
        assert_eq!(evaluate(&[json!({"allowedMcpServers": []})], &exe()), McpPolicy::NotListed, "an empty allowlist allows nothing");
        // Drop-in files add to the main file.
        assert_eq!(evaluate(&[other, json!({"allowedMcpServers": [{"serverName": "delphi"}]})], &exe()), McpPolicy::Allowed);
    }

    #[test]
    fn a_denial_wins_over_an_allowance() {
        let both = json!({"allowedMcpServers": [{"serverName": "delphi"}], "deniedMcpServers": [{"serverName": "delphi"}]});
        assert!(matches!(evaluate(&[both], &exe()), McpPolicy::Denied(_)));
    }

    #[test]
    fn the_explanation_carries_a_snippet_an_administrator_can_paste() {
        assert_eq!(explain(&McpPolicy::NoLocalPolicy, &exe()), None);
        assert_eq!(explain(&McpPolicy::Allowed, &exe()), None);
        let text = explain(&McpPolicy::NotListed, &exe()).unwrap();
        let json_start = text.find('{').unwrap();
        let snippet: Value = serde_json::from_str(&text[json_start..]).unwrap();
        assert_eq!(snippet["allowedMcpServers"][0]["serverName"], "delphi");
        assert_eq!(snippet["allowedMcpServers"][1]["serverCommand"], json!(["/opt/delphi/bin/delphi", "__mcp-bridge"]));
        assert!(explain(&McpPolicy::Denied("x".into()), &exe()).unwrap().contains("deny the Delphi tool bridge"));
    }

    #[test]
    fn files_are_read_from_the_named_path_and_its_drop_in_directory() {
        let d = tempfile::tempdir().unwrap();
        let main = d.path().join("managed-settings.json");
        std::fs::write(&main, r#"{"allowedMcpServers":[{"serverName":"github"}]}"#).unwrap();
        std::fs::create_dir(d.path().join("managed-settings.d")).unwrap();
        std::fs::write(d.path().join("managed-settings.d/10-delphi.json"), r#"{"allowedMcpServers":[{"serverName":"delphi"}]}"#).unwrap();
        std::fs::write(d.path().join("managed-settings.d/readme.txt"), "ignored").unwrap();
        // SAFETY: tests that read this variable run in this one test.
        unsafe { std::env::set_var("CLAUDE_CODE_MANAGED_SETTINGS_PATH", &main) };
        let files = policy_files();
        let policy = local_policy(&exe());
        unsafe { std::env::remove_var("CLAUDE_CODE_MANAGED_SETTINGS_PATH") };
        assert_eq!(files.len(), 2);
        assert_eq!(policy, McpPolicy::Allowed);
    }
}
