//! The spec tracker, read (spec 135 §4.1, §5.3). Pure: text in, rows out. The tracker is free prose kept by builders, so this
//! reads only what every slice row shares (the spec, the slice and the last cell, its state) and never writes it.

use std::path::{Path, PathBuf};

/// Where the tracker lives in a checkout.
pub const PATH: &str = "missions/2026-10-23-foundational-design/spec-tracker.md";

pub fn path(repo: &Path) -> PathBuf {
    repo.join(PATH)
}

/// One slice row: `| <spec> | <slice> | ... | <state> |`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub spec: String,
    pub slice: String,
    /// The middle cells, joined (the content of the slice).
    pub content: String,
    pub state: String,
    /// 1-based line in the tracker.
    pub line: usize,
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

/// Every slice row of the tracker: lines of at least four cells whose first cell is a spec number. The older summary table
/// (`| 005c Inline console | yes | ...`) has a title in its first cell, so it is not read as slices.
pub fn rows(text: &str) -> Vec<Row> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let c = cells(l)?;
            if c.len() < 4 || !is_spec_cell(&c[0]) || c[1].is_empty() || c[1].starts_with(':') {
                return None;
            }
            Some(Row { spec: c[0].clone(), slice: c[1].clone(), content: c[2..c.len() - 1].join(" | "), state: c[c.len() - 1].clone(), line: i + 1 })
        })
        .collect()
}

/// The rows of one spec, by its number (`135`).
pub fn rows_for<'a>(rows: &'a [Row], spec_number: &str) -> Vec<&'a Row> {
    rows.iter().filter(|r| r.spec.eq_ignore_ascii_case(spec_number)).collect()
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
