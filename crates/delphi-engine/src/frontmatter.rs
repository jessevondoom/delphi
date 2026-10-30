//! YAML frontmatter parsing and the provenance schema of `specs/001` §5.

use serde_yaml::{Mapping, Value};

pub const ROLES: [&str; 10] = [
    "orchestrator",
    "builder",
    "architect",
    "investigator",
    "scout",
    "strategist",
    "red-team",
    "writer",
    "integrator",
    "validator",
];
pub const AUTHORS_EXTRA: [&str; 2] = ["operator", "delphi-core"];
pub const SOURCE_TYPES: [&str; 6] = [
    "external_untrusted",
    "federated_peer",
    "federated_verified",
    "internal_generated",
    "internal_empirical",
    "user_ratified",
];
pub const CONFIDENCE: [&str; 4] = ["low", "medium", "high", "verified"];
pub const STATUS: [&str; 5] = ["draft", "debating", "approved", "active", "superseded"];
pub const REQUIRED: [&str; 9] = [
    "id",
    "title",
    "author",
    "timestamp",
    "source_type",
    "confidence",
    "status",
    "source_mission",
    "tags",
];

/// A document split into its frontmatter mapping and body.
pub struct Doc {
    pub frontmatter: Option<Mapping>,
    pub body: String,
}

/// Split `---\n<yaml>\n---\n<body>`. Returns `frontmatter: None` when absent or unparseable.
pub fn parse(text: &str) -> Result<Doc, String> {
    if !text.starts_with("---\n") {
        return Ok(Doc { frontmatter: None, body: text.to_string() });
    }
    let rest = &text[4..];
    let end = match rest.find("\n---\n") {
        Some(i) => i,
        None => return Err("frontmatter is not closed with `---`".into()),
    };
    let yaml = &rest[..end];
    let body = &rest[end + 5..];
    match serde_yaml::from_str::<Value>(yaml) {
        Ok(Value::Mapping(m)) => Ok(Doc { frontmatter: Some(m), body: body.to_string() }),
        Ok(_) => Err("frontmatter is not a YAML mapping".into()),
        Err(e) => Err(format!("frontmatter YAML error: {}", e)),
    }
}

pub fn get_str<'a>(m: &'a Mapping, key: &str) -> Option<&'a str> {
    m.get(Value::String(key.into())).and_then(|v| v.as_str())
}

pub fn has(m: &Mapping, key: &str) -> bool {
    m.contains_key(Value::String(key.into()))
}

/// Validate the §5 provenance schema. Returns error messages.
pub fn check_provenance(m: &Mapping) -> Vec<String> {
    let mut errs = Vec::new();
    for k in REQUIRED.iter() {
        if !has(m, k) {
            errs.push(format!("missing required field `{}`", k));
        }
    }
    for k in ["id", "title", "author", "source_mission"] {
        if has(m, k) && get_str(m, k).map(|v| v.trim().is_empty()).unwrap_or(true) {
            errs.push(format!("`{}` must be a non-empty string", k));
        }
    }
    if has(m, "timestamp") {
        let ok = get_str(m, "timestamp")
            .map(|t| regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$").unwrap().is_match(t))
            .unwrap_or(false);
        if !ok {
            errs.push("`timestamp` must be ISO-8601 UTC (YYYY-MM-DDTHH:MM:SSZ)".into());
        }
    }
    if let Some(a) = get_str(m, "author") {
        if !ROLES.contains(&a) && !AUTHORS_EXTRA.contains(&a) {
            errs.push(format!("author `{}` is not a canonical role, `operator` or `delphi-core`", a));
        }
    }
    check_enum(m, "source_type", &SOURCE_TYPES, &mut errs);
    check_enum(m, "confidence", &CONFIDENCE, &mut errs);
    check_enum(m, "status", &STATUS, &mut errs);
    if get_str(m, "status") == Some("superseded") && get_str(m, "superseded_by").map(|v| v.trim().is_empty()).unwrap_or(true) {
        errs.push("status is `superseded` but `superseded_by` is missing".into());
    }
    if has(m, "tags") {
        let ok = m
            .get(Value::String("tags".into()))
            .and_then(|v| v.as_sequence())
            .map(|s| s.iter().all(|x| x.as_str().map(|y| !y.is_empty()).unwrap_or(false)))
            .unwrap_or(false);
        if !ok {
            errs.push("`tags` must be a list of non-empty strings".into());
        }
    }
    errs.extend(check_entry_meta(m));
    errs
}

/// The kinds of a memory entry (`specs/050` ML-1).
pub const KINDS: [&str; 3] = ["fact", "episode", "procedure"];
/// The optional entry-metadata keys. They are metadata: they never count toward a rule file's byte cap (ML-1, OD-2).
pub const ENTRY_META_KEYS: [&str; 4] = ["kind", "created", "last_confirmed", "valid_until"];

/// `YYYY-MM-DD`, or a full UTC timestamp, with a real month and day.
fn entry_date_ok(s: &str) -> bool {
    let re = regex::Regex::new(r"^(\d{4})-(\d{2})-(\d{2})(T\d{2}:\d{2}:\d{2}(\.\d+)?Z)?$").unwrap();
    re.captures(s).is_some_and(|c| matches!(c[2].parse::<u32>(), Ok(1..=12)) && matches!(c[3].parse::<u32>(), Ok(1..=31)))
}

/// Validate the optional ML-1 fields: `kind` is one of [`KINDS`], dates are dates, and neither `last_confirmed` nor
/// `valid_until` is before `created`. A file without them is valid: the kind then comes from its store.
pub fn check_entry_meta(m: &Mapping) -> Vec<String> {
    let mut errs = Vec::new();
    check_enum(m, "kind", &KINDS, &mut errs);
    for k in ["created", "last_confirmed", "valid_until"] {
        if has(m, k) && !get_str(m, k).is_some_and(entry_date_ok) {
            errs.push(format!("`{}` must be a date (YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ)", k));
        }
    }
    if let Some(created) = get_str(m, "created").filter(|c| entry_date_ok(c)) {
        for k in ["last_confirmed", "valid_until"] {
            if get_str(m, k).is_some_and(|v| entry_date_ok(v) && v[..10] < created[..10]) {
                errs.push(format!("`{}` is before `created`", k));
            }
        }
    }
    errs
}

/// The kind a staged note has when it does not say: what the capture trigger records (`specs/050` ML-1). An agent's own
/// `note` is a claim about the world (`fact`); everything the daemon saw happen is an `episode`.
pub fn kind_for_trigger(trigger: &str) -> &'static str {
    if matches!(trigger, "agent_note" | "operator_note") {
        "fact"
    } else {
        "episode"
    }
}

/// The kind an entry has when its frontmatter does not say: rules, lessons and skills are `procedure`.
pub fn default_kind(zone: &str, trigger: Option<&str>) -> &'static str {
    match (zone, trigger) {
        ("staged", Some(t)) => kind_for_trigger(t),
        ("staged", None) => "fact",
        _ => "procedure",
    }
}

fn check_enum(m: &Mapping, key: &str, allowed: &[&str], errs: &mut Vec<String>) {
    if let Some(v) = m.get(Value::String(key.into())) {
        match v.as_str() {
            Some(s) if allowed.contains(&s) => {}
            Some(s) => errs.push(format!("`{}: {}` is not one of {:?}", key, s, allowed)),
            None => errs.push(format!("`{}` must be a string", key)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "---\nid: x\ntitle: \"T\"\nauthor: builder\ntimestamp: 2026-10-23T00:00:00Z\nsource_type: internal_generated\nconfidence: low\nstatus: draft\nsource_mission: m\ntags: [a]\n---\n\n# Body\n";

    #[test]
    fn valid_frontmatter_passes() {
        let d = parse(GOOD).unwrap();
        assert!(check_provenance(&d.frontmatter.unwrap()).is_empty());
    }

    #[test]
    fn theme_name_author_and_missing_fields_fail() {
        let bad = GOOD.replace("author: builder", "author: athena").replace("tags: [a]\n", "");
        let errs = check_provenance(&parse(&bad).unwrap().frontmatter.unwrap());
        assert!(errs.iter().any(|e| e.contains("athena")));
        assert!(errs.iter().any(|e| e.contains("`tags`")));
    }

    #[test]
    fn values_are_validated_not_just_keys() {
        let bad = GOOD.replace("id: x", "id: \"\"").replace("timestamp: 2026-10-23T00:00:00Z", "timestamp: yesterday").replace("tags: [a]", "tags: [1]");
        let errs = check_provenance(&parse(&bad).unwrap().frontmatter.unwrap());
        assert_eq!(errs.len(), 3, "{:?}", errs);
    }

    #[test]
    fn optional_fields_are_optional_but_superseded_needs_target() {
        let s = GOOD.replace("status: draft", "status: superseded");
        let errs = check_provenance(&parse(&s).unwrap().frontmatter.unwrap());
        assert_eq!(errs.len(), 1);
    }
}

#[cfg(test)]
mod entry_meta_tests {
    use super::*;

    fn fm(yaml: &str) -> Mapping {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn entry_metadata_is_optional_and_checked_when_present() {
        assert!(check_entry_meta(&fm("id: x\n")).is_empty(), "old entries stay valid");
        assert!(check_entry_meta(&fm("kind: fact\ncreated: 2026-10-01\nlast_confirmed: 2026-10-20\nvalid_until: 2027-01-01T00:00:00Z\n")).is_empty());
        assert!(check_entry_meta(&fm("kind: opinion\n")).iter().any(|e| e.contains("kind")));
        assert!(check_entry_meta(&fm("created: yesterday\n")).iter().any(|e| e.contains("created")));
        assert!(check_entry_meta(&fm("created: 2026-13-40\n")).iter().any(|e| e.contains("created")));
        assert!(check_entry_meta(&fm("created: 2026-10-10\nvalid_until: 2026-10-01\n")).iter().any(|e| e.contains("before `created`")));
        assert!(check_entry_meta(&fm("last_confirmed: 5\n")).iter().any(|e| e.contains("last_confirmed")));
    }

    #[test]
    fn kinds_default_from_the_store_and_the_trigger() {
        assert_eq!(default_kind("agents", None), "procedure");
        assert_eq!(default_kind("memory", None), "procedure");
        assert_eq!(default_kind("staged", Some("agent_note")), "fact");
        assert_eq!(default_kind("staged", Some("denied_card")), "episode");
    }
}
