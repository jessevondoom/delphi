//! The four things the window may ask the daemon to start (spec 120 §4.2, §6 item 1; slice K5): a further run on a
//! research topic, a follow-up question, a rebuild of a topic page, a dispute on a claim. They are the methods of the
//! daemon's `request` grant and the only methods this app has that are not reads.
//!
//! The window names a request and hands over an object. This picks the method and builds its parameters from that
//! object's known fields, each checked for shape, exactly as [`crate::ency::read`] does for a read: an id is an id, a
//! slug is a slug, a section is a heading's anchor, and the two fields that hold what the operator typed (`note`,
//! `question`) are one line of bounded length. Nothing else the window hands over is passed on, so a request cannot
//! carry a field the daemon's table does not list, and the daemon refuses one that does.
//!
//! What this cannot know is where the window got its words. The rule that a request carries ids and typed text only is
//! kept in the window's code, which gives these functions the value of an input field and never text taken from a page
//! (`app/test/request.test.tsx`); and, whatever the window sends, by the daemon: the words are a note or a question to a
//! research run, data as a console prompt is, never an instruction to anything that holds a tool.

use serde_json::{json, Value};

use crate::client::Method;

/// The most typed text a request carries, in characters (the daemon's bound; each command has a tighter one of its own).
pub const TEXT_MAX: usize = 2_000;
/// The longest section anchor (the daemon's bound).
const ANCHOR_MAX: usize = 80;

/// A research topic's id or a page's slug: lower-case letters, digits and single hyphens.
fn word(v: &Value, most: usize) -> Option<&str> {
    v.as_str().filter(|s| !s.is_empty() && s.len() <= most && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !s.starts_with('-') && !s.ends_with('-') && !s.contains("--"))
}

/// A claim's id: letters, digits, `-`, `_` and `.`, never a path.
fn id(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| !s.is_empty() && s.len() <= 120 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) && !s.contains("..") && !s.starts_with('.'))
}

/// A heading's anchor as the daemon makes one: lowercase letters, digits and hyphens.
fn anchor(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| !s.is_empty() && s.chars().count() <= ANCHOR_MAX && s.chars().all(|c| c == '-' || (c.is_alphanumeric() && c.to_lowercase().eq(std::iter::once(c)))))
}

/// What the operator typed, as one line: line ends and control characters become spaces, runs of space one space.
/// `None` when it is not text or is longer than [`TEXT_MAX`]; an empty line is `Some("")`.
fn typed(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    let line = s.chars().map(|c| if c.is_control() || c == '\u{2028}' || c == '\u{2029}' { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    (line.chars().count() <= TEXT_MAX).then_some(line)
}

/// The method and parameters for the request the window named, or `None` when it is not one of the four or a field is
/// missing or not of the shape that request takes. A field that is absent (`null`) is left out; one that is present and
/// of another shape refuses the request, it is never dropped silently.
pub fn request(what: &str, arg: &Value) -> Option<(Method, Value)> {
    let optional = |k: &str, read: &dyn Fn(&Value) -> Option<String>| -> Option<Option<String>> {
        match &arg[k] {
            Value::Null => Some(None),
            v => read(v).map(Some),
        }
    };
    let text = |v: &Value| typed(v);
    Some(match what {
        "research.continue" => {
            let mut p = json!({ "topic_id": word(&arg["topic_id"], 60)? });
            if let Some(a) = optional("section", &|v| anchor(v).map(String::from))? {
                p["section"] = json!(a);
            }
            if let Some(n) = optional("note", &text)?.filter(|n| !n.is_empty()) {
                p["note"] = json!(n);
            }
            (Method::ResearchContinue, p)
        }
        "research.ask" => {
            let question = typed(&arg["question"]).filter(|q| !q.is_empty())?;
            let mut p = json!({ "question": question });
            if let Some(f) = optional("follows", &|v| word(v, 60).map(String::from))? {
                p["follows"] = json!(f);
            }
            (Method::ResearchAsk, p)
        }
        "page.rebuild" => (Method::PageRebuild, json!({ "slug": word(&arg["slug"], 60)? })),
        "page.dispute" => (Method::PageDispute, json!({ "claim": id(&arg["claim"])?, "note": typed(&arg["note"]).filter(|n| !n.is_empty())? })),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_can_name_the_four_requests_and_each_is_given_its_own_fields_only() {
        let full = json!({"topic_id": "tides-0123abcd", "section": "why-twice-a-day", "note": "in winter", "question": "why twice?", "follows": "tides-0123abcd", "slug": "tides", "claim": "c-0123abcd",
            "query": "free text", "text": "a page's words", "markdown": "# x", "depth": "deep", "new": true, "cwd": "/", "project": "p", "status": "refuted", "force": true});
        let (m, p) = request("research.continue", &full).unwrap();
        assert_eq!((m, p), (Method::ResearchContinue, json!({"topic_id": "tides-0123abcd", "section": "why-twice-a-day", "note": "in winter"})));
        let (m, p) = request("research.ask", &full).unwrap();
        assert_eq!((m, p), (Method::ResearchAsk, json!({"question": "why twice?", "follows": "tides-0123abcd"})));
        let (m, p) = request("page.rebuild", &full).unwrap();
        assert_eq!((m, p), (Method::PageRebuild, json!({"slug": "tides"})));
        let (m, p) = request("page.dispute", &full).unwrap();
        assert_eq!((m, p), (Method::PageDispute, json!({"claim": "c-0123abcd", "note": "in winter"})));
        // The optional fields are left out when absent, and an empty note is no note.
        assert_eq!(request("research.continue", &json!({"topic_id": "tides-0123abcd"})).unwrap().1, json!({"topic_id": "tides-0123abcd"}));
        assert_eq!(request("research.continue", &json!({"topic_id": "tides-0123abcd", "note": "  "})).unwrap().1, json!({"topic_id": "tides-0123abcd"}));
        assert_eq!(request("research.ask", &json!({"question": "why"})).unwrap().1, json!({"question": "why"}));
        // Every request the window can name is one of the daemon's four, and the four are all it can name.
        let named: Vec<Method> = ["research.continue", "research.ask", "page.rebuild", "page.dispute"].iter().map(|w| request(w, &full).unwrap().0).collect();
        assert_eq!(named, Method::REQUESTS);
        assert!(named.iter().all(|m| m.asks() && !m.paired_only()));
    }

    #[test]
    fn no_other_method_can_be_named_and_a_field_of_another_shape_refuses_the_request() {
        let full = json!({"topic_id": "tides-0123abcd", "slug": "tides", "claim": "c-1", "note": "n", "question": "q"});
        for not in ["app.research_continue", "app.page_rebuild", "research.start", "research.cancel", "pages.promote", "pages.dispute", "approval.resolve", "settings.set", "task.submit", "app.grant", "app.pair", "pages.get", "", "page.rebuild\n"] {
            assert!(request(not, &full).is_none(), "{not}");
        }
        for bad in [json!({}), json!({"topic_id": "../x"}), json!({"topic_id": "Tides"}), json!({"topic_id": "a b"}), json!({"topic_id": 7}), json!({"topic_id": "a".repeat(61)}), json!({"topic_id": "-rf"}), json!({"topic_id": "a--b"})] {
            assert!(request("research.continue", &bad).is_none(), "{bad}");
        }
        // A section is an anchor; one of another shape refuses the request and is never dropped.
        for bad in ["Why twice", "why twice", "a/b", "a\nb", "", "$(id)", "x;y"] {
            assert!(request("research.continue", &json!({"topic_id": "tides-0123abcd", "section": bad})).is_none(), "{bad:?}");
        }
        assert!(request("research.continue", &json!({"topic_id": "tides-0123abcd", "section": 3})).is_none());
        assert!(request("research.continue", &json!({"topic_id": "tides-0123abcd", "note": ["a"]})).is_none());
        assert!(request("research.ask", &json!({"question": "q", "follows": "../x"})).is_none());
        for bad in [json!({}), json!({"question": ""}), json!({"question": " \n "}), json!({"question": 3}), json!({"question": "x".repeat(TEXT_MAX + 1)})] {
            assert!(request("research.ask", &bad).is_none(), "{bad}");
        }
        for bad in [json!({"claim": "c-1"}), json!({"claim": "c-1", "note": ""}), json!({"claim": "../x", "note": "n"}), json!({"claim": "a b", "note": "n"}), json!({"note": "n"})] {
            assert!(request("page.dispute", &bad).is_none(), "{bad}");
        }
        for bad in [json!({}), json!({"slug": "Tides"}), json!({"slug": "a/b"}), json!({"slug": ""})] {
            assert!(request("page.rebuild", &bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn typed_text_is_one_bounded_line() {
        assert_eq!(typed(&json!("  It is\nnot\ttwice\u{2028}everywhere \u{7} ")).as_deref(), Some("It is not twice everywhere"));
        assert_eq!(typed(&json!("x".repeat(TEXT_MAX))).map(|s| s.len()), Some(TEXT_MAX));
        assert!(typed(&json!("é".repeat(TEXT_MAX + 1))).is_none() && typed(&json!(7)).is_none() && typed(&Value::Null).is_none());
        let (_, p) = request("page.dispute", &json!({"claim": "c-1", "note": "a\nb"})).unwrap();
        assert_eq!(p["note"], "a b");
    }
}
