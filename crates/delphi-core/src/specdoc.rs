//! One parser for the text of spec documents (`specs/088` §2): the numbered headings a section reference points at, the section
//! references themselves, the amendment tags, and the number in a spec's file name.
//!
//! Everything that needs one of these reads it here: the reference gate (`crates/delphi/tests/spec_refs.rs`), `delphi lint`'s
//! spec-number check, and the spec compile of spec 088 when it is built. `spec_refs` has a test that fails when a second pattern for
//! any of them appears outside this file. Two parsers that disagree about what a heading or an ID is was the failure behind
//! OpenSpec's silent deletions; one parser cannot disagree with itself.
//!
//! The patterns are exactly the ones `spec_refs` used before they moved here, so moving them changed no result.

use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

/// A numbered heading: two to five `#`, white space, an optional `§`, a number such as `3` or `3.3`, an optional trailing dot, then
/// white space or the end of the line.
static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^#{2,5}\s+§?([0-9]+(?:\.[0-9]+)*)\.?(?:\s|$)").unwrap());

/// A section reference: a spec number (three digits and an optional letter), optionally followed by the rest of a file name, then
/// `§` or `§§` and a section number.
static SECTION_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)([0-9]{3}[a-z]?)(?:-[a-z0-9][a-z0-9-]*(?:\.md)?)?\s+§§?\s*([0-9]+(?:\.[0-9]+)*)").unwrap());

/// An amendment tag: a spec number, `-A`, and a number, standing alone.
static AMENDMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?-u:\b)([0-9]{3}[a-z]?)-A([0-9]+)(?-u:\b)").unwrap());

/// The name of a spec file: its number, a dash, a slug, `.md`. No directory part.
static FILE_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([0-9]{3}[a-z]?)-[^/]+\.md$").unwrap());

/// One section reference found in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionRef {
    /// The spec number cited, such as `004` or `005a`.
    pub spec: String,
    /// The section number cited, such as `3.3.2`.
    pub section: String,
}

/// One amendment tag found in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmendmentTag {
    /// The spec number the tag belongs to.
    pub spec: String,
    /// The whole tag as written, such as `004-A1`.
    pub tag: String,
}

/// The numbers of the headings in `text` that a section reference can point at.
pub fn heading_numbers(text: &str) -> BTreeSet<String> {
    HEADING.captures_iter(text).map(|c| c[1].to_string()).collect()
}

/// The heading a section reference points at. `§3.3.2` names item 2 of the numbered list under heading `3.3`, so a reference
/// with three or more parts resolves to the heading above its last part; one or two parts name a heading directly.
pub fn heading_of(section: &str) -> &str {
    match section.matches('.').count() {
        0 | 1 => section,
        _ => section.rsplit_once('.').unwrap().0,
    }
}

/// Every section reference in `text`, in the order written.
pub fn section_refs(text: &str) -> Vec<SectionRef> {
    if !text.contains('§') {
        return Vec::new();
    }
    SECTION_REF.captures_iter(text).map(|c| SectionRef { spec: c[1].to_string(), section: c[2].to_string() }).collect()
}

/// Every amendment tag in `text`, in the order written.
pub fn amendment_tags(text: &str) -> Vec<AmendmentTag> {
    if !text.contains("-A") {
        return Vec::new();
    }
    AMENDMENT.captures_iter(text).map(|c| AmendmentTag { spec: c[1].to_string(), tag: c[0].to_string() }).collect()
}

/// The spec number in a spec file's name (`005a-terminal-avatar-system.md` gives `005a`), or `None` for any other name. The name
/// has no directory part: a path with a `/` is not a spec file name.
pub fn spec_number(file_name: &str) -> Option<String> {
    FILE_NUMBER.captures(file_name).map(|c| c[1].to_string())
}

// ── extraction for the spec compile (specs/088 §2, §4) ───────────────────────────────────────────────────────────────
//
// What the compile copies out of a spec, byte for byte. Nothing here rewrites or summarizes: each result is a slice of the
// source with the place it came from, so a test can cut the same slice out of the file and compare. Lines are counted on `\n`,
// so a line that ends `\r\n` keeps its `\r`; a heading starts at the left margin; a fenced block (three or more backticks or
// tildes, closed by the same character) is not searched for headings; the frontmatter (a first line `---` up to the next line
// `---`) is not either, because a `#` there starts a YAML comment.

/// A piece of a source file, copied verbatim, with where it came from: byte offsets (`start..end`, so `text` is exactly
/// `source[start..end]`) and 1-based line numbers (`line` to `end_line`, both inclusive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub end_line: usize,
}

/// One heading of a spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    /// 1 to 6: the number of `#`.
    pub level: u8,
    /// The section number the heading starts with (`3.3` in `### 3.3 Name`), if it has one.
    pub number: Option<String>,
    /// The heading text after the marks and the number, as written.
    pub title: String,
    /// The 1-based line of the heading.
    pub line: usize,
}

struct SrcLine<'a> {
    text: &'a str,
    start: usize,
    no: usize,
}

fn split_lines(text: &str) -> Vec<SrcLine<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, raw) in text.split('\n').enumerate() {
        out.push(SrcLine { text: raw, start, no: i + 1 });
        start += raw.len() + 1;
    }
    out
}

/// The frontmatter's lines as (first inner line index, closing `---` line index), or `None` when the file has none.
fn frontmatter_lines(lines: &[SrcLine<'_>]) -> Option<(usize, usize)> {
    if lines.first().map(|l| l.text.trim_end_matches('\r')) != Some("---") {
        return None;
    }
    (1..lines.len()).find(|&i| lines[i].text.trim_end_matches('\r') == "---").map(|close| (1, close))
}

/// The first line after the frontmatter (0 when there is none).
fn body_from(lines: &[SrcLine<'_>]) -> usize {
    frontmatter_lines(lines).map(|(_, close)| close + 1).unwrap_or(0)
}

/// The fence a line opens or closes: its character and how many were written. A backtick fence's info string may not contain a
/// backtick, so a line like three backticks, a word and three backticks is text.
fn fence_marker(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start_matches(' ').trim_end_matches('\r');
    let c = t.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = t.chars().take_while(|x| *x == c).count();
    if n < 3 || (c == '`' && t[n..].contains('`')) {
        return None;
    }
    Some((c, n))
}

/// For each line from `from`: whether it is inside a fenced block or is one of its two marker lines.
fn fenced_flags(lines: &[SrcLine<'_>], from: usize) -> Vec<bool> {
    let mut flags = vec![false; lines.len()];
    let mut open: Option<(char, usize)> = None;
    for i in from..lines.len() {
        let marker = fence_marker(lines[i].text);
        match (open, marker) {
            (None, Some(m)) => {
                open = Some(m);
                flags[i] = true;
            }
            (Some((c, n)), Some((c2, n2))) if c2 == c && n2 >= n && lines[i].text.trim().chars().all(|x| x == c) => {
                open = None;
                flags[i] = true;
            }
            (Some(_), _) => flags[i] = true,
            (None, None) => {}
        }
    }
    flags
}

/// An ATX heading: one to six `#` at the left margin, a space or tab, then text.
fn atx_heading(line: &str) -> Option<(u8, &str)> {
    let t = line.trim_end_matches('\r');
    let n = t.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&n) {
        return None;
    }
    let rest = &t[n..];
    if !(rest.starts_with(' ') || rest.starts_with('\t')) || rest.trim().is_empty() {
        return None;
    }
    Some((n as u8, rest.trim()))
}

/// Splits a leading section number off a heading's text: `3.3 Name` gives (`3.3`, `Name`); `§4 Name` gives (`4`, `Name`); `5.1.`
/// gives (`5.1`, ``). A number must be followed by white space or the end, so `2026-11 plan` has none.
fn split_number(title: &str) -> (Option<String>, &str) {
    let s = title.strip_prefix('§').unwrap_or(title);
    let b = s.as_bytes();
    let mut i = 0;
    let mut groups = 0;
    loop {
        let st = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == st {
            break;
        }
        groups += 1;
        if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
            i += 1;
            continue;
        }
        break;
    }
    if groups == 0 {
        return (None, title);
    }
    let number = &s[..i];
    let mut j = i;
    if j < b.len() && b[j] == b'.' {
        j += 1;
    }
    if j < b.len() && !(b[j] == b' ' || b[j] == b'\t') {
        return (None, title);
    }
    (Some(number.to_string()), s[j..].trim())
}

fn quote_of(text: &str, lines: &[SrcLine<'_>], first: usize, last: usize) -> Quote {
    let start = lines[first].start;
    let end = lines[last].start + lines[last].text.len();
    Quote { text: text[start..end].to_string(), start, end, line: lines[first].no, end_line: lines[last].no }
}

/// Every heading of the body, in order: levels 1 to 6, outside fenced blocks and the frontmatter.
pub fn outline(text: &str) -> Vec<OutlineEntry> {
    let lines = split_lines(text);
    let from = body_from(&lines);
    let fenced = fenced_flags(&lines, from);
    let mut out = Vec::new();
    for i in from..lines.len() {
        if fenced[i] {
            continue;
        }
        if let Some((level, rest)) = atx_heading(lines[i].text) {
            let (number, title) = split_number(rest);
            out.push(OutlineEntry { level, number, title: title.to_string(), line: lines[i].no });
        }
    }
    out
}

/// The summary of a spec: the first block under its first level-2 heading that has "scope" in it (any case), where a block is the
/// run of lines up to the next blank line or heading. A block may be a paragraph, a blockquote, a list or a table. It is `None`
/// when the spec has no such heading, when the heading is followed at once by another heading, a fenced block or the end of the
/// file, or when the only "scope" heading is at another level or inside a fenced block. About half the specs have no such
/// heading; that is a fact about the sources and not an error.
pub fn scope_summary(text: &str) -> Option<Quote> {
    let lines = split_lines(text);
    let from = body_from(&lines);
    let fenced = fenced_flags(&lines, from);
    let head = (from..lines.len()).find(|&i| !fenced[i] && matches!(atx_heading(lines[i].text), Some((2, t)) if t.to_ascii_lowercase().contains("scope")))?;
    let first = (head + 1..lines.len()).find(|&i| !lines[i].text.trim().is_empty())?;
    if fenced[first] || atx_heading(lines[first].text).is_some() {
        return None;
    }
    let mut last = first;
    while last + 1 < lines.len() && !lines[last + 1].text.trim().is_empty() && !fenced[last + 1] && atx_heading(lines[last + 1].text).is_none() {
        last += 1;
    }
    Some(quote_of(text, &lines, first, last))
}

/// The frontmatter line that starts with `key:`, whole and verbatim (a trailing `# comment` included), or `None`.
pub fn frontmatter_line(text: &str, key: &str) -> Option<Quote> {
    let lines = split_lines(text);
    let (first, close) = frontmatter_lines(&lines)?;
    let prefix = format!("{}:", key);
    (first..close).find(|&i| lines[i].text.starts_with(&prefix)).map(|i| quote_of(text, &lines, i, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One row per case: the grammar of the summary (spec 088 §8, test 14). The third column is what the summary must be.
    #[test]
    fn the_summary_is_the_first_block_under_a_level_two_scope_heading() {
        let cases: &[(&str, &str, Option<&str>)] = &[
            ("a paragraph", "## 1. Scope and objectives\n\nFirst line.\nSecond line.\n\nOther.\n", Some("First line.\nSecond line.")),
            ("no blank line after the heading", "## Scope\nOnly.\n", Some("Only.")),
            ("blank lines between the heading and the block", "## Scope\n\n\n\nLate.\n", Some("Late.")),
            ("a blockquote", "## Scope\n\n> **Status:** draft\n> more\n\nafter\n", Some("> **Status:** draft\n> more")),
            ("a list", "## Scope\n\n- one\n- two\n\nafter\n", Some("- one\n- two")),
            ("a table", "## Scope\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\nafter\n", Some("| a | b |\n| - | - |\n| 1 | 2 |")),
            ("the block stops at the next heading", "## Scope\n\ntext\n### Next\n", Some("text")),
            ("a heading straight after", "## Scope\n## Next\n\ntext\n", None),
            ("a fence straight after", "## Scope\n\n```\ncode\n```\n", None),
            ("end of file after the heading", "## Scope\n", None),
            ("a level three scope heading", "### Scope\n\ntext\n", None),
            ("a scope heading inside a fence", "```\n## Scope\n\ntext\n```\n", None),
            ("a scope heading inside a tilde fence", "~~~~\n## Scope\n\ntext\n~~~~\n", None),
            ("a scope heading only in the frontmatter", "---\n# ## Scope\nstatus: draft\n---\n\n## Other\n\ntext\n", None),
            ("a later scope heading is not the first", "## Intro\n\ntext\n\n## Scope and Non-Goals\n\nreal\n", Some("real")),
            ("capitals", "## SCOPE\n\nloud\n", Some("loud")),
            // Lines are counted on `\n`, so each line keeps its `\r`, the last one of the quote too.
            ("carriage returns are kept", "## Scope\r\n\r\nline one\r\nline two\r\n\r\nafter\r\n", Some("line one\r\nline two\r")),
            ("no scope heading", "## Goal\n\ntext\n", None),
            ("a fence that closes, then a real scope heading", "```\ncode\n```\n\n## Scope\n\nreal\n", Some("real")),
            ("three backticks in a line of text are not a fence", "## Scope\n\n```text``` inline\n\nafter\n", Some("```text``` inline")),
        ];
        for (name, src, want) in cases {
            let got = scope_summary(src);
            assert_eq!(got.as_ref().map(|q| q.text.as_str()), *want, "{}", name);
            if let Some(q) = got {
                // The quote is exactly the slice of the source at its recorded span and lines.
                assert_eq!(&src[q.start..q.end], q.text, "{}: span", name);
                assert_eq!(src[..q.start].matches('\n').count() + 1, q.line, "{}: first line", name);
                assert_eq!(q.line + q.text.matches('\n').count(), q.end_line, "{}: last line", name);
            }
        }
    }

    #[test]
    fn spans_are_byte_offsets_so_multibyte_text_does_not_shift_them() {
        let src = "---\nid: x\nstatus: approved   # by the operator, 2026-11-07 (naïve café — ok)\n---\n\n## Scope\n\nnaïve café — ok. 日本語.\n\nafter\n";
        let q = scope_summary(src).unwrap();
        assert_eq!(q.text, "naïve café — ok. 日本語.");
        assert_eq!(&src[q.start..q.end], q.text);
        let s = frontmatter_line(src, "status").unwrap();
        assert_eq!(s.text, "status: approved   # by the operator, 2026-11-07 (naïve café — ok)");
        assert_eq!(&src[s.start..s.end], s.text);
        assert_eq!((s.line, s.end_line), (3, 3));
    }

    #[test]
    fn the_outline_lists_headings_of_all_levels_outside_fences_and_frontmatter() {
        let src = "---\nid: x\n# a yaml comment\n---\n\n# Title\n\n## 1. Scope and objectives\n```\n## Not a heading\n```\n### 3.3.2 Deep\n## §4 Marked\n#### 5.1.\n###### 6 tiny\n####### seven is text\n##No space\n## 2026-11 plan\n";
        let rows: Vec<(u8, Option<String>, String, usize)> = outline(src).into_iter().map(|e| (e.level, e.number, e.title, e.line)).collect();
        let want: Vec<(u8, Option<String>, String, usize)> = vec![
            (1, None, "Title".into(), 6),
            (2, Some("1".into()), "Scope and objectives".into(), 8),
            (3, Some("3.3.2".into()), "Deep".into(), 12),
            (2, Some("4".into()), "Marked".into(), 13),
            (4, Some("5.1".into()), "".into(), 14),
            (6, Some("6".into()), "tiny".into(), 15),
            (2, None, "2026-11 plan".into(), 18),
        ];
        assert_eq!(rows, want);
        assert!(outline("no headings at all\n").is_empty());
    }

    #[test]
    fn a_frontmatter_line_is_returned_whole_and_only_from_the_frontmatter() {
        let src = "---\nid: x\nstatus: approved   # by the operator\ntitle: \"T\"\n---\n\nstatus: not this one\n";
        let q = frontmatter_line(src, "status").unwrap();
        assert_eq!((q.text.as_str(), q.line), ("status: approved   # by the operator", 3));
        assert!(frontmatter_line(src, "missing").is_none());
        assert!(frontmatter_line("status: approved\n", "status").is_none(), "no frontmatter");
        assert!(frontmatter_line("---\nid: x\nno closing line\n", "id").is_none(), "an unclosed frontmatter is none");
        assert!(frontmatter_line("---\nxstatus: a\n---\n", "status").is_none(), "a longer key is not the key");
    }

    #[test]
    fn headings_are_numbered_at_levels_two_to_five() {
        let text = "# Title\n\n## 1. Scope\n### 3.3.2 Deep\n## §4 Marked\n#### 5.1.\n###### 6 too deep\n## Appendix A\ntext 7 not a heading\n";
        let got: Vec<String> = heading_numbers(text).into_iter().collect();
        assert_eq!(got, vec!["1", "3.3.2", "4", "5.1"]);
    }

    #[test]
    fn a_reference_with_three_parts_names_the_heading_above_its_last_part() {
        assert_eq!(heading_of("3"), "3");
        assert_eq!(heading_of("3.3"), "3.3");
        assert_eq!(heading_of("3.3.2"), "3.3");
        assert_eq!(heading_of("10.1.4.2"), "10.1.4");
    }

    #[test]
    fn section_references_are_found_by_number_file_name_and_double_mark() {
        let text = "see specs/001 §7 and 005a §1, also 004-memory-and-coaching.md §2.1 and 011 §§4.2 (not §9 alone, not 12 §3, not 0011 §3)";
        let got: Vec<(String, String)> = section_refs(text).into_iter().map(|r| (r.spec, r.section)).collect();
        let want: Vec<(String, String)> = [("001", "7"), ("005a", "1"), ("004", "2.1"), ("011", "4.2")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        assert_eq!(got, want);
        assert!(section_refs("no mark here: 001 7").is_empty());
    }

    #[test]
    fn amendment_tags_stand_alone() {
        let text = "004-A1 and 006-A12, but not 004-B1, 1004-A1 or 004-A1x; also 005a-A2.";
        let got: Vec<String> = amendment_tags(text).into_iter().map(|t| t.tag).collect();
        assert_eq!(got, vec!["004-A1", "006-A12", "005a-A2"]);
        assert_eq!(amendment_tags("004-A3")[0].spec, "004");
        assert!(amendment_tags("nothing to find").is_empty());
    }

    #[test]
    fn a_spec_file_name_gives_its_number() {
        assert_eq!(spec_number("005a-terminal-avatar-system.md").as_deref(), Some("005a"));
        assert_eq!(spec_number("014-relay-and-away-mode.md").as_deref(), Some("014"));
        for not in ["ui-wireframes-and-blueprints.md", "005.md", "0045-x.md", "123-x.txt", "specs/014-x.md", "history/005-x.md", "05-x.md"] {
            assert_eq!(spec_number(not), None, "{}", not);
        }
    }
}
