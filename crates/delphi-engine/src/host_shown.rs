//! Computed provenance on what a host session writes (`specs/109` §13.1 items 2 and 3).
//!
//! The daemon keeps, in memory and per host session, a tally of the results that session was shown THROUGH THE DAEMON since it last
//! staged something: how many, the lowest `checked` among them, and whether any was third-party text. When the session stages a note, a
//! session summary or a KB entry, the daemon takes the tally (resetting it) and stamps the staged event with it as `shown`. Nothing a
//! host sends sets the stamp: the staging methods overwrite any `shown` field, and the tally is fed only by the daemon's own
//! `host.search_kb` and by `host.reads_seen`, which Delphi's MCP process calls with the ids and labels of what its own tools returned.
//!
//! It is information for the operator at ratification. It gates nothing, raises nothing and changes no tier.
//!
//! **Stated limit.** The daemon sees only reads that go through it: a host reading files directly, a tool whose reads are not reported,
//! or a read made while the daemon was not running is outside the tally, so the stamp is a floor on what was read, not the whole of it,
//! and its words always say "through Delphi's daemon". `host.reads_seen` arrives on the host socket like every host call; it can only
//! add reads, never remove one, and its labels are checked against the fixed vocabulary.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// The `checked` values from lowest to highest (`specs/109` §13.1: ratified > validated > checking > contested > not checked > draft).
pub const CHECKED_ORDER: [&str; 6] = ["draft", "not checked", "contested", "checking", "validated", "ratified"];
/// The `author` values a result may carry (`specs/109` §12.1 item 1).
pub const AUTHORS: [&str; 5] = ["operator", "release", "agent", "host_session", "third_party"];
/// The fixed words that say where the tally comes from.
pub const THROUGH: &str = "through Delphi's daemon";
/// The most host sessions held at once; the one touched longest ago is dropped first.
pub const MAX_SESSIONS: usize = 256;
/// The most distinct result ids remembered per session to count a result once; past it, each further result counts.
pub const MAX_IDS: usize = 512;
/// The most reads one `host.reads_seen` call may report.
pub const MAX_REPORTED: usize = 50;

/// `checked` as one of [`CHECKED_ORDER`], or `None` for a word outside it.
pub fn checked_word(s: &str) -> Option<&'static str> {
    CHECKED_ORDER.iter().copied().find(|c| *c == s)
}

fn rank(checked: &str) -> usize {
    CHECKED_ORDER.iter().position(|c| *c == checked).unwrap_or(0)
}

/// What a staged event is stamped with: `{count, lowest_checked, any_third_party}`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stamp {
    pub count: u32,
    pub lowest_checked: Option<&'static str>,
    pub any_third_party: bool,
}

impl Stamp {
    /// Count one result shown. A `checked` outside the vocabulary counts as `draft`, the lowest: an unreadable label is never better.
    pub fn add(&mut self, checked: &str, author: &str) {
        let c = checked_word(checked).unwrap_or("draft");
        self.count = self.count.saturating_add(1);
        if self.lowest_checked.is_none_or(|l| rank(c) < rank(l)) {
            self.lowest_checked = Some(c);
        }
        self.any_third_party |= author == "third_party";
    }

    pub fn to_json(&self) -> Value {
        json!({"count": self.count, "lowest_checked": self.lowest_checked, "any_third_party": self.any_third_party})
    }

    /// The stamp of a staged event's payload (`payload["shown"]`), or `None` for an event staged without one (before this slice).
    pub fn from_payload(payload: &Value) -> Option<Stamp> {
        let s = payload.get("shown").filter(|s| s.is_object())?;
        let count = s["count"].as_u64()?.min(u32::MAX as u64) as u32;
        Some(Stamp { count, lowest_checked: s["lowest_checked"].as_str().and_then(checked_word), any_third_party: s["any_third_party"].as_bool().unwrap_or(false) })
    }

    /// The stamp in words, e.g. `written after reading 4 results through Delphi's daemon, lowest: not checked, third-party text among them`.
    pub fn words(&self) -> String {
        if self.count == 0 {
            return format!("written after reading no results {THROUGH}");
        }
        let mut s = format!("written after reading {} result{} {THROUGH}", self.count, if self.count == 1 { "" } else { "s" });
        if let Some(l) = self.lowest_checked {
            s.push_str(&format!(", lowest: {l}"));
        }
        if self.any_third_party {
            s.push_str(", third-party text among them");
        }
        s
    }
}

/// The words of a staged event's stamp, or the fixed words for an event staged before stamps existed.
pub fn words_of(payload: &Value) -> String {
    Stamp::from_payload(payload).map_or_else(|| "not recorded (staged before the daemon counted what a session read)".to_string(), |s| s.words())
}

#[derive(Default)]
struct Session {
    stamp: Stamp,
    ids: HashSet<String>,
    touched: u64,
}

/// The per-session tallies, keyed by the host session's mission (`host-<session>`). Bounded by [`MAX_SESSIONS`] and [`MAX_IDS`].
#[derive(Default)]
pub struct Tally {
    sessions: HashMap<String, Session>,
    tick: u64,
}

impl Tally {
    /// Count what `key` was shown: `(id, checked, author)` per result. A result already counted since the last staging counts once.
    pub fn record<'a>(&mut self, key: &str, reads: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>) {
        self.tick += 1;
        if !self.sessions.contains_key(key) && self.sessions.len() >= MAX_SESSIONS {
            if let Some(oldest) = self.sessions.iter().min_by_key(|(_, s)| s.touched).map(|(k, _)| k.clone()) {
                self.sessions.remove(&oldest);
            }
        }
        let s = self.sessions.entry(key.to_string()).or_default();
        s.touched = self.tick;
        for (id, checked, author) in reads {
            if !id.is_empty() && s.ids.contains(id) {
                continue;
            }
            if !id.is_empty() && s.ids.len() < MAX_IDS {
                s.ids.insert(id.to_string());
            }
            s.stamp.add(checked, author);
        }
    }

    /// The stamp for a staging by `key`, and the tally reset. A session that was shown nothing has a zero count.
    pub fn take(&mut self, key: &str) -> Stamp {
        self.sessions.remove(key).map(|s| s.stamp).unwrap_or_default()
    }

    /// The tally so far, without resetting it.
    pub fn peek(&self, key: &str) -> Stamp {
        self.sessions.get(key).map(|s| s.stamp.clone()).unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

/// The reads of a `host.search_kb` answer: every result of every group, by its pointer (or path), `checked` and `author`.
pub fn reads_of_search(out: &Value) -> Vec<(String, String, String)> {
    let mut reads = Vec::new();
    for g in ["core", "release", "findings", "noted", "kept", "project_results", "third_party"] {
        for r in out[g].as_array().into_iter().flatten() {
            let id = r["pointer"].as_str().or_else(|| r["path"].as_str()).unwrap_or("").to_string();
            reads.push((format!("{g}:{id}"), r["checked"].as_str().unwrap_or("draft").to_string(), r["author"].as_str().unwrap_or("third_party").to_string()));
        }
    }
    reads
}

/// The reads a `host.reads_seen` call reports: at most [`MAX_REPORTED`], each with an id of at most 300 characters and a `checked` and
/// `author` from the fixed vocabularies; anything else is dropped, never guessed.
pub fn reported(p: &Value) -> Vec<(String, &'static str, &'static str)> {
    p["reads"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_REPORTED)
        .filter_map(|r| {
            let id = r["id"].as_str().filter(|i| !i.is_empty() && i.chars().count() <= 300)?;
            let checked = checked_word(r["checked"].as_str()?)?;
            let author = AUTHORS.iter().copied().find(|a| Some(*a) == r["author"].as_str())?;
            Some((format!("reported:{id}"), checked, author))
        })
        .collect()
}

/// `checked` and `author` of a result of `search_project`, `search_memory` or `query_memory`, which carry a tier and a layer but no
/// status: the tier alone says how far it was checked (`user_ratified` ratified, `internal_empirical` validated, anything else not
/// checked), and a federated tier or a library or bundle layer is third-party text.
pub fn labels_of_tier(source_type: &str, layer: &str) -> (&'static str, &'static str) {
    let checked = match source_type {
        "user_ratified" => "ratified",
        "internal_empirical" => "validated",
        _ => "not checked",
    };
    let author = if matches!(source_type, "federated_peer" | "federated_verified") || matches!(layer, "library" | "bundles" | "xenia") {
        "third_party"
    } else if layer == "project" {
        "operator"
    } else if matches!(source_type, "internal_generated" | "internal_empirical") {
        "agent"
    } else {
        "operator"
    };
    (checked, author)
}

/// The reads of a JSON answer whose `results` carry `layer`, `path` and `source_type` (`search_project`, `search_memory`, `query_memory`).
pub fn reads_of_results(answer: &str) -> Vec<Value> {
    let Ok(v) = serde_json::from_str::<Value>(answer) else { return vec![] };
    v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let layer = r["layer"].as_str().unwrap_or("");
            let (checked, author) = labels_of_tier(r["source_type"].as_str().unwrap_or(""), layer);
            json!({"id": format!("{layer}:{}", r["path"].as_str().unwrap_or("")), "checked": checked, "author": author})
        })
        .collect()
}

/// The reads of a fenced passage answer (`kb_retrieve`, `get_agent`'s task section): each `<!-- delphi:rag chunk_id=… -->` marker's
/// `chunk_id` and `source`, `checked` and `author`. A marker without `checked` (the task section) reads its `status` as the knowledge
/// status and counts as `not checked` unless it says `validated`; one without `author` counts as the operator's unless its `label` says
/// third-party.
pub fn reads_of_passages(text: &str) -> Vec<Value> {
    let attr = |m: &str, k: &str| -> Option<String> {
        let at = m.find(&format!(" {k}=\""))? + k.len() + 3;
        m[at..].find('"').map(|end| m[at..at + end].to_string())
    };
    text.split("<!-- delphi:rag ")
        .skip(1)
        .filter_map(|m| {
            let m = format!(" {}", &m[..m.find("-->")?]);
            let m = m.as_str();
            let id = attr(m, "chunk_id").or_else(|| attr(m, "source"))?;
            let checked = match attr(m, "checked") {
                Some(c) => checked_word(&c).unwrap_or("draft"),
                None if attr(m, "status").as_deref() == Some("validated") => "validated",
                None => "not checked",
            };
            let author = match attr(m, "author") {
                Some(a) => AUTHORS.iter().copied().find(|x| *x == a).unwrap_or("third_party"),
                None if attr(m, "label").is_some_and(|l| l.contains("third-party") || l.contains("fetched")) => "third_party",
                None => "operator",
            };
            Some(json!({"id": id, "checked": checked, "author": author}))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lowest_checked_follows_the_fixed_order_and_third_party_is_remembered() {
        let mut s = Stamp::default();
        assert_eq!(s.words(), "written after reading no results through Delphi's daemon");
        s.add("ratified", "release");
        assert_eq!(s.words(), "written after reading 1 result through Delphi's daemon, lowest: ratified");
        for (c, a) in [("validated", "operator"), ("checking", "agent"), ("not checked", "third_party"), ("contested", "operator")] {
            s.add(c, a);
        }
        assert_eq!(s.lowest_checked, Some("not checked"));
        assert_eq!(s.words(), "written after reading 5 results through Delphi's daemon, lowest: not checked, third-party text among them");
        s.add("trust me", "operator");
        assert_eq!(s.lowest_checked, Some("draft"), "a word outside the vocabulary counts as the lowest");
        assert_eq!(Stamp::from_payload(&json!({"shown": s.to_json()})), Some(s));
        assert_eq!(words_of(&json!({})), "not recorded (staged before the daemon counted what a session read)");
    }

    #[test]
    fn the_tally_counts_a_result_once_resets_at_take_and_holds_a_bounded_number_of_sessions() {
        let mut t = Tally::default();
        t.record("host-a", [("x", "validated", "operator"), ("x", "validated", "operator"), ("y", "not checked", "third_party")]);
        assert_eq!(t.peek("host-a").count, 2);
        assert_eq!(t.peek("host-b"), Stamp::default());
        let s = t.take("host-a");
        assert_eq!((s.count, s.lowest_checked, s.any_third_party), (2, Some("not checked"), true));
        assert_eq!(t.take("host-a"), Stamp::default(), "reset at staging");
        for i in 0..MAX_SESSIONS + 10 {
            t.record(&format!("host-{i}"), [("x", "validated", "operator")]);
        }
        assert_eq!(t.len(), MAX_SESSIONS);
        assert_eq!(t.peek("host-0").count, 0, "the oldest session was dropped");
        assert_eq!(t.peek(&format!("host-{}", MAX_SESSIONS + 9)).count, 1);
    }

    #[test]
    fn reported_reads_keep_only_the_fixed_vocabulary_and_a_bounded_number() {
        let p = json!({"reads": [{"id": "a", "checked": "ratified", "author": "release"}, {"id": "b", "checked": "superb", "author": "release"}, {"id": "c", "checked": "validated", "author": "god"}, {"id": "", "checked": "validated", "author": "operator"}, {"checked": "validated", "author": "operator"}]});
        assert_eq!(reported(&p), vec![("reported:a".to_string(), "ratified", "release")]);
        let many = json!({"reads": (0..80).map(|i| json!({"id": format!("i{i}"), "checked": "draft", "author": "operator"})).collect::<Vec<_>>()});
        assert_eq!(reported(&many).len(), MAX_REPORTED);
    }

    #[test]
    fn passages_and_results_yield_their_ids_and_labels() {
        let text = "<!-- delphi:rag:meta ranking=\"x\" -->\n\n<!-- delphi:rag chunk_id=\"CHK-1\" status=\"noted\" source=\"a.md#L1\" confidence=\"low\" source_type=\"external_untrusted\" origin=\"research\" author=\"third_party\" checked=\"not checked\" label=\"third-party page, not checked\" -->\nx\n<!-- delphi:rag:end -->\n\n<!-- delphi:rag chunk_id=\"CHK-2\" status=\"validated\" source=\"b.md#L1\" confidence=\"high\" label=\"validated\" -->\ny\n<!-- delphi:rag:end -->";
        let r = reads_of_passages(text);
        assert_eq!(r, vec![json!({"id": "CHK-1", "checked": "not checked", "author": "third_party"}), json!({"id": "CHK-2", "checked": "validated", "author": "operator"})]);
        let answer = json!({"results": [{"layer": "project", "path": "notes.md", "source_type": "external_untrusted"}, {"layer": "team", "path": "memory/agents/builder/lessons.md", "source_type": "user_ratified"}]}).to_string();
        assert_eq!(reads_of_results(&answer), vec![json!({"id": "project:notes.md", "checked": "not checked", "author": "operator"}), json!({"id": "team:memory/agents/builder/lessons.md", "checked": "ratified", "author": "operator"})]);
        assert!(reads_of_results("not json").is_empty());
    }
}
