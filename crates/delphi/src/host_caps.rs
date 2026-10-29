//! `delphi capabilities`: what each model CLI on this machine says it can use (`specs/038`).
//!
//! The operator-level map of connections and tools. For every CLI Delphi can drive (`claude`, `agy`, `gemini`, `ollama`) it asks
//! the CLI's own list commands (MCP servers, plugins, models, extensions, agents), reads the skill and agent directories on
//! disk, and keeps one snapshot in `<home>/capabilities.json`. Every entry says where it came from: `cli` (the CLI reported it),
//! `disk` (a directory scan) or `model` (a model was asked and answered, which can be wrong). No model is called unless `--ask`.
//!
//! Setup and update start a refresh in the background (`refresh_in_background`) so the map is there when it is wanted. A CLI that
//! is missing, signed out or broken is recorded as such, with the reason; it never fails the command.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROBE_LIMIT: Duration = Duration::from_secs(45);
const ASK_LIMIT: Duration = Duration::from_secs(180);
const ASK_PROMPT: &str = "List every tool, skill and connector (MCP server) you can use in this session, one per line, as `kind: name`. No commentary.";
/// A snapshot younger than this is not refreshed again by a background start.
const FRESH_SECS: i64 = 60;

// The snapshot and its file live in the engine, so the orchestrator's `external_available` tool reads the same thing.
pub use delphi_engine::hostmap::{load, path, save, CliEntry, Item, ProjectEntry, Snapshot};

#[derive(Debug, Clone, PartialEq)]
pub enum Ran {
    Done { code: i32, out: String, err: String },
    Missing,
    TimedOut,
}

/// The machine, as far as discovery needs it.
pub trait Runner {
    fn run(&self, program: &str, args: &[&str], limit: Duration) -> Ran;
    fn which(&self, program: &str) -> Option<PathBuf>;
}

pub struct RealRunner;

impl Runner for RealRunner {
    fn run(&self, program: &str, args: &[&str], limit: Duration) -> Ran {
        let Ok(mut child) = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() else { return Ran::Missing };
        let drain = |s: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut s) = s {
                    let _ = s.read_to_end(&mut buf);
                }
                String::from_utf8_lossy(&buf).into_owned()
            })
        };
        let out = drain(child.stdout.take().map(|s| Box::new(s) as _));
        let err = drain(child.stderr.take().map(|s| Box::new(s) as _));
        let deadline = Instant::now() + limit;
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(40)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
        match status {
            Some(s) => Ran::Done { code: s.code().unwrap_or(-1), out, err },
            None => Ran::TimedOut,
        }
    }

    fn which(&self, program: &str) -> Option<PathBuf> {
        std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(program)).find(|p| p.is_file())
    }
}

fn first_line(s: &str) -> String {
    let l = s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("no message");
    l.chars().take(160).collect()
}

fn item(name: impl Into<String>, detail: impl Into<String>, source: &str) -> Item {
    Item { name: name.into(), detail: detail.into(), source: source.into() }
}

/// `claude mcp list`: `name: target - ✔ Connected`, after a "Checking…" line.
pub fn parse_claude_mcp(out: &str) -> Vec<Item> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("Checking"))
        .filter_map(|l| {
            let (name, rest) = l.split_once(": ")?;
            let (target, status) = rest.rsplit_once(" - ").unwrap_or((rest, ""));
            let status = status.trim_start_matches(|c: char| !c.is_alphanumeric());
            Some(item(name, if status.is_empty() { target.to_string() } else { format!("{target} · {status}") }, "cli"))
        })
        .collect()
}

/// `claude plugin list --json`: an array of objects. Tolerant of the field names.
pub fn parse_claude_plugins(out: &str) -> Vec<Item> {
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str::<serde_json::Value>(out) else { return vec![] };
    list.iter()
        .filter_map(|p| {
            let name = p["name"].as_str().or_else(|| p["id"].as_str())?;
            let mut detail = vec![];
            if let Some(v) = p["version"].as_str() {
                detail.push(v.to_string());
            }
            if p["enabled"] == false {
                detail.push("disabled".into());
            }
            Some(item(name, detail.join(" · "), "cli"))
        })
        .collect()
}

/// `agy models`: `id<TAB>label`, after a "Fetching…" line.
pub fn parse_tab_models(out: &str) -> Vec<Item> {
    out.lines().filter_map(|l| l.split_once('\t')).map(|(id, label)| item(id.trim(), label.trim(), "cli")).collect()
}

/// `ollama list`: a table; the first column is the model.
pub fn parse_ollama_list(out: &str) -> Vec<Item> {
    out.lines()
        .skip(1)
        .filter_map(|l| {
            let c: Vec<&str> = l.split_whitespace().collect();
            let name = c.first()?;
            Some(item(*name, if c.len() >= 4 { format!("{} {}", c[2], c[3]) } else { String::new() }, "cli"))
        })
        .collect()
}

/// A list whose format Delphi has not pinned: one item per line, and "No … configured/installed" means none.
pub fn parse_plain_list(out: &str) -> Vec<Item> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| {
            let low = l.to_lowercase();
            !(low.starts_with("no ") && ["configured", "installed", "found", "available"].iter().any(|w| low.contains(w)))
        })
        .map(|l| item(l, "", "cli"))
        .collect()
}

/// An `mcpServers` map from a config file (`~/.gemini/config/mcp_config.json`, `~/.gemini/settings.json`, a project `.mcp.json`).
/// Keeps the name and where it points (a URL without its query, or the command alone); never `env`, `headers` or `args`, which can
/// carry secrets.
pub fn parse_mcp_config(text: &str) -> Vec<Item> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return vec![] };
    let Some(map) = v["mcpServers"].as_object() else { return vec![] };
    map.iter()
        .map(|(name, c)| {
            let detail = match (c["url"].as_str().or_else(|| c["httpUrl"].as_str()), c["command"].as_str()) {
                (Some(u), _) => u.split(['?', '#']).next().unwrap_or(u).to_string(),
                (None, Some(cmd)) => cmd.to_string(),
                _ => String::new(),
            };
            let off = if c["disabled"] == true || c["enabled"] == false { " · disabled" } else { "" };
            item(name, format!("{detail}{off}"), "disk")
        })
        .collect()
}

/// Add the servers a config file names that the CLI's own list did not.
fn add_config(into: &mut Vec<Item>, file: &Path) {
    let Ok(text) = std::fs::read_to_string(file) else { return };
    for i in parse_mcp_config(&text) {
        if !into.iter().any(|x| x.name == i.name) {
            into.push(i);
        }
    }
}

/// Directories under `root` (down to `depth`) that hold a `SKILL.md`; the detail is the frontmatter `description`.
pub fn scan_skills(root: &Path, depth: usize, out: &mut Vec<Item>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for d in dirs {
        let md = d.join("SKILL.md");
        if let Ok(text) = std::fs::read_to_string(&md) {
            let desc = description(&text);
            out.push(item(d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), desc, "disk"));
        } else if depth > 0 {
            scan_skills(&d, depth - 1, out);
        }
    }
}

/// The frontmatter `description`; for a folded or literal block (`>-`, `|`) its first line.
fn description(text: &str) -> String {
    let mut lines = text.lines();
    let Some(v) = lines.find_map(|l| l.strip_prefix("description:")) else { return String::new() };
    let v = v.trim();
    let v = if v.starts_with(['>', '|']) { lines.map(str::trim).find(|l| !l.is_empty()).unwrap_or("") } else { v };
    v.trim_matches(['"', '\'']).chars().take(100).collect()
}

fn scan_agents(dir: &Path) -> Vec<Item> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut v: Vec<Item> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "md")).filter_map(|p| p.file_stem().map(|s| item(s.to_string_lossy(), "", "disk"))).collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// Run a list command; on a failure, note why and return nothing.
fn listed(r: &dyn Runner, e: &mut CliEntry, args: &[&str]) -> Option<String> {
    match r.run(&e.cli, args, PROBE_LIMIT) {
        // gemini crashes on a broken install and still exits 0, with the trace on stderr and nothing on stdout.
        Ran::Done { code: 0, out, err } if out.trim().is_empty() && !err.trim().is_empty() => {
            e.notes.push(format!("`{} {}` printed only an error: {}", e.cli, args.join(" "), first_line(&err)));
            None
        }
        Ran::Done { code: 0, out, .. } => Some(out),
        Ran::Done { code, err, out } => {
            let why = if err.trim().is_empty() { first_line(&out) } else { first_line(&err) };
            e.notes.push(format!("`{} {}` exited {code}: {why}", e.cli, args.join(" ")));
            None
        }
        Ran::TimedOut => {
            e.notes.push(format!("`{} {}` did not answer in {}s", e.cli, args.join(" "), PROBE_LIMIT.as_secs()));
            None
        }
        Ran::Missing => None,
    }
}

fn entry(r: &dyn Runner, cli: &str) -> Option<CliEntry> {
    let path = r.which(cli)?;
    let mut e = CliEntry { cli: cli.into(), installed: true, path: path.display().to_string(), ..Default::default() };
    e.version = listed(r, &mut e, &["--version"]).map(|v| first_line(&v)).unwrap_or_default();
    Some(e)
}

fn discover_claude(r: &dyn Runner, user: &Path) -> CliEntry {
    let Some(mut e) = entry(r, "claude") else { return CliEntry { cli: "claude".into(), ..Default::default() } };
    if let Some(out) = listed(r, &mut e, &["auth", "status", "--json"]) {
        if let Ok(s) = serde_json::from_str::<serde_json::Value>(&out) {
            e.auth = if s["loggedIn"] == true { format!("signed in, {}", s["subscriptionType"].as_str().unwrap_or("plan unknown")) } else { "signed out".into() };
        }
    }
    e.mcp = listed(r, &mut e, &["mcp", "list"]).map(|o| parse_claude_mcp(&o)).unwrap_or_default();
    e.plugins = listed(r, &mut e, &["plugin", "list", "--json"]).map(|o| parse_claude_plugins(&o)).unwrap_or_default();
    scan_skills(&user.join(".claude/skills"), 3, &mut e.skills);
    e.agents = scan_agents(&user.join(".claude/agents"));
    e
}

fn discover_agy(r: &dyn Runner, user: &Path) -> CliEntry {
    let Some(mut e) = entry(r, "agy") else { return CliEntry { cli: "agy".into(), ..Default::default() } };
    e.mcp = listed(r, &mut e, &["mcp", "list"]).map(|o| parse_plain_list(&o)).unwrap_or_default();
    add_config(&mut e.mcp, &user.join(".gemini/config/mcp_config.json"));
    e.plugins = listed(r, &mut e, &["plugin", "list"]).map(|o| parse_plain_list(&o)).unwrap_or_default();
    e.agents = listed(r, &mut e, &["agents"]).map(|o| parse_plain_list(&o)).unwrap_or_default();
    e.models = listed(r, &mut e, &["models"]).map(|o| parse_tab_models(&o)).unwrap_or_default();
    for dir in [".gemini/antigravity-cli/builtin/skills", ".gemini/skills", ".agents/skills"] {
        scan_skills(&user.join(dir), 2, &mut e.skills);
    }
    e
}

/// `codex mcp list --json`: a JSON array of servers. Only the name and transport are kept, never `env` or `args`.
pub fn parse_codex_mcp(out: &str) -> Vec<Item> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(out) else { return vec![] };
    v.as_array().into_iter().flatten().filter_map(|s| Some(item(s["name"].as_str()?, s["transport"]["type"].as_str().unwrap_or(""), "cli"))).collect()
}

fn discover_codex(r: &dyn Runner) -> CliEntry {
    let Some(mut e) = entry(r, "codex") else { return CliEntry { cli: "codex".into(), ..Default::default() } };
    e.mcp = listed(r, &mut e, &["mcp", "list", "--json"]).map(|o| parse_codex_mcp(&o)).unwrap_or_default();
    e.models = listed(r, &mut e, &["debug", "models"]).map(|o| crate::setup::pick_codex_models(&o).into_iter().map(|m| item(m, "", "cli")).collect()).unwrap_or_default();
    e
}

fn discover_gemini(r: &dyn Runner, user: &Path) -> CliEntry {
    let Some(mut e) = entry(r, "gemini") else { return CliEntry { cli: "gemini".into(), ..Default::default() } };
    e.mcp = listed(r, &mut e, &["mcp", "list"]).map(|o| parse_plain_list(&o)).unwrap_or_default();
    add_config(&mut e.mcp, &user.join(".gemini/settings.json"));
    e.extensions = listed(r, &mut e, &["extensions", "list"]).map(|o| parse_plain_list(&o)).unwrap_or_default();
    e
}

fn discover_ollama(r: &dyn Runner) -> CliEntry {
    let Some(mut e) = entry(r, "ollama") else { return CliEntry { cli: "ollama".into(), ..Default::default() } };
    e.models = listed(r, &mut e, &["list"]).map(|o| parse_ollama_list(&o)).unwrap_or_default();
    e
}

/// Ask a CLI's model what it can use. The answer is the model's word, kept apart and marked as such.
fn ask(r: &dyn Runner, e: &mut CliEntry) {
    let flag = match e.cli.as_str() {
        "claude" | "agy" => "-p",
        _ => return,
    };
    match r.run(&e.cli, &[flag, ASK_PROMPT], ASK_LIMIT) {
        Ran::Done { code: 0, out, .. } => e.asked = Some(out.trim().chars().take(8000).collect()),
        Ran::Done { code, err, .. } => e.notes.push(format!("asking `{}` exited {code}: {}", e.cli, first_line(&err))),
        Ran::TimedOut => e.notes.push(format!("asking `{}` did not answer in {}s", e.cli, ASK_LIMIT.as_secs())),
        Ran::Missing => {}
    }
}

pub fn collect(r: &dyn Runner, user_home: &Path, now: i64, with_ask: bool) -> Snapshot {
    let mut clis = vec![discover_claude(r, user_home), discover_agy(r, user_home), discover_codex(r), discover_gemini(r, user_home), discover_ollama(r)];
    if with_ask {
        clis.iter_mut().filter(|c| c.installed).for_each(|c| ask(r, c));
    }
    Snapshot { schema: 1, taken_unix: now, delphi_version: env!("CARGO_PKG_VERSION").into(), clis, projects: vec![] }
}

/// What one project directory adds. Files only; nothing is run in it.
pub fn scan_project(dir: &Path) -> ProjectEntry {
    let mut p = ProjectEntry { path: dir.display().to_string(), ..Default::default() };
    add_config(&mut p.mcp, &dir.join(".mcp.json"));
    for sub in [".claude/skills", ".agents/skills", ".gemini/skills"] {
        scan_skills(&dir.join(sub), 2, &mut p.skills);
    }
    p.agents = scan_agents(&dir.join(".claude/agents"));
    p
}

/// The projects of a new snapshot: the ones just scanned, then those of the last snapshot that still exist and were not rescanned.
fn merge_projects(scanned: Vec<ProjectEntry>, before: Option<&Snapshot>) -> Vec<ProjectEntry> {
    let kept = before.into_iter().flat_map(|b| b.projects.iter()).filter(|p| Path::new(&p.path).is_dir() && !scanned.iter().any(|s| s.path == p.path)).cloned();
    scanned.iter().cloned().chain(kept).collect()
}

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn age(secs: i64) -> String {
    match secs {
        s if s < 90 => format!("{}s ago", s.max(0)),
        s if s < 5400 => format!("{}m ago", s / 60),
        s if s < 172_800 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

pub fn render(s: &Snapshot, now: i64) -> String {
    let mut o = format!("Capabilities, taken {} by delphi {}\n", age(now - s.taken_unix), s.delphi_version);
    for c in &s.clis {
        if !c.installed {
            o.push_str(&format!("\n{}: not installed\n", c.cli));
            continue;
        }
        o.push_str(&format!("\n{} {}{}\n", c.cli, c.version, if c.auth.is_empty() { String::new() } else { format!("  ({})", c.auth) }));
        let groups: [(&str, &Vec<Item>); 6] = [("connectors", &c.mcp), ("plugins", &c.plugins), ("extensions", &c.extensions), ("skills", &c.skills), ("agents", &c.agents), ("models", &c.models)];
        for (label, items) in groups {
            if items.is_empty() {
                continue;
            }
            o.push_str(&format!("  {label} ({}, from {}):\n", items.len(), items.iter().map(|i| i.source.as_str()).fold(Vec::<&str>::new(), |mut a, s| { if !a.contains(&s) { a.push(s) } a }).join("+")));
            for i in items {
                o.push_str(&format!("    {}{}\n", i.name, if i.detail.is_empty() { String::new() } else { format!("  {}", i.detail) }));
            }
        }
        if let Some(a) = &c.asked {
            o.push_str("  the model's own list (asked, not verified):\n");
            a.lines().for_each(|l| o.push_str(&format!("    {l}\n")));
        }
        c.notes.iter().for_each(|n| o.push_str(&format!("  ! {n}\n")));
    }
    for p in s.projects.iter().filter(|p| !(p.mcp.is_empty() && p.skills.is_empty() && p.agents.is_empty())) {
        o.push_str(&format!("\nproject {}\n", p.path));
        for (label, items) in [("connector", &p.mcp), ("skill", &p.skills), ("agent", &p.agents)] {
            for i in items {
                o.push_str(&format!("    {label}: {}{}\n", i.name, if i.detail.is_empty() { String::new() } else { format!("  {}", i.detail) }));
            }
        }
    }
    o.push_str("\nBuilt-in tools of a CLI are not listed by any command; `delphi capabilities --ask` asks the model, and that answer is unverified.\n");
    o
}

/// The project directories to scan: each `--project <dir>`, the git root of the working directory, and every project a workspace in
/// this home registers.
fn project_dirs(args: &[String], home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = args.windows(2).filter(|w| w[0] == "--project").map(|w| PathBuf::from(&w[1])).collect();
    dirs.extend(std::env::current_dir().ok().and_then(|c| crate::home::git_root(&c)));
    if let Ok(rd) = std::fs::read_dir(home.join("workspaces")) {
        let mut ws: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        ws.sort();
        dirs.extend(ws.iter().flat_map(|w| crate::home::projects(w)));
    }
    let mut seen = vec![];
    dirs.retain(|d| d.is_dir() && !seen.contains(d) && { seen.push(d.clone()); true });
    dirs
}

/// The detached refresh: the program `exe` (after an update, the new one) run as `capabilities --refresh --quiet`.
fn background_command(exe: &Path, home: &Path) -> Command {
    let mut c = Command::new(exe);
    c.args(["capabilities", "--refresh", "--quiet", "--home"]).arg(home).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    c
}

/// Start a refresh as a detached process, so setup and update do not wait for the CLIs. Skipped when a snapshot is only seconds old
/// or `DELPHI_NO_BACKGROUND_PROBE` is set. Returns whether one was started.
pub fn refresh_in_background(exe: &Path, home: &Path) -> bool {
    if std::env::var_os("DELPHI_NO_BACKGROUND_PROBE").is_some() || load(home).is_some_and(|s| now_unix() - s.taken_unix < FRESH_SECS) {
        return false;
    }
    background_command(exe, home).spawn().is_ok()
}

/// `delphi capabilities [--refresh] [--ask] [--json] [--quiet]`.
pub fn cli(args: &[String], home: &Path) -> i32 {
    let has = |n: &str| args.iter().any(|a| a == n);
    let user = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()));
    let existing = load(home);
    let snap = match &existing {
        Some(s) if !has("--refresh") && !has("--ask") => s.clone(),
        _ => {
            let mut s = collect(&RealRunner, &user, now_unix(), has("--ask"));
            let scanned = project_dirs(args, home).iter().map(|d| scan_project(d)).collect();
            s.projects = merge_projects(scanned, existing.as_ref());
            if let Err(e) = save(home, &s) {
                eprintln!("delphi capabilities: cannot write {}: {e}", path(home).display());
                return 1;
            }
            s
        }
    };
    if has("--quiet") {
        return 0;
    }
    if has("--json") {
        println!("{}", serde_json::to_string_pretty(&snap).unwrap_or_default());
    } else {
        print!("{}", render(&snap, now_unix()));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        on_path: Vec<&'static str>,
        answers: HashMap<String, Ran>,
    }

    impl Runner for Fake {
        fn run(&self, program: &str, args: &[&str], _: Duration) -> Ran {
            self.answers.get(&format!("{program} {}", args.join(" "))).cloned().unwrap_or(Ran::Missing)
        }
        fn which(&self, program: &str) -> Option<PathBuf> {
            self.on_path.contains(&program).then(|| PathBuf::from("/bin").join(program))
        }
    }

    fn done(out: &str) -> Ran {
        Ran::Done { code: 0, out: out.into(), err: String::new() }
    }

    #[test]
    fn codex_is_discovered_from_its_own_json_listings_without_arguments_or_environment() {
        let mut f = Fake { on_path: vec!["codex"], ..Default::default() };
        f.answers.insert("codex --version".into(), done("codex-cli 0.165.0\n"));
        f.answers.insert("codex mcp list --json".into(), done(r#"[{"name":"docs","transport":{"type":"stdio","command":"npx","args":["--token","SECRET"],"env":{"K":"SECRET"}}}]"#));
        f.answers.insert("codex debug models".into(), done(r#"{"models":[{"slug":"hidden-one","visibility":"hide","priority":1},{"slug":"gpt-b","visibility":"list","priority":9},{"slug":"gpt-a","visibility":"list","priority":5}]}"#));
        let home = tempfile::tempdir().unwrap();
        let s = collect(&f, home.path(), 1, false);
        let c = s.clis.iter().find(|c| c.cli == "codex").unwrap();
        assert!(c.installed);
        assert_eq!((c.mcp[0].name.as_str(), c.mcp[0].detail.as_str()), ("docs", "stdio"));
        assert_eq!(c.models.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["gpt-a", "gpt-b"], "hidden models are left out, best-ranked first");
        assert!(!format!("{:?}", s).contains("SECRET"));
        assert!(parse_codex_mcp("[]").is_empty() && parse_codex_mcp("No MCP servers configured yet.").is_empty());
    }

    #[test]
    fn claude_connectors_parse_with_status() {
        let v = parse_claude_mcp("Checking MCP server health…\n\nclaude.ai Claude Docs: https://api.anthropic.com/v1/pages/mcp - ✔ Connected\nlocal: npx x - ✗ Failed to connect\n");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "claude.ai Claude Docs");
        assert_eq!(v[0].detail, "https://api.anthropic.com/v1/pages/mcp · Connected");
        assert_eq!(v[1].detail, "npx x · Failed to connect");
    }

    #[test]
    fn empty_messages_are_no_items() {
        assert!(parse_plain_list("No MCP servers configured.\n").is_empty());
        assert!(parse_plain_list("No plugins installed.").is_empty());
        assert_eq!(parse_plain_list("alpha\nbeta\n").len(), 2);
        assert!(parse_claude_plugins("[]").is_empty());
    }

    #[test]
    fn models_and_ollama_parse() {
        let m = parse_tab_models("Fetching available models...\ngemini-4-argon-low\tGemini 4 Argon (Low)\n");
        assert_eq!((m[0].name.as_str(), m[0].detail.as_str()), ("gemini-4-argon-low", "Gemini 4 Argon (Low)"));
        let o = parse_ollama_list("NAME ID SIZE MODIFIED\ngemma4:latest  c6eb  9.6 GB  4 weeks ago\n");
        assert_eq!((o[0].name.as_str(), o[0].detail.as_str()), ("gemma4:latest", "9.6 GB"));
        let p = parse_claude_plugins(r#"[{"name":"a","version":"1.0","enabled":false}]"#);
        assert_eq!(p[0].detail, "1.0 · disabled");
    }

    #[test]
    fn skills_are_found_nested_with_their_description() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("synced/docs");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("SKILL.md"), "---\nname: docs\ndescription: \"Edit docs\"\n---\n").unwrap();
        std::fs::create_dir_all(t.path().join("empty")).unwrap();
        let mut v = vec![];
        scan_skills(t.path(), 3, &mut v);
        assert_eq!(v, vec![item("docs", "Edit docs", "disk")]);
        assert_eq!(description("---\ndescription: >-\n  Folded text\n  more\n---"), "Folded text");
    }

    #[test]
    fn a_missing_cli_is_recorded_and_a_broken_one_says_why() {
        let mut f = Fake { on_path: vec!["gemini", "ollama"], ..Default::default() };
        f.answers.insert("gemini --version".into(), done("0.9\n"));
        f.answers.insert("gemini mcp list".into(), Ran::Done { code: 1, out: String::new(), err: "\ninnerError Error: Cannot find module 'pty.node'\n  at x".into() });
        f.answers.insert("gemini extensions list".into(), Ran::TimedOut);
        f.answers.insert("ollama list".into(), done("NAME ID SIZE MODIFIED\nmistral:latest  65  4.4 GB  4 weeks ago\n"));
        let home = tempfile::tempdir().unwrap();
        let s = collect(&f, home.path(), 100, false);
        let by = |n: &str| s.clis.iter().find(|c| c.cli == n).unwrap();
        assert!(!by("claude").installed && !by("agy").installed);
        assert!(by("gemini").installed && by("gemini").mcp.is_empty());
        assert!(by("gemini").notes[0].contains("Cannot find module"), "{:?}", by("gemini").notes);
        assert!(by("gemini").notes[1].contains("did not answer"));
        assert_eq!(by("ollama").models[0].name, "mistral:latest");
        assert!(render(&s, 160).contains("claude: not installed"));
    }

    #[test]
    fn claude_auth_connectors_and_ask_are_kept_apart() {
        let mut f = Fake { on_path: vec!["claude"], ..Default::default() };
        f.answers.insert("claude --version".into(), done("2.1.311 (Claude Code)\n"));
        f.answers.insert("claude auth status --json".into(), done(r#"{"loggedIn":true,"subscriptionType":"max"}"#));
        f.answers.insert("claude mcp list".into(), done("Checking MCP server health…\n\nx: http://h - ✔ Connected\n"));
        f.answers.insert("claude plugin list --json".into(), done("[]"));
        f.answers.insert(format!("claude -p {ASK_PROMPT}"), done("tool: Bash\n"));
        let home = tempfile::tempdir().unwrap();
        let s = collect(&f, home.path(), 100, true);
        let c = &s.clis[0];
        assert_eq!(c.auth, "signed in, max");
        assert_eq!(c.mcp[0].source, "cli");
        assert_eq!(c.asked.as_deref(), Some("tool: Bash"));
        assert!(render(&s, 100).contains("asked, not verified"));
    }

    #[test]
    fn the_snapshot_round_trips_and_a_bad_file_is_none() {
        let t = tempfile::tempdir().unwrap();
        assert!(load(t.path()).is_none());
        let s = Snapshot { schema: 1, taken_unix: 5, delphi_version: "x".into(), clis: vec![CliEntry { cli: "agy".into(), installed: true, ..Default::default() }], projects: vec![ProjectEntry { path: "/p".into(), ..Default::default() }] };
        save(t.path(), &s).unwrap();
        assert_eq!(load(t.path()), Some(s));
        std::fs::write(path(t.path()), "not json").unwrap();
        assert!(load(t.path()).is_none());
    }

    #[test]
    fn a_cli_that_crashes_but_exits_zero_is_a_note_not_an_empty_list() {
        let mut f = Fake { on_path: vec!["gemini"], ..Default::default() };
        f.answers.insert("gemini --version".into(), done("0.63.0\n"));
        f.answers.insert("gemini mcp list".into(), Ran::Done { code: 0, out: String::new(), err: "innerError Error: Cannot find module 'pty.node'\n".into() });
        let home = tempfile::tempdir().unwrap();
        let snap = collect(&f, home.path(), 1, false);
        let g = snap.clis.iter().find(|c| c.cli == "gemini").unwrap();
        assert!(g.mcp.is_empty());
        assert!(g.notes.iter().any(|n| n.contains("printed only an error") && n.contains("pty.node")), "{:?}", g.notes);
    }

    #[test]
    fn config_files_give_names_and_targets_but_never_secrets() {
        let v = parse_mcp_config(
            r#"{"mcpServers":{"a":{"url":"https://h/mcp?token=SECRET","headers":{"Authorization":"Bearer SECRET"}},
                "b":{"command":"npx","args":["-y","x","--key=SECRET"],"env":{"K":"SECRET"}},"c":{"command":"x","disabled":true}}}"#,
        );
        assert_eq!(v.iter().map(|i| (i.name.as_str(), i.detail.as_str())).collect::<Vec<_>>(), vec![("a", "https://h/mcp"), ("b", "npx"), ("c", "x · disabled")]);
        assert!(!format!("{v:?}").contains("SECRET"));
        assert!(parse_mcp_config("").is_empty() && parse_mcp_config("{}").is_empty());
    }

    #[test]
    fn agy_servers_come_from_its_config_file_when_the_list_is_free_text() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".gemini/config")).unwrap();
        std::fs::write(home.path().join(".gemini/config/mcp_config.json"), r#"{"mcpServers":{"fs":{"command":"npx"}}}"#).unwrap();
        let mut f = Fake { on_path: vec!["agy"], ..Default::default() };
        f.answers.insert("agy mcp list".into(), done("No MCP servers configured.\n"));
        let a = &collect(&f, home.path(), 1, false).clis[1];
        assert_eq!(a.mcp, vec![item("fs", "npx", "disk")]);
    }

    #[test]
    fn a_project_adds_its_connectors_skills_and_agents_and_old_projects_are_kept() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join(".mcp.json"), r#"{"mcpServers":{"db":{"url":"http://x"}}}"#).unwrap();
        std::fs::create_dir_all(t.path().join(".claude/skills/lint")).unwrap();
        std::fs::write(t.path().join(".claude/skills/lint/SKILL.md"), "description: Lint it\n").unwrap();
        std::fs::create_dir_all(t.path().join(".claude/agents")).unwrap();
        std::fs::write(t.path().join(".claude/agents/reviewer.md"), "x").unwrap();
        let p = scan_project(t.path());
        assert_eq!((p.mcp[0].name.as_str(), p.skills[0].name.as_str(), p.agents[0].name.as_str()), ("db", "lint", "reviewer"));
        let gone = ProjectEntry { path: "/no/such/dir".into(), ..Default::default() };
        let kept = ProjectEntry { path: t.path().join(".claude").display().to_string(), ..Default::default() };
        let before = Snapshot { projects: vec![gone, kept.clone(), ProjectEntry { path: p.path.clone(), ..Default::default() }], ..Default::default() };
        let merged = merge_projects(vec![p.clone()], Some(&before));
        assert_eq!(merged, vec![p, kept], "rescanned replaces, a vanished directory drops, the rest stays");
    }

    #[test]
    fn the_background_start_runs_the_given_program_with_the_refresh_arguments() {
        let c = background_command(Path::new("/new/delphi"), Path::new("/h"));
        assert_eq!(c.get_program(), "/new/delphi");
        assert_eq!(c.get_args().map(|a| a.to_string_lossy().to_string()).collect::<Vec<_>>(), ["capabilities", "--refresh", "--quiet", "--home", "/h"]);
    }

    #[test]
    fn a_background_start_happens_unless_the_snapshot_is_seconds_old() {
        let t = tempfile::tempdir().unwrap();
        assert!(refresh_in_background(Path::new("/usr/bin/true"), t.path()), "no snapshot: started");
        save(t.path(), &Snapshot { taken_unix: now_unix(), ..Default::default() }).unwrap();
        assert!(!refresh_in_background(Path::new("/usr/bin/true"), t.path()), "a fresh snapshot: not started");
        save(t.path(), &Snapshot { taken_unix: now_unix() - 3600, ..Default::default() }).unwrap();
        assert!(refresh_in_background(Path::new("/usr/bin/true"), t.path()), "an old one: started");
    }

    #[test]
    fn ages_read_plainly() {
        assert_eq!(age(30), "30s ago");
        assert_eq!(age(600), "10m ago");
        assert_eq!(age(7200), "2h ago");
        assert_eq!(age(400_000), "4d ago");
    }
}
