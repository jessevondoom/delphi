//! Spreadsheets we write (`specs/014` §7.4): `.xlsx` (a zip of spec-conformant XML) or `.csv`, from sheets of
//! typed rows. The rules that keep a generated file safe to open:
//!
//! - **Text is text.** A string is always stored as a string, never as a formula, whatever it starts with. In CSV,
//!   where the reader decides, a string starting with `=`, `+`, `-`, `@`, tab or carriage return gets a leading
//!   apostrophe so a spreadsheet program shows it instead of evaluating it. Numbers are never altered.
//! - **Formulas are explicit** (`formulas: [{cell, f}]`), written without a cached value so the program computes
//!   them on open, and are refused if they reach outside the workbook (external references, DDE, web or file
//!   functions) or are not plain formulas.
//! - **Deterministic**: the same input gives the same bytes (no timestamps beyond the zip's fixed one).

use crate::markdown::escape;
use crate::table::col_letters;
use crate::zipwriter::ZipWriter;
use crate::DocError;
use serde_json::Value;
use std::collections::BTreeMap;

pub const MAX_SHEETS: usize = 20;
pub const MAX_ROWS: usize = 50_000;
pub const MAX_COLS: usize = 200;
const MAX_CELL_CHARS: usize = 32_767;
const MAX_FORMULA: usize = 1000;
const MAX_FORMULAS: usize = 5000;

/// Functions that fetch from the network, run programs or read files.
const FORBIDDEN_FUNCTIONS: [&str; 16] = [
    "HYPERLINK", "WEBSERVICE", "FILTERXML", "IMPORTXML", "IMPORTDATA", "IMPORTHTML", "IMPORTFEED", "IMPORTRANGE", "EXEC", "CALL", "REGISTER", "RTD", "DDE", "DDEAUTO", "SHELL", "MSQUERY",
];

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Empty,
    Text(String),
    Num(String),
    Bool(bool),
    Formula(String),
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub name: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    /// Formula cells by (row, column), 1-based, over and above the data.
    pub formulas: BTreeMap<(u32, u32), String>,
    pub widths: Vec<f64>,
    pub formats: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
}

fn bad(msg: impl Into<String>) -> DocError {
    DocError(msg.into())
}

fn cell_of(v: &Value, what: &str) -> Result<Cell, DocError> {
    Ok(match v {
        Value::Null => Cell::Empty,
        Value::Bool(b) => Cell::Bool(*b),
        Value::Number(n) => Cell::Num(n.to_string()),
        Value::String(s) if s.is_empty() => Cell::Empty,
        Value::String(s) => {
            if s.chars().count() > MAX_CELL_CHARS {
                return Err(bad(format!("{} is over {} characters; a cell holds at most that", what, MAX_CELL_CHARS)));
            }
            Cell::Text(s.clone())
        }
        _ => return Err(bad(format!("{} is a list or object; a cell holds text, a number, true/false or nothing", what))),
    })
}

/// `C12` as (row 12, column 3).
pub fn parse_cell_ref(r: &str) -> Option<(u32, u32)> {
    let r = r.trim();
    let split = r.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = r.split_at(split);
    if letters.is_empty() || letters.len() > 3 || !letters.chars().all(|c| c.is_ascii_alphabetic()) || digits.is_empty() || digits.len() > 6 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let col = letters.to_ascii_uppercase().bytes().fold(0u32, |a, b| a * 26 + (b - b'A' + 1) as u32);
    let row: u32 = digits.parse().ok()?;
    (row >= 1 && col >= 1 && col as usize <= MAX_COLS && row as usize <= MAX_ROWS + 2).then_some((row, col))
}

/// Refuse a formula that is not plain arithmetic and functions over this workbook's cells.
pub fn check_formula(f: &str) -> Result<String, DocError> {
    let f = f.trim().trim_start_matches('=').trim();
    if f.is_empty() || f.len() > MAX_FORMULA {
        return Err(bad(format!("a formula is between 1 and {} characters", MAX_FORMULA)));
    }
    if f.chars().any(|c| c.is_control()) {
        return Err(bad("a formula has no control characters"));
    }
    for (ch, why) in [('[', "external workbook references"), (']', "external workbook references"), ('|', "DDE"), ('{', "array constants and macros"), ('\\', "file paths"), ('<', "markup")] {
        if f.contains(ch) && !(ch == '<' && f.contains("<=") || ch == '<' && f.contains("<>")) {
            return Err(bad(format!("formula `{}` uses `{}` ({} are not allowed)", f.chars().take(60).collect::<String>(), ch, why)));
        }
    }
    let upper = f.to_ascii_uppercase();
    for name in FORBIDDEN_FUNCTIONS {
        if upper.contains(&format!("{}(", name)) {
            return Err(bad(format!("formula `{}` calls {}, which reaches outside the workbook and is not allowed", f.chars().take(60).collect::<String>(), name)));
        }
    }
    if upper.contains("HTTP:") || upper.contains("HTTPS:") || upper.contains("FILE:") || upper.contains("FTP:") || upper.contains("://") {
        return Err(bad("a formula may not contain an address"));
    }
    Ok(f.to_string())
}

fn sheet_name(n: &str) -> Result<String, DocError> {
    let n = n.trim();
    if n.is_empty() || n.chars().count() > 31 {
        return Err(bad("a sheet `name` is 1 to 31 characters"));
    }
    if n.chars().any(|c| matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\') || c.is_control()) || n.starts_with('\'') || n.ends_with('\'') {
        return Err(bad(format!("sheet name `{}` cannot contain [ ] : * ? / \\ or start or end with an apostrophe", n)));
    }
    Ok(n.to_string())
}

fn format_code(f: &str) -> Result<String, DocError> {
    let f = f.trim();
    if f.chars().count() > 40 || !f.chars().all(|c| c.is_ascii_alphanumeric() || " #0.,%-+/:()$_\"€£".contains(c)) {
        return Err(bad(format!("number format `{}` is not allowed: use digits, # 0 . , % and a few symbols, like `0.0%` or `#,##0.00`", f.chars().take(40).collect::<String>())));
    }
    Ok(f.to_string())
}

fn sheet_from_json(v: &Value, idx: usize) -> Result<Sheet, DocError> {
    let name = sheet_name(v.get("name").and_then(|n| n.as_str()).unwrap_or(&format!("Sheet{}", idx + 1)))?;
    let columns: Vec<String> = v
        .get("columns")
        .and_then(|c| c.as_array())
        .ok_or_else(|| bad(format!("sheet `{}` needs `columns`: the header names", name)))?
        .iter()
        .map(|c| c.as_str().map(str::to_string).or_else(|| c.as_i64().map(|n| n.to_string())).ok_or_else(|| bad("every column name is text")))
        .collect::<Result<_, _>>()?;
    if columns.is_empty() || columns.len() > MAX_COLS {
        return Err(bad(format!("sheet `{}` has between 1 and {} columns", name, MAX_COLS)));
    }
    let raw = v.get("rows").and_then(|r| r.as_array()).ok_or_else(|| bad(format!("sheet `{}` needs `rows`: an array of arrays", name)))?;
    if raw.len() > MAX_ROWS {
        return Err(bad(format!("sheet `{}` has {} rows; the limit is {}. Summarise it, or write a .csv and read it in pieces", name, raw.len(), MAX_ROWS)));
    }
    let mut rows = Vec::with_capacity(raw.len());
    for (i, r) in raw.iter().enumerate() {
        let r = r.as_array().ok_or_else(|| bad(format!("sheet `{}` row {} is not an array", name, i + 1)))?;
        if r.len() > columns.len() {
            return Err(bad(format!("sheet `{}` row {} has {} values but there are {} columns", name, i + 1, r.len(), columns.len())));
        }
        rows.push(r.iter().enumerate().map(|(j, c)| cell_of(c, &format!("sheet `{}` row {} column {}", name, i + 1, j + 1))).collect::<Result<Vec<_>, _>>()?);
    }
    let mut formulas = BTreeMap::new();
    if let Some(list) = v.get("formulas") {
        let list = list.as_array().ok_or_else(|| bad("`formulas` is an array of {cell, f}"))?;
        if list.len() > MAX_FORMULAS {
            return Err(bad(format!("at most {} formulas per sheet", MAX_FORMULAS)));
        }
        for f in list {
            let cell = f.get("cell").and_then(|c| c.as_str()).ok_or_else(|| bad("each formula needs a `cell` like \"D2\""))?;
            let (row, col) = parse_cell_ref(cell).ok_or_else(|| bad(format!("`{}` is not a cell reference within this sheet's limits", cell)))?;
            if row == 1 && col as usize <= columns.len() {
                return Err(bad(format!("{} is in the header row", cell)));
            }
            let text = f.get("f").and_then(|c| c.as_str()).ok_or_else(|| bad(format!("formula for {} needs `f`", cell)))?;
            if formulas.insert((row, col), check_formula(text)?).is_some() {
                return Err(bad(format!("{} has two formulas", cell)));
            }
        }
    }
    let widths = match v.get("widths") {
        Some(w) => w
            .as_array()
            .ok_or_else(|| bad("`widths` is an array of numbers"))?
            .iter()
            .map(|x| x.as_f64().filter(|w| (1.0..=255.0).contains(w)).ok_or_else(|| bad("each width is a number of characters from 1 to 255")))
            .collect::<Result<_, _>>()?,
        None => vec![],
    };
    let formats = match v.get("formats") {
        Some(f) => f.as_array().ok_or_else(|| bad("`formats` is an array of number formats, one per column (\"\" for none)"))?.iter().map(|x| format_code(x.as_str().unwrap_or(""))).collect::<Result<_, _>>()?,
        None => vec![],
    };
    if widths.len() > columns.len() || formats.len() > columns.len() {
        return Err(bad("`widths` and `formats` have at most one entry per column"));
    }
    Ok(Sheet { name, columns, rows, formulas, widths, formats })
}

/// A workbook from `{sheets: [...]}`, or from one sheet's `columns`/`rows` given at the top level.
pub fn from_json(v: &Value) -> Result<Workbook, DocError> {
    let sheets: Vec<Sheet> = match v.get("sheets") {
        Some(list) => {
            let list = list.as_array().ok_or_else(|| bad("`sheets` is an array"))?;
            list.iter().enumerate().map(|(i, s)| sheet_from_json(s, i)).collect::<Result<_, _>>()?
        }
        None if v.get("columns").is_some() => vec![sheet_from_json(v, 0)?],
        None => return Err(bad("give `sheets`: [{name, columns, rows, formulas?, widths?, formats?}] (or `columns` and `rows` for a single sheet)")),
    };
    if sheets.is_empty() || sheets.len() > MAX_SHEETS {
        return Err(bad(format!("a workbook has between 1 and {} sheets", MAX_SHEETS)));
    }
    let mut seen = std::collections::HashSet::new();
    for s in &sheets {
        if !seen.insert(s.name.to_lowercase()) {
            return Err(bad(format!("two sheets are named `{}` (names are not case-sensitive)", s.name)));
        }
    }
    Ok(Workbook { sheets })
}

// ---- CSV

fn csv_field(c: &Cell, out: &mut String) {
    let text = match c {
        Cell::Empty => return,
        Cell::Num(n) => {
            out.push_str(n);
            return;
        }
        Cell::Bool(b) => {
            out.push_str(if *b { "TRUE" } else { "FALSE" });
            return;
        }
        Cell::Text(t) | Cell::Formula(t) => t.as_str(),
    };
    let danger = text.starts_with(['=', '+', '-', '@', '\t', '\r']);
    let needs_quote = danger || text.contains([',', '"', '\n', '\r']) || text.starts_with(' ') || text.ends_with(' ');
    if !needs_quote {
        out.push_str(text);
        return;
    }
    out.push('"');
    if danger {
        out.push('\'');
    }
    out.push_str(&text.replace('"', "\"\""));
    out.push('"');
}

/// One sheet as CSV (a workbook of several cannot be). Formulas are written as the value cells they sit in: none.
pub fn to_csv(wb: &Workbook) -> Result<String, DocError> {
    let [s] = wb.sheets.as_slice() else { return Err(bad("a .csv holds one sheet; give one sheet, or write an .xlsx")) };
    if !s.formulas.is_empty() {
        return Err(bad("a .csv cannot hold formulas; write an .xlsx, or put the computed values in `rows`"));
    }
    let mut out = String::new();
    let header: Vec<Cell> = s.columns.iter().map(|c| Cell::Text(c.clone())).collect();
    for row in std::iter::once(&header).chain(s.rows.iter()) {
        for j in 0..s.columns.len() {
            if j > 0 {
                out.push(',');
            }
            csv_field(row.get(j).unwrap_or(&Cell::Empty), &mut out);
        }
        out.push_str("\r\n");
    }
    Ok(out)
}

// ---- XLSX

/// Remove characters XML 1.0 cannot carry.
fn xml_text(s: &str) -> String {
    escape(&s.chars().filter(|c| matches!(*c, '\t' | '\n' | '\r') || (*c >= ' ' && *c != '\u{fffe}' && *c != '\u{ffff}')).collect::<String>())
}

fn sheet_xml(s: &Sheet, style_of_format: &BTreeMap<String, usize>) -> String {
    let mut grid: BTreeMap<(u32, u32), Cell> = BTreeMap::new();
    for (j, c) in s.columns.iter().enumerate() {
        grid.insert((1, j as u32 + 1), Cell::Text(c.clone()));
    }
    for (i, r) in s.rows.iter().enumerate() {
        for (j, c) in r.iter().enumerate() {
            if *c != Cell::Empty {
                grid.insert((i as u32 + 2, j as u32 + 1), c.clone());
            }
        }
    }
    for (pos, f) in &s.formulas {
        grid.insert(*pos, Cell::Formula(f.clone()));
    }
    let (max_row, max_col) = grid.keys().fold((1, 1), |(r, c), (rr, cc)| (r.max(*rr), c.max(*cc)));
    let mut x = String::with_capacity(grid.len() * 40 + 1024);
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">");
    x.push_str(&format!("<dimension ref=\"A1:{}{}\"/>", col_letters(max_col), max_row));
    x.push_str("<sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/><selection pane=\"bottomLeft\"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/>");
    if !s.widths.is_empty() {
        x.push_str("<cols>");
        for (j, w) in s.widths.iter().enumerate() {
            x.push_str(&format!("<col min=\"{n}\" max=\"{n}\" width=\"{w}\" customWidth=\"1\"/>", n = j + 1, w = w));
        }
        x.push_str("</cols>");
    }
    x.push_str("<sheetData>");
    let mut cur = 0u32;
    for ((row, col), cell) in &grid {
        if *row != cur {
            if cur != 0 {
                x.push_str("</row>");
            }
            x.push_str(&format!("<row r=\"{}\">", row));
            cur = *row;
        }
        let r = format!("{}{}", col_letters(*col), row);
        let style = if *row == 1 && (*col as usize) <= s.columns.len() {
            1
        } else {
            s.formats.get(*col as usize - 1).and_then(|f| style_of_format.get(f)).copied().unwrap_or(0)
        };
        let s_attr = if style > 0 { format!(" s=\"{}\"", style) } else { String::new() };
        match cell {
            Cell::Empty => {}
            Cell::Text(t) => x.push_str(&format!("<c r=\"{}\"{} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", r, s_attr, xml_text(t))),
            Cell::Num(n) => x.push_str(&format!("<c r=\"{}\"{}><v>{}</v></c>", r, s_attr, n)),
            Cell::Bool(b) => x.push_str(&format!("<c r=\"{}\"{} t=\"b\"><v>{}</v></c>", r, s_attr, *b as u8)),
            Cell::Formula(f) => x.push_str(&format!("<c r=\"{}\"{}><f>{}</f></c>", r, s_attr, xml_text(f))),
        }
    }
    if cur != 0 {
        x.push_str("</row>");
    }
    x.push_str("</sheetData></worksheet>");
    x
}

fn styles_xml(formats: &[String]) -> String {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">");
    if !formats.is_empty() {
        x.push_str(&format!("<numFmts count=\"{}\">", formats.len()));
        for (i, f) in formats.iter().enumerate() {
            x.push_str(&format!("<numFmt numFmtId=\"{}\" formatCode=\"{}\"/>", 164 + i, xml_text(f)));
        }
        x.push_str("</numFmts>");
    }
    x.push_str("<fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font><font><b/><sz val=\"11\"/><name val=\"Calibri\"/></font></fonts>");
    x.push_str("<fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills>");
    x.push_str("<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>");
    x.push_str("<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>");
    x.push_str(&format!("<cellXfs count=\"{}\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/><xf numFmtId=\"0\" fontId=\"1\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\"/>", 2 + formats.len()));
    for i in 0..formats.len() {
        x.push_str(&format!("<xf numFmtId=\"{}\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/>", 164 + i));
    }
    x.push_str("</cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>");
    x
}

pub fn to_xlsx(wb: &Workbook) -> Result<Vec<u8>, DocError> {
    let mut formats: Vec<String> = vec![];
    for s in &wb.sheets {
        for f in s.formats.iter().filter(|f| !f.is_empty()) {
            if !formats.contains(f) {
                formats.push(f.clone());
            }
        }
    }
    // style 0 is plain, 1 is the header; each distinct format follows
    let style_of: BTreeMap<String, usize> = formats.iter().enumerate().map(|(i, f)| (f.clone(), 2 + i)).collect();
    let mut sheets_xml = vec![];
    let mut ct = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>");
    let mut book = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><bookViews><workbookView/></bookViews><sheets>");
    let mut rels = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for (i, s) in wb.sheets.iter().enumerate() {
        let n = i + 1;
        ct.push_str(&format!("<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>", n));
        book.push_str(&format!("<sheet name=\"{}\" sheetId=\"{n}\" r:id=\"rId{n}\"/>", xml_text(&s.name), n = n));
        rels.push_str(&format!("<Relationship Id=\"rId{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{n}.xml\"/>", n = n));
        sheets_xml.push(sheet_xml(s, &style_of));
    }
    let st = wb.sheets.len() + 1;
    rels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>", st));
    ct.push_str("</Types>");
    book.push_str("</sheets><calcPr calcId=\"191029\" fullCalcOnLoad=\"1\"/></workbook>");
    let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>";
    // [Content_Types].xml first, as packages conventionally have it
    let mut out = ZipWriter::new();
    out.add("[Content_Types].xml", ct.as_bytes())?;
    out.add("_rels/.rels", root_rels.as_bytes())?;
    out.add("xl/workbook.xml", book.as_bytes())?;
    out.add("xl/_rels/workbook.xml.rels", rels.as_bytes())?;
    out.add("xl/styles.xml", styles_xml(&formats).as_bytes())?;
    for (i, x) in sheets_xml.iter().enumerate() {
        out.add(&format!("xl/worksheets/sheet{}.xml", i + 1), x.as_bytes())?;
    }
    Ok(out.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{extract, Limits, Request};
    use serde_json::json;

    fn wb() -> Value {
        json!({"sheets": [
            {"name": "Latency", "columns": ["release", "p50", "p95", "ok"], "rows": [["0.2.1", 120, 340.5, true], ["0.2.2", 104, 255, false], ["0.2.3", null, 248, true]],
             "widths": [12, 8, 8, 6], "formats": ["", "#,##0", "0.0", ""]},
            {"name": "Totals", "columns": ["what", "n"], "rows": [["rows", 3]], "formulas": [{"cell": "B3", "f": "=SUM(Latency!B2:B4)"}]}
        ]})
    }

    #[test]
    fn an_xlsx_is_a_valid_package_our_own_reader_reads_back_with_its_types_and_formulas() {
        let w = from_json(&wb()).unwrap();
        let bytes = to_xlsx(&w).unwrap();
        assert!(bytes.starts_with(b"PK"));
        assert_eq!(crate::detect(&bytes, "x.xlsx"), crate::Format::Zip, "sniffed as a zip; the package contents make it a workbook");
        let e = extract(&bytes, "x.xlsx", &Request::default(), &Limits::default()).unwrap();
        for want in ["Latency", "Totals", "release", "0.2.1", "340.5", "104", "TRUE", "FALSE", "(=SUM(Latency!B2:B4))"] {
            assert!(e.text.contains(want), "{}: {}", want, e.text);
        }
        // a named sheet can be selected
        let t = extract(&bytes, "x.xlsx", &Request { sheet: Some("Totals".into()), ..Request::default() }, &Limits::default()).unwrap();
        assert!(t.text.contains("rows") && !t.text.contains("0.2.1"), "{}", t.text);
    }

    #[test]
    fn the_same_workbook_is_the_same_bytes() {
        let w = from_json(&wb()).unwrap();
        assert_eq!(to_xlsx(&w).unwrap(), to_xlsx(&w).unwrap());
    }

    #[test]
    fn text_is_never_a_formula_and_csv_neutralises_what_a_program_would_evaluate() {
        let evil = json!({"columns": ["a", "b"], "rows": [["=HYPERLINK(\"http://e.com\",\"x\")", "+1+1"], ["@SUM(1)", "-2+3"], ["tab\there", "line\nbreak"], ["a,b", "say \"hi\""], [-5, 1.5]]});
        let w = from_json(&evil).unwrap();
        let x = to_xlsx(&w).unwrap();
        let e = extract(&x, "x.xlsx", &Request::default(), &Limits::default()).unwrap();
        assert!(e.text.contains("HYPERLINK") && !e.text.contains("(=HYPERLINK"), "text stays text in xlsx: {}", e.text);
        let csv = to_csv(&w).unwrap();
        assert_eq!(csv, "a,b\r\n\"'=HYPERLINK(\"\"http://e.com\"\",\"\"x\"\")\",\"'+1+1\"\r\n\"'@SUM(1)\",\"'-2+3\"\r\ntab\there,\"line\nbreak\"\r\n\"a,b\",\"say \"\"hi\"\"\"\r\n-5,1.5\r\n");
        // and reads back as the same table (apostrophes aside)
        let (cols, rows) = crate::read_table(csv.as_bytes(), 100).unwrap();
        assert_eq!(cols, ["a", "b"]);
        assert_eq!(rows.len(), 5);
        assert_eq!((rows[4][0].as_f64(), rows[4][1].as_f64()), (Some(-5.0), Some(1.5)));
    }

    #[test]
    fn formulas_that_reach_outside_the_workbook_are_refused() {
        for f in ["HYPERLINK(\"http://e.com\")", "=WEBSERVICE(\"http://e.com\")", "'[book.xlsx]Sheet1'!A1", "cmd|' /c calc'!A0", "IMPORTXML(A1,B1)", "A1+file://x", "SUM(A1:A3)\\x", "{=A1}", "", "   "] {
            assert!(check_formula(f).is_err(), "{} was accepted", f);
        }
        assert_eq!(check_formula("=SUM(B2:B9)*1.2").unwrap(), "SUM(B2:B9)*1.2");
        assert_eq!(check_formula("IF(A1<=B1,A1<>B1,\"x\")").unwrap(), "IF(A1<=B1,A1<>B1,\"x\")");
    }

    #[test]
    fn bad_workbooks_are_refused_with_a_reason() {
        for (v, expect) in [
            (json!({}), "give `sheets`"),
            (json!({"sheets": []}), "between 1 and 20"),
            (json!({"sheets": [{"name": "a:b", "columns": ["x"], "rows": []}]}), "cannot contain"),
            (json!({"sheets": [{"name": "A", "columns": ["x"], "rows": []}, {"name": "a", "columns": ["x"], "rows": []}]}), "two sheets"),
            (json!({"columns": ["x"], "rows": [[1, 2]]}), "has 2 values but there are 1 columns"),
            (json!({"columns": ["x"], "rows": [[[1]]]}), "list or object"),
            (json!({"columns": ["x"], "rows": [[1]], "formulas": [{"cell": "A1", "f": "1"}]}), "header row"),
            (json!({"columns": ["x"], "rows": [[1]], "formulas": [{"cell": "B2", "f": "1"}, {"cell": "b2", "f": "2"}]}), "two formulas"),
            (json!({"columns": ["x"], "rows": [[1]], "formulas": [{"cell": "ZZZZ9", "f": "1"}]}), "not a cell reference"),
            (json!({"columns": ["x"], "rows": [[1]], "formats": ["[Red]0"]}), "not allowed"),
            (json!({"columns": ["x"], "rows": [[1]], "widths": [0]}), "from 1 to 255"),
            (json!({"columns": ["x"], "rows": [[1]], "widths": [5, 5]}), "at most one entry"),
            (json!({"columns": ["x"], "rows": [[1]], "formulas": [{"cell": "B2", "f": "WEBSERVICE(\"x\")"}]}), "outside the workbook"),
        ] {
            let e = from_json(&v).unwrap_err();
            assert!(e.0.contains(expect), "{} -> {}", v, e);
        }
        let big = json!({"columns": ["x"], "rows": vec![json!([1]); MAX_ROWS + 1]});
        assert!(from_json(&big).unwrap_err().0.contains("50000"));
        // a csv holds one sheet and no formulas
        assert!(to_csv(&from_json(&wb()).unwrap()).unwrap_err().0.contains("one sheet"));
        let one = from_json(&json!({"columns": ["x"], "rows": [[1]], "formulas": [{"cell": "B2", "f": "1"}]})).unwrap();
        assert!(to_csv(&one).unwrap_err().0.contains("formulas"));
    }

    #[test]
    fn xml_unsafe_text_is_escaped_and_control_characters_are_dropped() {
        let w = from_json(&json!({"name": "A&B", "columns": ["<x>"], "rows": [["a\u{0}b\u{1}</t><c>&\"'"]]})).unwrap();
        let x = to_xlsx(&w).unwrap();
        let e = extract(&x, "x.xlsx", &Request::default(), &Limits::default()).unwrap();
        assert!(e.text.contains("A&B") && e.text.contains("<x>") && e.text.contains("ab</t><c>&\"'"), "{}", e.text);
    }

    #[test]
    fn cell_references_parse_within_limits() {
        assert_eq!(parse_cell_ref("C12"), Some((12, 3)));
        assert_eq!(parse_cell_ref("aa1"), Some((1, 27)));
        for r in ["", "A", "1", "A0", "1A", "A1B", "GGG1", "A-1", "A1000000"] {
            assert_eq!(parse_cell_ref(r), None, "{}", r);
        }
    }

    /// `DELPHI_DOCS_SHEET_OUT=/dir cargo test -p delphi-docs write_sheet_samples -- --ignored`, then open the file in
    /// Excel or LibreOffice (an operator-gated check, spec §10).
    #[test]
    #[ignore = "writes files for a spreadsheet program to open"]
    fn write_sheet_samples() {
        let Ok(dir) = std::env::var("DELPHI_DOCS_SHEET_OUT") else { return };
        std::fs::write(format!("{}/sample.xlsx", dir), to_xlsx(&from_json(&wb()).unwrap()).unwrap()).unwrap();
    }
}
