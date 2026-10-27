//! Inert MCP servers and `delphi bundle connectors map` (`specs/025` §4.1, §10.2).
//!
//! A bundle's `.mcp.json` is a list of things the bundle *wanted*. `add` records each clean declaration in the lock as
//! inert (name, transport, url or command, the names of its headers): nothing is connected, listed, launched or offered
//! to a role. The only road from a declaration to a connector is `connectors map`, an act of the operator at a terminal:
//! it writes a `[connectors.<bundle>-<server>]` entry whose `url` and `hosts` come from the declaration and whose roles,
//! tool allowlist and classifications come **only from the operator's flags**. A server's own descriptions and
//! annotations are untrusted and are never read here. `client_id`, `client_secret` and every header or env value are
//! absent from the entry; a login, if the operator names one, is an `[auth.*]` block that is already ratified.

use crate::bundle_cmd::Real;
use delphi_bundle::mcp_decl::{self, McpServer};
use delphi_bundle::Host;
use delphi_core::bundles::{self, Entry, InertServer};
use std::path::Path;

const MAX_SERVERS: usize = 32;

fn clean(s: &str, cap: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(cap).collect()
}

/// The clean declarations of a verified tree, as the lock keeps them. A declaration with a problem (a credential-shaped
/// value, a plain-http URL) fails validation before this is reached; it is dropped here too, so the lock never holds one.
pub(crate) fn inert_servers(dir: &Path) -> Vec<InertServer> {
    let Ok(servers) = mcp_decl::read(&delphi_bundle::Tree::open(dir)) else { return Vec::new() };
    servers
        .into_iter()
        .filter(|s| s.problems.is_empty())
        .take(MAX_SERVERS)
        .map(|s| InertServer {
            name: s.name,
            transport: clean(&s.transport, 16),
            url: clean(s.url.as_deref().unwrap_or(""), 512),
            command: clean(s.command.as_deref().unwrap_or(""), 256),
            header_keys: s.header_keys.iter().take(16).map(|k| clean(k, 64)).collect(),
            env_keys: s.env_keys.iter().take(16).map(|k| clean(k, 64)).collect(),
        })
        .collect()
}

/// `<bundle>-<server>`, the connector's name in `config.toml`. `None` when it would not be a plain, lowercase name.
pub fn connector_name(bundle: &str, server: &str) -> Option<String> {
    let n = format!("{}-{}", bundle, server.to_ascii_lowercase());
    (n.len() <= 96 && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') && !n.starts_with(['-', '_'])).then_some(n)
}

/// Is this server's connector already in the workspace's `config.toml`?
fn catalogued(ws: &Path, name: &str) -> bool {
    std::fs::read_to_string(ws.join("config.toml")).ok().and_then(|t| toml::from_str::<toml::Value>(&t).ok()).is_some_and(|v| v.get("connectors").and_then(|c| c.get(name)).is_some())
}

/// The lines of the add summary and of `list` that say what is inert and how to map it.
pub(crate) fn describe(ws: &Path, e: &Entry) -> Vec<String> {
    e.inert_servers
        .iter()
        .map(|s| {
            let cname = connector_name(&e.name, &s.name);
            let mapped = cname.as_deref().is_some_and(|n| catalogued(ws, n));
            let what = match s.transport.as_str() {
                "http" => format!("http {}", s.url),
                "stdio" => format!("stdio `{}` (listed only: a local command is never launched or mapped)", s.command),
                t => format!("{} (not supported, listed only)", t),
            };
            let state = if mapped { "mapped".to_string() } else if s.transport == "http" { format!("inert; map it: delphi bundle connectors map {} {} --roles <role,role> --tool <name>=read|draft|write", e.id(), s.name) } else { "inert".into() };
            format!("{}: {} [{}]", s.name, what, state)
        })
        .collect()
}

/// Print the inert servers of a bundle just verified at `dir` (the add summary).
pub(crate) fn print_inert(org: &str, name: &str, dir: &Path) {
    let servers = inert_servers(dir);
    if servers.is_empty() {
        return;
    }
    let pseudo = Entry { org: org.into(), name: name.into(), inert_servers: servers, ..blank_entry() };
    println!("    MCP servers it declares (all inert: nothing is connected, listed or offered until you map one):");
    for l in describe(Path::new("/nonexistent"), &pseudo) {
        println!("      {}", l);
    }
}

fn blank_entry() -> Entry {
    Entry {
        org: String::new(),
        name: String::new(),
        version: String::new(),
        hash: String::new(),
        publisher_kid: String::new(),
        provenance: String::new(),
        source: bundles::Source { kind: String::new(), path: String::new() },
        added_at: String::new(),
        order: 0,
        status: String::new(),
        review_by: String::new(),
        title: String::new(),
        skill_roles: Default::default(),
        history: Vec::new(),
        inert_servers: Vec::new(),
    }
}

/// What `remove` says about the servers of a bundle it removed: the inert records go with the lock entry; a connector the
/// operator mapped stays in the catalog, because removing it is a ratified change of its own.
pub(crate) fn removal_note(ws: &Path, e: &Entry) -> Vec<String> {
    let mapped: Vec<String> = e.inert_servers.iter().filter_map(|s| connector_name(&e.name, &s.name)).filter(|n| catalogued(ws, n)).collect();
    let mut out = Vec::new();
    let inert = e.inert_servers.len() - mapped.len().min(e.inert_servers.len());
    if inert > 0 {
        out.push(format!("{} inert MCP server record(s) left with it", inert));
    }
    for n in mapped {
        out.push(format!("[connectors.{}] stays in config.toml, ratified, and still works: delete it there and /ratify the removal if you no longer want it", n));
    }
    out
}

/// What the operator is asked to confirm.
pub struct Challenge {
    pub text: String,
    pub expect: String,
}

/// The real confirmation: a terminal on both ends, and the connector's name typed back.
pub fn tty_confirm(c: &Challenge) -> Result<bool, String> {
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if !on_terminal {
        return Err("mapping a connector is a human act: run `delphi bundle connectors map` in a terminal (it has no non-interactive mode)".into());
    }
    println!("{}", c.text);
    print!("type the connector name ({}) to add it to the ratified catalog, anything else cancels: ", c.expect);
    use std::io::{BufRead, Write};
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(line.trim() == c.expect)
}

fn usage(msg: &str) -> i32 {
    eprintln!("delphi bundle connectors map: {}\nusage: delphi bundle connectors map <bundle> <server> --roles <role,role> --tool <name>=read|draft|write [--tool ...] [--first-use ask|allow] [--login <auth block> --account <who> --identity google]", msg);
    2
}

fn tool_name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// What the operator's flags say: roles, tools with their classes, the optional `first_use`, login and account.
struct Wanted {
    roles: Vec<String>,
    tools: Vec<(String, String)>,
    first_use: Option<String>,
    login: Option<String>,
    account: Option<String>,
    identity: Option<String>,
}

fn wanted(args: &[String], host: &dyn Host) -> Result<Wanted, String> {
    let values = |name: &str| crate::bundle_cmd::flag_values(args, name);
    let roles: Vec<String> = values("--roles").iter().flat_map(|v| v.split(',')).map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect();
    if roles.is_empty() {
        return Err("--roles is required: the roles that may use this server's tools (you decide, the bundle does not)".into());
    }
    if let Some(bad) = roles.iter().find(|r| host.role_scope(r).is_none()) {
        return Err(format!("`{}` is not a role of this install", bad));
    }
    let mut tools: Vec<(String, String)> = Vec::new();
    for t in values("--tool") {
        let Some((name, class)) = t.split_once('=') else { return Err(format!("--tool wants <name>=read|draft|write, got `{}`", t)) };
        if !tool_name_ok(name) {
            return Err(format!("`{}` is not a plain tool name", name));
        }
        if !matches!(class, "read" | "draft" | "write") {
            return Err(format!("`{}`: the class is read, draft or write (you classify; the server's own annotations are not trusted)", t));
        }
        if tools.iter().any(|(n, _)| n == name) {
            return Err(format!("tool `{}` is classified twice", name));
        }
        tools.push((name.to_string(), class.to_string()));
    }
    if tools.is_empty() {
        return Err("--tool is required at least once: a tool you do not list stays invisible to every agent".into());
    }
    let first_use = values("--first-use").into_iter().next();
    if first_use.as_deref().is_some_and(|f| !matches!(f, "ask" | "allow")) {
        return Err("--first-use is ask or allow".into());
    }
    let (login, account) = (values("--login").into_iter().next(), values("--account").into_iter().next());
    let identity = values("--identity").into_iter().next();
    if login.is_some() != account.is_some() || login.is_some() != identity.is_some() {
        return Err("--login, --account and --identity go together: after the sign-in the daemon looks up who signed in (--identity, today `google`) and compares it with the account you name".into());
    }
    if identity.as_deref().is_some_and(|i| i != "google") {
        return Err("--identity is `google` (the only lookup this version has)".into());
    }
    if let Some(a) = &account {
        if a.trim().is_empty() || a.chars().any(|c| c.is_control()) || a.len() > 128 {
            return Err("--account is not a plain identity".into());
        }
    }
    Ok(Wanted { roles, tools, first_use, login, account, identity })
}

/// The ratified `[auth.*]` block names of the workspace.
fn ratified_auth(ws: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(ws.join("local/ratified/config.access.toml")).unwrap_or_default();
    toml::from_str::<toml::Value>(&text).ok().and_then(|v| v.get("auth").and_then(|a| a.as_table().map(|t| t.keys().cloned().collect()))).unwrap_or_default()
}

/// The declaration `server` of a cached, re-verified bundle: refused unless it is a clean `http` one.
fn declaration(dir: &Path, server: &str) -> Result<McpServer, String> {
    let servers = mcp_decl::read(&delphi_bundle::Tree::open(dir))?;
    let s = servers.into_iter().find(|s| s.name == server).ok_or_else(|| format!("the bundle declares no server `{}`", server))?;
    if let Some(p) = s.problems.first() {
        return Err(format!("the declaration of `{}` is refused: {}", server, p));
    }
    match s.transport.as_str() {
        "http" => {}
        "stdio" => return Err(format!("`{}` is a stdio server: mapping one would run a local command on this machine, which a bundle may never cause (specs/025 §4.1). It stays listed, inert", server)),
        t => return Err(format!("`{}` uses the `{}` transport, which this version does not support; only Streamable HTTP can be mapped", server, t)),
    }
    if s.url.is_none() || s.host.is_none() {
        return Err(format!("`{}` has no usable https url", server));
    }
    Ok(s)
}

/// `delphi bundle connectors map <bundle> <server> …`. `confirm` is the operator's decision; tests pass their own.
pub fn map(home: &Path, ws: &Path, args: &[String], host: &dyn Host, confirm: &dyn Fn(&Challenge) -> Result<bool, String>) -> i32 {
    let pos: Vec<&String> = crate::bundle_cmd::positionals(args);
    let (Some(bundle), Some(server)) = (pos.get(2), pos.get(3)) else { return usage("name the bundle and the server") };
    let want = match wanted(args, host) {
        Ok(w) => w,
        Err(e) => return usage(&e),
    };
    let (entry, dir) = match crate::bundle_compat::cached(home, ws, bundle) {
        Ok(v) => v,
        Err(e) => return crate::bundle_add::fail(e),
    };
    let decl = match declaration(&dir, server) {
        Ok(d) => d,
        Err(e) => return crate::bundle_add::fail(e),
    };
    let Some(cname) = connector_name(&entry.name, &decl.name) else { return crate::bundle_add::fail(format!("`{}-{}` is not a plain connector name; ask the publisher to rename the server", entry.name, decl.name)) };
    if let Some(l) = &want.login {
        if !ratified_auth(ws).contains(l) {
            return crate::bundle_add::fail(format!("no ratified [auth.{}] block: sign-in settings (client id, secret) are yours to add and ratify, a bundle never supplies them", l));
        }
    }
    let (url, host_name) = (decl.url.clone().unwrap_or_default(), decl.host.clone().unwrap_or_default());
    let mut table = toml::value::Table::new();
    table.insert("transport".into(), "http".into());
    table.insert("url".into(), url.clone().into());
    // The identity lookup is made by the operator's choice of `--identity google` and needs its own host (`specs/010` §4.10); the bundle never names it.
    let mut hosts = vec![host_name.clone()];
    if want.identity.as_deref() == Some("google") {
        hosts.push("openidconnect.googleapis.com".into());
    }
    table.insert("hosts".into(), toml::Value::Array(hosts.iter().cloned().map(Into::into).collect()));
    table.insert("roles".into(), toml::Value::Array(want.roles.iter().cloned().map(Into::into).collect()));
    table.insert("tools".into(), toml::Value::Table(want.tools.iter().map(|(n, c)| (n.clone(), toml::Value::String(c.clone()))).collect()));
    if let Some(f) = &want.first_use {
        table.insert("first_use".into(), f.clone().into());
    }
    if let (Some(l), Some(a)) = (&want.login, &want.account) {
        table.insert("login".into(), l.clone().into());
        table.insert("account".into(), a.clone().into());
        table.insert("identity".into(), want.identity.clone().unwrap_or_default().into());
    }
    let tool_lines: Vec<String> = want.tools.iter().map(|(n, c)| format!("      {:<28} {}", n, c)).collect();
    let challenge = Challenge {
        text: format!(
            "Map {} server `{}` of {}@{} (hash {}) into the ratified connector catalog as [connectors.{}]:\n  url      {}\n  hosts    {} (exact names, no wildcard)\n  roles    {}\n  tools    (your classification; any other tool the server offers stays invisible)\n{}\n  login    {}\n  {}\nThe bundle supplies the address only. No credential, header value or client id is copied; the server's descriptions are not trusted. Each tool is pinned by definition hash before any agent sees it.",
            decl.transport,
            decl.name,
            entry.id(),
            entry.version,
            entry.hash.trim_start_matches("blake3:").chars().take(12).collect::<String>(),
            cname,
            url,
            hosts.join(", "),
            want.roles.join(", "),
            tool_lines.join("\n"),
            want.login.as_deref().map(|l| format!("[auth.{}], account {} (looked up as {})", l, want.account.as_deref().unwrap_or(""), want.identity.as_deref().unwrap_or(""))).unwrap_or_else(|| "none (the server is called without a sign-in)".into()),
            "results enter at the lowest tier; reads ask on first use unless you chose allow; writes always ask",
        ),
        expect: cname.clone(),
    };
    match confirm(&challenge) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("cancelled; nothing was changed");
            return 1;
        }
        Err(e) => return crate::bundle_add::fail(e),
    }
    if let Err(e) = delphi_engine::ratify::add_connector(ws, &cname, &table) {
        return crate::bundle_add::fail(format!("nothing was changed: {}", e));
    }
    println!("mapped: [connectors.{}] is in config.toml and ratified (this command was the approval of that entry).", cname);
    println!("next: `delphi connectors pin {}` reads the server's tool definitions and proposes their pins; /ratify them in the console, and nothing is offered to an agent before that.", cname);
    if want.login.is_some() {
        println!("      then `delphi auth login {}` to sign in.", want.login.as_deref().unwrap_or(""));
    } else {
        println!("      no login is configured; if the server needs one, add and ratify an [auth.*] block and set `login` and `account` on the entry.");
    }
    0
}

/// The command-line entry: the real host and the terminal challenge.
pub fn run(home: &Path, ws: &Path, args: &[String]) -> i32 {
    match crate::bundle_cmd::positionals(args).get(1).map(|s| s.as_str()) {
        Some("map") => map(home, ws, args, &Real, &tty_confirm),
        _ => usage("the only subcommand is `map`"),
    }
}

#[cfg(test)]
#[path = "bundle_connect_tests.rs"]
mod tests;
