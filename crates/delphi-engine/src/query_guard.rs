//! The identifier guard for outbound search queries (`specs/119` §2 item 7).
//!
//! A workspace names its own private words in `[privacy] identifiers` (`specs/004` §5.4): repository names, hosts, project terms.
//! A query that contains one is not sent as written. The word carrying the identifier is taken out when the query still has
//! [`MIN_CONTENT_WORDS`] content words without it; otherwise the query is withheld, and the search log lists it as withheld with no
//! text. This is in addition to `research_plan::clean_query`, which already removes secrets, paths and private addresses.
//!
//! Deterministic code: no model, no network. Every path that hands a query to a search engine calls [`guard`] first: the host
//! research runner (which also writes the search log), the console's research job, and the searcher behind the page writer's
//! external search and the validator's contradiction search. The seam for a host's own search tool (§2.1, a later slice) is the
//! same call on the planned queries before they are handed over.
//!
//! Matching: an identifier matches a word of the query when it appears in it, ignoring case, with no letter or digit on either
//! side (`heron` matches `heron`, `Heron's`, `project-heron` and `site:heron.example`, not `herons`). An identifier of several
//! words matches the same words in a row. The whole word is removed, never part of it.

use std::path::Path;

/// A query needs this many content words left to be sent without its identifier.
pub const MIN_CONTENT_WORDS: usize = 3;

/// What the guard decided for one query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guarded {
    /// No identifier in it: send it as it is.
    Clean,
    /// An identifier was removed and enough is left: send this instead.
    Stripped(String),
    /// Too little is left without the identifier: send nothing, and record no text.
    Withheld,
}

impl Guarded {
    /// The text to send for `query`, or `None` when nothing may be sent.
    pub fn to_send<'a>(&'a self, query: &'a str) -> Option<&'a str> {
        match self {
            Guarded::Clean => Some(query),
            Guarded::Stripped(q) => Some(q),
            Guarded::Withheld => None,
        }
    }
}

/// What a caller with no search log tells its own caller when a query was withheld. It names no word of the query.
pub const WITHHELD_MESSAGE: &str = "the query names one of the workspace's private identifiers ([privacy] identifiers) and too little is left without it, so nothing was sent";

/// The workspace's `[privacy] identifiers`, lower-cased, without empty entries. Reads `config.toml`; a file that cannot be read has none.
pub fn identifiers(ws_dir: &Path) -> Vec<String> {
    crate::distill::privacy(ws_dir).identifiers.into_iter().map(|i| i.trim().to_lowercase()).filter(|i| !i.is_empty()).collect()
}

const FILLERS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "did", "do", "does", "for", "from", "has", "have", "how", "in", "is", "it", "its", "not", "of", "on", "or", "that", "the", "their", "this", "to", "was", "were", "what", "when", "where", "which", "who", "why", "will", "with",
];

/// A word that says something about the subject: not a search operator, not a filler, and at least two letters or digits.
fn content_word(word: &str) -> bool {
    let w = word.to_lowercase();
    if w.contains(':') || w.starts_with('-') {
        return false; // `site:example.org`, `-term`
    }
    let core: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
    core.chars().count() >= 2 && !FILLERS.contains(&core.as_str())
}

/// Does `word` (lower-case) carry the one-word identifier `id` (lower-case) at a boundary?
fn carries(word: &str, id: &str) -> bool {
    let mut from = 0;
    while let Some(at) = word[from..].find(id) {
        let (start, end) = (from + at, from + at + id.len());
        let before = word[..start].chars().next_back().is_some_and(char::is_alphanumeric);
        let after = word[end..].chars().next().is_some_and(char::is_alphanumeric);
        if !before && !after {
            return true;
        }
        from = start + word[start..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// A word with the punctuation around it taken off, lower-case, for comparing against a word of a several-word identifier.
fn bare(word: &str) -> String {
    word.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

/// Check `query` against `identifiers` (as [`identifiers`] returns them).
pub fn guard(query: &str, identifiers: &[String]) -> Guarded {
    let words: Vec<&str> = query.split_whitespace().collect();
    let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    let mut remove = vec![false; words.len()];
    for id in identifiers {
        let id = id.trim().to_lowercase();
        let parts: Vec<&str> = id.split_whitespace().collect();
        match parts.len() {
            0 => {}
            1 => {
                for (i, w) in lower.iter().enumerate() {
                    if carries(w, parts[0]) {
                        remove[i] = true;
                    }
                }
            }
            n => {
                for i in 0..words.len().saturating_sub(n - 1) {
                    if (0..n).all(|k| bare(words[i + k]) == bare(parts[k])) {
                        remove[i..i + n].iter_mut().for_each(|r| *r = true);
                    }
                }
            }
        }
    }
    if !remove.iter().any(|r| *r) {
        return Guarded::Clean;
    }
    let kept: Vec<&str> = words.iter().zip(&remove).filter(|(_, r)| !**r).map(|(w, _)| *w).collect();
    if kept.iter().filter(|w| content_word(w)).count() < MIN_CONTENT_WORDS {
        return Guarded::Withheld;
    }
    // An operator left dangling by a removed word says nothing: drop it from either end.
    let dangling = |w: &&str| matches!(*w, "OR" | "AND" | "NOT");
    let start = kept.iter().position(|w| !dangling(w)).unwrap_or(0);
    let end = kept.iter().rposition(|w| !dangling(w)).map_or(0, |i| i + 1);
    Guarded::Stripped(kept[start..end.max(start)].join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // specs/119 §2 item 7: removed when three content words remain, otherwise withheld.
    #[test]
    fn an_identifier_is_removed_when_three_content_words_remain_and_the_query_is_withheld_otherwise() {
        let id = ids(&["heron"]);
        assert_eq!(guard("postgres index bloat causes", &id), Guarded::Clean);
        assert_eq!(guard("heron postgres index bloat", &id), Guarded::Stripped("postgres index bloat".into()));
        assert_eq!(guard("postgres index bloat in Project-Heron", &id), Guarded::Stripped("postgres index bloat in".into()));
        assert_eq!(guard("heron index bloat", &id), Guarded::Withheld, "two content words are too few");
        assert_eq!(guard("what is the heron of it", &id), Guarded::Withheld, "fillers are not content");
        assert_eq!(guard("heron", &id), Guarded::Withheld);
        // Search operators are not content, and one that names the identifier goes with it.
        assert_eq!(guard("index bloat site:heron.example.com site:postgresql.org", &id), Guarded::Withheld);
        assert_eq!(guard("postgres index bloat site:heron.example.com", &id), Guarded::Stripped("postgres index bloat".into()));
        assert_eq!(guard("postgres index bloat limitations OR heron", &id), Guarded::Stripped("postgres index bloat limitations".into()));
    }

    #[test]
    fn matching_ignores_case_respects_word_boundaries_and_handles_several_words() {
        let id = ids(&["heron", "blue falcon"]);
        assert_eq!(guard("herons nest near rivers often", &id), Guarded::Clean, "a longer word is another word");
        assert_eq!(guard("HERON's nest near rivers often", &id), Guarded::Stripped("nest near rivers often".into()));
        assert_eq!(guard("the Blue Falcon, launch schedule delays explained", &id), Guarded::Stripped("the launch schedule delays explained".into()));
        assert_eq!(guard("blue whales and falcon migration routes", &id), Guarded::Clean, "the words must be in a row");
        assert_eq!(guard("Ünïcode héron words stay intact here", &ids(&["héron"])), Guarded::Stripped("Ünïcode words stay intact here".into()));
        assert_eq!(guard("anything at all here", &[]), Guarded::Clean);
        assert_eq!(guard("anything at all here", &ids(&["", "  "])), Guarded::Clean, "an empty identifier matches nothing");
    }

    #[test]
    fn what_is_sent_never_contains_the_identifier() {
        let id = ids(&["acme-internal", "heron"]);
        for q in ["acme-internal build cache eviction policy", "how does heron handle build cache eviction", "build cache eviction heron acme-internal policy notes", "heron"] {
            let g = guard(q, &id);
            if let Some(sent) = g.to_send(q) {
                assert!(!sent.to_lowercase().contains("heron") && !sent.to_lowercase().contains("acme-internal"), "{q} -> {sent}");
            }
        }
        assert_eq!(Guarded::Withheld.to_send("heron"), None);
        assert!(!WITHHELD_MESSAGE.contains("heron"));
    }

    #[test]
    fn the_workspaces_identifiers_come_from_its_config_and_a_missing_file_has_none() {
        let t = tempfile::tempdir().unwrap();
        assert!(identifiers(t.path()).is_empty());
        std::fs::write(t.path().join("config.toml"), "[privacy]\nidentifiers = [\"Project-Heron\", \"\", \" acme.internal \"]\nhandles = [\"@someone\"]\n").unwrap();
        assert_eq!(identifiers(t.path()), ["project-heron", "acme.internal"]);
    }
}
