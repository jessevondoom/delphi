//! The spec tracker, read (spec 135 §4.1, §5.3). Pure: text in, rows out. The tracker is free prose kept by builders, so this
//! reads only what every slice row shares (the spec, the slice and the last cell, its state) and never writes it.

use std::path::{Path, PathBuf};

/// Where the tracker lives in a checkout.
pub const PATH: &str = "missions/2026-10-23-foundational-design/spec-tracker.md";

pub fn path(repo: &Path) -> PathBuf {
    repo.join(PATH)
}

/// One slice row: `| <spec> | <slice> | ... | <state> |`, or `| <slice> | ... | <state> |` inside a spec's section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub spec: String,
    pub slice: String,
    /// The middle cells, joined (the content of the slice).
    pub content: String,
    pub state: String,
    /// 1-based line in the tracker.
    pub line: usize,
    /// For a row that starts with its slice: the heading whose section gave it its spec.
    pub section: Option<String>,
}

/// The cells of a table line, split on `|` not escaped with `\`. A pipe inside backticks that was not escaped splits a cell too;
/// that only moves text between middle cells, and only the first two and the last are read.
fn cells(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    let inner = t.strip_prefix('|')?.strip_suffix('|')?;
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '|' => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    Some(out)
}

/// A spec cell: a spec number such as `135`, `005g` or `028-A1`.
fn is_spec_cell(s: &str) -> bool {
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    digits == 3 && s.len() <= 10 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A heading of the tracker and the spec numbers it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub level: usize,
    pub title: String,
    /// 1-based line of the heading.
    pub line: usize,
    /// The numbers it names: `094`, `005g`; an amendment `028-A1` names `028`; a range `104 to 110` or `078-081` names each
    /// number in it. Empty when the heading does not say `spec` or `specs` (`Council 013: ...` names no spec).
    pub specs: Vec<String>,
}

/// A markdown heading: its level and title.
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|&c| c == b'#').count();
    if level == 0 || level > 6 || !line[level..].starts_with(' ') {
        return None;
    }
    Some((level, line[level..].trim()))
}

/// The spec numbers a heading names, sorted, without duplicates.
fn heading_specs(title: &str) -> Vec<String> {
    if !title.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w.eq_ignore_ascii_case("spec") || w.eq_ignore_ascii_case("specs")) {
        return Vec::new();
    }
    // Tokens: exactly three digits, an optional lower-case letter, not inside a longer word or number (a date's `2026`, a
    // path's `/013`, a pull request's `#273`). With the byte range of each.
    let b = title.as_bytes();
    let mut tokens: Vec<(String, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let mut end = i;
        if i - start == 3 && end < b.len() && b[end].is_ascii_lowercase() {
            end += 1;
        }
        let before_ok = start == 0 || !(b[start - 1].is_ascii_alphanumeric() || b[start - 1] == b'#' || b[start - 1] == b'/');
        let after_ok = end == b.len() || !b[end].is_ascii_alphanumeric();
        if i - start == 3 && before_ok && after_ok {
            tokens.push((title[start..end].to_string(), start, end));
        }
        i = end.max(i);
    }
    let mut out: Vec<String> = tokens.iter().map(|t| t.0.clone()).collect();
    for w in tokens.windows(2) {
        let between = title[w[0].2..w[1].1].trim();
        if matches!(between, "to" | "-" | "\u{2013}") {
            if let (Ok(a), Ok(z)) = (w[0].0.parse::<u32>(), w[1].0.parse::<u32>()) {
                if a < z && z - a <= 30 {
                    out.extend((a + 1..z).map(|n| format!("{:03}", n)));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Every heading of the tracker.
pub fn sections(text: &str) -> Vec<Section> {
    text.lines().enumerate().filter_map(|(i, l)| heading(l).map(|(level, title)| Section { level, title: title.to_string(), line: i + 1, specs: heading_specs(title) })).collect()
}

/// The headings that name a spec number (`094`): where its slice-first rows are looked for.
pub fn sections_for(text: &str, spec_number: &str) -> Vec<Section> {
    sections(text).into_iter().filter(|s| s.specs.iter().any(|n| n.eq_ignore_ascii_case(spec_number))).collect()
}

/// A table's separator line (`| :-- | --: |`).
fn is_separator(c: &[String]) -> bool {
    c.iter().all(|x| !x.is_empty() && x.chars().all(|ch| matches!(ch, ':' | '-')))
}

/// A slice cell without bold or backticks around it (`**S11**`, `` `S11` ``).
fn bare(s: &str) -> String {
    s.trim().trim_matches(|c| c == '*' || c == '`').trim().to_string()
}

/// Every slice row of the tracker, in either of its two shapes:
/// - `| <spec> | <slice> | ... | <state> |`, anywhere: at least four cells, the first a spec number. The older summary table
///   (`| 005c Inline console | yes | ...`) has a title in its first cell, so it is not read this way.
/// - `| <slice> | ... | <state> |`, at least three cells, inside the section of a heading that names the spec. The nearest
///   heading above the row that names any spec decides; a section ends at the next heading of the same or a higher level. A
///   heading that names several specs gives the row to each.
///
/// A table's header line (the line above its separator) and the separator are not rows.
pub fn rows(text: &str) -> Vec<Row> {
    let lines: Vec<&str> = text.lines().collect();
    let parsed: Vec<Option<Vec<String>>> = lines.iter().map(|l| cells(l)).collect();
    let mut stack: Vec<Section> = Vec::new();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some((level, title)) = heading(l) {
            while stack.last().is_some_and(|s| s.level >= level) {
                stack.pop();
            }
            stack.push(Section { level, title: title.to_string(), line: i + 1, specs: heading_specs(title) });
            continue;
        }
        let Some(c) = &parsed[i] else { continue };
        let header = parsed.get(i + 1).and_then(|n| n.as_deref()).is_some_and(is_separator);
        if is_separator(c) || header {
            continue;
        }
        if is_spec_cell(&c[0]) {
            if c.len() < 4 || c[1].is_empty() || c[1].starts_with(':') {
                continue;
            }
            out.push(Row { spec: c[0].clone(), slice: bare(&c[1]), content: c[2..c.len() - 1].join(" | "), state: c[c.len() - 1].clone(), line: i + 1, section: None });
            continue;
        }
        let slice = bare(&c[0]);
        if c.len() < 3 || slice.is_empty() {
            continue;
        }
        let Some(sec) = stack.iter().rev().find(|s| !s.specs.is_empty()) else { continue };
        for n in &sec.specs {
            out.push(Row { spec: n.clone(), slice: slice.clone(), content: c[1..c.len() - 1].join(" | "), state: c[c.len() - 1].clone(), line: i + 1, section: Some(sec.title.clone()) });
        }
    }
    out
}

/// The rows of one spec, by its number (`135`).
pub fn rows_for<'a>(rows: &'a [Row], spec_number: &str) -> Vec<&'a Row> {
    rows.iter().filter(|r| r.spec.eq_ignore_ascii_case(spec_number)).collect()
}

/// The row of one slice of one spec. The slice id matches whole, ignoring case: `S1` is not `S16`.
pub fn find<'a>(rows: &'a [Row], spec_number: &str, slice: &str) -> Option<&'a Row> {
    rows.iter().find(|r| r.spec.eq_ignore_ascii_case(spec_number) && r.slice.eq_ignore_ascii_case(slice.trim()))
}

/// The number a spec stem carries (`135-the-forge...` -> `135`), as the tracker writes it.
pub fn spec_number(stem: &str) -> Option<String> {
    delphi_core::specdoc::spec_number(&format!("{}.md", stem))
}

/// Does a state say the row was not updated for finished work (spec 135 §5.3)?
pub fn state_unfinished(state: &str) -> bool {
    let s = state.trim().to_lowercase();
    ["not built", "not started", "approved in advance"].iter().any(|p| s.starts_with(p))
}

/// Does a state say the slice is already merged (a warning when it is claimed, §4.1)?
pub fn state_merged(state: &str) -> bool {
    let s = state.trim().to_lowercase();
    s.starts_with("built and merged") || s.starts_with("merged")
}

/// The `status:` of a spec's frontmatter, without its comment (`approved`, `draft`, ...).
pub fn spec_status(spec_text: &str) -> Option<String> {
    let mut lines = spec_text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for l in lines {
        if l.trim() == "---" {
            return None;
        }
        if let Some(v) = l.strip_prefix("status:") {
            let v = v.split('#').next().unwrap_or("").trim().trim_matches('"').to_string();
            return Some(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Spec tracker\n\n| Spec | Approved | Built | Proven | What is left |\n| :-- | :-- | :-- | :-- | :-- |\n| 005c Inline console | yes | yes | tests | none |\n\n| Spec | Slice | Content | State |\n| :-- | :-- | :-- | :-- |\n| 135 | F0 | `delphi forge claim\\|release\\|list` | approved in advance on the plan; not built |\n| 135 | F1 | `a|b` unescaped | built (this PR) |\n| 028-A1 | E2 | expand | built and merged (#273) |\n| 132 | C1 | quick wins | not built |\n";

    #[test]
    fn slice_rows_are_read_and_the_summary_table_is_not() {
        let r = rows(SAMPLE);
        assert_eq!(r.iter().map(|x| (x.spec.as_str(), x.slice.as_str())).collect::<Vec<_>>(), vec![("135", "F0"), ("135", "F1"), ("028-A1", "E2"), ("132", "C1")]);
        assert_eq!(r[0].state, "approved in advance on the plan; not built");
        // An unescaped pipe in a middle cell does not move the state.
        assert_eq!(r[1].state, "built (this PR)");
        assert_eq!(r[0].line, 9);
        assert_eq!(rows_for(&r, "135").len(), 2);
    }

    const SECTIONS: &str = "# Spec tracker (2026-10-25)\n\n| 005c Inline console | yes | yes | none |\n\n## Spec 094: the merge flow (2026-11-07, approved)\n\nProse.\n\n| Slice | Content | Hours | State |\n| :-- | :-- | --: | :-- |\n| S1 | the spec | 3 | merged (#561) |\n| **S16** | the commits job | 2 | built (this PR) |\n| `S2a` | the skill | 4 | not built |\n\n### Notes on 094\n\n| S17 | a sub-section row | built |\n\n## Council 013: containment (verdict: `councils/013-verdict.md`)\n\n| Slice | Content | State |\n| :-- | :-- | :-- |\n| S3 | not a spec's row | merged |\n\n## Benchmarks (specs 048, 049 and 028-A1; drafted 2026-10-28)\n\n| E2 | expand | built |\n\n## Host program: specs 104 to 106\n\n| Spec | Slice | Content | State |\n| :-- | :-- | :-- | :-- |\n| 105 | S1 | spec-first | merged |\n| S9 | slice-first under a range | merged |\n";

    #[test]
    fn slice_first_rows_take_the_spec_of_their_section() {
        let r = rows(SECTIONS);
        let s1 = find(&r, "094", "S1").unwrap();
        assert_eq!((s1.line, s1.state.as_str(), s1.content.as_str()), (11, "merged (#561)", "the spec | 3"));
        assert_eq!(s1.section.as_deref(), Some("Spec 094: the merge flow (2026-11-07, approved)"));
        // S1 is not S16, and bold or backticks around the id are not part of it.
        assert_eq!(find(&r, "094", "S16").unwrap().line, 12);
        assert_eq!(find(&r, "094", "s2a").unwrap().state, "not built");
        assert!(find(&r, "094", "S").is_none());
        // A sub-heading that names no spec stays in its parent's section.
        assert_eq!(find(&r, "094", "S17").unwrap().line, 17);
        // The header line of a table is not a row, and the summary table names no spec.
        assert!(find(&r, "094", "Slice").is_none());
        assert!(r.iter().all(|x| !x.slice.starts_with("005c")));
        // A Council's section names no spec: its rows belong to nobody, not to the section above it.
        assert!(r.iter().all(|x| x.slice != "S3"), "{:?}", r);
        // A heading naming several specs gives the row to each; an amendment names its base number.
        for n in ["048", "049", "028"] {
            assert_eq!(find(&r, n, "E2").unwrap().line, 27, "{}", n);
        }
        assert!(find(&r, "094", "E2").is_none());
        // Both shapes in one section: spec-first keeps its own spec; slice-first takes each number of the range.
        assert_eq!(find(&r, "105", "S1").unwrap().section, None);
        assert!(find(&r, "104", "S1").is_none());
        for n in ["104", "105", "106"] {
            assert_eq!(find(&r, n, "S9").unwrap().line, 34, "{}", n);
        }
    }

    #[test]
    fn headings_name_specs_and_not_dates_paths_or_councils() {
        let h = |t: &str| heading_specs(t);
        assert_eq!(h("Spec 094: the core-repo merge flow (2026-11-07, approved)"), vec!["094"]);
        assert_eq!(h("Layered learning and training mode (spec 031, approved, 2026-10-26)"), vec!["031"]);
        assert_eq!(h("Benchmarking, context and memory (specs 048, 049, 050; drafted 2026-10-28)"), vec!["048", "049", "050"]);
        assert_eq!(h("Pictures (specs 029-A1 and 028-A2, approved 2026-10-28)"), vec!["028", "029"]);
        assert_eq!(h("Council specs: evidence (specs 078-081, 2026-11-05)"), vec!["078", "079", "080", "081"]);
        assert_eq!(h("Sharing-layer slices (specs 022\u{2013}024; estimates)"), vec!["022", "023", "024"]);
        assert_eq!(h("The knowledge loop: specs 112 to 114 (2026-11-14, approved)"), vec!["112", "113", "114"]);
        assert_eq!(h("Epic threads (spec 005g)"), vec!["005g"]);
        assert!(h("Council 013: containment increment (verdict: `councils/013-verdict.md`)").is_empty());
        assert!(h("Spec tracker (2026-10-25)").is_empty());
        assert!(h("Spec fold (docs only, 2026-10-30)").is_empty());
        assert!(h("Merged in #273 (spec)").is_empty());
    }

    /// The real tracker: a sample of slices in both shapes is found on the right line, and slices of one section are not found
    /// for another spec.
    #[test]
    fn the_real_tracker_rows_are_found_in_both_shapes() {
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(PATH)).unwrap();
        let r = rows(&text);
        let line = |n: &str| text.lines().position(|l| l.trim_start().starts_with(n)).map(|i| i + 1);
        let sections = sections(&text);
        let section_of = |spec: &str| sections.iter().find(|s| s.specs == vec![spec.to_string()]).map(|s| s.title.clone());
        // Slice-first, under the spec's own heading.
        for (spec, slice) in [("094", "S13"), ("094", "S14"), ("094", "S15"), ("094", "S16"), ("094", "S1"), ("093", "B1"), ("093", "B2"), ("031", "L2"), ("041", "S3"), ("092", "S2"), ("089", "P4a")] {
            let row = find(&r, spec, slice).unwrap_or_else(|| panic!("no row for {} {}", spec, slice));
            assert_eq!(row.section, section_of(spec), "{} {}", spec, slice);
            let in_section = sections_for(&text, spec)[0].line;
            assert!(row.line > in_section, "{} {} at line {}, before its heading (line {})", spec, slice, row.line, in_section);
            assert_eq!(bare(&cells(text.lines().nth(row.line - 1).unwrap()).unwrap()[0]), slice);
        }
        // An amendment's heading (`spec 028-A1`) and a range (`specs 022-027`) name their numbers.
        assert!(find(&r, "028", "E2").is_some());
        assert!(find(&r, "024", "B3").is_some());
        // Spec-first rows, as before.
        for (spec, slice) in [("135", "F0"), ("135", "F1"), ("135", "F2"), ("125", "S1"), ("123", "S0")] {
            let row = find(&r, spec, slice).unwrap_or_else(|| panic!("no row for {} {}", spec, slice));
            assert_eq!((row.section.as_deref(), Some(row.line)), (None, line(&format!("| {} | {} |", spec, slice))), "{} {}", spec, slice);
        }
        // No false positives across sections: S16 is 094's alone, 093 has no S slices, 094 has no B slices, S1 is not S16.
        assert!(find(&r, "093", "S16").is_none());
        assert!(find(&r, "093", "S1").is_none());
        assert!(find(&r, "094", "B1").is_none());
        assert_ne!(find(&r, "094", "S1").unwrap().line, find(&r, "094", "S16").unwrap().line);
        assert!(find(&r, "094", "S99").is_none());
        // Header lines are never rows.
        assert!(r.iter().all(|x| !["Slice", "Spec"].contains(&x.slice.as_str())), "a header line was read as a row");
    }

    #[test]
    fn states_are_classified() {
        assert!(state_unfinished("Not built; waits for S0"));
        assert!(state_unfinished("approved in advance on the plan; not built"));
        assert!(!state_unfinished("built (this PR)"));
        assert!(state_merged("built and merged (#731)"));
        assert!(state_merged("merged 2026-11-20"));
        assert!(!state_merged("built, not merged"));
    }

    #[test]
    fn the_frontmatter_status_is_read_without_its_comment() {
        assert_eq!(spec_status("---\nid: x\nstatus: approved   # by the operator\n---\n# x").as_deref(), Some("approved"));
        assert_eq!(spec_status("---\nstatus: draft\n---\n").as_deref(), Some("draft"));
        assert_eq!(spec_status("# no frontmatter\nstatus: approved\n"), None);
        assert_eq!(spec_status("---\nid: x\n---\nstatus: approved\n"), None);
    }

    #[test]
    fn spec_numbers_come_from_stems() {
        assert_eq!(spec_number("135-the-forge").as_deref(), Some("135"));
    }
}
