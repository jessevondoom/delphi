//! CSV and TSV as tables, read the same way as a spreadsheet sheet (`specs/014` §4): dimensions, header
//! row and a bounded slice. The whole file is scanned for the row count; only the slice is kept.

use crate::sniff::Format;
use crate::table::{parse_range, Collector, Range, DEFAULT_ROWS};
use crate::{DocError, Extract, Request, Unit};

/// The most likely delimiter: the candidate whose count per line is most consistent over the first lines.
fn sniff_delimiter(text: &str) -> char {
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).take(20).collect();
    let mut best = (',', 0usize, 0usize);
    for d in [',', ';', '\t', '|'] {
        let counts: Vec<usize> = lines.iter().map(|l| l.matches(d).count()).collect();
        let min = counts.iter().copied().min().unwrap_or(0);
        let total: usize = counts.iter().sum();
        if (min, total) > (best.1, best.2) {
            best = (d, min, total);
        }
    }
    best.0
}

pub fn extract(bytes: &[u8], format: Format, req: &Request) -> Result<Extract, DocError> {
    let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let text = String::from_utf8_lossy(body);
    let delim = if format == Format::Tsv { '\t' } else { sniff_delimiter(&text) };
    let range = match &req.range {
        Some(r) => parse_range(r).ok_or_else(|| DocError(format!("`{}` is not a range; use A1:F200, 5:20 or B:D", r)))?,
        None => Range::rows(1, DEFAULT_ROWS),
    };
    let mut col = Collector::new(range);
    let (mut row, mut c) = (1u32, 1u32);
    let mut field = String::new();
    let (mut in_q, mut was_quoted, mut any) = (false, false, false);
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        any = true;
        if in_q {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_q = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() && !was_quoted => {
                in_q = true;
                was_quoted = true;
            }
            c2 if c2 == delim => {
                col.push(row, c, std::mem::take(&mut field));
                c += 1;
                was_quoted = false;
            }
            '\r' | '\n' => {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                col.push(row, c, std::mem::take(&mut field));
                row += 1;
                c = 1;
                was_quoted = false;
                any = false;
            }
            _ => field.push(ch),
        }
    }
    if any || !field.is_empty() {
        col.push(row, c, field);
    }
    let name = if delim == '\t' { "TSV".to_string() } else { format!("CSV, delimiter `{}`", delim) };
    let (text, mut loss) = col.render(&name, None);
    if in_q {
        loss.push("a quoted field is never closed; the rest of the file is one cell".to_string());
    }
    Ok(Extract { format, text, unit: Unit::Line, total_units: Some(col.max_row as usize), truncated: col.max_row > range.r2, loss, supported: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv(s: &str) -> Extract {
        extract(s.as_bytes(), Format::Csv, &Request::default()).unwrap()
    }

    #[test]
    fn quoted_fields_embedded_newlines_and_doubled_quotes() {
        let e = csv("name,note\n\"Smith, J\",\"said \"\"hi\"\"\nthen left\"\nPat,ok\n");
        assert!(e.text.contains("(3 rows × 2 columns"), "{}", e.text);
        assert!(e.text.contains("Smith, J") && e.text.contains("said \"hi\" then left") && e.text.contains("| 3 | Pat | ok |"), "{}", e.text);
    }

    #[test]
    fn the_delimiter_is_sniffed_and_a_bom_and_crlf_are_handled() {
        let e = csv("\u{feff}a;b;c\r\n1;2;3\r\n");
        assert!(e.text.contains("delimiter `;`") && e.text.contains("| 2 | 1 | 2 | 3 |"), "{}", e.text);
        let t = extract(b"a\tb\n1\t2\n", Format::Tsv, &Request::default()).unwrap();
        assert!(t.text.starts_with("TSV (2 rows × 2 columns"), "{}", t.text);
    }

    #[test]
    fn ranges_slice_a_large_file_and_the_totals_cover_all_of_it() {
        let mut s = String::from("id,v\n");
        for i in 1..=1000 {
            s.push_str(&format!("{},{}\n", i, i * 2));
        }
        let e = extract(s.as_bytes(), Format::Csv, &Request { range: Some("A500:B502".into()), ..Request::default() }).unwrap();
        assert!(e.text.contains("(1001 rows × 2 columns") && e.text.contains("Header (row 1): A=id, B=v") && e.text.contains("| 500 | 499 | 998 |"), "{}", e.text);
        assert!(!e.text.contains("| 503 |"));
        let d = csv(&s);
        assert!(d.truncated && d.text.contains("rows 1-50 of 1001 shown"), "{}", &d.text[d.text.len() - 150..]);
    }

    #[test]
    fn ragged_rows_blank_lines_and_an_unclosed_quote_do_not_break_it() {
        let e = csv("a,b,c\n1,2\n\n4,5,6,7\n");
        assert!(e.text.contains("4 columns"), "{}", e.text);
        let e = csv("a,b\n\"never closed,1\n2,3\n");
        assert!(e.loss.iter().any(|l| l.contains("never closed")), "{:?}", e.loss);
        assert!(csv("").text.contains("(empty)"));
    }

    #[test]
    fn a_bad_range_is_an_error_with_the_accepted_forms() {
        let r = extract(b"a,b\n", Format::Csv, &Request { range: Some("zzz!!".into()), ..Request::default() });
        assert!(r.unwrap_err().0.contains("A1:F200"));
    }
}
