//! A role the operator names (`specs/107` §8.2): the fixed rule that says when a task asks for a role by name, in words ("ask the
//! strategist", "a red-team review", "the architect's view") or by an `@` mention. One copy, used by `route`, the briefing and the
//! prompt hook. It reads the task text and nothing else, runs no model, and a name only chooses which role's advice the host is
//! pointed at: it dispatches nothing, raises no score and grants nothing.

use delphi_core::theme_names::ThemeResolver;

/// At most this many roles are named by one task.
pub const MAX_NAMED: usize = 3;

const ARTICLES: &[&str] = &["the", "a", "an", "our", "my", "your"];
/// Verbs of asking: `ask the strategist`, `get a strategist review`.
const ASK: &[&str] = &["ask", "have", "get", "let", "want", "need", "consult", "involve", "summon", "invite", "ping"];
/// `bring in the red team`.
const IN: &[&str] = &["bring", "loop", "call", "pull"];
/// `hand this to the writer`, `pass the draft to the writer` (at most two words between the verb and `to`).
const TO: &[&str] = &["hand", "send", "pass", "give", "route", "delegate", "assign", "forward", "escalate", "take", "talk", "speak"];
/// `check with the architect`.
const WITH: &[&str] = &["check", "consult", "pair"];
/// `act as a strategist`.
const AS: &[&str] = &["act", "answer", "respond", "reply", "speak", "think"];
/// After a name (`a strategist review`, `the architect's view`), or before `from` (`feedback from the architect`).
const REVIEW: &[&str] = &["review", "reviews", "pass", "take", "view", "views", "opinion", "perspective", "critique", "lens", "read", "look", "eyes", "input", "feedback", "assessment", "analysis", "verdict", "sign-off", "signoff", "hat"];
/// Besides the review words, what may come up to three words before `from`: `a second opinion from`, `a word from`.
const FROM_EXTRA: &[&str] = &["opinion", "word", "help"];
/// A token that ends a phrase: no frame reaches across it.
const BREAK: &str = "|";

/// One name of a role: its words, lower-cased.
#[derive(Debug, Clone, PartialEq)]
pub struct Name {
    pub words: Vec<String>,
    pub role: String,
}

/// The names of `roles` (§8.2 item 2): the role's ID, the ID with its hyphen as a space or removed, and the theme's display name
/// when a resolver is given. The orchestrator is left out (it is the default already), and so is `delphi`.
pub fn names(roles: &[&str], resolver: Option<&ThemeResolver>) -> Vec<Name> {
    let mut out: Vec<Name> = Vec::new();
    for &role in roles {
        if role == "orchestrator" {
            continue;
        }
        let mut forms = vec![role.to_lowercase(), role.replace('-', " ").to_lowercase(), role.replace('-', "").to_lowercase()];
        if let Some(r) = resolver {
            forms.push(r.display_name(role).to_lowercase());
        }
        for f in forms {
            let words: Vec<String> = tokens(&f).into_iter().filter(|w| w != BREAK).collect();
            if words.is_empty() || (words.len() == 1 && words[0] == "delphi") {
                continue;
            }
            let n = Name { words, role: role.to_string() };
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// The task with its fenced and inline code taken out (each piece becomes a break), so a role's word in code names nothing.
fn prose(text: &str) -> String {
    let mut out = String::new();
    let mut fence = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            out.push_str(" . \n");
            continue;
        }
        if fence {
            continue;
        }
        let mut code = false;
        for c in line.chars() {
            if c == '`' {
                code = !code;
                out.push_str(" . ");
            } else if !code {
                out.push(c);
            }
        }
        out.push('\n');
    }
    out
}

/// Lower-cased words (letters, digits, `-`), with `'s` dropped; punctuation that ends a phrase becomes [`BREAK`].
fn tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let mut w = cur.trim_matches(|c: char| c == '-' || c == '\'').to_string();
        for s in ["'s", "\u{2019}s"] {
            if let Some(x) = w.strip_suffix(s) {
                w = x.to_string();
            }
        }
        let w = w.trim_matches(|c: char| c == '-' || c == '\'' || c == '\u{2019}').to_string();
        if !w.is_empty() {
            out.push(w);
        }
        cur.clear();
    };
    for c in text.to_lowercase().chars() {
        if c.is_alphanumeric() || c == '-' || c == '\'' || c == '\u{2019}' {
            cur.push(c);
        } else {
            flush(&mut cur, &mut out);
            if matches!(c, '.' | '!' | '?' | ';' | ':' | ',' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\n') && out.last().is_some_and(|l| l != BREAK) {
                out.push(BREAK.to_string());
            }
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn is(t: Option<&String>, set: &[&str]) -> bool {
    t.is_some_and(|w| set.contains(&w.as_str()))
}

/// Does the name at `t[i..i + n]` stand in one of the frames of §8.2 item 1?
fn in_frame(t: &[String], i: usize, n: usize) -> bool {
    let at = |k: isize| -> Option<&String> { usize::try_from(k).ok().and_then(|k| t.get(k)) };
    if is(t.get(i + n), REVIEW) {
        return true;
    }
    let mut j = i as isize;
    let article = at(j - 1).filter(|w| ARTICLES.contains(&w.as_str())).cloned();
    if article.is_some() {
        j -= 1;
    }
    let Some(p) = at(j - 1) else { return false };
    match p.as_str() {
        w if ASK.contains(&w) => true,
        "in" => is(at(j - 2), IN),
        "to" => (2..=4).map(|k| at(j - k)).take_while(|w| w.is_some_and(|w| w != BREAK)).flatten().any(|w| TO.contains(&w.as_str())),
        "with" => is(at(j - 2), WITH),
        "as" => matches!(article.as_deref(), Some("the" | "our")) || is(at(j - 2), AS),
        "from" => (2..=4).filter_map(|k| at(j - k)).take_while(|w| *w != BREAK).any(|w| REVIEW.contains(&w.as_str()) || FROM_EXTRA.contains(&w.as_str())),
        _ => false,
    }
}

/// The roles `text` names (§8.2 items 1 and 3), in the order they are first named, at most [`MAX_NAMED`]: by a frame in words, by
/// the task opening with a name and `,` or `:`, or by an `@` mention of one of `names`. Only roles among `names` are returned.
pub fn named(text: &str, names: &[Name]) -> Vec<String> {
    let mut found: Vec<(usize, String)> = Vec::new();
    let mut add = |pos: usize, role: &str| {
        if let Some(f) = found.iter_mut().find(|(_, r)| r == role) {
            f.0 = f.0.min(pos);
        } else {
            found.push((pos, role.to_string()));
        }
    };
    // An `@` mention, read as the prompt hook reads one; its place is taken as 0 (a mention is the plainest way to name).
    for w in crate::host_hooks::mentions(text) {
        let w = w.to_lowercase();
        if let Some(n) = names.iter().find(|n| n.words.len() == 1 && n.words[0] == w) {
            add(0, &n.role);
        }
    }
    let p = prose(text);
    let t = tokens(&p);
    // The task opens with a name and `,` or `:` ("Strategist, look at this").
    let head = p.trim_start().to_lowercase();
    for n in names {
        let joined = n.words.join(" ");
        if head.strip_prefix(&joined).is_some_and(|rest| rest.starts_with(',') || rest.starts_with(':')) {
            add(0, &n.role);
        }
    }
    for i in 0..t.len() {
        for n in names {
            let k = n.words.len();
            if t.len() >= i + k && t[i..i + k] == n.words[..] && in_frame(&t, i, k) {
                add(i, &n.role);
            }
        }
    }
    found.sort_by_key(|(pos, _)| *pos);
    found.into_iter().map(|(_, r)| r).take(MAX_NAMED).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Name> {
        let ids: Vec<&str> = crate::mcp_hosts::WORKER_ROLES.iter().map(|(r, _)| *r).collect();
        names(&ids, Some(&ThemeResolver::default()))
    }

    #[test]
    fn the_five_prompts_of_the_audit_name_their_role() {
        let n = all();
        assert_eq!(named("get a strategist review of this plan", &n), ["strategist"]);
        assert_eq!(named("strategist review", &n), ["strategist"]);
        assert_eq!(named("have the strategist look at the sequencing of the Q1 roadmap", &n), ["strategist"]);
        assert_eq!(named("ask the red team to attack this design", &n), ["red-team"]);
        assert_eq!(named("get a second opinion from the architect on this schema", &n), ["architect"]);
    }

    #[test]
    fn each_frame_names_and_a_role_word_in_other_use_does_not() {
        let n = all();
        for (t, want) in [
            ("bring in the red team before we ship", "red-team"),
            ("hand this to the writer", "writer"),
            ("check with the architect first", "architect"),
            ("answer as the strategist", "strategist"),
            ("act as a scout and find sources", "scout"),
            ("what is the architect's view on this?", "architect"),
            ("Strategist, look at the pricing", "strategist"),
            ("@investigator why is it slow", "investigator"),
            ("do a red-team pass over the plan", "red-team"),
        ] {
            assert_eq!(named(t, &n), [want], "{t}");
        }
        for t in ["use the builder pattern for the config", "fix the validator so it rejects empty names", "the writer thread panics", "review the architect field in the schema", "run `ask the strategist` in the shell", "ask. The strategist field is empty"] {
            assert!(named(t, &n).is_empty(), "{t}: {:?}", named(t, &n));
        }
    }

    #[test]
    fn a_themed_name_counts_the_orchestrator_and_delphi_do_not_and_at_most_three_in_order() {
        let n = all();
        let r = ThemeResolver::default();
        let themed = r.display_name("strategist");
        assert_ne!(themed, "strategist");
        assert_eq!(named(&format!("ask {themed} about it"), &n), ["strategist"]);
        assert!(named("ask the orchestrator who should do it", &n).is_empty());
        assert!(named("ask delphi", &n).is_empty());
        assert_eq!(named("ask the writer, then the strategist review, then a scout pass and an architect review", &n), ["writer", "strategist", "scout"]);
        // Without a resolver only the IDs and their forms are names (the hook's in-memory check).
        let ids = names(&["strategist"], None);
        assert!(named(&format!("ask {themed}"), &ids).is_empty());
        assert_eq!(named("ask the strategist", &ids), ["strategist"]);
    }
}
