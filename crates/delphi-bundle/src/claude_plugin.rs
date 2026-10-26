//! Step 13 of `specs/023` §10 and `specs/025` §2: the Claude Code `plugin.json`, read for agreement with `bundle.toml`,
//! for the keys Delphi ignores and for the options it refuses; and the text a plugin author wrote for Claude Code only.

use crate::manifest::Manifest;
use crate::tree::Tree;
use crate::Report;
use serde_json::Value;

const AT: &str = ".claude-plugin/plugin.json";

/// Keys of a `plugin.json` that run, grant or configure something. Delphi ignores them and leaves them as written for
/// Claude Code (`specs/025` §2.2). `mcpServers` is read as inert entries (`mcp_decl`); `userConfig` and `channels` are
/// refused when an option is `sensitive`.
pub const IGNORED_KEYS: [&str; 12] = ["commands", "hooks", "mcpServers", "lspServers", "userConfig", "channels", "outputStyles", "workflows", "experimental", "dependencies", "settings", "monitors"];

/// What an importer reads from a plugin's `plugin.json`. Every field is a plain string; nothing in it is trusted.
#[derive(Debug, Default, Clone)]
pub struct PluginMeta {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author_name: Option<String>,
    pub author_email: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub keywords: Vec<String>,
    /// Which of [`IGNORED_KEYS`] the file carries.
    pub ignored: Vec<String>,
    /// Option names under `userConfig` / `channels` marked `sensitive`: a bundle must not ask for credentials.
    pub sensitive: Vec<String>,
    /// Option names under `userConfig` that are not sensitive: ignored with a note.
    pub plain_options: Vec<String>,
}

/// Every object under `v` that says `"sensitive": true`, by path.
fn sensitive_options(v: &Value, path: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            if o.get("sensitive") == Some(&Value::Bool(true)) {
                out.push(path.to_string());
            }
            for (k, child) in o {
                sensitive_options(child, &format!("{}.{}", path, k), out);
            }
        }
        Value::Array(a) => {
            for (i, child) in a.iter().enumerate() {
                sensitive_options(child, &format!("{}[{}]", path, i), out);
            }
        }
        _ => {}
    }
}

pub fn parse(text: &str) -> Result<PluginMeta, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {}", e))?;
    let Some(o) = v.as_object() else { return Err("must be a JSON object".into()) };
    let s = |k: &str| o.get(k).and_then(|x| x.as_str()).map(String::from);
    let author = o.get("author");
    let mut m = PluginMeta {
        name: s("name"),
        version: s("version"),
        description: s("description"),
        author_name: author.and_then(|a| a.get("name")).and_then(|x| x.as_str()).map(String::from).or_else(|| author.and_then(|a| a.as_str()).map(String::from)),
        author_email: author.and_then(|a| a.get("email")).and_then(|x| x.as_str()).map(String::from),
        license: s("license"),
        homepage: s("homepage"),
        keywords: o.get("keywords").and_then(|k| k.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        ..PluginMeta::default()
    };
    for k in IGNORED_KEYS {
        if o.contains_key(k) {
            m.ignored.push(k.to_string());
        }
    }
    for k in ["userConfig", "channels"] {
        if let Some(c) = o.get(k) {
            sensitive_options(c, k, &mut m.sensitive);
        }
    }
    if let Some(u) = o.get("userConfig").and_then(|u| u.as_object()) {
        m.plain_options = u.iter().filter(|(_, v)| v.is_object() && v.get("sensitive") != Some(&Value::Bool(true))).map(|(k, _)| k.clone()).collect();
    }
    Ok(m)
}

/// `${CLAUDE_PLUGIN_ROOT}` (and the data-dir twin) stay as text; they mean nothing to Delphi (`specs/025` §2.3).
fn claude_only_text(tree: &Tree, r: &mut Report) {
    let mut paths: Vec<&str> = tree.under("skills/").chain(tree.under("agents/")).filter(|e| e.path.ends_with(".md")).map(|e| e.path.as_str()).collect();
    paths.sort();
    for p in paths {
        if tree.text(p).is_some_and(|t| t.contains("${CLAUDE_PLUGIN_ROOT}") || t.contains("${CLAUDE_PLUGIN_DATA}")) {
            r.note(13, p, "names ${CLAUDE_PLUGIN_ROOT}: left as text, so it works in Claude Code only (Delphi has no plugin root to substitute)");
        }
    }
}

pub fn check(tree: &Tree, m: Option<&Manifest>, claude: bool, r: &mut Report) {
    claude_only_text(tree, r);
    let Some(text) = tree.text(AT) else {
        if claude && m.is_some() {
            r.note(13, AT, "absent; `delphi bundle manifest --claude` generates it");
        }
        return;
    };
    let meta = match parse(&text) {
        Ok(p) => p,
        Err(e) => {
            if claude {
                r.fail(13, AT, e);
            } else {
                r.note(13, AT, format!("{}; Delphi does not read it", e));
            }
            return;
        }
    };
    for k in &meta.sensitive {
        r.fail(13, AT, format!("`{}` is a sensitive option: a bundle must not ask for credentials (specs/025 §2.3)", k));
    }
    if !meta.plain_options.is_empty() {
        r.note(13, AT, format!("userConfig options ({}) are ignored: Delphi has no per-install settings for a bundle", meta.plain_options.join(", ")));
    }
    for k in meta.ignored.iter().filter(|k| !matches!(k.as_str(), "mcpServers" | "userConfig" | "channels")) {
        r.note(13, AT, format!("`{}` is ignored by Delphi and left as written for Claude Code", k));
    }
    let (true, Some(m)) = (claude, m) else { return };
    let got = |k: &str| match k {
        "name" => &meta.name,
        "version" => &meta.version,
        "description" => &meta.description,
        "license" => &meta.license,
        "homepage" => &meta.homepage,
        _ => &None,
    };
    for (k, want) in [("name", &m.name), ("version", &m.version), ("description", &m.description), ("license", &m.license)] {
        match got(k) {
            Some(g) if g == want => {}
            Some(g) => r.fail(13, AT, format!("`{}` is `{}` but bundle.toml says `{}`", k, g, want)),
            None => r.fail(13, AT, format!("`{}` is missing (bundle.toml says `{}`)", k, want)),
        }
    }
    if let (Some(g), Some(want)) = (got("homepage"), bundle_homepage(tree)) {
        if *g != want {
            r.fail(13, AT, format!("`homepage` is `{}` but bundle.toml says `{}`", g, want));
        }
    }
    if let Some(a) = &meta.author_name {
        if *a != m.publisher_name {
            r.fail(13, AT, format!("`author.name` is `{}` but publisher.name says `{}`", a, m.publisher_name));
        }
    }
    if meta.author_email != m.publisher_contact && meta.author_email.is_some() {
        r.fail(13, AT, format!("`author.email` is `{}` but publisher.contact says `{}`", meta.author_email.as_deref().unwrap_or(""), m.publisher_contact.as_deref().unwrap_or("")));
    }
}

/// `bundle.homepage` is not kept in [`Manifest`]; read it again from the file (the manifest check already validated it).
fn bundle_homepage(tree: &Tree) -> Option<String> {
    let t: toml::Table = tree.text("bundle.toml")?.parse().ok()?;
    t.get("bundle")?.get("homepage")?.as_str().map(String::from)
}
