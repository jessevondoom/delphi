//! Step 7 of `specs/023` §10: provenance frontmatter on every knowledge and behavior document (§4.2), pointers (§4.4)
//! and the completeness of `knowledge/index.md` (§4.5).

use crate::ids::{body_of, frontmatter_of, get_str};
use crate::manifest::Manifest;
use crate::tree::Tree;
use crate::{Host, Report};
use regex::Regex;
use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

const SUMMARY_CHARS: usize = 160;
const MAX_POINTERS: usize = 8;

static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap());
static USERINFO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z]+://[^/]*@").unwrap());
static SIGNED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[?&](token|key|sig|signature|access_token|x-amz-signature)=").unwrap());

pub fn check(tree: &Tree, m: Option<&Manifest>, host: &dyn Host, r: &mut Report) {
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    let mut docs = tree.texts("knowledge/", ".md");
    docs.extend(tree.texts("behavior/", ".md"));
    for (path, text) in &docs {
        let fm = match frontmatter_of(text) {
            Some(fm) => fm,
            None => {
                let why = if text.starts_with("---\n") { "frontmatter is not a closed YAML mapping" } else { "missing provenance frontmatter (specs/001 §5)" };
                r.fail(7, path, why);
                continue;
            }
        };
        for e in host.provenance(text) {
            r.fail(7, path, e);
        }
        if let (Some(m), Some(sm)) = (m, get_str(&fm, "source_mission")) {
            if sm != m.name {
                r.fail(7, path, format!("source_mission `{}` must be the bundle name `{}`, never a local mission id", sm, m.name));
            }
        }
        if let Some(id) = get_str(&fm, "id") {
            if let Some(prev) = ids.insert(id.to_string(), path.clone()) {
                r.fail(7, path, format!("id `{}` is also used by {}", id, prev));
            }
        }
        match fm.get(Value::String("summary".into())) {
            None => {
                if path.starts_with("knowledge/") && path != "knowledge/index.md" {
                    r.warn(7, path, "no `summary` (the index and the org-context slice show it)");
                }
            }
            Some(Value::String(s)) if s.chars().count() <= SUMMARY_CHARS => {}
            Some(Value::String(_)) => r.fail(7, path, format!("summary is over {} characters", SUMMARY_CHARS)),
            Some(_) => r.fail(7, path, "summary must be a string"),
        }
        if let Some(d) = get_str(&fm, "review_by") {
            if !DATE.is_match(d) {
                r.fail(7, path, format!("review_by `{}` is not a YYYY-MM-DD date", d));
            }
        }
        crate::sources::check_frontmatter(tree, path, text, r);
        if let Some(p) = fm.get(Value::String("pointers".into())) {
            pointers(p, path, r);
        }
    }
    if tree.has_dir("knowledge") {
        index(tree, &docs, r);
    }
}

fn pointers(v: &Value, path: &str, r: &mut Report) {
    let Some(list) = v.as_sequence() else { return r.fail(7, path, "pointers must be a list") };
    if list.len() > MAX_POINTERS {
        r.fail(7, path, format!("{} pointers (at most {})", list.len(), MAX_POINTERS));
    }
    for (i, p) in list.iter().enumerate() {
        let at = format!("{} pointers[{}]", path, i + 1);
        let Some(map) = p.as_mapping() else {
            r.fail(7, &at, "must be a mapping");
            continue;
        };
        let s = |k: &str| get_str(map, k).unwrap_or("");
        let (scheme, locator) = (s("scheme"), s("locator"));
        if locator.is_empty() {
            r.fail(7, &at, "no locator");
            continue;
        }
        let pin_kind = map.get(Value::String("pin".into())).and_then(|x| x.as_mapping()).and_then(|x| get_str(x, "kind")).unwrap_or("none");
        let lower = locator.to_ascii_lowercase();
        if lower.starts_with("file:") || locator.starts_with('/') || locator.starts_with('~') || locator.starts_with('.') || locator.contains('\\') {
            r.fail(7, &at, "file: and local-path locators are refused (specs/012 §4)");
            continue;
        }
        if lower.starts_with("delphi:") {
            r.fail(7, &at, "delphi:// is not a pointer scheme; use xenia:// (specs/012 §4)");
            continue;
        }
        if lower == "xenia://cap" || lower.starts_with("xenia://cap/") {
            r.fail(7, &at, "xenia://cap/ is reserved for capability URIs (specs/007 Plane 2), not built");
            continue;
        }
        let allowed: &[&str] = match scheme {
            "git" => &["git-commit", "git-tree"],
            "https" => &["content-hash", "none"],
            "xenia" => &["none"],
            "drive" => &["drive-revision", "content-hash"],
            _ => {
                r.fail(7, &at, format!("scheme `{}` is not one of git, https, xenia, drive", scheme));
                continue;
            }
        };
        if !allowed.contains(&pin_kind) {
            r.fail(7, &at, format!("pin.kind `{}` is not allowed for scheme `{}`", pin_kind, scheme));
        }
        if scheme == "xenia" && !lower.starts_with("xenia://") {
            r.fail(7, &at, "a xenia locator is xenia://<kind>/<id>[@rev]");
        }
        if matches!(scheme, "https" | "git") {
            if !(lower.starts_with("https://") || (scheme == "git" && lower.starts_with("ssh://"))) {
                r.fail(7, &at, "locator must be an https URL (git also allows ssh://)");
            } else if USERINFO.is_match(&lower) && !lower.starts_with("ssh://git@") {
                r.fail(7, &at, "locator carries credentials");
            } else if SIGNED.is_match(&lower) {
                r.fail(7, &at, "locator carries a token or a signed-URL parameter");
            }
        }
        if s("label").chars().count() > 80 {
            r.fail(7, &at, "label is over 80 characters");
        }
        if s("access_hint").chars().count() > 120 {
            r.fail(7, &at, "access_hint is over 120 characters");
        }
    }
}

/// `knowledge/index.md`: one list item per file, `[title](path): summary`; no file missing, no line pointing at nothing.
fn index(tree: &Tree, docs: &BTreeMap<String, String>, r: &mut Report) {
    let Some(text) = tree.text("knowledge/index.md") else {
        return r.fail(7, "knowledge/index.md", "missing (required when knowledge/ exists)");
    };
    let item = Regex::new(r"(?m)^\s*[-*]\s+\[([^\]]*)\]\(([^)]*)\)\s*:\s*(.*)$").unwrap();
    let mut linked: BTreeSet<String> = BTreeSet::new();
    for c in item.captures_iter(body_of(&text)) {
        let target = c[2].trim();
        let full = format!("knowledge/{}", target.trim_start_matches("./"));
        if tree.get(&full).is_none() {
            r.fail(7, "knowledge/index.md", format!("`{}` points at nothing", target));
        } else if !linked.insert(full.clone()) {
            r.fail(7, "knowledge/index.md", format!("`{}` is listed twice", target));
        }
    }
    for path in docs.keys().filter(|p| p.starts_with("knowledge/") && p.as_str() != "knowledge/index.md") {
        if !linked.contains(path) {
            r.fail(7, "knowledge/index.md", format!("does not list {}", path.trim_start_matches("knowledge/")));
        }
    }
}
