//! The encyclopedia's reads as the window may ask for them (spec 117 §3 item 1, slices E5 to E7): eleven named reads, each
//! with the few arguments the daemon's method takes and no others. The window names a read and hands over an object; this
//! picks the method and builds the parameters from that object's known fields, each checked for shape, so nothing the
//! window (or a page drawn in it) writes is passed through to the daemon as it stands.
//!
//! All eleven are reads the daemon proved write nothing (`delphi-engine`'s
//! `every_encyclopedia_read_leaves_the_workspace_byte_identical_and_calls_no_model`), and all are paired-only: the client
//! refuses them on a connection that proved no key (`client::Method::paired_only`).

use serde_json::{json, Value};

use crate::client::Method;

/// A slug, a role id: lower-case letters, digits and single hyphens, as the daemon's `valid_slug` and `valid_role` take them.
fn word(v: &Value, most: usize) -> Option<&str> {
    v.as_str().filter(|s| !s.is_empty() && s.len() <= most && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !s.starts_with('-') && !s.ends_with('-'))
}

/// A record's id or a version's stamp: letters, digits, `-`, `_` and `.`, never a path.
fn id(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| !s.is_empty() && s.len() <= 120 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) && !s.contains("..") && !s.starts_with('.'))
}

/// The method and parameters for the read the window named, or `None` when it is not one of the eleven or an argument is
/// missing or not of the shape that read takes.
pub fn read(what: &str, arg: &Value) -> Option<(Method, Value)> {
    Some(match what {
        "pages.list" => (Method::PagesList, json!({})),
        "pages.get" => {
            let slug = word(&arg["slug"], 60)?;
            match &arg["version"] {
                Value::Null => (Method::PagesGet, json!({ "slug": slug })),
                v => (Method::PagesGet, json!({ "slug": slug, "version": id(v)? })),
            }
        }
        "pages.versions" => (Method::PagesVersions, json!({ "slug": word(&arg["slug"], 60)? })),
        "pages.links" => (Method::PagesLinks, json!({ "slug": word(&arg["slug"], 60)? })),
        "topics.list" => (Method::TopicsList, json!({})),
        "topics.get" => (Method::TopicsGet, json!({ "id": id(&arg["id"])? })),
        "library.source" => (Method::LibrarySource, json!({ "id": id(&arg["id"])? })),
        "library.claim" => (Method::LibraryClaim, json!({ "id": id(&arg["id"])? })),
        "memory.roles" => (Method::MemoryRoles, json!({})),
        "memory.role" => (Method::MemoryRole, json!({ "role": word(&arg["role"], 40)? })),
        "changes.since" => (Method::ChangesSince, json!({ "time": arg["time"].as_u64()? })),
        _ => return None,
    })
}

/// The longest cited passage the window may name.
const QUOTE_MAX: usize = 4000;

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the window is given of the daemon's answer. One read is cut down (spec 117 §3 item 3): the text of a source the
/// library keeps privately for retrieval (spec 119 §3; its record says `kept_privately`) is shown only as the cited
/// passage. The window names the passage it came for (`quote`, a claim's anchor); if the source's text holds it, that
/// passage is what is passed on, and otherwise no text is. The record itself is passed on whole: the operator sees what
/// the library holds (spec 115 §11), and the text is theirs to read at a terminal (`delphi sources show`).
pub fn shown(method: Method, arg: &Value, mut reply: Value) -> Value {
    if method == Method::LibrarySource && reply["meta"]["kept_privately"].is_string() {
        let quote = arg["quote"].as_str().map(norm).filter(|q| !q.is_empty() && q.len() <= QUOTE_MAX);
        let held = quote.filter(|q| reply["text"].as_str().is_some_and(|t| norm(t).contains(q.as_str())));
        reply["text"] = json!(held.unwrap_or_default());
        reply["truncated"] = json!(false);
        reply["passage_only"] = json!(true);
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_kept_privately_for_retrieval_is_shown_only_as_the_cited_passage() {
        let reply = || json!({"id": "s1", "private": true, "meta": {"title": "Ledger", "kept_privately": "unknown_licence"}, "text": "The keeper's ledger lists\nevery visitor. It also lists what each one paid.", "truncated": true});
        // The passage the window came for, when the text holds it: that and no more.
        let cut = shown(Method::LibrarySource, &json!({"id": "s1", "quote": "lists every   visitor"}), reply());
        assert_eq!((cut["text"].as_str(), cut["passage_only"].as_bool(), cut["truncated"].as_bool()), (Some("lists every visitor"), Some(true), Some(false)));
        assert_eq!(cut["meta"]["title"], "Ledger", "the record is the operator's to see");
        // No passage named, or one the text does not hold: no text at all.
        for arg in [json!({"id": "s1"}), json!({"id": "s1", "quote": "what the keeper thought"}), json!({"id": "s1", "quote": ""}), json!({"id": "s1", "quote": 7})] {
            let cut = shown(Method::LibrarySource, &arg, reply());
            assert_eq!((cut["text"].as_str(), cut["passage_only"].as_bool()), (Some(""), Some(true)), "{arg}");
            assert!(!cut.to_string().contains("paid"));
        }
        // Any other source (a private one the operator added by hand included), and any other read, is passed on as it came.
        let plain = json!({"id": "s2", "private": true, "meta": {"title": "Notes"}, "text": "whole text"});
        assert_eq!(shown(Method::LibrarySource, &json!({"id": "s2"}), plain.clone()), plain);
        assert_eq!(shown(Method::LibraryClaim, &json!({"id": "c"}), reply()), reply());
    }

    #[test]
    fn the_window_can_name_the_eleven_reads_and_each_is_paired_only() {
        let full = json!({"slug": "tides", "version": "20261115T101500Z", "id": "c-0123abcd", "role": "scout", "time": 1_700_000_000u64});
        let mut named = Vec::new();
        for m in Method::PAIRED_ONLY {
            let (method, params) = read(m.name(), &full).unwrap_or_else(|| panic!("{} is not a read the window can name", m.name()));
            assert_eq!(method, m);
            assert!(method.paired_only());
            named.push(params);
        }
        // Each read is given its own arguments and nothing else of what the window handed over.
        assert_eq!(named[0], json!({}));
        assert_eq!(named[1], json!({"slug": "tides", "version": "20261115T101500Z"}));
        assert_eq!(named[2], json!({"slug": "tides"}));
        assert_eq!(named[5], json!({"id": "c-0123abcd"}));
        assert_eq!(named[9], json!({"role": "scout"}));
        assert_eq!(named[10], json!({"time": 1_700_000_000u64}));
        assert_eq!(read("pages.get", &json!({"slug": "tides"})).unwrap().1, json!({"slug": "tides"}), "no version: the current page");
    }

    #[test]
    fn nothing_else_can_be_named_and_an_argument_of_another_shape_is_refused() {
        // A narrator read, an acting method, a method the console has and the app does not: none is an encyclopedia read.
        for not in ["kb.get", "session.ping", "pages.promote", "pages.refresh", "pages.search", "pages.open", "research.start", "task.submit", "approval.resolve", "app.pair", "", "pages.get\n"] {
            assert!(read(not, &json!({"slug": "tides", "id": "x", "role": "scout", "time": 1})).is_none(), "{not}");
        }
        for bad in [json!({}), json!({"slug": ""}), json!({"slug": "../x"}), json!({"slug": "Tides"}), json!({"slug": "a/b"}), json!({"slug": 7}), json!({"slug": "a".repeat(61)}), json!({"slug": "tides", "version": "../../x"}), json!({"slug": "tides", "version": 3})] {
            assert!(read("pages.get", &bad).is_none(), "{bad}");
        }
        for bad in [json!({}), json!({"id": "a/b"}), json!({"id": ".."}), json!({"id": ".hidden"}), json!({"id": "x y"}), json!({"id": "x".repeat(121)}), json!({"id": ["x"]})] {
            assert!(read("library.source", &bad).is_none() && read("library.claim", &bad).is_none() && read("topics.get", &bad).is_none(), "{bad}");
        }
        for bad in [json!({}), json!({"role": "Scout"}), json!({"role": "a/b"}), json!({"role": "-x"})] {
            assert!(read("memory.role", &bad).is_none(), "{bad}");
        }
        for bad in [json!({}), json!({"time": -1}), json!({"time": "yesterday"}), json!({"time": 1.5})] {
            assert!(read("changes.since", &bad).is_none(), "{bad}");
        }
    }
}
