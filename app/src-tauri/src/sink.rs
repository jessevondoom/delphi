//! The app's sinks (spec 141 §2.4): a page exported to a file and a page printed (the page-export row). What may leave is the
//! daemon's decision, never the window's: before it writes or prints, the app reads the page again (`pages.get`, a read its key
//! already holds) and acts on the reply's `export` decision, which the daemon computed from its own records at that moment.
//!
//! * `refuse`: nothing is written or printed, and the reason (the marks and the items they came from, never any text) is shown.
//! * `labelled`: the operator confirms once in the window, and an exported file begins with the visible line.
//! * `send`: as before labels.
//!
//! A reply with no `export` decision is from a daemon older than labels: nothing is exported or printed until it is updated.
//! The window's copy follows the reply's `copy` decision in the window itself (`src/ency.ts`, `copyDecision`); no command of this
//! crate writes to the clipboard.

use serde_json::{json, Value};

use crate::client::Method;

/// The read that is asked again before a page leaves: the page shown, and the version shown if it is an earlier one.
pub fn page_read(slug: &str, version: Option<&str>) -> Option<(Method, Value)> {
    let arg = match version {
        Some(v) => json!({ "slug": slug, "version": v }),
        None => json!({ "slug": slug }),
    };
    crate::ency::read("pages.get", &arg)
}

/// What the daemon's reply allows: `Ok(None)` to send as it is, `Ok(Some(line))` to send labelled (only once the operator
/// `confirmed`), `Err` to refuse.
pub fn decide(reply: &Value, confirmed: bool) -> Result<Option<String>, String> {
    let d = &reply["export"];
    match d["decision"].as_str() {
        Some("send") => Ok(None),
        Some("labelled") => {
            let line = d["line"].as_str().filter(|l| !l.trim().is_empty()).ok_or("the daemon named no visible line for a labelled page: not sent")?;
            if confirmed {
                Ok(Some(line.to_string()))
            } else {
                Err(format!("this page is labelled ({line}): confirm the export or print in the window first"))
            }
        }
        Some("refuse") => Err(format!("Not exported or printed: {}", d["reason"].as_str().unwrap_or("the page's label refuses it"))),
        _ => Err("this daemon does not say what may leave this machine (it is older than labels): update it, then export or print".into()),
    }
}

/// The exported text for a page the daemon judged: the window's text must begin with the page the daemon returned (so what is
/// written is what was decided), and a labelled one begins with its visible line.
pub fn exported(reply: &Value, text: &str, line: Option<&str>) -> Result<String, String> {
    let norm = |s: &str| s.replace("\r\n", "\n").replace('\r', "\n");
    // the visible line, where the page or the text already begins with it, is not part of what is compared
    let unlined = |s: &str| -> String {
        let s = s.trim_start();
        match line.and_then(|l| s.strip_prefix(l)) {
            Some(r) => r.trim_start().to_string(),
            None => s.to_string(),
        }
    };
    let page = unlined(&norm(reply["markdown"].as_str().unwrap_or("")));
    let page = page.trim();
    let text = norm(text);
    let body = unlined(&text);
    if page.is_empty() || !body.starts_with(page) {
        return Err("the text to export is not the page the daemon holds: not exported".into());
    }
    Ok(match line {
        Some(l) if !text.starts_with(l) => format!("{l}\n\n{text}"),
        _ => text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(decision: &str) -> Value {
        json!({"markdown": "> Handling: INTERNAL\n\nGenerated page. Not authoritative.\n\nBody.", "export": {"decision": decision, "reason": "refused: personal (handling:x)", "line": "> Handling: INTERNAL"}})
    }

    #[test]
    fn export_refuses_a_private_page() {
        let e = decide(&reply("refuse"), true).unwrap_err();
        assert!(e.contains("personal (handling:x)"), "{e}");
    }

    #[test]
    fn a_labelled_page_asks_once_and_its_file_begins_with_the_visible_line() {
        assert!(decide(&reply("labelled"), false).unwrap_err().contains("confirm"));
        let line = decide(&reply("labelled"), true).unwrap();
        assert_eq!(line.as_deref(), Some("> Handling: INTERNAL"));
        let r = reply("labelled");
        let out = exported(&r, "> Handling: INTERNAL\n\nGenerated page. Not authoritative.\n\nBody.\n\n---\n\nExported.", line.as_deref()).unwrap();
        assert!(out.starts_with("> Handling: INTERNAL\n"));
        // a window text without the line gets it at the top
        let r2 = json!({"markdown": "Generated page. Not authoritative.\n\nBody."});
        let out = exported(&r2, "Generated page. Not authoritative.\n\nBody.", Some("> Handling: INTERNAL")).unwrap();
        assert!(out.starts_with("> Handling: INTERNAL\n\nGenerated page."), "{out}");
    }

    #[test]
    fn an_unlabelled_page_is_exported_as_before_and_an_older_daemon_or_other_text_is_not() {
        assert_eq!(decide(&reply("send"), false).unwrap(), None);
        let r = json!({"markdown": "Generated page. Not authoritative.\n\nBody."});
        assert_eq!(exported(&r, "Generated page. Not authoritative.\n\nBody.\n\nExported.", None).unwrap(), "Generated page. Not authoritative.\n\nBody.\n\nExported.");
        assert!(decide(&json!({"may_export": true}), true).is_err(), "no decision: an older daemon");
        assert!(exported(&r, "Generated page. Not authoritative.\n\nSomething else.", None).is_err());
    }
}
