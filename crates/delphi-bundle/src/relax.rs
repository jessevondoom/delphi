//! The deterministic no-relaxation check (`specs/001` §6.4 spirit; `specs/023` §4.2, §5B, §6.2, §6.3).
//!
//! Text an agent will follow must not try to loosen the rules that bind it: skip an approval, ignore governance, override
//! a gate, widen a tool. This is a keyword and pattern check, not a model's judgment. It is an error where the text is
//! instruction (agents, role guidance, perspectives) and a warning in `knowledge/`, where prose about deployment may
//! legitimately say "never deploy on Fridays".

use crate::tree::Tree;
use crate::Report;
use regex::Regex;

/// Patterns for text that tries to relax a rule. Each is matched case-insensitively against one line at a time.
const RELAXING: [&str; 12] = [
    r"\b(skip|bypass|waive|disable)\w*\b.{0,30}\b(approv\w*|review\w*|gate\w*|check\w*|hitl|red[- ]team|governance|taint)\b",
    r"\b(without|no need (for|to get)|don'?t (need|wait for)|never (ask|wait|seek|request))\b.{0,30}\b(approval|permission|review|confirmation|sign-?off)\b",
    r"\bauto[- ]?(approve|merge|accept)\b",
    r"\b(ignore|disregard|forget|override|overrule|supersede)\b.{0,40}\b(previous|prior|above|earlier|system|governance|rules?|instructions?|guidelines?|constraints?|review\.md)\b",
    r"\b(you may|you can|feel free to|is allowed to|are allowed to)\b.{0,30}\b(skip|ignore|bypass|override)\b",
    r"\b(treat|consider)\b.{0,40}\b(as|to be)\b.{0,20}\b(trusted|approved|ratified|verified)\b",
    r"\b(widen|expand|escalate|grant)\w*\b.{0,30}\b(permissions?|scope|access|tools?)\b",
    r"\b(run|execute)\b.{0,30}\b(any|every|all)\b.{0,20}\bcommands?\b",
    r"\bsudo\b",
    r"\bdangerously[- ]?(skip|disable|allow)\w*",
    r"\bpermission[- ]?mode\b",
    r"\b(raise|lift|remove|relax)\b.{0,20}\b(cap|limit|budget)s?\b",
];

/// Lines (1-based) of `text` that look like an attempt to relax a rule, with the pattern that matched. Fenced code blocks
/// are skipped: a quotation is not an instruction.
pub fn no_relaxation(text: &str) -> Vec<(usize, String)> {
    let res: Vec<Regex> = RELAXING.iter().map(|p| Regex::new(&format!("(?i){}", p)).unwrap()).collect();
    let mut out = Vec::new();
    let mut fenced = false;
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if res.iter().any(|re| re.is_match(line)) {
            out.push((i + 1, line.trim().chars().take(120).collect()));
        }
    }
    out
}

/// [`no_relaxation`] over a document's body, with line numbers counted from the top of the file (frontmatter included).
pub fn no_relaxation_doc(text: &str) -> Vec<(usize, String)> {
    let body = crate::ids::body_of(text);
    let skipped = text.lines().count().saturating_sub(body.lines().count());
    no_relaxation(body).into_iter().map(|(l, q)| (l + skipped, q)).collect()
}

/// Step 11: instruction-shaped lines in `knowledge/**`, as warnings with path and line.
pub fn check_knowledge(tree: &Tree, r: &mut Report) {
    for (path, text) in tree.texts("knowledge/", ".md") {
        for (line, quote) in no_relaxation_doc(&text) {
            r.warn(11, &format!("{}:{}", path, line), format!("reads as an instruction that relaxes a rule; instructions belong in behavior/ (`{}`)", quote));
        }
    }
}
