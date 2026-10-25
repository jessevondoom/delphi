//! What each model path can actually do (`specs/020` §4).
//!
//! A role that dispatches work (the orchestrator) chooses a family and a tier, and Delphi's router then picks the
//! endpoint. What a task can do depends on that endpoint: an API seat has every Delphi tool; a Claude subscription
//! seat has them over the MCP bridge plus whatever its profile adds; an `agy` seat is advice only. Without this the
//! orchestrator plans from a belief, which is how a text-only seat was asked to set timers.
//!
//! Two sources feed [`render`]: the ratified configuration (what Delphi will start and offer) and the record each
//! CLI session leaves of what it really reported at start-up (`system/init`: tool names and MCP server states), so a
//! tool an organization's policy removed shows up as missing rather than assumed.

use delphi_adapter::config::{AccessPath, ApprovalSource, Config, Endpoint, HarnessProfile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// What one subscription CLI reported when it last started a span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeatRecord {
    pub endpoint_id: String,
    pub harness: String,
    pub model: String,
    /// The tool names the CLI offered the model.
    pub tools: Vec<String>,
    /// `(server, status)` for every MCP server the CLI loaded.
    pub mcp_servers: Vec<(String, String)>,
    /// Did this launch carry the Delphi bridge?
    pub bridge: bool,
    pub seen_unix: i64,
}

/// The records, kept in `local/harness-tools.json` so the picture survives a daemon restart.
pub struct Inventory {
    path: Option<PathBuf>,
    seats: Mutex<BTreeMap<String, SeatRecord>>,
}

impl Inventory {
    pub fn memory() -> Inventory {
        Inventory { path: None, seats: Mutex::new(BTreeMap::new()) }
    }

    /// Load the file if it is there and parses; anything else starts empty (the record is a cache of what the CLIs said).
    pub fn open(path: PathBuf) -> Inventory {
        let seats = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Vec<SeatRecord>>(&t).ok()).unwrap_or_default();
        Inventory { path: Some(path), seats: Mutex::new(seats.into_iter().map(|s| (s.endpoint_id.clone(), s)).collect()) }
    }

    /// Take the file's content, if there is a file that parses: `delphi endpoints test --probe` writes it from another process.
    fn refresh(&self, seats: &mut BTreeMap<String, SeatRecord>) {
        if let Some(list) = self.path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<Vec<SeatRecord>>(&t).ok()) {
            *seats = list.into_iter().map(|s| (s.endpoint_id.clone(), s)).collect();
        }
    }

    /// Remember a session's start-up report. Written only when it changed, so a busy daemon does not rewrite the file per span.
    pub fn record(&self, rec: SeatRecord) {
        let mut seats = self.seats.lock().unwrap();
        self.refresh(&mut seats);
        let same = seats.get(&rec.endpoint_id).is_some_and(|old| SeatRecord { seen_unix: 0, ..old.clone() } == SeatRecord { seen_unix: 0, ..rec.clone() });
        if same {
            return;
        }
        seats.insert(rec.endpoint_id.clone(), rec);
        if let Some(p) = &self.path {
            let list: Vec<&SeatRecord> = seats.values().collect();
            if let Ok(text) = serde_json::to_vec_pretty(&list) {
                let _ = delphi_core::lock::write_atomic(p, &text);
            }
        }
    }

    pub fn seats(&self) -> Vec<SeatRecord> {
        let mut seats = self.seats.lock().unwrap();
        self.refresh(&mut seats);
        seats.values().cloned().collect()
    }
}

/// Names shown for a long tool list: the first few, then a count.
fn brief(names: &[String], max: usize) -> String {
    if names.len() <= max {
        names.join(", ")
    } else {
        format!("{}, and {} more", names[..max].join(", "), names.len() - max)
    }
}

fn models(e: &Endpoint) -> String {
    ["fast", "workhorse", "frontier"].iter().filter_map(|t| e.models.get(*t).map(|m| format!("{}={}", t, m))).collect::<Vec<_>>().join(", ")
}

fn claude_seat(e: &Endpoint, p: &HarnessProfile, seen: Option<&SeatRecord>) -> String {
    let mut s = format!("- `{}` (family claude; Claude Code signed in to the organization's or your subscription{}):\n", e.id, if models(e).is_empty() { String::new() } else { format!("; models {}", models(e)) });
    if p.bridge {
        s.push_str("  - Delphi tools: YES, over the MCP bridge (they appear to the model as `mcp__delphi__<tool>`). A role gets the same Delphi tools it would get on an API seat, each call run under Delphi's own classification, taint gates and approvals. Builders use the CLI's own file and shell tools instead of Delphi's file-writing and shell tools.\n");
    } else {
        s.push_str("  - Delphi tools: NO (the bridge is off in `[harness.claude]`). Roles without a worktree run text-only here: no timer, no dispatch, no file or connector tools.\n");
    }
    if p.native_tools.is_empty() {
        s.push_str("  - CLI built-in tools for roles without a worktree: none.\n");
    } else {
        s.push_str(&format!("  - CLI built-in tools for roles without a worktree: {}. Content they fetch is untrusted (L0).\n", p.native_tools.join(", ")));
    }
    s.push_str("  - Roles with a worktree and shell (Builders): the CLI's standard tools, every request answered by Delphi's rules.\n");
    if p.claudeai_connectors {
        s.push_str("  - Connectors: the CLI's own MCP servers load: the signed-in account's connectors (set up by the organization) and any you configured for the CLI. Anything they return is untrusted (L0); the organization's policy decides which exist, and the list below shows which loaded.\n");
    } else {
        s.push_str("  - Connectors: the account's own connectors are OFF here. Delphi's native connectors (`[connectors.*]`) still work through the bridge.\n");
    }
    s.push_str(&match p.approvals {
        ApprovalSource::Delegate => "  - Approvals: delegated. The CLI runs in auto mode, so the organization's policy and the CLI's own classifier answer first; Delphi asks the operator only for what they cannot settle. Delphi's own tools and Builders stay on Delphi's rules.\n".to_string(),
        ApprovalSource::Delphi => "  - Approvals: Delphi answers every request.\n".to_string(),
    });
    match seen {
        Some(r) => {
            let others: Vec<String> = r.mcp_servers.iter().map(|(n, st)| format!("{} ({})", n, st)).collect();
            s.push_str(&format!("  - Last session reported {} tool(s): {}. MCP servers: {}.\n", r.tools.len(), brief(&r.tools, 24), if others.is_empty() { "none".into() } else { others.join(", ") }));
        }
        None => s.push_str("  - No session has reported its tools yet; the lists above are what Delphi will ask for, not yet confirmed.\n"),
    }
    s
}

fn agy_seat(e: &Endpoint, seen: Option<&SeatRecord>) -> String {
    let mut s = format!("- `{}` (family gemini; the Antigravity CLI `agy`{}): ADVICE-ONLY.\n", e.id, if models(e).is_empty() { String::new() } else { format!("; models {}", models(e)) });
    s.push_str("  - No Delphi tool reaches it: it has no way to load Delphi's bridge or ask Delphi for approval. Do not give it work that needs `timer`, `dispatch_agent`, scoped file access or connectors, and never a Builder task.\n");
    s.push_str("  - It brings its own tools (browser, web search, URL reading, shell, file edit, image generation, scheduling, MCP calls) and they cannot be turned off. Delphi therefore runs it in an empty directory, treats everything it returns as untrusted (L0) and discards the run if a project checkout changed.\n");
    s.push_str("  - Good for: a second opinion from another model family, Council seats, red-team review of text you quote to it.\n");
    match seen {
        Some(r) => s.push_str(&format!("  - Last session reported {} tool(s): {}.\n", r.tools.len(), brief(&r.tools, 20))),
        None => s.push_str("  - No session has reported its tools yet.\n"),
    }
    s
}

fn api_seat(e: &Endpoint) -> String {
    let how = match e.path {
        AccessPath::Api => "direct API",
        AccessPath::Cloud => "cloud platform",
        AccessPath::GatewayNative => "gateway (native)",
        AccessPath::GatewayCompat => "gateway (compat; reasoning continuity is degraded)",
        AccessPath::Harness => "harness",
    };
    format!("- `{}` (family {}; {}{}): FULL. Every Delphi tool the role is entitled to, turn by turn, each call under Delphi's approvals.\n", e.id, e.family, how, if models(e).is_empty() { String::new() } else { format!("; models {}", models(e)) })
}

/// The text given to a role that dispatches work. `seen` are the CLIs' own start-up reports.
pub fn render(cfg: &Config, seen: &[SeatRecord]) -> String {
    let mut out = String::from("Tools by model path, from the ratified configuration and what each CLI last reported. Delphi chooses the endpoint for a task from the family and tier you ask for; plan around what that path can do.\n\n");
    let mut any = false;
    for e in cfg.endpoints.iter().filter(|e| cfg.is_allowed(&e.id)) {
        any = true;
        let rec = seen.iter().find(|r| r.endpoint_id == e.id);
        out.push_str(&match (e.path, e.harness.as_deref().unwrap_or("claude")) {
            (AccessPath::Harness, "claude") => claude_seat(e, &cfg.harness.get("claude").cloned().unwrap_or_default(), rec),
            (AccessPath::Harness, "agy") => agy_seat(e, rec),
            (AccessPath::Harness, other) => format!("- `{}` (harness `{}`): not supported; Delphi cannot run it.\n", e.id, other),
            _ => api_seat(e),
        });
    }
    if !any {
        out.push_str("(no endpoint is configured)\n");
    }
    out.push_str("\nWhen a family has more than one path, the ratified order in `[access.order]` decides which is tried first, and a path that is out of quota hands the task to the next. A task that has already acted is never retried elsewhere.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"
[[endpoints]]
id = "claude-sub"
family = "claude"
path = "harness"
harness = "claude"
credential = "harness-login"
cost_model = "subscription"
models = { fast = "haiku", workhorse = "sonnet", frontier = "opus" }

[[endpoints]]
id = "agy-sub"
family = "gemini"
path = "harness"
harness = "agy"
credential = "harness-login"
cost_model = "subscription"
models = { fast = "gemini-3.8-flash-low" }

[access]
allowed_endpoints = ["claude-sub", "agy-sub"]
[access.order]
claude = ["claude-sub"]
gemini = ["agy-sub"]
"#;

    fn seat(id: &str, tools: &[&str]) -> SeatRecord {
        SeatRecord { endpoint_id: id.into(), harness: "claude".into(), model: "opus".into(), tools: tools.iter().map(|s| s.to_string()).collect(), mcp_servers: vec![("delphi".into(), "connected".into())], bridge: true, seen_unix: 1 }
    }

    #[test]
    fn the_orchestrator_is_told_which_seat_has_which_tools_by_default() {
        let cfg = Config::parse(BASE).unwrap();
        let text = render(&cfg, &[]);
        assert!(text.contains("`claude-sub`") && text.contains("Delphi tools: YES, over the MCP bridge") && text.contains("mcp__delphi__"));
        assert!(text.contains("CLI built-in tools for roles without a worktree: WebSearch, WebFetch. Content they fetch is untrusted"));
        assert!(text.contains("the CLI's own MCP servers load"));
        assert!(text.contains("Approvals: delegated") && text.contains("Builders stay on Delphi's rules"));
        assert!(text.contains("`agy-sub`") && text.contains("ADVICE-ONLY") && text.contains("never a Builder task"));
        assert!(text.contains("No session has reported its tools yet"));
    }

    #[test]
    fn a_narrowed_profile_changes_what_the_seat_claims_and_what_the_cli_reported_is_shown() {
        let cfg = Config::parse(&format!("{}\n[harness.claude]\napprovals = \"delphi\"\nnative_tools = []\nclaudeai_connectors = false\n", BASE)).unwrap();
        let text = render(&cfg, &[seat("claude-sub", &["mcp__delphi__timer", "WebSearch"])]);
        assert!(text.contains("CLI built-in tools for roles without a worktree: none"));
        assert!(text.contains("the account's own connectors are OFF"));
        assert!(text.contains("Approvals: Delphi answers every request"));
        assert!(text.contains("Last session reported 2 tool(s): mcp__delphi__timer, WebSearch. MCP servers: delphi (connected)"), "{}", text);
        let off = Config::parse(&format!("{}\n[harness.claude]\nbridge = false\n", BASE)).unwrap();
        assert!(render(&off, &[]).contains("Delphi tools: NO (the bridge is off"));
    }

    #[test]
    fn an_api_endpoint_is_full_and_an_endpoint_outside_the_pin_is_not_mentioned() {
        let cfg = Config::parse(&format!("{}\n[[endpoints]]\nid = \"direct\"\nfamily = \"claude\"\npath = \"api\"\ncredential = \"env:K\"\n", BASE)).unwrap();
        assert!(!render(&cfg, &[]).contains("`direct`"), "not in allowed_endpoints");
        let all = Config::parse(&BASE.replace("allowed_endpoints = [\"claude-sub\", \"agy-sub\"]", "allowed_endpoints = [\"claude-sub\", \"agy-sub\", \"direct\"]").replace("[access.order]", "[[endpoints]]\nid = \"direct\"\nfamily = \"claude\"\npath = \"api\"\ncredential = \"env:K\"\n[access.order]")).unwrap();
        assert!(render(&all, &[]).contains("`direct` (family claude; direct API): FULL"));
    }

    #[test]
    fn records_persist_and_an_unchanged_report_does_not_rewrite_the_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("harness-tools.json");
        let inv = Inventory::open(p.clone());
        inv.record(seat("claude-sub", &["a", "b"]));
        let first = std::fs::metadata(&p).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        inv.record(SeatRecord { seen_unix: 99, ..seat("claude-sub", &["a", "b"]) });
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), first, "same tools, same servers: nothing to write");
        inv.record(seat("claude-sub", &["a", "b", "c"]));
        assert_eq!(Inventory::open(p).seats()[0].tools.len(), 3);
        std::fs::write(d.path().join("bad.json"), "not json").unwrap();
        assert!(Inventory::open(d.path().join("bad.json")).seats().is_empty(), "a damaged cache starts empty");
    }

    #[test]
    fn long_tool_lists_are_shortened() {
        let names: Vec<String> = (0..60).map(|i| format!("t{}", i)).collect();
        let b = brief(&names, 24);
        assert!(b.ends_with("and 36 more") && b.starts_with("t0, t1"));
    }
}
