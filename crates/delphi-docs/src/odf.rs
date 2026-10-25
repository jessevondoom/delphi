//! OpenDocument text, spreadsheets and presentations (`specs/014` §4): the same results as Word, Excel and
//! PowerPoint files, from `content.xml`. Macros are never run; tracked-change history and images are not read.

use crate::ooxml::Pkg;
use crate::sniff::Format;
use crate::table::{parse_range, Collector, Range, DEFAULT_ROWS};
use crate::xml::{attr, walk, Ev};
use crate::{DocError, Extract, Request, Unit};

fn content(pkg: &Pkg) -> Result<Vec<u8>, DocError> {
    pkg.part("content.xml")?.ok_or_else(|| DocError("the OpenDocument file has no content.xml".into()))
}

#[derive(Default)]
struct Tbl {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
}

fn render_table(t: &Tbl) -> String {
    let cols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (i, r) in t.rows.iter().enumerate() {
        out.push('|');
        for c in 0..cols {
            out.push_str(&format!(" {} |", r.get(c).map(|s| s.replace('|', "\\|")).unwrap_or_default()));
        }
        out.push('\n');
        if i == 0 {
            out.push_str(&format!("{}\n", "|---".repeat(cols) + "|"));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------- text

pub fn odt(pkg: &Pkg) -> Result<Extract, DocError> {
    let xml = content(pkg)?;
    let mut out = String::new();
    let mut notes: Vec<String> = vec![];
    // Paragraphs can nest (a footnote's paragraph sits inside the paragraph that cites it), so each open
    // paragraph or heading has its own buffer.
    let mut pstack: Vec<(String, usize)> = vec![];
    let (mut list_depth, mut in_note_body, mut skip_depth, mut ignore_text) = (0usize, false, 0usize, false);
    let mut tbls: Vec<Tbl> = vec![];
    let mut links: Vec<Option<String>> = vec![];
    let (mut images, mut prev_list) = (0usize, false);
    let err = walk(&xml, pkg.limits.max_nodes, |ev| match ev {
        Ev::Start { name, attrs } => {
            if skip_depth > 0 {
                skip_depth += 1;
                return;
            }
            match name.as_str() {
                // deleted text lives here; the visible text is in the body
                "tracked-changes" | "automatic-styles" | "styles" | "scripts" | "font-face-decls" => skip_depth = 1,
                "h" | "p" => {
                    let level = if name == "h" { attr(&attrs, "outline-level").and_then(|l| l.parse().ok()).unwrap_or(1).clamp(1, 6) } else { 0 };
                    pstack.push((String::new(), level));
                }
                "s" => {
                    if let Some(t) = pstack.last_mut() {
                        t.0.push_str(&" ".repeat(attr(&attrs, "c").and_then(|c| c.parse().ok()).unwrap_or(1).clamp(1, 100)));
                    }
                }
                "tab" => {
                    if let Some(t) = pstack.last_mut() {
                        t.0.push('\t');
                    }
                }
                "line-break" => {
                    if let Some(t) = pstack.last_mut() {
                        t.0.push(if tbls.is_empty() { '\n' } else { ' ' });
                    }
                }
                "list" => list_depth += 1,
                "a" => {
                    let href = attr(&attrs, "href").filter(|h| !h.is_empty() && !h.starts_with('#') && !h.to_ascii_lowercase().starts_with("javascript:"));
                    if let (Some(_), Some(t)) = (href, pstack.last_mut()) {
                        t.0.push('[');
                    }
                    links.push(href.map(String::from));
                }
                "note" => {
                    let n = notes.len() + 1;
                    if let Some(t) = pstack.last_mut() {
                        t.0.push_str(&format!("[^{}]", n));
                    }
                }
                "note-citation" => ignore_text = true,
                "note-body" => {
                    in_note_body = true;
                    notes.push(String::new());
                }
                "image" => images += 1,
                "table" => tbls.push(Tbl::default()),
                "table-row" => {
                    if let Some(t) = tbls.last_mut() {
                        t.row = vec![];
                    }
                }
                "table-cell" => {
                    if let Some(t) = tbls.last_mut() {
                        t.cell.clear();
                    }
                }
                _ => {}
            }
        }
        Ev::Text(t) => {
            if skip_depth == 0 && !ignore_text {
                if let Some(top) = pstack.last_mut() {
                    top.0.push_str(&t);
                }
            }
        }
        Ev::End { name } => {
            if skip_depth > 0 {
                skip_depth -= 1;
                return;
            }
            match name.as_str() {
                "h" | "p" => {
                    let Some((buf, level)) = pstack.pop() else { return };
                    let text = buf.trim_end().to_string();
                    if text.is_empty() {
                        return;
                    }
                    if in_note_body {
                        if let Some(n) = notes.last_mut() {
                            if !n.is_empty() {
                                n.push(' ');
                            }
                            n.push_str(&text);
                        }
                    } else if let Some(t) = tbls.last_mut() {
                        if !t.cell.is_empty() {
                            t.cell.push(' ');
                        }
                        t.cell.push_str(&text);
                    } else if let Some(outer) = pstack.last_mut() {
                        // a paragraph inside a paragraph (a text box): fold it into the outer one
                        outer.0.push_str(&text);
                    } else if list_depth > 0 {
                        out.push_str(&format!("{}- {}\n", "  ".repeat(list_depth - 1), text));
                        prev_list = true;
                    } else {
                        if prev_list {
                            out.push('\n');
                            prev_list = false;
                        }
                        if level > 0 {
                            out.push_str(&format!("{} {}\n\n", "#".repeat(level), text));
                        } else {
                            out.push_str(&format!("{}\n\n", text));
                        }
                    }
                }
                "list" => list_depth = list_depth.saturating_sub(1),
                "note-citation" => ignore_text = false,
                "note-body" => in_note_body = false,
                "a" => {
                    if let Some(Some(h)) = links.pop() {
                        if let Some(t) = pstack.last_mut() {
                            t.0.push_str(&format!("]({})", h));
                        }
                    }
                }
                "table-cell" => {
                    if let Some(t) = tbls.last_mut() {
                        let c = std::mem::take(&mut t.cell);
                        t.row.push(c);
                    }
                }
                "table-row" => {
                    if let Some(t) = tbls.last_mut() {
                        let r = std::mem::take(&mut t.row);
                        t.rows.push(r);
                    }
                }
                "table" => {
                    if let Some(t) = tbls.pop() {
                        let md = render_table(&t);
                        if let Some(o) = tbls.last_mut() {
                            o.cell.push_str(&md.lines().filter(|l| !l.starts_with("|---")).collect::<Vec<_>>().join(" "));
                        } else if !md.is_empty() {
                            if prev_list {
                                out.push('\n');
                                prev_list = false;
                            }
                            out.push_str(&md);
                            out.push('\n');
                        }
                    }
                }
                _ => {}
            }
        }
    });
    let mut text = out.trim_end().to_string();
    if !notes.is_empty() {
        text.push_str("\n\n## Notes\n\n");
        for (i, n) in notes.iter().enumerate() {
            text.push_str(&format!("[^{}]: {}\n", i + 1, n));
        }
    }
    let mut loss = vec![];
    if images > 0 {
        loss.push(format!("{} image(s) not read", images));
    }
    if let Err(e) = err {
        loss.push(format!("the document text stops early: {}", e));
    }
    Ok(Extract { format: Format::Odt, text: text.trim_end().to_string(), unit: Unit::Line, total_units: None, truncated: false, loss, supported: true })
}

// ---------------------------------------------------------------------------------------------- spreadsheets

struct SheetData {
    name: String,
    cells: Collector,
}

fn read_sheets(xml: &[u8], range_for: impl Fn(usize) -> Range, max_nodes: usize) -> (Vec<SheetData>, Option<DocError>) {
    let mut sheets: Vec<SheetData> = vec![];
    let (mut row, mut col) = (0u32, 0u32);
    let (mut rep_row, mut rep_col) = (1u32, 1u32);
    let (mut row_cells, mut in_table) = (Vec::<(u32, String)>::new(), false);
    let (mut cell_text, mut cell_attr, mut formula) = (String::new(), String::new(), String::new());
    let (mut in_p, mut first_p) = (false, true);
    let err = walk(xml, max_nodes, |ev| match ev {
        Ev::Start { name, attrs } => match name.as_str() {
            "table" if !in_table => {
                in_table = true;
                let i = sheets.len();
                sheets.push(SheetData { name: attr(&attrs, "name").unwrap_or("?").to_string(), cells: Collector::new(range_for(i)) });
                row = 0;
            }
            "table-row" => {
                row += 1;
                col = 0;
                rep_row = attr(&attrs, "number-rows-repeated").and_then(|r| r.parse().ok()).unwrap_or(1u32).clamp(1, 1_048_576);
                row_cells.clear();
            }
            "table-cell" | "covered-table-cell" => {
                col += 1;
                rep_col = attr(&attrs, "number-columns-repeated").and_then(|r| r.parse().ok()).unwrap_or(1u32).clamp(1, 16_384);
                cell_text.clear();
                first_p = true;
                formula = attr(&attrs, "formula").map(|f| f.trim_start_matches("of:").trim_start_matches('=').to_string()).unwrap_or_default();
                cell_attr = ["value", "date-value", "time-value", "boolean-value", "string-value"].iter().find_map(|k| attr(&attrs, k).map(String::from)).unwrap_or_default();
            }
            "p" => {
                in_p = true;
                if !first_p && !cell_text.is_empty() {
                    cell_text.push(' ');
                }
                first_p = false;
            }
            "s" if in_p => cell_text.push(' '),
            _ => {}
        },
        Ev::Text(t) => {
            if in_p {
                cell_text.push_str(&t);
            }
        }
        Ev::End { name } => match name.as_str() {
            "p" => in_p = false,
            "table-cell" | "covered-table-cell" => {
                let shown = if cell_text.trim().is_empty() { cell_attr.clone() } else { cell_text.trim().to_string() };
                let text = match (shown.is_empty(), formula.is_empty()) {
                    (_, true) => shown,
                    (true, false) => format!("(={})", formula),
                    (false, false) => format!("{} (={})", shown, formula),
                };
                if !text.is_empty() {
                    // a repeated non-empty cell is repeated a few times at most
                    for k in 0..rep_col.min(20) {
                        row_cells.push((col + k, text.clone()));
                    }
                }
                col += rep_col - 1;
            }
            "table-row" => {
                if !row_cells.is_empty() {
                    if let Some(s) = sheets.last_mut() {
                        for r in 0..rep_row.min(50) {
                            for (c, t) in &row_cells {
                                s.cells.push(row + r, *c, t.clone());
                            }
                        }
                    }
                }
                row += rep_row - 1;
            }
            "table" => in_table = false,
            _ => {}
        },
    });
    (sheets, err.err())
}

pub fn ods(pkg: &Pkg, req: &Request) -> Result<Extract, DocError> {
    let xml = content(pkg)?;
    let user_range = match &req.range {
        Some(r) => Some(parse_range(r).ok_or_else(|| DocError(format!("`{}` is not a range; use A1:F200, 5:20 or B:D", r)))?),
        None => None,
    };
    // first pass: the sheet names, so a selection can be resolved
    let (names, _) = {
        let (s, e) = read_sheets(&xml, |_| Range::rows(1, 1), pkg.limits.max_nodes);
        (s.into_iter().map(|s| s.name).collect::<Vec<_>>(), e)
    };
    if names.is_empty() {
        return Err(DocError("the spreadsheet has no sheets".into()));
    }
    let list = || names.iter().enumerate().map(|(i, n)| format!("{}: {}", i + 1, n)).collect::<Vec<_>>().join(", ");
    let selected = match &req.sheet {
        Some(w) => Some(names.iter().position(|n| n.eq_ignore_ascii_case(w)).or_else(|| w.parse::<usize>().ok().filter(|n| (1..=names.len()).contains(n)).map(|n| n - 1)).ok_or_else(|| DocError(format!("no sheet `{}`; the spreadsheet has {}", w, list())))?),
        None if names.len() == 1 => Some(0),
        None => None,
    };
    if selected.is_none() && user_range.is_some() {
        return Err(DocError(format!("name a sheet to use a range; the spreadsheet has {}", list())));
    }
    let want = user_range.unwrap_or(Range::rows(1, DEFAULT_ROWS));
    let (sheets, err) = read_sheets(&xml, |i| if selected == Some(i) { want } else if selected.is_some() { Range::rows(1, 1) } else { Range::rows(1, 6) }, pkg.limits.max_nodes);
    let mut loss = vec![];
    if let Some(e) = err {
        loss.push(format!("the spreadsheet stops early: {}", e));
    }
    let mut text = String::new();
    match selected {
        Some(i) => {
            let (t, l) = sheets[i].cells.render(&format!("## Sheet {}: {}", i + 1, sheets[i].name), None);
            text.push_str(&t);
            loss.extend(l);
        }
        None => {
            text.push_str(&format!("Spreadsheet with {} sheets: {}\n\n", sheets.len(), list()));
            for (i, s) in sheets.iter().enumerate().take(10) {
                let (t, l) = s.cells.render(&format!("## Sheet {}: {}", i + 1, s.name), None);
                text.push_str(&t);
                text.push('\n');
                loss.extend(l);
            }
            text.push_str("Pass `sheet` (name or number) and `range` (for example A1:F200) to read more.\n");
        }
    }
    Ok(Extract { format: Format::Ods, text, unit: Unit::Sheet, total_units: Some(names.len()), truncated: false, loss, supported: true })
}

// ---------------------------------------------------------------------------------------------- presentations

#[derive(Default)]
struct Slide {
    name: String,
    title: String,
    body: Vec<String>,
    notes: Vec<String>,
}

pub fn odp(pkg: &Pkg, req: &Request) -> Result<Extract, DocError> {
    let xml = content(pkg)?;
    let mut slides: Vec<Slide> = vec![];
    let (mut in_page, mut in_notes, mut title_frame, mut list_depth) = (false, false, false, 0usize);
    let (mut p, mut in_p) = (String::new(), false);
    let mut images = 0usize;
    let err = walk(&xml, pkg.limits.max_nodes, |ev| match ev {
        Ev::Start { name, attrs } => match name.as_str() {
            "page" => {
                in_page = true;
                slides.push(Slide { name: attr(&attrs, "name").unwrap_or("").to_string(), ..Slide::default() });
            }
            "notes" => in_notes = true,
            "frame" => title_frame = attr(&attrs, "class") == Some("title"),
            "list" => list_depth += 1,
            "image" => images += 1,
            "p" if in_page && !in_p => {
                in_p = true;
                p.clear();
            }
            "s" if in_p => p.push(' '),
            "tab" if in_p => p.push('\t'),
            "line-break" if in_p => p.push(' '),
            _ => {}
        },
        Ev::Text(t) => {
            if in_p {
                p.push_str(&t);
            }
        }
        Ev::End { name } => match name.as_str() {
            "p" if in_p => {
                in_p = false;
                let text = p.trim().to_string();
                if text.is_empty() {
                    return;
                }
                if let Some(s) = slides.last_mut() {
                    if in_notes {
                        s.notes.push(text);
                    } else if title_frame {
                        if !s.title.is_empty() {
                            s.title.push(' ');
                        }
                        s.title.push_str(&text);
                    } else if list_depth > 0 {
                        s.body.push(format!("{}- {}", "  ".repeat(list_depth - 1), text));
                    } else {
                        s.body.push(text);
                    }
                }
            }
            "list" => list_depth = list_depth.saturating_sub(1),
            "frame" => title_frame = false,
            "notes" => in_notes = false,
            "page" => in_page = false,
            _ => {}
        },
    });
    if slides.is_empty() {
        return Err(DocError("the presentation has no slides".into()));
    }
    let total = slides.len();
    let first = req.start.unwrap_or(1).max(1);
    let last = req.end.unwrap_or(total).min(total);
    if first > total {
        return Err(DocError(format!("the presentation has {} slides; start is {}", total, first)));
    }
    let mut text = String::new();
    let mut shown_to = first - 1;
    for n in first..=last {
        let s = &slides[n - 1];
        let title = if s.title.is_empty() { s.name.clone() } else { s.title.clone() };
        let mut block = format!("## Slide {}{}\n\n", n, if title.is_empty() { String::new() } else { format!(": {}", title) });
        for l in &s.body {
            block.push_str(l);
            block.push('\n');
        }
        if !s.notes.is_empty() {
            block.push_str(&format!("\nNotes: {}\n", s.notes.join(" ")));
        }
        block.push('\n');
        if shown_to >= first && text.len() + block.len() > pkg.limits.max_output {
            break;
        }
        text.push_str(&block);
        shown_to = n;
    }
    if shown_to < total {
        text.push_str(&format!("(slides {}-{} of {} shown; pass start/end to page)\n", first, shown_to, total));
    }
    let mut loss = vec![];
    if images > 0 {
        loss.push(format!("{} image(s) not read", images));
    }
    if let Err(e) = err {
        loss.push(format!("the presentation stops early: {}", e));
    }
    Ok(Extract { format: Format::Odp, text, unit: Unit::Slide, total_units: Some(total), truncated: shown_to < last, loss, supported: true })
}
