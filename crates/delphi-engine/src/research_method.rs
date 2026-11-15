//! The scout's method, as code (`specs/114` §3 to §5): what is known before a run searches, and when a question overlaps a topic
//! Delphi already has.
//!
//! Deterministic code only: no model, no network, no file. The runner (`research_run.rs`) calls it; the search that fills a
//! [`Known`] is the caller's own (`research_known.rs`), with the caller's visibility and nothing wider.
//!
//! The rule the whole method keeps (`specs/114` §1.2, §10): code decides what is fetched, and no model call that reads page text
//! holds a tool. Nothing here is page text: a [`Known`] holds claim ids with the claim's one-line statement and status, topic ids
//! with their question, and source ids with their age. A quote, a title and a snippet are a page's own words and are not kept
//! here.

use serde_json::{json, Value};

/// What one line of a [`Known`] holds at most, in characters.
const LINE: usize = 240;
/// What each list of a [`Known`] holds at most.
pub const KNOWN_CAP: usize = 20;

fn clip(s: &str, n: usize) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect()
}

/// A library claim the caller's search found for the question.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownClaim {
    /// The library's id for the claim (`c-…`).
    pub id: String,
    /// The claim's derived status, as a word (`noted`, `validated`, `contested`, …).
    pub status: String,
    /// The claim as one line. Never its quote.
    pub text: String,
}

/// A research topic the caller's search found for the question.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownTopic {
    pub id: String,
    pub question: String,
    /// The share of the new question's content words that the topic's question also has, 0 to 1.
    pub overlap: f64,
    /// The retrieval score of the topic's best result; `None` for a topic that was found without the index.
    pub score: Option<f64>,
    /// The topic is the workspace's (`false`: it is in the caller's own project).
    pub workspace: bool,
}

/// A source the library holds that the caller's search found for the question.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownSource {
    pub id: String,
    pub url: String,
    /// Days since the library last checked it; `None` when its record does not say.
    pub age_days: Option<i64>,
    /// Checked within its recheck window.
    pub fresh: bool,
}

/// What Delphi already holds on a question, as the caller could have found it by searching (`specs/114` §3 items 1 and 2).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Known {
    pub claims: Vec<KnownClaim>,
    pub topics: Vec<KnownTopic>,
    pub sources: Vec<KnownSource>,
    /// Pointers to what the knowledge base itself holds on the question (a file and a line; no text).
    pub kb: Vec<String>,
}

impl Known {
    pub fn is_empty(&self) -> bool {
        self.claims.is_empty() && self.topics.is_empty() && self.sources.is_empty() && self.kb.is_empty()
    }

    /// Keep each list inside [`KNOWN_CAP`] and each line inside its length.
    pub fn capped(mut self) -> Known {
        self.claims.truncate(KNOWN_CAP);
        self.topics.truncate(KNOWN_CAP);
        self.sources.truncate(KNOWN_CAP);
        self.kb.truncate(KNOWN_CAP);
        for c in &mut self.claims {
            c.text = clip(&c.text, LINE);
        }
        for t in &mut self.topics {
            t.question = clip(&t.question, LINE);
        }
        self
    }

    /// The `known` of a record and the `known_before` of a result: ids, statuses, ages and the one-line texts.
    pub fn to_json(&self) -> Value {
        json!({
            "claims": self.claims.iter().map(|c| json!({"id": clip(&c.id, 40), "status": clip(&c.status, 20), "text": clip(&c.text, LINE)})).collect::<Vec<_>>(),
            "topics": self.topics.iter().map(|t| json!({"id": clip(&t.id, 60), "question": clip(&t.question, LINE), "overlap": (t.overlap * 100.0).round() / 100.0, "score": t.score, "workspace": t.workspace})).collect::<Vec<_>>(),
            "sources": self.sources.iter().map(|s| json!({"id": clip(&s.id, 80), "url": clip(&s.url, 300), "age_days": s.age_days, "fresh": s.fresh})).collect::<Vec<_>>(),
            "kb": self.kb.iter().map(|p| clip(p, 200)).collect::<Vec<_>>(),
        })
    }
}

/// The search a run makes before its first query (`specs/114` §3 item 1). An implementation searches with the caller's own
/// visibility: what it returns is never more than that caller could have found by searching.
pub trait KnownFirst: Send + Sync {
    fn find(&self, question: &str) -> Known;
}

const FILLERS: [&str; 44] = [
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "did", "do", "does", "for", "from", "has", "have", "how", "in", "is", "it", "its", "not", "of", "on", "or", "that", "the", "their", "this", "to", "was", "were", "what", "when", "where", "which",
    "who", "why", "will", "with", "should", "would", "could", "about",
];

/// The words of `s` that say something about its subject: lower case, at least two letters or digits, no filler, each once.
pub fn content_words(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in s.split(|c: char| !c.is_alphanumeric()).map(str::to_lowercase) {
        if w.chars().count() >= 2 && !FILLERS.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// The share of `question`'s content words that `other` also has, 0 to 1. A question with no content word overlaps nothing.
pub fn overlap(question: &str, other: &str) -> f64 {
    let (q, o) = (content_words(question), content_words(other));
    if q.is_empty() {
        return 0.0;
    }
    q.iter().filter(|w| o.contains(w)).count() as f64 / q.len() as f64
}

/// When a question is the same research as a topic Delphi has (`specs/114` §3 item 3, `[research] continue_overlap` and
/// `continue_score`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContinueBar {
    /// The share of the question's content words the topic's question must have.
    pub overlap: f64,
    /// The retrieval score the topic's best result must reach.
    pub score: f64,
}

impl Default for ContinueBar {
    fn default() -> ContinueBar {
        ContinueBar { overlap: 0.5, score: 0.003 }
    }
}

/// The topic a new question would continue: the one that overlaps most among those over both bars. A topic with no retrieval
/// score is never offered.
pub fn strongest<'a>(known: &'a Known, bar: &ContinueBar) -> Option<&'a KnownTopic> {
    known.topics.iter().filter(|t| t.overlap >= bar.overlap && t.score.is_some_and(|s| s >= bar.score)).max_by(|a, b| a.overlap.partial_cmp(&b.overlap).unwrap_or(std::cmp::Ordering::Equal).then(a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topic(id: &str, question: &str, asked: &str, score: Option<f64>) -> KnownTopic {
        KnownTopic { id: id.into(), question: question.into(), overlap: overlap(asked, question), score, workspace: true }
    }

    #[test]
    fn overlap_is_the_share_of_the_questions_content_words_the_other_has() {
        assert_eq!(content_words("What temperature do zebrafish tanks need, and why do the tanks need it?"), ["temperature", "zebrafish", "tanks", "need"]);
        assert_eq!(overlap("zebrafish tank temperature", "What temperature suits a zebrafish?"), 2.0 / 3.0);
        assert_eq!(overlap("the of and", "anything"), 0.0, "a question of fillers overlaps nothing");
        assert_eq!(overlap("aspirin fever", "aspirin fever in adults"), 1.0);
    }

    // specs/114 §3 item 3: a topic is offered only over both bars, and the bars are the configured ones.
    #[test]
    fn a_topic_is_offered_only_over_both_bars() {
        let asked = "which temperature do zebrafish tanks need";
        let known = Known {
            topics: vec![
                topic("weak-words-00000001", "how are numbat burrows dug", asked, Some(0.9)),
                topic("low-score-00000002", "temperature of zebrafish tanks", asked, Some(0.001)),
                topic("no-score-00000003", "temperature zebrafish tanks need", asked, None),
                topic("half-00000004", "zebrafish tanks", asked, Some(0.05)),
                topic("best-00000005", "what temperature zebrafish tanks need", asked, Some(0.04)),
            ],
            ..Default::default()
        };
        let bar = ContinueBar::default();
        assert_eq!((bar.overlap, bar.score), (0.5, 0.003));
        assert_eq!(strongest(&known, &bar).map(|t| t.id.as_str()), Some("best-00000005"), "the one that overlaps most");
        assert_eq!(strongest(&known, &ContinueBar { overlap: 0.5, score: 0.045 }).map(|t| t.id.as_str()), Some("half-00000004"));
        assert!(strongest(&known, &ContinueBar { overlap: 1.01, score: 0.0 }).is_none());
        assert!(strongest(&Known::default(), &bar).is_none());
    }

    #[test]
    fn what_is_known_is_capped_and_holds_one_clean_line_each() {
        let known = Known { claims: (0..40).map(|i| KnownClaim { id: format!("c-{i:012}"), status: "noted".into(), text: format!("a claim\nwith a break {}", "x".repeat(400)) }).collect(), ..Default::default() }.capped();
        assert_eq!(known.claims.len(), KNOWN_CAP);
        assert!(known.claims.iter().all(|c| c.text.chars().count() <= LINE && !c.text.contains('\n')));
        let v = known.to_json();
        assert_eq!(v["claims"][0]["id"], "c-000000000000");
        assert!(v["topics"].as_array().unwrap().is_empty() && v["sources"].as_array().unwrap().is_empty() && v["kb"].as_array().unwrap().is_empty());
        assert!(Known::default().is_empty() && !known.is_empty());
    }
}
