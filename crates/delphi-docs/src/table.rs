//! Tabular data as the agent sees it: dimensions, the header row, and a bounded slice of rows
//! (`specs/014` §4). Spreadsheets and CSV files share this, so a sheet and its CSV export read the same way.
//!
//! A [`Collector`] keeps only the header row and the rows inside the requested range, while counting the
//! whole table, so a million-row sheet costs memory for the slice, not the sheet.

use std::collections::BTreeMap;

/// 1-based, inclusive. `u32::MAX` means "to the end".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub r1: u32,
    pub r2: u32,
    pub c1: u32,
    pub c2: u32,
}

impl Range {
    pub fn rows(r1: u32, r2: u32) -> Range {
        Range { r1, r2, c1: 1, c2: u32::MAX }
    }
}

/// Rows shown when no range is given.
pub const DEFAULT_ROWS: u32 = 50;
/// Columns rendered at most.
pub const MAX_COLS: u32 = 40;
const MAX_CELL: usize = 200;

pub fn col_letters(mut c: u32) -> String {
    let mut s = Vec::new();
    while c > 0 {
        let r = (c - 1) % 26;
        s.push(b'A' + r as u8);
        c = (c - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// `A1` -> (row 1, col 1). Row-only or column-only parts are `None`.
fn cell_ref(s: &str) -> Option<(Option<u32>, Option<u32>)> {
    let letters: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits = &s[letters.len()..];
    if letters.len() > 3 || (letters.is_empty() && digits.is_empty()) || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let col = if letters.is_empty() { None } else { Some(letters.to_ascii_uppercase().bytes().fold(0u32, |a, b| a * 26 + (b - b'A' + 1) as u32)) };
    let row = if digits.is_empty() { None } else { Some(digits.parse().ok().filter(|&r| r >= 1)?) };
    Some((row, col))
}

/// `A1:F200`, `A1` (one cell), `5:20` (rows), `B:D` (columns).
pub fn parse_range(s: &str) -> Option<Range> {
    let s = s.trim();
    let r = match s.split_once(':') {
        None => {
            // A single cell needs both its row and its column.
            let (row, col) = cell_ref(s)?;
            let (row, col) = (row?, col?);
            Range { r1: row, r2: row, c1: col, c2: col }
        }
        Some((a, b)) => {
            let (ar, ac) = cell_ref(a)?;
            let (br, bc) = cell_ref(b)?;
            Range { r1: ar.unwrap_or(1), r2: br.unwrap_or(u32::MAX), c1: ac.unwrap_or(1), c2: bc.unwrap_or(u32::MAX) }
        }
    };
    (r.r1 <= r.r2 && r.c1 <= r.c2).then_some(r)
}

#[derive(Debug, Default)]
pub struct Collector {
    range: Option<Range>,
    header: BTreeMap<u32, String>,
    cells: BTreeMap<(u32, u32), String>,
    pub max_row: u32,
    pub max_col: u32,
    pub cell_count: u64,
}

impl Collector {
    pub fn new(range: Range) -> Collector {
        Collector { range: Some(range), ..Collector::default() }
    }

    pub fn push(&mut self, row: u32, col: u32, text: String) {
        if text.is_empty() {
            return;
        }
        self.cell_count += 1;
        self.max_row = self.max_row.max(row);
        self.max_col = self.max_col.max(col);
        let r = self.range.unwrap_or(Range::rows(1, DEFAULT_ROWS));
        if row == 1 {
            self.header.insert(col, text.clone());
        }
        if row >= r.r1 && row <= r.r2 && col >= r.c1 && col <= r.c2 {
            self.cells.insert((row, col), text);
        }
    }

    /// Render the slice as a Markdown table with column letters and row numbers.
    pub fn render(&self, title: &str, hint: Option<&str>) -> (String, Vec<String>) {
        let r = self.range.unwrap_or(Range::rows(1, DEFAULT_ROWS));
        let mut loss = vec![];
        let last = if self.max_row == 0 { String::new() } else { format!(" · last cell {}{}", col_letters(self.max_col), self.max_row) };
        let mut out = format!("{} ({} row{} × {} column{}{}{})\n", title, self.max_row, if self.max_row == 1 { "" } else { "s" }, self.max_col, if self.max_col == 1 { "" } else { "s" }, last, hint.map(|h| format!(" · {}", h)).unwrap_or_default());
        if self.cell_count == 0 {
            out.push_str("(empty)\n");
            return (out, loss);
        }
        let c1 = r.c1.max(1);
        let c2 = r.c2.min(self.max_col).min(c1.saturating_add(MAX_COLS - 1));
        if c2 < r.c2.min(self.max_col) {
            loss.push(format!("{} has {} columns; showing {}-{} (pass a range for others)", title, self.max_col, col_letters(c1), col_letters(c2)));
        }
        let r_last = r.r2.min(self.max_row);
        if r.r1 > 1 && !self.header.is_empty() {
            let h: Vec<String> = self.header.iter().filter(|(c, _)| **c >= c1 && **c <= c2).map(|(c, v)| format!("{}={}", col_letters(*c), clip(v))).collect();
            out.push_str(&format!("Header (row 1): {}\n", h.join(", ")));
        }
        out.push_str("| row |");
        for c in c1..=c2 {
            out.push_str(&format!(" {} |", col_letters(c)));
        }
        out.push_str("\n|---|");
        for _ in c1..=c2 {
            out.push_str("---|");
        }
        out.push('\n');
        for row in r.r1..=r_last {
            if self.cells.range((row, 0)..(row + 1, 0)).next().is_none() {
                continue;
            }
            out.push_str(&format!("| {} |", row));
            for c in c1..=c2 {
                out.push_str(&format!(" {} |", self.cells.get(&(row, c)).map(|v| clip(v)).unwrap_or_default()));
            }
            out.push('\n');
        }
        if r_last < self.max_row {
            out.push_str(&format!("(rows {}-{} of {} shown; pass range, for example `A{}:{}{}`)\n", r.r1, r_last, self.max_row, r_last + 1, col_letters(self.max_col), (r_last + DEFAULT_ROWS).min(self.max_row)));
        }
        (out, loss)
    }
}

fn clip(v: &str) -> String {
    let flat: String = v.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().replace('|', "\\|");
    let flat = flat.trim();
    if flat.chars().count() > MAX_CELL {
        format!("{}…", flat.chars().take(MAX_CELL).collect::<String>())
    } else {
        flat.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_letters_round_trip() {
        assert_eq!(col_letters(1), "A");
        assert_eq!(col_letters(26), "Z");
        assert_eq!(col_letters(27), "AA");
        assert_eq!(col_letters(702), "ZZ");
        assert_eq!(col_letters(703), "AAA");
        assert_eq!(cell_ref("AA7"), Some((Some(7), Some(27))));
    }

    #[test]
    fn ranges_parse_in_the_forms_an_agent_would_write() {
        assert_eq!(parse_range("A1:F200"), Some(Range { r1: 1, r2: 200, c1: 1, c2: 6 }));
        assert_eq!(parse_range("b3"), Some(Range { r1: 3, r2: 3, c1: 2, c2: 2 }));
        assert_eq!(parse_range("5:20"), Some(Range { r1: 5, r2: 20, c1: 1, c2: u32::MAX }));
        assert_eq!(parse_range("B:D"), Some(Range { r1: 1, r2: u32::MAX, c1: 2, c2: 4 }));
        assert_eq!(parse_range("A10:B2"), None);
        assert_eq!(parse_range("nonsense!"), None);
        assert_eq!(parse_range("A0:B2"), None);
        assert_eq!(parse_range(""), None);
    }

    #[test]
    fn the_slice_is_rendered_with_header_and_a_paging_hint() {
        let mut c = Collector::new(Range { r1: 3, r2: 4, c1: 1, c2: u32::MAX });
        for r in 1..=10u32 {
            for col in 1..=3u32 {
                c.push(r, col, if r == 1 { format!("h{}", col) } else { format!("v{}-{}", r, col) });
            }
        }
        let (t, loss) = c.render("Sheet1", None);
        assert!(t.starts_with("Sheet1 (10 rows × 3 columns · last cell C10)"), "{}", t);
        assert!(t.contains("Header (row 1): A=h1, B=h2, C=h3") && t.contains("| 3 | v3-1 | v3-2 | v3-3 |") && t.contains("| 4 |"), "{}", t);
        assert!(!t.contains("| 5 |") && t.contains("rows 3-4 of 10 shown"), "{}", t);
        assert!(loss.is_empty());
    }

    #[test]
    fn wide_tables_are_cut_and_the_cut_is_reported() {
        let mut c = Collector::new(Range::rows(1, 5));
        for col in 1..=60u32 {
            c.push(1, col, format!("c{}", col));
        }
        let (t, loss) = c.render("S", None);
        assert!(t.contains(" AN |") && !t.contains(" AO |"), "{}", t);
        assert!(loss[0].contains("60 columns"), "{:?}", loss);
    }

    #[test]
    fn cells_are_flattened_escaped_and_clipped() {
        let mut c = Collector::new(Range::rows(1, 5));
        c.push(1, 1, "a|b\nc".into());
        c.push(1, 2, "x".repeat(500));
        let (t, _) = c.render("S", None);
        assert!(t.contains("a\\|b c") && t.contains('…') && !t.contains(&"x".repeat(250)), "{}", t);
    }

    #[test]
    fn an_empty_table_says_so() {
        let (t, _) = Collector::new(Range::rows(1, 5)).render("S", None);
        assert!(t.contains("(empty)"));
    }
}
