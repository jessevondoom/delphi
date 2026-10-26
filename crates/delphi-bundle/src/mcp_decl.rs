//! `.mcp.json` and `plugin.json` `mcpServers` (`specs/025` §4.1): the servers a bundle declares, read as **inert** entries.
//!
//! Only the server name, its transport, the `url` of an `http` server, the `command`/`args` of a `stdio` one and the
//! *names* of its `headers` and `env` are kept; a value is never read into anything. Nothing here connects, launches or
//! lists a server: `connectors map` (an operator act) is the only road from a declaration to a connector, and it is not
//! built yet. The file is hostile input like every other file of a bundle.

use crate::tree::Tree;
use crate::Report;
use delphi_core::redact::Redactor;
use regex::Regex;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    /// `http`, `stdio`, or the word the file used for a transport this build does not know (`sse`).
    pub transport: String,
    pub url: Option<String>,
    /// The exact host of `url`: what `connectors map` would write to `hosts`, with no wildcard.
    pub host: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    /// Names only. A value is never kept, so nothing can leak through this struct.
    pub header_keys: Vec<String>,
    pub env_keys: Vec<String>,
    /// Why this declaration is refused (a credential-shaped value, a plain-http URL). Empty for a clean one.
    pub problems: Vec<String>,
}

impl McpServer {
    /// One line for the component table and the add summary.
    pub fn describe(&self) -> String {
        match (self.transport.as_str(), &self.url) {
            ("http", Some(u)) => format!("{} (http {}; inert until mapped)", self.name, u),
            ("stdio", _) => format!("{} (stdio `{}`; listed only, never launched or mapped)", self.name, self.command.as_deref().unwrap_or("?")),
            (t, _) => format!("{} ({}; not supported, listed only)", self.name, t),
        }
    }
}

fn secret_header_name(n: &str) -> bool {
    Regex::new(r"(?i)authorization|api[-_]?key|token|secret|cookie|password|credential").unwrap().is_match(n)
}

/// A value that stands for the operator's own environment (`${VAR}`), which Claude Code substitutes and Delphi never reads.
fn placeholder(v: &str) -> bool {
    Regex::new(r"^\$\{[A-Za-z_][A-Za-z0-9_]*(:-[^}]*)?\}$").unwrap().is_match(v.trim())
}

fn parse_server(name: &str, v: &Value, red: &Redactor) -> McpServer {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
    let url = s("url");
    let command = s("command");
    let transport = match s("type").or_else(|| s("transport")) {
        Some(t) => t.to_ascii_lowercase().replace("streamable-http", "http").replace("streamablehttp", "http"),
        None if url.is_some() => "http".into(),
        None if command.is_some() => "stdio".into(),
        None => "unknown".into(),
    };
    let keys_of = |k: &str| -> Vec<String> { v.get(k).and_then(|x| x.as_object()).map(|o| o.keys().cloned().collect()).unwrap_or_default() };
    let mut m = McpServer {
        name: name.into(),
        transport,
        host: url.as_deref().and_then(host_of),
        url,
        command,
        args: v.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        header_keys: keys_of("headers"),
        env_keys: keys_of("env"),
        problems: Vec::new(),
    };
    if !Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,47}$").unwrap().is_match(name) {
        m.problems.push("the server name must match ^[A-Za-z0-9][A-Za-z0-9_-]{0,47}$ (it becomes part of a connector name)".into());
    }
    if let Some(u) = &m.url {
        if !u.starts_with("https://") {
            m.problems.push(format!("url `{}` is not https", red.redact_str(u).0));
        } else if m.host.is_none() || u.split('/').nth(2).is_some_and(|a| a.contains('@')) {
            m.problems.push("url carries credentials or has no host".into());
        }
        if red.redact_str(u).1 {
            m.problems.push("url contains a secret".into());
        }
    }
    if red.redact_str(&format!("{} {}", m.command.as_deref().unwrap_or(""), m.args.join(" "))).1 {
        m.problems.push("command or args carry a literal secret".into());
    }
    for (group, field) in [("headers", "header"), ("env", "env entry")] {
        for (k, val) in v.get(group).and_then(|x| x.as_object()).into_iter().flatten() {
            let Some(text) = val.as_str() else { continue };
            if text.is_empty() || placeholder(text) {
                continue;
            }
            if red.redact_str(text).1 || red.redact_str(&format!("{}: {}", k, text)).1 || secret_header_name(k) {
                m.problems.push(format!("{} `{}` carries a literal value that looks like a credential (a bundle never asks for or ships one)", field, k));
            }
        }
    }
    m
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let auth = rest.split(['/', '?', '#']).next()?;
    if auth.contains('@') || auth.is_empty() {
        return None;
    }
    let host = auth.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map(|(h, _)| h).unwrap_or(auth);
    Regex::new(r"^[A-Za-z0-9.-]+$").unwrap().is_match(host).then(|| host.to_ascii_lowercase())
}

/// Servers from a JSON document: `{"mcpServers": {...}}` or, as older plugins wrote it, the server map itself.
fn from_value(v: &Value, red: &Redactor) -> Vec<McpServer> {
    let map = v.get("mcpServers").unwrap_or(v);
    let Some(o) = map.as_object() else { return Vec::new() };
    o.iter().filter(|(_, s)| s.is_object()).map(|(n, s)| parse_server(n, s, red)).collect()
}

/// Every server the tree declares: `.mcp.json`, then an inline `mcpServers` object or a tree-relative file named by
/// `.claude-plugin/plugin.json`. A name declared twice keeps its first declaration.
pub fn read(tree: &Tree) -> Result<Vec<McpServer>, String> {
    let red = Redactor::new();
    let mut out: Vec<McpServer> = Vec::new();
    let mut add = |list: Vec<McpServer>| {
        for s in list {
            if !out.iter().any(|o| o.name == s.name) {
                out.push(s);
            }
        }
    };
    if let Some(t) = tree.text(".mcp.json") {
        let v: Value = serde_json::from_str(&t).map_err(|e| format!(".mcp.json is not valid JSON: {}", e))?;
        add(from_value(&v, &red));
    }
    if let Some(p) = tree.text(".claude-plugin/plugin.json").and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        match p.get("mcpServers") {
            Some(inline) if inline.is_object() => add(from_value(inline, &red)),
            Some(Value::String(path)) => {
                let rel = path.trim_start_matches("./");
                let safe = !rel.is_empty() && !rel.starts_with('/') && !rel.split('/').any(|c| c == ".." || c.is_empty() || c == ".");
                if safe {
                    if let Some(v) = tree.text(rel).and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                        add(from_value(&v, &red));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The `.mcp.json` an import writes: names, transports, urls and commands, and the *names* of headers and env entries
/// with an empty value. Every value the plugin carried is dropped, so a bundle never stores one.
pub fn sanitized_json(servers: &[McpServer]) -> String {
    let mut map = serde_json::Map::new();
    for s in servers {
        let mut o = serde_json::Map::new();
        o.insert("type".into(), s.transport.clone().into());
        if let Some(u) = &s.url {
            o.insert("url".into(), u.clone().into());
        }
        if let Some(c) = &s.command {
            o.insert("command".into(), c.clone().into());
            o.insert("args".into(), s.args.clone().into());
        }
        for (field, keys) in [("headers", &s.header_keys), ("env", &s.env_keys)] {
            if !keys.is_empty() {
                o.insert(field.into(), keys.iter().map(|k| (k.clone(), Value::String(String::new()))).collect::<serde_json::Map<_, _>>().into());
            }
        }
        map.insert(s.name.clone(), o.into());
    }
    serde_json::to_string_pretty(&serde_json::json!({ "mcpServers": map })).unwrap() + "\n"
}

/// Step 8 (secrets), MCP half: a declaration with a credential-shaped value fails; the rest is reported as inert.
pub fn check(tree: &Tree, r: &mut Report) {
    if tree.get(".mcp.json").is_none() && tree.text(".claude-plugin/plugin.json").is_none_or(|t| !t.contains("mcpServers")) {
        return;
    }
    let servers = match read(tree) {
        Ok(s) => s,
        Err(e) => return r.fail(8, ".mcp.json", e),
    };
    for s in &servers {
        for p in &s.problems {
            r.fail(8, ".mcp.json", format!("server `{}`: {}", s.name, p));
        }
        r.note(8, ".mcp.json", format!("server {}", s.describe()));
    }
}
