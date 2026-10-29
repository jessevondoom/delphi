//! The optional `sources/` section (`specs/023` §2.1, step 7a; `specs/024` §4.1a; `specs/055` §3.2.1 and §3.3).
//!
//! A source is data: its text is never instructions and never raises a tier. What this module decides is whether a
//! source may *travel*: only licences that allow redistribution are accepted, and the refusal names the source id and
//! never its content. The same function runs at `validate` and at `add`.

use crate::ids::frontmatter_of;
use crate::manifest::Manifest;
use crate::tree::Tree;
use crate::Report;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The separate size class of `sources/` (`specs/023` §2.1), outside the 2,000-file and 64 MiB tree limits.
pub const MAX_FILES: usize = 20_000;
pub const MAX_TEXT_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_ASSET_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// Content types an asset may have, by extension.
const ASSET_EXTS: [&str; 7] = ["png", "jpg", "jpeg", "webp", "svg", "pdf", "csv"];
const CAPTURE_LEVELS: [&str; 3] = ["full", "excerpts", "pointer"];
/// Licence classes that never travel (`specs/055` §3.3).
const REFUSED: [&str; 3] = ["all-rights-reserved", "unknown", "private"];
const CITATION_STRINGS: [&str; 14] = [
    "type", "title", "container-title", "publisher", "publisher-place", "volume", "issue", "page", "edition", "URL", "DOI", "ISBN", "ISSN", "language",
];
const DELPHI_STRINGS: [&str; 7] = ["rights_holder", "attribution_text", "content_hash", "version", "origin", "source_class", "canonical_url"];

static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{0,63}$").unwrap());
static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}").unwrap());
static CC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^CC-BY(-SA)?-[1-4]\.0(-[A-Z]{2,3})?$").unwrap());

pub fn in_sources(path: &str) -> bool {
    path.starts_with("sources/")
}

/// Is `path` (under `sources/`) an asset, which has the larger per-file cap?
pub fn is_asset(path: &str) -> bool {
    path.split('/').nth(2) == Some("assets")
}

/// What a licence string means for redistribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Licence {
    /// An open licence, an `own` record or a recorded public-domain basis.
    Open { attribution: bool, share_alike: bool },
    /// One of the refused classes, or something that is not a recognized open licence: the reason.
    Refused(String),
}

fn open_spdx(id: &str) -> Option<(bool, bool)> {
    const NO_ATTRIBUTION: [&str; 5] = ["CC0-1.0", "Unlicense", "0BSD", "PDDL-1.0", "WTFPL"];
    const ATTRIBUTION: [&str; 17] = [
        "MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "BSL-1.0", "MPL-2.0", "ODC-By-1.0", "OGL-UK-3.0", "OGL-Canada-2.0", "Artistic-2.0", "CC-BY-3.0", "CC-BY-4.0", "MIT-0", "NCSA", "Python-2.0",
    ];
    const SHARE_ALIKE: [&str; 9] = ["ODbL-1.0", "GFDL-1.3-or-later", "GFDL-1.3-only", "GPL-3.0-or-later", "GPL-3.0-only", "AGPL-3.0-or-later", "LGPL-3.0-or-later", "EUPL-1.2", "GPL-2.0-or-later"];
    if NO_ATTRIBUTION.contains(&id) {
        Some((false, false))
    } else if CC.is_match(id) {
        Some((true, id.contains("-SA-")))
    } else if ATTRIBUTION.contains(&id) {
        Some((true, false))
    } else if SHARE_ALIKE.contains(&id) {
        Some((true, true))
    } else {
        None
    }
}

/// Classify a `licence` value (`specs/023` §2.1): an open SPDX id, `public-domain:<jurisdiction>:<basis>`, or `own`.
pub fn classify(licence: &str) -> Licence {
    let l = licence.trim();
    if REFUSED.contains(&l.to_ascii_lowercase().as_str()) {
        return Licence::Refused(format!("licence class `{}` does not allow redistribution", l.to_ascii_lowercase()));
    }
    if l == "own" {
        return Licence::Open { attribution: false, share_alike: false };
    }
    if let Some(rest) = l.strip_prefix("public-domain:") {
        let mut parts = rest.splitn(2, ':');
        let (j, basis) = (parts.next().unwrap_or("").trim(), parts.next().unwrap_or("").trim());
        return if j.is_empty() || basis.is_empty() {
            Licence::Refused("a public-domain licence records its jurisdiction and basis: `public-domain:<jurisdiction>:<basis>`".into())
        } else {
            Licence::Open { attribution: false, share_alike: false }
        };
    }
    match open_spdx(l) {
        Some((attribution, share_alike)) => Licence::Open { attribution, share_alike },
        None => Licence::Refused(format!("`{}` is not an open SPDX licence, `public-domain:<jurisdiction>:<basis>` or `own`", l.chars().take(60).collect::<String>())),
    }
}

fn notes_share_alike(attribution: &str, licence: &str) -> bool {
    let norm = |s: &str| s.to_ascii_lowercase().replace(['-', ' ', '_'], "");
    let (a, l) = (norm(attribution), norm(licence));
    a.contains("sharealike") || a.contains("samelicen") || (!l.is_empty() && a.contains(&l))
}

/// One source as `validate --report` lists it: counts and sizes, never text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSummary {
    pub id: String,
    pub licence: String,
    pub capture_level: String,
    pub text_bytes: u64,
    pub assets: usize,
    pub asset_bytes: u64,
}

/// The sources of the tree, best effort (a source with an unreadable `meta.json` shows `?`). Nothing is checked here.
pub fn list(tree: &Tree) -> Vec<SourceSummary> {
    let mut by_id: BTreeMap<String, SourceSummary> = BTreeMap::new();
    for e in tree.under("sources/") {
        let Some(id) = e.path.split('/').nth(1) else { continue };
        let s = by_id.entry(id.to_string()).or_insert_with(|| {
            let meta = tree.text(&format!("sources/{}/meta.json", id)).and_then(|t| serde_json::from_str::<Value>(&t).ok());
            let field = |k: &str| meta.as_ref().and_then(|m| m[k].as_str()).map(|s| s.chars().take(80).collect::<String>()).unwrap_or_else(|| "?".into());
            SourceSummary { id: id.to_string(), licence: field("licence"), capture_level: field("capture_level"), text_bytes: 0, assets: 0, asset_bytes: 0 }
        });
        if is_asset(&e.path) {
            s.assets += 1;
            s.asset_bytes += e.bytes;
        } else {
            s.text_bytes += e.bytes;
        }
    }
    by_id.into_values().collect()
}

fn str_field<'a>(meta: &'a serde_json::Map<String, Value>, k: &str) -> Option<&'a str> {
    meta.get(k).and_then(|v| v.as_str())
}

pub fn check(tree: &Tree, m: Option<&Manifest>, r: &mut Report) {
    if !tree.has_dir("sources") {
        return;
    }
    // An undeclared section is not skipped: a source that is not listed must still pass the licence gate.
    if m.is_some_and(|m| m.sections.get("sources") != Some(&true)) {
        r.fail(7, "sources/", "exists but [sections] sources is not true; declare the section so that its licences are checked");
    }
    let mut ids: Vec<String> = tree.under("sources/").filter_map(|e| e.path.split('/').nth(1).map(String::from)).collect();
    ids.dedup();
    for id in &ids {
        check_source(tree, id, r);
    }
}

fn check_source(tree: &Tree, id: &str, r: &mut Report) {
    let dir = format!("sources/{}", id);
    if !ID.is_match(id) {
        r.fail(7, &dir, "source id must match ^[a-z0-9][a-z0-9-]{0,63}$");
    }
    for e in tree.under(&format!("{}/", dir)) {
        let rest = &e.path[dir.len() + 1..];
        if is_asset(&e.path) {
            let ext = rest.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            if !rest.contains('.') || !ASSET_EXTS.contains(&ext.as_str()) {
                r.fail(7, &e.path, "an asset must be image/png, image/jpeg, image/webp, image/svg+xml, application/pdf or text/csv (by extension)");
            }
        } else if !matches!(rest, "source.md" | "meta.json" | "excerpts.json") {
            r.fail(7, &e.path, "not part of the sources layout (source.md, meta.json, excerpts.json, assets/)");
        }
    }
    let has_text = tree.get(&format!("{}/source.md", dir)).is_some();
    let Some(raw) = tree.text(&format!("{}/meta.json", dir)) else {
        return r.fail(7, &dir, "source id has no readable meta.json (the record of its licence and citation)");
    };
    let at = format!("{}/meta.json", dir);
    let meta = match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(o)) => o,
        Ok(_) => return r.fail(7, &at, "must be a JSON object (CSL-JSON with Delphi's fields)"),
        Err(e) => return r.fail(7, &at, format!("not strict JSON: {}", e)),
    };
    if let Some(j) = tree.text(&format!("{}/excerpts.json", dir)) {
        if !matches!(serde_json::from_str::<Value>(&j), Ok(Value::Array(_)) | Ok(Value::Object(_))) {
            r.fail(7, &format!("{}/excerpts.json", dir), "not strict JSON (an array or an object)");
        }
    }
    for k in CITATION_STRINGS.iter().chain(DELPHI_STRINGS.iter()) {
        if meta.get(*k).is_some_and(|v| !v.is_string()) {
            r.fail(7, &at, format!("`{}` must be a string", k));
        }
    }
    for k in ["type", "title"] {
        if str_field(&meta, k).is_none_or(|s| s.trim().is_empty()) {
            r.fail(7, &at, format!("`{}` is required (CSL-JSON)", k));
        }
    }
    for k in ["author", "editor", "translator"] {
        let Some(v) = meta.get(k) else { continue };
        let ok = v.as_array().is_some_and(|a| a.iter().all(|p| p.as_object().is_some_and(|p| ["family", "given", "literal"].iter().any(|f| p.get(*f).is_some_and(|x| x.is_string())))));
        if !ok {
            r.fail(7, &at, format!("`{}` must be a list of names with `family`/`given` or `literal`", k));
        }
    }
    if meta.get("issued").is_some_and(|v| !v.is_object() && !v.is_string()) {
        r.fail(7, &at, "`issued` must be a CSL date object or a string");
    }
    if meta.get("tags").is_some_and(|v| !v.as_array().is_some_and(|a| a.iter().all(|t| t.is_string()))) {
        r.fail(7, &at, "`tags` must be a list of strings");
    }
    if meta.get("share_alike").is_some_and(|v| !v.is_boolean()) {
        r.fail(7, &at, "`share_alike` must be true or false");
    }
    for k in ["retrieved", "last_checked"] {
        if str_field(&meta, k).is_some_and(|s| !DATE.is_match(s)) {
            r.fail(7, &at, format!("`{}` must be a date (YYYY-MM-DD, optionally with a time)", k));
        }
    }
    if let Some(f) = str_field(&meta, "fetch_mode") {
        if !["daemon", "operator"].contains(&f) {
            r.fail(7, &at, "`fetch_mode` must be `daemon` or `operator`");
        }
    }
    for k in ["canonical_url", "URL"] {
        if let Some(u) = str_field(&meta, k) {
            let l = u.to_ascii_lowercase();
            if !l.starts_with("https://") || l.split('/').nth(2).is_some_and(|h| h.contains('@')) {
                r.fail(7, &at, format!("`{}` must be an https URL without credentials", k));
            }
        }
    }
    match str_field(&meta, "capture_level") {
        None => {}
        Some(l) if !CAPTURE_LEVELS.contains(&l) => r.fail(7, &at, "`capture_level` must be full, excerpts or pointer"),
        Some("pointer") if has_text => r.fail(7, &at, "capture_level is pointer but source.md is present"),
        Some("full") if !has_text => r.fail(7, &at, "capture_level is full but there is no source.md"),
        Some(_) => {}
    }

    // The licence gate. The message carries the id and the class, never content.
    if meta.get("licence").is_some_and(|v| !v.is_string()) {
        return r.fail(7, &at, "`licence` must be a string");
    }
    let Some(licence) = str_field(&meta, "licence").map(str::trim).filter(|s| !s.is_empty()) else {
        return r.fail(7, &at, format!("source `{}` has no `licence`; a source without one cannot travel", id));
    };
    match classify(licence) {
        Licence::Refused(why) => r.fail(7, &at, format!("source `{}` refused: {}", id, why)),
        Licence::Open { attribution, share_alike } => {
            let text = str_field(&meta, "attribution_text").map(str::trim).unwrap_or("");
            if attribution && text.is_empty() {
                r.fail(7, &at, format!("source `{}`: licence `{}` requires a non-empty `attribution_text`", id, licence));
            }
            if share_alike {
                if meta.get("share_alike").and_then(|v| v.as_bool()) != Some(true) {
                    r.fail(7, &at, format!("source `{}`: share-alike licence `{}` requires share_alike = true", id, licence));
                }
                if !notes_share_alike(text, licence) {
                    r.fail(7, &at, format!("source `{}`: share-alike licence `{}` requires the share-alike notice in `attribution_text`", id, licence));
                }
            }
        }
    }
}

/// The `sources:` and `licence` frontmatter of the knowledge documents (`specs/023` §4.2): every cited id must exist, and a
/// file's own licence must be in a redistributable class.
pub fn check_frontmatter(tree: &Tree, path: &str, text: &str, r: &mut Report) {
    let Some(fm) = frontmatter_of(text) else { return };
    if let Some(v) = fm.get(serde_yaml::Value::String("sources".into())) {
        match v.as_sequence() {
            None => r.fail(7, path, "sources must be a list of ids under sources/"),
            Some(list) => {
                for item in list {
                    let id = item.as_str().unwrap_or("");
                    if !ID.is_match(id) {
                        r.fail(7, path, "a `sources` entry must be a source id (^[a-z0-9][a-z0-9-]{0,63}$)");
                    } else if tree.get(&format!("sources/{}/meta.json", id)).is_none() {
                        r.fail(7, path, format!("sources cites `{}`, which does not exist under sources/", id));
                    }
                }
            }
        }
    }
    match fm.get(serde_yaml::Value::String("licence".into())) {
        None => {}
        Some(serde_yaml::Value::String(l)) => {
            if let Licence::Refused(why) = classify(l) {
                r.fail(7, path, format!("licence: {}", why));
            }
        }
        Some(_) => r.fail(7, path, "licence must be a string"),
    }
}

/// Step 11 for sources: instruction-shaped lines in `source.md`, as warnings (a source may quote them).
pub fn check_instructions(tree: &Tree, r: &mut Report) {
    for e in tree.under("sources/").filter(|e| e.path.ends_with("/source.md") && e.path.split('/').count() == 3) {
        let Some(text) = tree.text(&e.path) else { continue };
        for (line, _) in crate::relax::no_relaxation(&text) {
            r.warn(11, &format!("{}:{}", e.path, line), "reads like an instruction that relaxes a rule; a source is data and is never followed");
        }
    }
}
