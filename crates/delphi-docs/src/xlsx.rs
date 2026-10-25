//! Excel workbooks as tables: per-sheet dimensions, header row and a bounded slice, with cached formula
//! values next to the formula text (`specs/014` §4). Charts, pivot tables and macros are not read.

use crate::ooxml::Pkg;
use crate::sniff::Format;
use crate::table::{parse_range, Collector, Range, DEFAULT_ROWS};
use crate::xml::{attr, walk, Ev};
use crate::{DocError, Extract, Request, Unit};
use std::collections::HashMap;

const PREVIEW_ROWS: u32 = 6;
const MAX_SUMMARY_SHEETS: usize = 10;

struct Sheet {
    name: String,
    path: String,
    hidden: bool,
}

fn workbook(pkg: &Pkg) -> Result<(Vec<Sheet>, bool), DocError> {
    let xml = pkg.part("xl/workbook.xml")?.ok_or_else(|| DocError("the workbook has no xl/workbook.xml".into()))?;
    let rels = pkg.rels("xl/workbook.xml");
    let mut sheets = vec![];
    let mut date1904 = false;
    let _ = walk(&xml, pkg.limits.max_nodes, |ev| {
        if let Ev::Start { name, attrs } = ev {
            match name.as_str() {
                "workbookPr" => date1904 = matches!(attr(&attrs, "date1904"), Some("1" | "true")),
                "sheet" => {
                    let Some(rel) = attr(&attrs, "r:id").and_then(|id| rels.get(id)) else { return };
                    sheets.push(Sheet { name: attr(&attrs, "name").unwrap_or("?").to_string(), path: rel.target.clone(), hidden: attr(&attrs, "state").is_some_and(|s| s != "visible") });
                }
                _ => {}
            }
        }
    });
    Ok((sheets, date1904))
}

fn shared_strings(pkg: &Pkg) -> Result<Vec<String>, DocError> {
    let mut out = vec![];
    let Some(xml) = pkg.part("xl/sharedStrings.xml")? else { return Ok(out) };
    let (mut cur, mut in_si, mut in_t, mut phonetic) = (String::new(), false, false, 0usize);
    let _ = walk(&xml, pkg.limits.max_nodes, |ev| match ev {
        Ev::Start { name, .. } => match name.as_str() {
            "si" => {
                in_si = true;
                cur.clear();
            }
            "rPh" => phonetic += 1,
            "t" if in_si && phonetic == 0 => in_t = true,
            _ => {}
        },
        Ev::Text(t) if in_t => cur.push_str(&t),
        Ev::End { name } => match name.as_str() {
            "t" => in_t = false,
            "rPh" => phonetic = phonetic.saturating_sub(1),
            "si" => {
                in_si = false;
                out.push(std::mem::take(&mut cur));
            }
            _ => {}
        },
        _ => {}
    });
    Ok(out)
}

/// For each cell format index: does it show a date or time?
fn date_styles(pkg: &Pkg) -> Result<Vec<bool>, DocError> {
    let Some(xml) = pkg.part("xl/styles.xml")? else { return Ok(vec![]) };
    let mut custom: HashMap<u32, String> = HashMap::new();
    let mut xfs: Vec<u32> = vec![];
    let mut in_xfs = false;
    let _ = walk(&xml, pkg.limits.max_nodes, |ev| match ev {
        Ev::Start { name, attrs } => match name.as_str() {
            "numFmt" => {
                if let (Some(id), Some(code)) = (attr(&attrs, "numFmtId").and_then(|s| s.parse().ok()), attr(&attrs, "formatCode")) {
                    custom.insert(id, code.to_string());
                }
            }
            "cellXfs" => in_xfs = true,
            "xf" if in_xfs => xfs.push(attr(&attrs, "numFmtId").and_then(|s| s.parse().ok()).unwrap_or(0)),
            _ => {}
        },
        Ev::End { name } if name == "cellXfs" => in_xfs = false,
        _ => {}
    });
    Ok(xfs
        .iter()
        .map(|id| match id {
            14..=22 | 27..=36 | 45..=47 | 50..=58 => true,
            _ => custom.get(id).is_some_and(|c| is_date_format(c)),
        })
        .collect())
}

/// A custom number format is a date if it has date or time letters outside quotes, brackets and escapes.
fn is_date_format(code: &str) -> bool {
    let (mut in_q, mut in_b, mut esc) = (false, false, false);
    for c in code.chars() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' | '_' | '*' => esc = true,
            '"' => in_q = !in_q,
            '[' if !in_q => in_b = true,
            ']' if !in_q => in_b = false,
            'y' | 'Y' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' | 'm' | 'M' if !in_q && !in_b => return true,
            _ => {}
        }
    }
    false
}

/// Days since 1970-01-01 to (year, month, day); Howard Hinnant's civil-from-days.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// An Excel serial number as `YYYY-MM-DD` (with `HH:MM:SS` when it has a time part).
fn serial_to_date(v: &str, date1904: bool) -> Option<String> {
    let f: f64 = v.trim().parse().ok()?;
    if !(0.0..2_958_466.0).contains(&f) {
        return None;
    }
    let whole = f.floor() as i64;
    let secs = ((f - f.floor()) * 86_400.0).round() as i64;
    let (whole, secs) = if secs >= 86_400 { (whole + 1, 0) } else { (whole, secs) };
    // 1900 system: serial 1 is 1900-01-01 and 1900 is wrongly a leap year, so serials from 61 on are offset by 25569.
    let unix = if date1904 { whole - 24_107 } else if whole >= 61 { whole - 25_569 } else { whole - 25_568 };
    let (y, m, d) = civil(unix);
    Some(if secs == 0 { format!("{:04}-{:02}-{:02}", y, m, d) } else { format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, secs / 3600, secs % 3600 / 60, secs % 60) })
}

fn parse_ref(r: &str) -> Option<(u32, u32)> {
    let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let row: u32 = r[letters.len()..].parse().ok()?;
    let col = letters.to_ascii_uppercase().bytes().fold(0u32, |a, b| a.saturating_mul(26).saturating_add((b - b'A' + 1) as u32));
    (!letters.is_empty() && row >= 1 && col >= 1).then_some((row, col))
}

struct Ctx<'a> {
    strings: &'a [String],
    date_styles: &'a [bool],
    date1904: bool,
}

fn read_sheet(xml: &[u8], ctx: &Ctx, range: Range, max_nodes: usize) -> (Collector, Option<DocError>) {
    let mut col = Collector::new(range);
    let (mut row, mut next_col) = (0u32, 1u32);
    let (mut pos, mut ty, mut style) = ((0u32, 0u32), String::new(), 0usize);
    let (mut v, mut f, mut is) = (String::new(), String::new(), String::new());
    let (mut in_v, mut in_f, mut in_is, mut in_t) = (false, false, false, false);
    let err = walk(xml, max_nodes, |ev| match ev {
        Ev::Start { name, attrs } => match name.as_str() {
            "row" => {
                row = attr(&attrs, "r").and_then(|r| r.parse().ok()).unwrap_or(row + 1);
                next_col = 1;
            }
            "c" => {
                pos = attr(&attrs, "r").and_then(parse_ref).unwrap_or((row.max(1), next_col));
                ty = attr(&attrs, "t").unwrap_or("").to_string();
                style = attr(&attrs, "s").and_then(|s| s.parse().ok()).unwrap_or(0);
                v.clear();
                f.clear();
                is.clear();
            }
            "v" => in_v = true,
            "f" => in_f = true,
            "is" => in_is = true,
            "t" if in_is => in_t = true,
            _ => {}
        },
        Ev::Text(t) => {
            if in_v {
                v.push_str(&t);
            } else if in_f {
                f.push_str(&t);
            } else if in_t {
                is.push_str(&t);
            }
        }
        Ev::End { name } => match name.as_str() {
            "v" => in_v = false,
            "f" => in_f = false,
            "is" => in_is = false,
            "t" => in_t = false,
            "c" => {
                let shown = match ty.as_str() {
                    "s" => v.trim().parse::<usize>().ok().and_then(|i| ctx.strings.get(i)).cloned().unwrap_or_default(),
                    "inlineStr" => is.clone(),
                    "b" => if v.trim() == "1" { "TRUE".into() } else { "FALSE".into() },
                    "str" | "e" | "d" => v.clone(),
                    _ if v.is_empty() => String::new(),
                    _ if ctx.date_styles.get(style).copied().unwrap_or(false) => serial_to_date(&v, ctx.date1904).unwrap_or_else(|| v.clone()),
                    _ => v.clone(),
                };
                let formula = f.trim();
                let text = match (shown.is_empty(), formula.is_empty()) {
                    (_, true) => shown,
                    (true, false) => format!("(={})", formula),
                    (false, false) => format!("{} (={})", shown, formula),
                };
                col.push(pos.0, pos.1, text);
                next_col = pos.1 + 1;
            }
            _ => {}
        },
    });
    (col, err.err())
}

pub fn extract(pkg: &Pkg, req: &Request) -> Result<Extract, DocError> {
    let (sheets, date1904) = workbook(pkg)?;
    if sheets.is_empty() {
        return Err(DocError("the workbook lists no sheets".into()));
    }
    let strings = shared_strings(pkg)?;
    let date_styles = date_styles(pkg)?;
    let ctx = Ctx { strings: &strings, date_styles: &date_styles, date1904 };
    let user_range = match &req.range {
        Some(r) => Some(parse_range(r).ok_or_else(|| DocError(format!("`{}` is not a range; use A1:F200, 5:20 or B:D", r)))?),
        None => None,
    };
    let names = || sheets.iter().enumerate().map(|(i, s)| format!("{}: {}", i + 1, s.name)).collect::<Vec<_>>().join(", ");
    let mut loss = vec![];
    let mut text = String::new();
    let render = |i: usize, range: Range, loss: &mut Vec<String>| -> Result<String, DocError> {
        let s = &sheets[i];
        let xml = pkg.part(&s.path)?.ok_or_else(|| DocError(format!("sheet `{}` points at a missing part", s.name)))?;
        let (col, err) = read_sheet(&xml, &ctx, range, pkg.limits.max_nodes);
        if let Some(e) = err {
            loss.push(format!("sheet `{}` stops early: {}", s.name, e));
        }
        let (t, l) = col.render(&format!("## Sheet {}: {}", i + 1, s.name), s.hidden.then_some("hidden"));
        loss.extend(l);
        Ok(t)
    };
    let selected = match &req.sheet {
        Some(want) => Some(
            sheets
                .iter()
                .position(|s| s.name.eq_ignore_ascii_case(want))
                .or_else(|| want.parse::<usize>().ok().filter(|n| (1..=sheets.len()).contains(n)).map(|n| n - 1))
                .ok_or_else(|| DocError(format!("no sheet `{}`; the workbook has {}", want, names())))?,
        ),
        None if sheets.len() == 1 => Some(0),
        None => None,
    };
    match selected {
        Some(i) => text.push_str(&render(i, user_range.unwrap_or(Range::rows(1, DEFAULT_ROWS)), &mut loss)?),
        None => {
            if user_range.is_some() {
                return Err(DocError(format!("name a sheet to use a range; the workbook has {}", names())));
            }
            text.push_str(&format!("Workbook with {} sheets: {}\n\n", sheets.len(), names()));
            for i in 0..sheets.len().min(MAX_SUMMARY_SHEETS) {
                text.push_str(&render(i, Range::rows(1, PREVIEW_ROWS), &mut loss)?);
                text.push('\n');
            }
            if sheets.len() > MAX_SUMMARY_SHEETS {
                text.push_str(&format!("({} more sheets not previewed)\n", sheets.len() - MAX_SUMMARY_SHEETS));
            }
            text.push_str("Pass `sheet` (name or number) and `range` (for example A1:F200) to read more.\n");
        }
    }
    let charts = pkg.parts_under("xl/charts/").iter().filter(|p| p.ends_with(".xml")).count();
    if charts > 0 {
        loss.push(format!("{} chart(s) not read", charts));
    }
    if pkg.has("xl/vbaProject.bin") {
        loss.push("the workbook contains macros; they are never run".to_string());
    }
    if !pkg.parts_under("xl/pivotTables/").is_empty() {
        loss.push("pivot tables not read (their cell values are)".to_string());
    }
    Ok(Extract { format: Format::Xlsx, text, unit: Unit::Sheet, total_units: Some(sheets.len()), truncated: false, loss, supported: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_numbers_become_dates() {
        assert_eq!(serial_to_date("45292", false).as_deref(), Some("2024-01-01"));
        assert_eq!(serial_to_date("1", false).as_deref(), Some("1900-01-01"));
        assert_eq!(serial_to_date("61", false).as_deref(), Some("1900-03-01"));
        assert_eq!(serial_to_date("45292.5", false).as_deref(), Some("2024-01-01 12:00:00"));
        assert_eq!(serial_to_date("0", true).as_deref(), Some("1904-01-01"));
        assert_eq!(serial_to_date("-3", false), None);
        assert_eq!(serial_to_date("abc", false), None);
        assert_eq!(serial_to_date("99999999", false), None);
    }

    #[test]
    fn custom_formats_are_dates_only_when_they_show_date_parts() {
        assert!(is_date_format("yyyy-mm-dd"));
        assert!(is_date_format("[$-409]d-mmm-yy"));
        assert!(is_date_format("h:mm AM/PM"));
        assert!(!is_date_format("#,##0.00"));
        assert!(!is_date_format("0.0\"days\""));
        assert!(!is_date_format("[Red]0.00"));
        assert!(!is_date_format("\\d0"));
    }

    #[test]
    fn cell_references_parse() {
        assert_eq!(parse_ref("A1"), Some((1, 1)));
        assert_eq!(parse_ref("AB12"), Some((12, 28)));
        assert_eq!(parse_ref("12"), None);
        assert_eq!(parse_ref("A0"), None);
        assert_eq!(parse_ref("A"), None);
    }
}
