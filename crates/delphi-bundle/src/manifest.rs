//! Step 4 of `specs/023` §10 (`bundle.toml`), step 13 (the Claude Code `plugin.json` agreement), step 14 (the evals
//! section) and the `--against` version-bump check of §9.

use crate::tree::Tree;
use crate::{Host, Options, Report, ROLES, SCHEMA};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use toml::{Table, Value};

/// The fields the rest of the crate reads. Anything else in the file was checked here and is not kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Manifest {
    pub schema: u32,
    pub name: String,
    pub org: String,
    pub version: String,
    pub title: String,
    pub description: String,
    pub license: String,
    pub audience: String,
    pub review_by: String,
    pub publisher_name: String,
    pub publisher_contact: Option<String>,
    pub publisher_did: Option<String>,
    pub sections: BTreeMap<String, bool>,
    /// `[agents.<name>] role`: the host role of each bundle agent.
    pub agents: BTreeMap<String, String>,
    pub touches_roles: Vec<String>,
    pub touches_connectors: Vec<String>,
    pub keywords: Vec<String>,
}

const SECTIONS: [&str; 6] = ["knowledge", "skills", "agents", "behavior", "evals", "vectors"];
const AUDIENCES: [&str; 4] = ["org", "team", "project", "person"];
/// Keys that state trust (`specs/012` §2.1) or grant authority (`specs/023` §3.1) are refused anywhere in the file.
const REJECTED: [&str; 16] = [
    "tier", "trust", "source_type", "endorsed", "verified", "approved", "ratified", "permissions", "allow", "allowed_tools", "hooks", "env", "secrets", "user_config", "mcp_servers", "credentials",
];

/// A bundle whose `schema` is a major this build does not know is refused alone (AC-11).
pub fn unsupported_schema(dir: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("bundle.toml")).ok()?;
    let t: Table = text.parse().ok()?;
    let s = t.get("bundle")?.as_table()?.get("schema")?.as_integer()?;
    (s > SCHEMA as i64).then(|| format!("unsupported-schema: bundle.schema is {} and this build reads schema {} (keep the last added version loaded)", s, SCHEMA))
}

fn scan_rejected(v: &Value, path: &str, r: &mut Report) {
    if let Value::Table(t) = v {
        for (k, v) in t {
            let here = if path.is_empty() { k.clone() } else { format!("{}.{}", path, k) };
            if REJECTED.contains(&k.as_str()) {
                r.fail(4, "bundle.toml", format!("`{}` is not allowed: a bundle states no trust and grants nothing (specs/023 §3.1)", here));
            }
            scan_rejected(v, &here, r);
        }
    }
}

fn unknown_keys(t: &Table, allowed: &[&str], table: &str, r: &mut Report) {
    for k in t.keys() {
        if !allowed.contains(&k.as_str()) && !REJECTED.contains(&k.as_str()) {
            r.fail(4, "bundle.toml", format!("unknown key `{}` in [{}] (an ignored key is a smuggling channel)", k, table));
        }
    }
}

fn string(t: &Table, k: &str, table: &str, r: &mut Report) -> Option<String> {
    match t.get(k) {
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            r.fail(4, "bundle.toml", format!("[{}] `{}` must be a string", table, k));
            None
        }
        None => None,
    }
}

fn required(t: &Table, k: &str, table: &str, r: &mut Report) -> String {
    match string(t, k, table, r) {
        Some(s) => s,
        None => {
            if !t.contains_key(k) {
                r.fail(4, "bundle.toml", format!("[{}] is missing required `{}`", table, k));
            }
            String::new()
        }
    }
}

fn strings(t: &Table, k: &str, table: &str, r: &mut Report) -> Vec<String> {
    match t.get(k) {
        None => Vec::new(),
        Some(Value::Array(a)) if a.iter().all(|x| x.is_str()) => a.iter().map(|x| x.as_str().unwrap().to_string()).collect(),
        Some(_) => {
            r.fail(4, "bundle.toml", format!("[{}] `{}` must be a list of strings", table, k));
            Vec::new()
        }
    }
}

fn sub<'a>(root: &'a Table, k: &str, r: &mut Report) -> Option<&'a Table> {
    match root.get(k) {
        None => None,
        Some(Value::Table(t)) => Some(t),
        Some(_) => {
            r.fail(4, "bundle.toml", format!("`{}` must be a table", k));
            None
        }
    }
}

/// Today as `YYYY-MM-DD` (UTC), from the system clock.
fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn valid_date(s: &str) -> bool {
    let Some(c) = Regex::new(r"^(\d{4})-(\d{2})-(\d{2})$").unwrap().captures(s) else { return false };
    let (y, m, d): (u32, u32, u32) = (c[1].parse().unwrap(), c[2].parse().unwrap(), c[3].parse().unwrap());
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    (1..=12).contains(&m) && d >= 1 && d <= days[(m - 1) as usize]
}

pub fn check(tree: &Tree, opts: &Options, r: &mut Report) -> Option<Manifest> {
    let Some(text) = tree.text("bundle.toml") else {
        r.fail(4, "bundle.toml", "missing");
        return None;
    };
    let root: Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            r.fail(4, "bundle.toml", format!("parse error: {}", e.message()));
            return None;
        }
    };
    scan_rejected(&Value::Table(root.clone()), "", r);
    unknown_keys(&root, &["bundle", "compat", "publisher", "applies_to", "sections", "agents", "touches", "overlays"], "(top level)", r);

    let mut m = Manifest::default();
    let empty = Table::new();
    let b = sub(&root, "bundle", r).unwrap_or_else(|| {
        r.fail(4, "bundle.toml", "missing [bundle]");
        &empty
    });
    unknown_keys(b, &["schema", "name", "org", "version", "title", "description", "license", "homepage", "audience", "review_by", "language"], "bundle", r);
    match b.get("schema") {
        Some(Value::Integer(s)) if *s == SCHEMA as i64 => m.schema = SCHEMA,
        Some(Value::Integer(s)) => r.fail(4, "bundle.toml", format!("bundle.schema is {} (this build reads {})", s, SCHEMA)),
        _ => r.fail(4, "bundle.toml", "[bundle] `schema` must be an integer"),
    }
    m.name = required(b, "name", "bundle", r);
    if !m.name.is_empty() && !Regex::new(r"^[a-z0-9][a-z0-9-]{0,47}$").unwrap().is_match(&m.name) {
        r.fail(4, "bundle.toml", format!("name `{}` must match ^[a-z0-9][a-z0-9-]{{0,47}}$", m.name));
    }
    m.org = required(b, "org", "bundle", r);
    if !m.org.is_empty() && !Regex::new(r"^[a-z0-9-]{1,32}$").unwrap().is_match(&m.org) {
        r.fail(4, "bundle.toml", format!("org `{}` must match ^[a-z0-9-]{{1,32}}$", m.org));
    }
    m.version = required(b, "version", "bundle", r);
    if !m.version.is_empty() && semver::Version::parse(&m.version).is_err() {
        r.fail(4, "bundle.toml", format!("version `{}` is not semver", m.version));
    }
    m.title = required(b, "title", "bundle", r);
    if m.title.chars().count() > 80 {
        r.fail(4, "bundle.toml", "title is over 80 characters");
    }
    m.description = required(b, "description", "bundle", r);
    if m.description.chars().count() > 200 {
        r.fail(4, "bundle.toml", "description is over 200 characters");
    }
    if m.description.contains("](") {
        r.fail(4, "bundle.toml", "description must be plain text (no Markdown links)");
    }
    m.license = required(b, "license", "bundle", r);
    if let Some(h) = string(b, "homepage", "bundle", r) {
        if !h.starts_with("https://") {
            r.fail(4, "bundle.toml", "homepage must be an https URL");
        }
    }
    m.audience = required(b, "audience", "bundle", r);
    if !m.audience.is_empty() && !AUDIENCES.contains(&m.audience.as_str()) {
        r.fail(4, "bundle.toml", format!("audience `{}` is not one of {:?}", m.audience, AUDIENCES));
    }
    m.review_by = required(b, "review_by", "bundle", r);
    if !m.review_by.is_empty() {
        if !valid_date(&m.review_by) {
            r.fail(4, "bundle.toml", format!("review_by `{}` is not a YYYY-MM-DD date", m.review_by));
        } else if m.review_by < today() {
            let msg = format!("review_by {} is in the past", m.review_by);
            if opts.packaging { r.fail(4, "bundle.toml", msg) } else { r.warn(4, "bundle.toml", format!("{} (every surface will show the bundle as stale)", msg)) }
        }
    }
    if let Some(l) = string(b, "language", "bundle", r) {
        if !Regex::new(r"^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$").unwrap().is_match(&l) {
            r.fail(4, "bundle.toml", format!("language `{}` is not a BCP 47 tag", l));
        }
    }

    if let Some(c) = sub(&root, "compat", r) {
        unknown_keys(c, &["delphi", "bundle_schema"], "compat", r);
        if let Some(req) = string(c, "delphi", "compat", r) {
            if semver::VersionReq::parse(&req).is_err() {
                r.fail(4, "bundle.toml", format!("compat.delphi `{}` is not a semver requirement", req));
            }
        }
        match c.get("bundle_schema") {
            None => {}
            Some(Value::Integer(s)) if *s == m.schema as i64 => {}
            Some(_) => r.fail(4, "bundle.toml", "compat.bundle_schema must equal bundle.schema"),
        }
    }

    match sub(&root, "publisher", r) {
        None => r.fail(4, "bundle.toml", "missing [publisher]"),
        Some(p) => {
            unknown_keys(p, &["name", "contact", "did", "predecessors"], "publisher", r);
            m.publisher_name = required(p, "name", "publisher", r);
            if m.publisher_name.chars().count() > 80 {
                r.fail(4, "bundle.toml", "publisher.name is over 80 characters");
            }
            m.publisher_contact = string(p, "contact", "publisher", r);
            m.publisher_did = string(p, "did", "publisher", r);
            if m.publisher_did.as_deref().is_some_and(|d| !d.starts_with("did:key:z")) {
                r.fail(4, "bundle.toml", "publisher.did must be a did:key");
            }
            for d in strings(p, "predecessors", "publisher", r) {
                if !d.starts_with("did:key:z") {
                    r.fail(4, "bundle.toml", format!("publisher.predecessors: `{}` is not a did:key", d));
                }
            }
        }
    }

    if let Some(a) = sub(&root, "applies_to", r) {
        unknown_keys(a, &["repos", "tech", "paths"], "applies_to", r);
        for k in ["repos", "tech", "paths"] {
            let _ = strings(a, k, "applies_to", r);
        }
        m.keywords = strings(a, "tech", "applies_to", r);
    }

    if let Some(s) = sub(&root, "sections", r) {
        unknown_keys(s, &SECTIONS, "sections", r);
        for k in SECTIONS {
            match s.get(k) {
                None => {}
                Some(Value::Boolean(on)) => {
                    m.sections.insert(k.into(), *on);
                    if *on && !tree.has_dir(k) {
                        r.fail(4, "bundle.toml", format!("[sections] {} = true but there is no {}/ directory", k, k));
                    }
                }
                Some(_) => r.fail(4, "bundle.toml", format!("[sections] `{}` must be true or false", k)),
            }
        }
    }
    for s in SECTIONS {
        if tree.has_dir(s) && m.sections.get(s) != Some(&true) {
            r.note(4, "bundle.toml", format!("{}/ exists but [sections] does not list it; it is ignored", s));
        }
    }

    if let Some(a) = sub(&root, "agents", r) {
        for (name, v) in a {
            let Value::Table(t) = v else {
                r.fail(4, "bundle.toml", format!("[agents.{}] must be a table", name));
                continue;
            };
            unknown_keys(t, &["role"], &format!("agents.{}", name), r);
            let role = required(t, "role", &format!("agents.{}", name), r);
            if !role.is_empty() && !ROLES.contains(&role.as_str()) {
                r.fail(4, "bundle.toml", format!("[agents.{}] role `{}` is not a canonical role id", name, role));
            }
            if tree.get(&format!("agents/{}.md", name)).is_none() {
                r.fail(4, "bundle.toml", format!("[agents.{}] has no agents/{}.md", name, name));
            }
            m.agents.insert(name.clone(), role);
        }
    }

    if let Some(t) = sub(&root, "touches", r) {
        unknown_keys(t, &["roles", "connectors", "network"], "touches", r);
        m.touches_roles = strings(t, "roles", "touches", r);
        for role in &m.touches_roles {
            if !ROLES.contains(&role.as_str()) {
                r.fail(4, "bundle.toml", format!("touches.roles: `{}` is not a canonical role id", role));
            }
        }
        m.touches_connectors = strings(t, "connectors", "touches", r);
        if t.get("network") == Some(&Value::Boolean(true)) {
            r.fail(4, "bundle.toml", "touches.network = true is refused in schema 1 (a bundle never needs the network at add)");
        }
    }

    if let Some(o) = sub(&root, "overlays", r) {
        unknown_keys(o, &["depends_on", "supersedes"], "overlays", r);
        let re = Regex::new(r"^[a-z0-9][a-z0-9-]{0,47}@.+$").unwrap();
        for d in strings(o, "depends_on", "overlays", r) {
            if !re.is_match(&d) || semver::VersionReq::parse(d.split_once('@').unwrap().1).is_err() {
                r.fail(4, "bundle.toml", format!("overlays.depends_on: `{}` is not `name@<semver requirement>`", d));
            }
        }
        let _ = strings(o, "supersedes", "overlays", r);
    }
    Some(m)
}

// ── step 13: --claude ────────────────────────────────────────────────────────

/// The `.claude-plugin/plugin.json` this bundle generates (`specs/023` §11); the bundle hash fills `delphi.bundle_hash`.
pub fn plugin_json(m: &Manifest, bundle_hash: Option<&str>) -> String {
    let mut author = serde_json::json!({"name": m.publisher_name});
    if let Some(c) = &m.publisher_contact {
        author["email"] = c.clone().into();
    }
    let mut meta = serde_json::json!({"delphi.bundle": m.schema.to_string(), "delphi.org": m.org});
    if let Some(h) = bundle_hash {
        meta["delphi.bundle_hash"] = h.into();
    }
    let mut keywords: Vec<String> = vec![m.org.clone()];
    keywords.extend(m.keywords.iter().cloned());
    let v = serde_json::json!({"name": m.name, "version": m.version, "description": m.description, "author": author, "license": m.license, "keywords": keywords, "metadata": meta});
    serde_json::to_string_pretty(&v).unwrap() + "\n"
}

pub fn check_claude(tree: &Tree, m: Option<&Manifest>, r: &mut Report) {
    let Some(m) = m else { return };
    let Some(text) = tree.text(".claude-plugin/plugin.json") else {
        r.note(13, ".claude-plugin/plugin.json", "absent; `delphi bundle manifest --claude` generates it");
        return;
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return r.fail(13, ".claude-plugin/plugin.json", format!("not valid JSON: {}", e)),
    };
    for (k, want) in [("name", &m.name), ("version", &m.version), ("description", &m.description), ("license", &m.license)] {
        match v.get(k).and_then(|x| x.as_str()) {
            Some(got) if got == want => {}
            Some(got) => r.fail(13, ".claude-plugin/plugin.json", format!("`{}` is `{}` but bundle.toml says `{}`", k, got, want)),
            None => r.fail(13, ".claude-plugin/plugin.json", format!("`{}` is missing (bundle.toml says `{}`)", k, want)),
        }
    }
}

// ── step 14: evals ───────────────────────────────────────────────────────────

pub fn check_evals(tree: &Tree, m: Option<&Manifest>, r: &mut Report) {
    if !m.is_some_and(|m| m.sections.get("evals") == Some(&true)) {
        return;
    }
    let path = "evals/delphi/kb-eval.json";
    let Some(text) = tree.text(path) else {
        return r.fail(14, path, "missing (sections.evals = true needs at least 10 questions)");
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return r.fail(14, path, format!("not valid JSON: {}", e)),
    };
    let Some(qs) = v.as_array() else { return r.fail(14, path, "must be a JSON array of questions") };
    let mut controls = 0;
    for (i, q) in qs.iter().enumerate() {
        if !q.get("q").and_then(|x| x.as_str()).is_some_and(|s| !s.trim().is_empty()) {
            r.fail(14, path, format!("question {} has no `q`", i + 1));
        }
        if q.get("expect").is_some_and(|e| e.is_null()) {
            controls += 1;
        }
    }
    if qs.len() < 10 {
        r.fail(14, path, format!("{} questions (at least 10, written before tuning)", qs.len()));
    }
    if controls < 2 {
        r.fail(14, path, format!("{} off-topic controls (at least 2 questions with `\"expect\": null`)", controls));
    }
    let headings = ["Hypothesis", "Metric", "Stopping rule"];
    for e in tree.under("evals/delphi/prereg/").filter(|e| e.path.ends_with(".md")) {
        let t = tree.text(&e.path).unwrap_or_default();
        for h in headings {
            if !t.lines().any(|l| l.starts_with('#') && l.trim_start_matches('#').trim().eq_ignore_ascii_case(h)) {
                r.fail(14, &e.path, format!("pre-registration lacks a `{}` heading (specs/027 §6.2)", h));
            }
        }
    }
}

// ── --against: the smallest honest version bump (specs/023 §9) ───────────────

pub fn check_against(tree: &Tree, m: Option<&Manifest>, prev: &std::path::Path, host: &dyn Host, r: &mut Report) {
    let _ = host;
    let Some(m) = m else { return };
    let mut scratch = Report::default();
    let prev_tree = crate::tree::scan(prev, &mut scratch);
    let Some(pm) = check(&prev_tree, &Options::default(), &mut scratch) else {
        return r.fail(4, "--against", "the previous bundle has no readable bundle.toml");
    };
    let (Ok(cur), Ok(old)) = (semver::Version::parse(&m.version), semver::Version::parse(&pm.version)) else { return };
    let (a, b) = (crate::ids::collect(&prev_tree), crate::ids::collect(tree));
    let removed: Vec<&String> = a.difference(&b).collect();
    let added = b.difference(&a).count();
    let changed = changed_files(&prev_tree, tree);
    let need = if !removed.is_empty() {
        semver::Version::new(old.major + 1, 0, 0)
    } else if added > 0 {
        semver::Version::new(old.major, old.minor + 1, 0)
    } else if changed {
        semver::Version::new(old.major, old.minor, old.patch + 1)
    } else {
        old.clone()
    };
    if cur < need {
        let why = if !removed.is_empty() { format!("removed or renamed ids: {}", removed.iter().take(5).map(|s| s.as_str()).collect::<Vec<_>>().join(", ")) } else if added > 0 { "new ids".into() } else { "changed files".into() };
        r.fail(4, "bundle.toml", format!("version {} is lower than the smallest honest bump from {}: {} ({})", m.version, pm.version, need, why));
    } else {
        r.note(4, "bundle.toml", format!("version {} is at least the smallest honest bump from {} ({})", m.version, pm.version, need));
    }
}

fn changed_files(a: &Tree, b: &Tree) -> bool {
    let h = |t: &Tree| -> BTreeSet<(String, [u8; 32])> {
        t.entries
            .iter()
            .filter(|e| !e.path.starts_with(".delphi-bundle/") && e.path != "bundle.toml")
            .filter_map(|e| Some((e.path.clone(), *blake3::hash(&t.read(&e.path)?).as_bytes())))
            .collect()
    };
    h(a) != h(b)
}
