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

#[cfg(test)]
mod tests {
    use super::*;

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
