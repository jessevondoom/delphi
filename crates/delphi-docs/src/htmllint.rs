//! The safety check for generated pages (`specs/014` §7.3). A page an agent writes is a program the operator
//! opens in a browser, so before it is written it is checked for the three ways a page does harm without being
//! asked: reaching the network, running code it assembled at run time, and navigating away with data.
//!
//! What this check is, and is not. For **markup** it is exact: every piece of markup in a page comes from the
//! generator, which escapes all agent text, and the check re-parses the finished page and rejects anything
//! outside a small allow-list. For **script** it is a guard rail, not a sandbox: no list of words can make
//! arbitrary JavaScript safe, because code can build any name at run time. So the page's real protections are
//! structural: the default page contains no agent-written script at all (only the audited runtime in `runtime.js`),
//! a page with a free-form script is always put to the operator in full, and the browser enforces the
//! Content-Security-Policy the generator adds. The word list below catches accidents and the common tricks; it
//! is deliberately strict, and a script it rejects can be rewritten without the word.
//!
//! Measured, not assumed (headless Chrome, a hand-made page carrying exactly the policy in `htmlpage::CSP`, a
//! script trying every route to a local server): `fetch`, `XMLHttpRequest`, an image, a dynamic script, a frame,
//! a stylesheet, a form submission, `sendBeacon` and a WebSocket all failed, and the server saw none of them. A
//! plain navigation (`location = url + data`) went through, because a content security policy does not govern
//! navigation. That is why `location` and its relatives are on the list, why a page with a script is reviewed by
//! the operator every time, and why a page without one, which cannot navigate, needs no review.

use crate::html;
use crate::xml::{attr, Ev};

/// The audited runtime, shipped in the binary. A script identical to it gets the runtime profile.
pub const RUNTIME_JS: &str = include_str!("runtime.js");

const FORBIDDEN_TAGS: &[&str] = &[
    "link", "iframe", "frame", "frameset", "object", "embed", "applet", "base", "form", "img", "picture", "source", "track", "audio", "video", "portal", "foreignobject", "use", "image", "animate", "set", "animatemotion",
];
/// Attributes that carry an address. Only `href="#anchor"` is allowed.
const URL_ATTRS: &[&str] = &[
    "href", "src", "action", "formaction", "data", "poster", "srcset", "ping", "background", "xlink:href", "manifest", "cite", "longdesc", "usemap", "codebase", "icon", "profile", "lowsrc", "dynsrc", "srcdoc",
];
const OK_META_NAMES: &[&str] = &["viewport", "color-scheme", "description", "generator", "theme-color"];

/// Words that must not appear in a free-form script (whole identifiers). Strict profile.
const SCRIPT_WORDS: &[&str] = &[
    "eval", "Function", "constructor", "__proto__", "__defineGetter__", "__defineSetter__", "__lookupGetter__", "setTimeout", "setInterval", "setImmediate", "fetch", "XMLHttpRequest", "WebSocket", "EventSource",
    "sendBeacon", "importScripts", "Worker", "SharedWorker", "ServiceWorker", "WebAssembly", "location", "navigator", "cookie", "localStorage", "sessionStorage", "indexedDB", "postMessage", "opener", "globalThis", "atob",
    "btoa", "fromCharCode", "fromCodePoint", "unescape", "Reflect", "Proxy", "innerHTML", "outerHTML", "insertAdjacentHTML", "insertAdjacentElement", "srcdoc", "createElement", "createElementNS", "createContextualFragment",
    "importNode", "adoptNode", "DOMParser", "setAttribute", "setAttributeNS", "setAttributeNode", "execCommand", "Image", "Audio", "FileReader", "BroadcastChannel", "MessageChannel", "Notification",
];
/// Words that are ordinary English or class names when they stand alone but a problem as a property: `a.href = …`,
/// `win.open(…)`, `document.write(…)`. Flagged only after a `.`.
const MEMBER_WORDS: &[&str] = &["href", "src", "action", "formAction", "open", "write", "writeln", "history"];
/// Calls that need a different test than a whole word.
const CALL_PATTERNS: &[&str] = &["import("];
/// The runtime profile allows the handful of words the audited runtime needs; everything else above still applies.
const RUNTIME_ALLOWED: &[&str] = &["setAttribute", "createElement", "execCommand", "setTimeout"];
/// Identifiers that are only a problem on their own (`window.top` is caught separately; `rect.top` is fine).
const BARE_ONLY: &[&str] = &["top", "parent", "frames"];
const GLOBALS_DOT: &[&str] = &["window", "self", "this", "document", "globalThis", "top"];
const GLOBAL_PROPS: &[&str] = &["top", "parent", "frames", "opener", "location", "open", "history"];

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// `w` as a whole identifier in `hay`.
fn has_word(hay: &str, w: &str) -> bool {
    let mut from = 0;
    while let Some(p) = hay[from..].find(w) {
        let at = from + p;
        let before = hay[..at].chars().next_back();
        let after = hay[at + w.len()..].chars().next();
        if !before.is_some_and(is_ident) && !after.is_some_and(is_ident) {
            return true;
        }
        from = at + w.len();
    }
    false
}

/// Like [`has_word`] but only where the word is not a property access (`.top`).
fn has_bare_word(hay: &str, w: &str) -> bool {
    let mut from = 0;
    while let Some(p) = hay[from..].find(w) {
        let at = from + p;
        let before = hay[..at].chars().next_back();
        let after = hay[at + w.len()..].chars().next();
        if !before.is_some_and(|c| is_ident(c) || c == '.') && !after.is_some_and(is_ident) {
            return true;
        }
        from = at + w.len();
    }
    false
}

/// `w` used as a property (`x.w`), whatever precedes the dot.
fn has_member_word(hay: &str, w: &str) -> bool {
    let mut from = 0;
    while let Some(p) = hay[from..].find(w) {
        let at = from + p;
        let before = hay[..at].trim_end().chars().next_back();
        let after = hay[at + w.len()..].chars().next();
        if before == Some('.') && !after.is_some_and(is_ident) {
            return true;
        }
        from = at + w.len();
    }
    false
}

fn strip_js_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut it = src.chars().peekable();
    while let Some(c) = it.next() {
        if c == '/' && it.peek() == Some(&'/') {
            for n in it.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && it.peek() == Some(&'*') {
            it.next();
            let mut prev = ' ';
            for n in it.by_ref() {
                if prev == '*' && n == '/' {
                    break;
                }
                prev = n;
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Profile {
    User,
    Runtime,
}

fn lint_script(src: &str, profile: Profile, out: &mut Vec<String>) {
    let code = strip_js_comments(src);
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let mut bad = |rule: &str, what: String| out.push(format!("{}: {}", rule, what));
    for w in SCRIPT_WORDS {
        if profile == Profile::Runtime && RUNTIME_ALLOWED.contains(w) {
            continue;
        }
        if has_word(&code, w) {
            bad("script", format!("`{}` is not allowed (a page may not reach the network, build code at run time, navigate, or change its own markup); rewrite the script without it", w));
        }
    }
    for w in MEMBER_WORDS {
        if has_member_word(&code, w) {
            bad("script", format!("`.{}` is not allowed (it can load a file, navigate, or rewrite the page)", w));
        }
    }
    for pat in CALL_PATTERNS {
        if compact.contains(pat) {
            bad("script", format!("`{}` is not allowed (it loads other code)", pat));
        }
    }
    for w in BARE_ONLY {
        if has_bare_word(&code, w) {
            bad("script", format!("`{}` on its own is the browser window's frame object and is not allowed", w));
        }
    }
    for g in GLOBALS_DOT {
        for p in GLOBAL_PROPS {
            if compact.contains(&format!("{}.{}", g, p)) {
                bad("script", format!("`{}.{}` is not allowed", g, p));
            }
        }
        if compact.contains(&format!("{}[", g)) {
            bad("script", format!("`{}[` (looking up a global by a computed name) is not allowed", g));
        }
    }
    if profile == Profile::User {
        for pat in ["['", "[\"", "[`"] {
            if compact.contains(pat) {
                bad("script", "indexing by a string (`obj['name']`) can hide a forbidden name; use dot access".to_string());
                break;
            }
        }
    }
    // escapes can spell any identifier (`fetch`)
    let bytes = code.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\\' {
            match bytes.get(i + 1) {
                Some(b'u') | Some(b'x') | Some(b'0'..=b'9') => {
                    bad("script", "`\\u`, `\\x` and numeric escapes are not allowed (they can spell any name)".to_string());
                    break;
                }
                _ => {}
            }
        }
    }
    let lower = code.to_ascii_lowercase();
    for scheme in ["http:", "https:", "ftp:", "ws:", "wss:", "javascript:", "data:", "blob:", "file:", "vbscript:"] {
        if lower.contains(scheme) {
            bad("rule 1 (no network)", format!("the address scheme `{}` appears in the script", scheme));
        }
    }
    if lower.contains("//") && code.contains("://") {
        bad("rule 1 (no network)", "a `://` address appears in the script".to_string());
    }
    // The browser ends a script element at `</script` whatever the JavaScript around it says, comments included,
    // so these are checked on the source as written.
    let raw_lower = src.to_ascii_lowercase();
    for pat in ["<!--", "-->", "</", "<script", "<style"] {
        if raw_lower.contains(pat) {
            bad("script", format!("`{}` inside a script can change how the browser reads the page", pat));
        }
    }
}

fn lint_css(css: &str, out: &mut Vec<String>) {
    // comments carry no meaning for the checks below, and CSS escapes are not allowed at all
    let mut plain = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        plain.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => {
                rest = "";
            }
        }
    }
    plain.push_str(rest);
    let lower = plain.to_ascii_lowercase();
    for (pat, why) in [
        ("url(", "`url(` can load a file"),
        ("@import", "`@import` loads a stylesheet"),
        ("image-set(", "`image-set(` can load a file"),
        ("expression(", "`expression(` runs code"),
        ("behavior", "`behavior` attaches code"),
        ("-moz-binding", "`-moz-binding` attaches code"),
        ("javascript:", "`javascript:` is code"),
        ("\\", "CSS escapes can hide a forbidden word"),
    ] {
        if lower.contains(pat) {
            out.push(format!("rule 1 (no network): style: {}", why));
        }
    }
    if lower.contains("</") || lower.contains("<!--") {
        out.push("style: `</` or `<!--` inside a style changes how the browser reads the page".to_string());
    }
}

/// Every problem with `page`; empty means it may be written.
pub fn lint(page: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let (mut in_script, mut in_style) = (false, false);
    let mut buf = String::new();
    let mut script_type_ok = true;
    let mut seen_csp = false;
    let walked = html::walk(page, 2_000_000, |ev| match ev {
        Ev::Start { name, attrs } => {
            let n = name.as_str();
            if FORBIDDEN_TAGS.contains(&n) {
                out.push(format!("rule 1 (no network): `<{}>` is not allowed in a generated page", n));
            }
            match n {
                "meta" => {
                    let equiv = attr(&attrs, "http-equiv").map(|s| s.to_ascii_lowercase());
                    let named = attr(&attrs, "name").map(|s| s.to_ascii_lowercase());
                    match (equiv.as_deref(), named.as_deref()) {
                        (Some("content-security-policy"), _) => seen_csp = true,
                        (Some(other), _) => out.push(format!("rule 1 (no network): `<meta http-equiv=\"{}\">` is not allowed (it can redirect or reload the page)", other)),
                        (None, Some(nm)) if OK_META_NAMES.contains(&nm) => {}
                        (None, None) if attrs.iter().any(|(k, _)| k == "charset") => {}
                        _ => out.push("`<meta>` may only set the character set, viewport, colour scheme, description or the page's content security policy".to_string()),
                    }
                }
                "script" => {
                    in_script = true;
                    buf.clear();
                    script_type_ok = true;
                    if attr(&attrs, "src").is_some() {
                        out.push("rule 1 (no network): `<script src>` loads code from elsewhere".to_string());
                    }
                    if let Some(t) = attr(&attrs, "type") {
                        let t = t.to_ascii_lowercase();
                        if t != "text/javascript" && t != "application/javascript" {
                            script_type_ok = false;
                            out.push(format!("rule 2 (no dynamic code): `<script type=\"{}\">` is not allowed (modules and import maps load other code)", t));
                        }
                    }
                }
                "style" => {
                    in_style = true;
                    buf.clear();
                }
                _ => {}
            }
            for (k, v) in &attrs {
                let key = k.to_ascii_lowercase();
                if key.starts_with("on") {
                    out.push(format!("rule 2 (no dynamic code): the `{}` event-handler attribute is not allowed; the runtime attaches handlers itself", key));
                } else if key == "style" {
                    lint_css(v, &mut out);
                } else if URL_ATTRS.contains(&key.as_str()) {
                    let ok = (key == "href" || key == "xlink:href") && v.trim().starts_with('#') && !v.contains("javascript:");
                    if !ok && !v.trim().is_empty() {
                        out.push(format!("rule 1 (no network): `{}=\"{}\"` is not allowed; only in-page `#anchor` links are", key, v.chars().take(60).collect::<String>()));
                    } else if v.trim().is_empty() && key == "srcdoc" {
                        out.push("rule 2 (no dynamic code): `srcdoc` is not allowed".to_string());
                    }
                }
            }
        }
        Ev::Text(t) => {
            if in_script || in_style {
                buf.push_str(&t);
            }
        }
        Ev::End { name } => match name.as_str() {
            "script" if in_script => {
                in_script = false;
                if script_type_ok {
                    let profile = if buf.trim() == RUNTIME_JS.trim() { Profile::Runtime } else { Profile::User };
                    lint_script(&buf, profile, &mut out);
                }
            }
            "style" if in_style => {
                in_style = false;
                lint_css(&buf, &mut out);
            }
            _ => {}
        },
    });
    if walked.is_err() {
        out.push("the page is too large or too deeply nested to check".to_string());
    }
    if in_script || in_style {
        out.push("an unterminated `<script>` or `<style>` hides the rest of the page from the check".to_string());
    }
    if !seen_csp {
        out.push("rule 4: the page has no Content-Security-Policy meta tag".to_string());
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|v| seen.insert(v.clone()));
    out
}

/// Check a free-form script on its own (before it is placed in a page), for the card and for early refusal.
pub fn lint_user_script(src: &str) -> Vec<String> {
    let mut out = vec![];
    lint_script(src, Profile::User, &mut out);
    let mut seen = std::collections::HashSet::new();
    out.retain(|v| seen.insert(v.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSP: &str = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'\">";

    fn page(body: &str) -> String {
        format!("<!doctype html><html><head><meta charset=\"utf-8\">{}<title>t</title></head><body>{}</body></html>", CSP, body)
    }

    fn rejected(body: &str) -> Vec<String> {
        lint(&page(body))
    }

    #[test]
    fn a_plain_page_passes_and_one_without_a_csp_does_not() {
        assert!(lint(&page("<h1>Hi</h1><p>text <a href=\"#top\">top</a></p><table><tr><td>1</td></tr></table>")).is_empty());
        let v = lint("<!doctype html><html><head><title>t</title></head><body>x</body></html>");
        assert!(v.iter().any(|m| m.contains("no Content-Security-Policy")), "{:?}", v);
    }

    #[test]
    fn the_runtime_passes_its_own_profile_and_a_user_script_with_the_same_words_does_not() {
        let mut v = vec![];
        lint_script(RUNTIME_JS, Profile::Runtime, &mut v);
        assert!(v.is_empty(), "the audited runtime must pass: {:?}", v);
        assert!(lint(&page(&format!("<script>{}</script>", RUNTIME_JS))).is_empty());
        // the same code with one byte changed is a user script, held to the strict list
        let altered = format!("{} ", RUNTIME_JS.replace("'use strict';", "'use strict'; var x = 1;"));
        assert!(!lint(&page(&format!("<script>{}</script>", altered))).is_empty());
    }

    #[test]
    fn rule_1_nothing_reaches_the_network() {
        for (html, expect) in [
            ("<link rel=stylesheet href=x.css>", "<link>"),
            ("<iframe src=x></iframe>", "<iframe>"),
            ("<object data=x></object>", "<object>"),
            ("<embed src=x>", "<embed>"),
            ("<img src=x>", "<img>"),
            ("<base href=x>", "<base>"),
            ("<form action=x><input></form>", "<form>"),
            ("<script src=\"https://e.com/x.js\"></script>", "<script src>"),
            ("<a href=\"https://e.com\">x</a>", "href="),
            ("<a href=\"//e.com\">x</a>", "href="),
            ("<a href=\"javascript:alert(1)\">x</a>", "href="),
            ("<a href=\" data:text/html,x\">x</a>", "href="),
            ("<button formaction=\"x\">b</button>", "formaction"),
            ("<svg><use href=\"x.svg#a\"/></svg>", "<use>"),
            ("<svg><image href=x /></svg>", "<image>"),
            ("<svg><foreignObject><div></div></foreignObject></svg>", "<foreignobject>"),
            ("<video src=x></video>", "<video>"),
            ("<p style=\"background:url(http://e.com/x)\">x</p>", "url("),
            ("<style>@import 'x.css';</style>", "@import"),
            ("<style>p{background:u\\72l(x)}</style>", "CSS escapes"),
        ] {
            let v = rejected(html);
            assert!(v.iter().any(|m| m.contains(expect)), "{:?} should be rejected for {:?}, got {:?}", html, expect, v);
        }
        let v = lint(&format!("<!doctype html><html><head><meta http-equiv=refresh content=\"0;url=https://e.com\">{}</head><body></body></html>", CSP));
        assert!(v.iter().any(|m| m.contains("http-equiv=\"refresh\"")), "{:?}", v);
    }

    #[test]
    fn rule_2_no_code_assembled_at_run_time() {
        for (js, word) in [
            ("eval('1')", "eval"),
            ("new Function('return 1')()", "Function"),
            ("setTimeout('x()', 1)", "setTimeout"),
            ("document.write('x')", "write"),
            ("el.innerHTML = x", "innerHTML"),
            ("el.insertAdjacentHTML('beforeend', x)", "insertAdjacentHTML"),
            ("import('x')", "import"),
            ("fetch('x')", "fetch"),
            ("new XMLHttpRequest()", "XMLHttpRequest"),
            ("new WebSocket('x')", "WebSocket"),
            ("navigator.sendBeacon('x')", "sendBeacon"),
            ("location.href = 'x'", "location"),
            ("window.open('x')", "open"),
            ("window.top.x", "window.top"),
            ("this[k]", "this["),
            ("window['loc'+'ation']", "window["),
            ("el['inner'+'HTML']", "indexing by a string"),
            ("\\u0066etch('x')", "escapes"),
            ("var a = document.createElement('script')", "createElement"),
            ("a.setAttribute('onclick', 'x')", "setAttribute"),
            ("a.href = 'x'", "href"),
            ("top.x()", "`top`"),
            ("var u = 'https://e.com'", "https:"),
            ("var u = 'x'; // </script><script src=y>", "</"),
            ("document.cookie", "cookie"),
            ("atob(x)", "atob"),
            ("Proxy", "Proxy"),
        ] {
            let v = lint_user_script(js);
            assert!(v.iter().any(|m| m.contains(word)), "{:?} should be rejected for {:?}, got {:?}", js, word, v);
        }
        for attr in ["onclick=\"x()\"", "onload=x", "onerror=\"x\"", "srcdoc=\"<b>\""] {
            let v = rejected(&format!("<p {}>x</p>", attr));
            assert!(v.iter().any(|m| m.contains("rule 2") || m.contains("rule 1")), "{:?}: {:?}", attr, v);
        }
        assert!(rejected("<script type=\"module\">x</script>").iter().any(|m| m.contains("rule 2")));
    }

    #[test]
    fn ordinary_scripts_pass() {
        let ok = "document.querySelectorAll('.card').forEach(function (c) {\n  c.addEventListener('click', function () { c.classList.toggle('open'); c.dataset.n = String(Number(c.dataset.n || 0) + 1); });\n});\nvar r = el.getBoundingClientRect(); var y = r.top + window.innerHeight; // positions are fine\nfor (var i = 0; i < items.length; i++) { total += items[i].value; }";
        assert!(lint_user_script(ok).is_empty(), "{:?}", lint_user_script(ok));
        assert!(lint(&page(&format!("<script>{}</script>", ok))).is_empty());
    }

    #[test]
    fn breaking_out_of_a_script_element_is_caught_by_re_parsing_the_whole_page() {
        let user = "var a = 1;</script><script src=\"https://e.com/x.js\"></script><script>var b = 2;";
        let v = lint(&page(&format!("<script>{}</script>", user)));
        assert!(v.iter().any(|m| m.contains("<script src>")), "{:?}", v);
        let v = lint(&page("<script>var a = 1;<!-- <script>"));
        assert!(!v.is_empty(), "{:?}", v);
        let v = lint(&page("<script>var a = 1;"));
        assert!(v.iter().any(|m| m.contains("unterminated")), "{:?}", v);
    }

    #[test]
    fn junk_does_not_panic_or_hang() {
        let mut x: u64 = 0x0bad_f00d_dead_beef;
        for n in 0..1500usize {
            let s: String = (0..n % 200)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    ["<", ">", "/", "script", "style", "on", "href=", "\"", "'", "=", " ", "meta", "http-equiv", "url(", "\\", "[", "window", "."][(x as usize) % 18]
                })
                .collect();
            let _ = lint(&s);
            let _ = lint_user_script(&s);
        }
    }
}
