//! The one parsed form of a topic page (`specs/144` §2.2; slice A).
//!
//! Code parses a draft once into **units**: a heading, a sentence of body prose, a list item, a table row, a figure caption or a
//! `Key terms` line. The same parse is what the page renders and what the validator judges ([`parse`]): the units are listed by number
//! in the validator's contract, and every unit but a heading carries at least one citation marker of its own. A marker covers only the
//! unit it ends.
//!
//! **What carries no marker, by a code test:** headings; the `Contents` and `See also` sections and the hatnote, which code writes
//! ([`strip_code_written`] takes a draft's own copies out, [`code_written`] writes them); and a lead-in ending in `:` that is on the
//! fixed list [`LEAD_INS`]. Any other lead-in is a sentence and needs a marker (`a lead-in that states a fact`).
//!
//! **What the parse cannot place** fails (`a construct the page check cannot read`): raw HTML, an image with a descriptive text, a
//! footnote, a fenced block. Search pages are not parsed: they keep `specs/057` as it is.

use super::page_check::{markers, strip_markers};

/// The lead-ins that state nothing and carry no marker, compared lowercase with spaces collapsed and emphasis removed.
pub const LEAD_INS: [&str; 24] = [
    "the main approaches are:",
    "it has these parts:",
    "it has these main parts:",
    "the main parts are:",
    "the parts are:",
    "these include:",
    "they include:",
    "it includes:",
    "examples include:",
    "for example:",
    "the steps are:",
    "the main steps are:",
    "the stages are:",
    "the main kinds are:",
    "the main types are:",
    "the kinds are:",
    "the types are:",
    "the options are:",
    "the main options are:",
    "the components are:",
    "the main components are:",
    "the main differences are:",
    "in summary:",
    "the following:",
];

/// How the check names a construct the parse cannot place in a unit class.
pub const UNREADABLE: &str = "a construct the page check cannot read";
/// How the check names a unit that needs a marker and has none.
pub const NO_CITATION: &str = "a statement with no citation";
/// How the check names a lead-in not on [`LEAD_INS`] with no marker.
pub const LEAD_IN_FACT: &str = "a lead-in that states a fact";

/// The sections code writes on a topic page, never the writer (§2.2 item 1), and the daemon's own.
pub const CODE_SECTIONS: [&str; 5] = ["Contents", "See also", "Open questions", "Contested", "Unchecked"];

/// The longest image text that is a label, not a description, in words.
pub const IMAGE_LABEL_WORDS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Heading,
    Sentence,
    Item,
    Row,
    Caption,
    KeyTerm,
}

impl UnitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitKind::Heading => "heading",
            UnitKind::Sentence => "sentence",
            UnitKind::Item => "list item",
            UnitKind::Row => "table row",
            UnitKind::Caption => "caption",
            UnitKind::KeyTerm => "key term",
        }
    }
}

/// One unit of a parsed page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// From 1, in page order.
    pub n: usize,
    pub kind: UnitKind,
    /// The unit as the page renders it, markers included, on one line.
    pub text: String,
    /// The labels of the unit's own markers.
    pub cites: Vec<String>,
    /// The number of the heading the unit sits under (0: the lead, before any `## `).
    pub section: usize,
}

impl Unit {
    /// The unit's words without its markers or Markdown emphasis.
    pub fn plain(&self) -> String {
        norm(&strip_markers(&self.text).replace(['*', '`'], "").trim_start_matches('#').replace("  ", " "))
    }
}

/// A parse: the units, and what the check refuses in the draft.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub units: Vec<Unit>,
    pub findings: Vec<String>,
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    (hashes > 0 && hashes <= 6 && t[hashes..].starts_with(' ')).then(|| (hashes, t[hashes..].trim()))
}

fn code_section(name: &str) -> bool {
    CODE_SECTIONS.iter().any(|c| c.eq_ignore_ascii_case(name.trim()))
}

/// An italic one-line pointer to another page (`> *This page is about X. For Y, see [Y](page:y).*`).
pub fn is_hatnote(line: &str) -> bool {
    line.trim_start().strip_prefix('>').map(str::trim).is_some_and(|q| q.len() > 2 && q.starts_with('*') && q.ends_with('*') && q.contains("](page:"))
}

/// A draft without the parts code writes: its own hatnote and its own `Contents` and `See also` sections (and any of the daemon's).
pub fn strip_code_written(body: &str) -> String {
    // A frontmatter block the writer put first: the daemon writes the page's own, so the draft's is no part of the page.
    let body = match body.trim_start().strip_prefix("---\n").and_then(|r| r.split_once("\n---\n")) {
        Some((_, rest)) => rest,
        None => body,
    };
    let mut out: Vec<&str> = Vec::new();
    let mut skip_level: Option<usize> = None;
    let mut fence = false;
    for l in body.lines() {
        if l.trim_start().starts_with("```") {
            fence = !fence;
        }
        if !fence {
            if let Some((level, name)) = heading(l) {
                if skip_level.is_some_and(|s| level <= s) {
                    skip_level = None;
                }
                if level >= 2 && code_section(name) {
                    skip_level = Some(level);
                    continue;
                }
            }
        }
        if skip_level.is_some() || (!fence && is_hatnote(l)) {
            continue;
        }
        out.push(l);
    }
    let s = out.join("\n");
    let mut t = s.trim().to_string();
    while t.contains("\n\n\n") {
        t = t.replace("\n\n\n", "\n\n");
    }
    t + "\n"
}

/// The parts code writes on a topic page (§2.2 item 1): the hatnote under the title, `## Contents` after the lead when there are two or
/// more `## ` sections, and `## See also` last, from `see_also` (slug, title). `body` is a parsed draft without them.
pub fn code_written(body: &str, hatnote: Option<&str>, see_also: &[(String, String)]) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let sections: Vec<&str> = lines.iter().filter_map(|l| heading(l).filter(|(lv, _)| *lv == 2).map(|(_, n)| n)).collect();
    let mut out = String::new();
    let mut put_hat = hatnote.is_some();
    let mut put_contents = sections.len() >= 2;
    for (i, l) in lines.iter().enumerate() {
        if put_contents && heading(l).is_some_and(|(lv, _)| lv == 2) {
            out.push_str("## Contents\n\n");
            for (k, s) in sections.iter().enumerate() {
                out.push_str(&format!("{}. [{}](#{})\n", k + 1, s.replace(['[', ']'], ""), super::page_check::slug_of(s)));
            }
            out.push('\n');
            put_contents = false;
        }
        out.push_str(l);
        out.push('\n');
        if put_hat && i == 0 && heading(l).is_some_and(|(lv, _)| lv == 1) {
            out.push_str(&format!("\n> *{}*\n", hatnote.unwrap_or_default()));
            put_hat = false;
        }
    }
    if put_hat {
        out = format!("> *{}*\n\n{}", hatnote.unwrap_or_default(), out);
    }
    if !see_also.is_empty() {
        out = out.trim_end().to_string();
        out.push_str("\n\n## See also\n\n");
        for (slug, title) in see_also {
            out.push_str(&format!("- [{}](page:{})\n", title.replace(['[', ']', '\n'], ""), slug));
        }
    }
    out
}

const ABBREVIATIONS: [&str; 16] = ["e.g.", "i.e.", "etc.", "vs.", "cf.", "al.", "dr.", "mr.", "mrs.", "ms.", "no.", "fig.", "approx.", "st.", "inc.", "ltd."];

/// The sentences of a run of prose, each with the markers that follow its end. A sentence ends at `.`, `!` or `?` (after any closing
/// quotation mark, bracket or emphasis and any markers) where a space and a capital, a digit, a quotation mark or emphasis follow, or at
/// the end. A known abbreviation or a single capital letter before the stop does not end one.
pub fn sentences(text: &str) -> Vec<String> {
    let text = norm(text);
    let spans = super::page_check::marker_spans(&text);
    let in_marker = |i: usize| spans.iter().any(|(s, e)| i >= *s && i < *e);
    let b: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut k = 0usize;
    while k < b.len() {
        let (i, c) = b[k];
        if matches!(c, '.' | '!' | '?') && !in_marker(i) {
            let word_start = text[..i].rfind(' ').map_or(0, |p| p + 1);
            let word = text[word_start..=i].to_lowercase();
            let single = word.trim_start_matches(['(', '"', '“', '*']).chars().count() == 2 && word.chars().next().is_some_and(|c| c.is_alphabetic());
            let bare = word.trim_start_matches(['(', '"', '“', '*']);
            let abbreviation = c == '.' && (ABBREVIATIONS.contains(&bare) || single);
            if !abbreviation {
                // Past the closers and any markers.
                let mut j = i + c.len_utf8();
                loop {
                    let rest = &text[j..];
                    if let Some(ch) = rest.chars().next().filter(|ch| matches!(ch, '"' | '”' | '’' | ')' | '*' | '_')) {
                        j += ch.len_utf8();
                        continue;
                    }
                    let trimmed = rest.trim_start();
                    let ws = rest.len() - trimmed.len();
                    if let Some((_, e)) = spans.iter().find(|(s, _)| *s == j + ws) {
                        j = *e;
                        continue;
                    }
                    break;
                }
                let rest = &text[j..];
                let next = rest.trim_start().chars().next();
                let boundary = match next {
                    None => true,
                    Some(n) => rest.starts_with(' ') && (n.is_uppercase() || n.is_ascii_digit() || matches!(n, '"' | '“' | '*' | '(' | '[' | '_')),
                };
                if boundary {
                    out.push(text[start..j].trim().to_string());
                    start = j;
                    while k < b.len() && b[k].0 < j {
                        k += 1;
                    }
                    continue;
                }
            }
        }
        k += 1;
    }
    if start < text.len() && !text[start..].trim().is_empty() {
        out.push(text[start..].trim().to_string());
    }
    out.retain(|s| !s.is_empty());
    out
}

fn list_item(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if let Some(r) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")) {
        return Some(r.trim());
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    (digits > 0).then(|| t[digits..].strip_prefix(". ").or_else(|| t[digits..].strip_prefix(") "))).flatten().map(str::trim)
}

fn table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

fn separator_row(l: &str) -> bool {
    let t = l.trim();
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// Raw HTML: a tag or a comment.
fn has_html(s: &str) -> bool {
    let b = s.as_bytes();
    (0..b.len()).any(|i| b[i] == b'<' && b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'/' || *c == b'!'))
}

/// The image of a line that is only an image: (its text, its target).
fn image_line(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    let r = t.strip_prefix("![")?;
    let (alt, rest) = r.split_once("](")?;
    let target = rest.strip_suffix(')')?;
    (!target.contains(')')).then_some((alt, target))
}

/// Does a lead-in, without its markers and emphasis, match one of [`LEAD_INS`]?
pub fn fixed_lead_in(sentence: &str) -> bool {
    let t = norm(&strip_markers(sentence).replace(['*', '_'], "")).to_lowercase();
    LEAD_INS.contains(&t.as_str())
}

/// Parse a topic page's body (a draft without the parts code writes) into its units, with what the check refuses.
pub fn parse(body: &str) -> Parsed {
    let mut p = Parsed::default();
    let mut section = 0usize;
    let mut in_key_terms = false;
    let mut skip_level: Option<usize> = None;
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0usize;
    let push = |p: &mut Parsed, kind: UnitKind, text: &str, section: usize| -> usize {
        let text = norm(text);
        let n = p.units.len() + 1;
        let cites: Vec<String> = markers(&text).iter().map(|c| c.label()).collect();
        if kind != UnitKind::Heading && cites.is_empty() {
            p.findings.push(format!("{}: {}", NO_CITATION, super::page_check::short(&text)));
        }
        p.units.push(Unit { n, kind, text, cites, section });
        n
    };
    let unreadable = |p: &mut Parsed, what: &str, line: &str| p.findings.push(format!("{} ({}): {}", UNREADABLE, what, super::page_check::short(line.trim())));
    while i < lines.len() {
        let l = lines[i];
        let t = l.trim();
        // A blank line, or a thematic break (`---`, `***`, `___`), states nothing.
        if t.is_empty() || (t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))) {
            i += 1;
            continue;
        }
        if t.starts_with("```") {
            unreadable(&mut p, "a fenced block", t);
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                i += 1;
            }
            i += 1;
            continue;
        }
        if let Some((level, name)) = heading(l) {
            if skip_level.is_some_and(|s| level <= s) {
                skip_level = None;
            }
            if level >= 2 && code_section(name) {
                skip_level = Some(level);
                i += 1;
                continue;
            }
            if skip_level.is_none() {
                in_key_terms = name.eq_ignore_ascii_case(super::topics::KEY_TERMS);
                section = push(&mut p, UnitKind::Heading, t, 0);
                if has_html(t) {
                    unreadable(&mut p, "raw HTML", t);
                }
            }
            i += 1;
            continue;
        }
        if skip_level.is_some() || is_hatnote(l) {
            i += 1;
            continue;
        }
        if has_html(t) {
            unreadable(&mut p, "raw HTML", t);
            i += 1;
            continue;
        }
        if t.contains("[^") {
            unreadable(&mut p, "a footnote", t);
            i += 1;
            continue;
        }
        if let Some((alt, _)) = image_line(t) {
            let words = alt.split_whitespace().count();
            if words > IMAGE_LABEL_WORDS || alt.contains(['.', '!', '?', ';']) {
                unreadable(&mut p, "an image with a descriptive text", t);
            }
            i += 1;
            continue;
        }
        if t.contains("![") {
            unreadable(&mut p, "an image inside text", t);
            i += 1;
            continue;
        }
        // A caption: an italic line under an image, or one that starts `*Figure` or `*Table`.
        let after_image = i > 0 && image_line(lines[i - 1].trim()).is_some();
        if t.len() > 2 && t.starts_with('*') && !t.starts_with("**") && t.ends_with(['*', ']', '.']) && (after_image || t.starts_with("*Figure") || t.starts_with("*Table")) {
            push(&mut p, UnitKind::Caption, t, section);
            i += 1;
            continue;
        }
        if table_line(t) {
            let start = i;
            while i < lines.len() && table_line(lines[i]) {
                i += 1;
            }
            let rows = &lines[start..i];
            let header = rows.get(1).is_some_and(|r| separator_row(r));
            for (k, r) in rows.iter().enumerate() {
                if separator_row(r) || (header && k == 0) {
                    continue;
                }
                push(&mut p, UnitKind::Row, r, section);
            }
            continue;
        }
        if let Some(item) = list_item(l) {
            let mut text = item.to_string();
            i += 1;
            // Continuation lines of the item: indented, not blank, not another item.
            while i < lines.len() && !lines[i].trim().is_empty() && lines[i].starts_with([' ', '\t']) && list_item(lines[i]).is_none() {
                text.push(' ');
                text.push_str(lines[i].trim());
                i += 1;
            }
            push(&mut p, if in_key_terms { UnitKind::KeyTerm } else { UnitKind::Item }, &text, section);
            continue;
        }
        // A paragraph of prose (a quotation block's lines are prose too): up to a blank line, a heading, a list, a table or an image.
        let mut para: Vec<&str> = Vec::new();
        while i < lines.len() {
            let x = lines[i].trim();
            if x.is_empty() || (x.len() >= 3 && x.chars().all(|c| c == '-')) || heading(lines[i]).is_some() || table_line(x) || list_item(lines[i]).is_some() || image_line(x).is_some() || x.starts_with("```") || is_hatnote(lines[i]) {
                break;
            }
            para.push(x.trim_start_matches('>').trim());
            i += 1;
        }
        if para.is_empty() {
            i += 1;
            continue;
        }
        let joined = para.join(" ");
        if has_html(&joined) || joined.contains("[^") {
            unreadable(&mut p, if joined.contains("[^") { "a footnote" } else { "raw HTML" }, &joined);
            continue;
        }
        // A lead-in: the paragraph's last sentence ends in `:` and a list or a table follows.
        let next = lines[i..].iter().find(|x| !x.trim().is_empty()).copied().unwrap_or("");
        let leads = list_item(next).is_some() || table_line(next.trim());
        let all = sentences(&joined);
        let last = all.len().saturating_sub(1);
        for (k, s) in all.iter().enumerate() {
            let lead_in = k == last && leads && strip_markers(s).trim_end().ends_with(':');
            if lead_in && fixed_lead_in(s) {
                continue;
            }
            if lead_in && markers(s).is_empty() {
                p.findings.push(format!("{}: {}", LEAD_IN_FACT, super::page_check::short(s)));
                let n = p.units.len() + 1;
                p.units.push(Unit { n, kind: UnitKind::Sentence, text: s.clone(), cites: Vec::new(), section });
                continue;
            }
            push(&mut p, UnitKind::Sentence, s, section);
        }
    }
    p
}

/// The rule text of `specs/144` §2.3 item 3, word for word in the writer's brief and the validator's contract.
pub const RULE_TEXT: &str = "Joining two or more records into a conclusion that none of them states is not support. Neither is a stronger wording than the record's (\"may\" made \"does\", \"some\" made \"all\"), a part made the whole, or a condition the record states left out.";

/// The words that qualify a statement (§2.3 item 4): a negation, a restriction, an exception or a condition. A number qualifies too.
pub const QUALIFIERS: [&str; 14] = ["not", "no", "never", "none", "nor", "only", "unless", "except", "without", "if", "when", "provided", "cannot", "neither"];

/// One part of a record's **source text** as the validator is shown it (`specs/144` §2.3 item 1): the passage shown, and the stored
/// capture it was cut from, which a pinpoint is resolved in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePart {
    /// The source the text is from: `s-<id>`, or a passage's own label.
    pub from: String,
    pub shown: String,
    pub capture: String,
}

/// Text for comparing a passage with a capture: emphasis and code marks gone, a line's heading or list mark gone, spaces collapsed.
fn plain_text(s: &str) -> String {
    let t: String = s.chars().filter(|c| !matches!(c, '*' | '`' | '_')).collect();
    norm(t.trim_start_matches(['#', '-', '>', ' ']).trim().trim_matches(['"', '“', '”', '\'']))
}

/// A capture's paragraphs, each as its sentences (a line is a sentence boundary too), in plain text.
fn capture_sentences(capture: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for line in capture.lines() {
        if line.trim().is_empty() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        for s in sentences(&plain_text(line)) {
            cur.push(s);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The qualifiers of a sentence: the [`QUALIFIERS`] words in it, words ending `n't`, and its numbers.
fn qualifiers(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’' || c == '.' || c == ','))
        .map(|w| w.trim_matches(['.', ',', '\'', '’']).to_lowercase())
        .filter(|w| !w.is_empty() && (QUALIFIERS.contains(&w.as_str()) || w.ends_with("n't") || w.ends_with("n’t") || w.chars().any(|c| c.is_ascii_digit())))
        .collect()
}

/// Why a pinpoint passage does not count (`specs/144` §2.3 item 4), or `Ok` with the passage's place in its capture as (part, paragraph,
/// first sentence, sentences). `parts` are the source text of the records the unit cites, `other` the claim, finding and summary text
/// shown with them, `statement` the unit's words.
pub fn pinpoint(passage: &str, parts: &[SourcePart], other: &[&str], statement: &str) -> Result<(usize, usize, usize, usize), &'static str> {
    let want = plain_text(passage);
    if want.is_empty() || want == "-" {
        return Err("no passage was given");
    }
    let Some(pi) = parts.iter().position(|p| plain_text(&p.shown.replace('\n', " ")).contains(&want) || capture_sentences(&p.shown).iter().any(|para| para.join(" ").contains(&want))) else {
        return Err(if other.iter().any(|o| plain_text(&o.replace('\n', " ")).contains(&want)) { "the passage is claim, finding or summary text, not source text" } else { "the passage is not in the cited record's source text" });
    };
    // Resolved in the stored capture, not only in what was shown.
    let paras = capture_sentences(&parts[pi].capture);
    let mut contained = false;
    for (k, para) in paras.iter().enumerate() {
        for i in 0..para.len() {
            let mut joined = String::new();
            for j in i..para.len() {
                if !joined.is_empty() {
                    joined.push(' ');
                }
                joined.push_str(&para[j]);
                if joined == want {
                    let said: Vec<String> = qualifiers(statement);
                    let dropped = |s: &String| qualifiers(s).iter().any(|q| !said.contains(q));
                    if (i > 0 && dropped(&para[i - 1])) || para.get(j + 1).is_some_and(dropped) {
                        return Err("it leaves out an adjacent sentence whose qualifier the statement drops");
                    }
                    return Ok((pi, k, i, j + 1 - i));
                }
                if joined.len() > want.len() {
                    break;
                }
            }
        }
        if para.join(" ").contains(&want) {
            contained = true;
        }
    }
    Err(if contained { "the passage is not whole sentences" } else { "the passage is not in the stored capture" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(text: &str) -> SourcePart {
        SourcePart { from: "s-1".into(), shown: text.into(), capture: text.into() }
    }

    /// AC-8: a pinpoint counts when it is whole sentences of the shown source text, found in the stored capture, with any adjacent
    /// sentence whose qualifier the statement drops.
    #[test]
    fn a_pinpoint_is_whole_sentences_of_source_text_with_the_qualifiers_it_needs() {
        let src = "# Tides\n\nThe tide turns twice a day. It does not turn at the neap if the wind holds. Spring tides are higher.\n\nA second paragraph says the lag is one hour.";
        let p = [part(src)];
        assert_eq!(pinpoint("A second paragraph says the lag is one hour.", &p, &[], "The lag is one hour"), Ok((0, 2, 0, 1)));
        // The sentence before carries a "not" and an "if" the statement drops.
        assert!(pinpoint("Spring tides are higher.", &p, &[], "Spring tides are higher").is_err());
        assert!(pinpoint("It does not turn at the neap if the wind holds. Spring tides are higher.", &p, &[], "Spring tides are higher; the tide does not turn at the neap if the wind holds").is_ok());
        assert_eq!(pinpoint("tides are higher", &p, &[], "x"), Err("the passage is not whole sentences"));
        assert_eq!(pinpoint("The moon is made of cheese.", &p, &[], "x"), Err("the passage is not in the cited record's source text"));
        assert_eq!(pinpoint("The tide turns twice a day.", &p, &["The tide turns twice a day."], "x"), Err("it leaves out an adjacent sentence whose qualifier the statement drops"));
        // The cross-sentence fixture: the next sentence says "not ... if"; a statement that drops it fails, the passage that keeps it passes.
        assert_eq!(pinpoint("The tide turns twice a day.", &p, &[], "The tide turns twice a day, always"), Err("it leaves out an adjacent sentence whose qualifier the statement drops"));
        assert!(pinpoint("The tide turns twice a day. It does not turn at the neap if the wind holds.", &p, &[], "The tide turns twice a day, but not at the neap if the wind holds").is_ok());
        // Claim text is not source text.
        assert_eq!(pinpoint("Tides are made by the moon.", &p, &["Tides are made by the moon."], "x"), Err("the passage is claim, finding or summary text, not source text"));
        // Shown, but not in the stored capture (a shown excerpt that is not what was captured).
        let changed = [SourcePart { from: "s-1".into(), shown: "Spring tides are higher.".into(), capture: "Neap tides are lower.".into() }];
        assert_eq!(pinpoint("Spring tides are higher.", &changed, &[], "x"), Err("the passage is not in the stored capture"));
        assert_eq!(pinpoint("-", &p, &[], "x"), Err("no passage was given"));
    }

    #[test]
    fn sentences_end_at_a_stop_and_keep_the_markers_that_follow() {
        assert_eq!(sentences("Tides lag. [s-kb-0000000000000001] The lag is an hour [c-1]. It grows in spring."), vec!["Tides lag. [s-kb-0000000000000001]", "The lag is an hour [c-1].", "It grows in spring."]);
        assert_eq!(sentences("See e.g. the gauge at St. Ives. It is 1.5 m deep [c-2]."), vec!["See e.g. the gauge at St. Ives.", "It is 1.5 m deep [c-2]."]);
        assert_eq!(sentences("J. Smith measured it. \"It lags,\" he said."), vec!["J. Smith measured it.", "\"It lags,\" he said."]);
    }

    /// AC-5: every sentence, item, row, caption and key-term line needs a marker of its own; a marker covers only what it ends; headings
    /// need none; a lead-in on the fixed list needs none, any other does; code-written sections are not units.
    #[test]
    fn every_unit_but_a_heading_needs_its_own_marker() {
        let body = "# Tides\n\n**Tides** lag in a shallow estuary [c-1]. The lag grows in spring.\n\n## Parts\n\nThe main parts are:\n\n- The channel [c-2]\n- The bar\n\n| Place | Lag |\n| :-- | :-- |\n| Harbour | 1 h [c-1] |\n| Quay | 2 h |\n\n![Chart](lib:s-1/chart.png)\n*Figure 1: The lag by month [c-1].*\n\n## Key terms\n\n- **Bar**: a ridge of sand [c-2]\n- **Lag**: the delay\n\n## See also\n\n- [Waves](page:waves)\n";
        let p = parse(body);
        let kinds: Vec<UnitKind> = p.units.iter().map(|u| u.kind).collect();
        use UnitKind::*;
        assert_eq!(kinds, vec![Heading, Sentence, Sentence, Heading, Item, Item, Row, Row, Caption, Heading, KeyTerm, KeyTerm], "{:#?}", p.units);
        let missing: Vec<&String> = p.findings.iter().collect();
        assert_eq!(missing.len(), 4, "{missing:?}");
        for want in ["The lag grows in spring.", "The bar", "| Quay | 2 h |", "**Lag**: the delay"] {
            assert!(p.findings.iter().any(|f| f.starts_with(NO_CITATION) && f.contains(want)), "{want}: {:?}", p.findings);
        }
        assert_eq!(p.units[2].cites, Vec::<String>::new(), "the marker of the first sentence does not cover the second");
        assert_eq!(p.units[4].section, 4, "an item sits under its heading");
        // A lead-in that states a fact needs a marker; the same with one passes.
        let lead = parse("# T\n\nBenchmarks show X is twice as fast as Y:\n\n- A [c-1]\n");
        assert!(lead.findings.iter().any(|f| f.starts_with(LEAD_IN_FACT)), "{:?}", lead.findings);
        assert!(parse("# T\n\nBenchmarks show X is twice as fast as Y [c-2]:\n\n- A [c-1]\n").findings.is_empty());
    }

    /// AC-18: raw HTML, a descriptive image text, a footnote and a fenced block cannot be read.
    #[test]
    fn a_construct_the_parse_cannot_place_fails() {
        for body in ["# T\n\n<div>Tides lag.</div> [c-1]\n", "# T\n\n![The chart shows that the lag doubled after the dredging.](lib:s-1/c.png)\n*Figure 1: Lag [c-1].*\n", "# T\n\nTides lag.[^1] [c-1]\n\n[^1]: Since 1900.\n", "# T\n\n```\nlag = 1h\n```\n"] {
            let p = parse(body);
            assert!(p.findings.iter().any(|f| f.starts_with(UNREADABLE)), "{body}: {:?}", p.findings);
        }
        assert!(parse("# T\n\n![Chart](lib:s-1/c.png)\n*Figure 1: Lag [c-1].*\n").findings.is_empty());
    }

    /// Code writes the hatnote, `Contents` and `See also`; a draft's own copies are taken out first.
    #[test]
    fn code_writes_the_hatnote_contents_and_see_also() {
        let draft = "# Tides\n\n> *For the music, see [Tides](page:tides-music).*\n\nLead [c-1].\n\n## Contents\n\n1. [Wrong](#wrong)\n\n## Cause\n\nA [c-1].\n\n## Effect\n\nB [c-1].\n\n## See also\n\n- [Made up](page:made-up)\n";
        let stripped = strip_code_written(draft);
        assert!(!stripped.contains("Wrong") && !stripped.contains("made-up") && !stripped.contains("> *"), "{stripped}");
        let page = code_written(&stripped, Some("This page is about Tides. For Tides (music), see [Tides (music)](page:tides-music)."), &[("waves".into(), "Waves".into())]);
        assert!(page.starts_with("# Tides\n\n> *This page is about Tides."), "{page}");
        assert!(page.contains("## Contents\n\n1. [Cause](#cause)\n2. [Effect](#effect)\n\n## Cause"), "{page}");
        assert!(page.trim_end().ends_with("## See also\n\n- [Waves](page:waves)"), "{page}");
        // The written parts are not units.
        assert_eq!(parse(&page).units, parse(&stripped).units);
    }
}
