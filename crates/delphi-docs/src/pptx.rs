//! PowerPoint decks as text: per slide, the title, body text, tables, picture alt text and speaker notes
//! (`specs/014` §4). Layouts, animations and the pictures themselves are not read.

use crate::ooxml::Pkg;
use crate::sniff::Format;
use crate::xml::{attr, walk, Ev};
use crate::{DocError, Extract, Request, Unit};

#[derive(Default)]
struct Slide {
    title: String,
    body: String,
    images: usize,
    described: usize,
}

#[derive(Default)]
struct Tbl {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
}

/// Placeholders that carry no content of their own: slide number, footer, date, header, the slide image on a notes page.
fn is_chrome(ph: &str) -> bool {
    matches!(ph, "sldNum" | "ftr" | "dt" | "hdr" | "sldImg")
}

fn read_slide(xml: &[u8], max_nodes: usize) -> (Slide, Option<DocError>) {
    let mut s = Slide::default();
    let (mut is_title, mut chrome, mut in_p, mut in_t) = (false, false, false, false);
    let (mut lvl, mut para) = (0usize, String::new());
    let mut tbls: Vec<Tbl> = vec![];
    let mut stack: Vec<String> = vec![];
    let err = walk(xml, max_nodes, |ev| match ev {
        Ev::Start { name, attrs } => {
            let parent = stack.last().cloned().unwrap_or_default();
            stack.push(name.clone());
            match name.as_str() {
                "sp" => {
                    is_title = false;
                    chrome = false;
                }
                "ph" => {
                    let t = attr(&attrs, "type").unwrap_or("body");
                    is_title = matches!(t, "title" | "ctrTitle");
                    chrome = is_chrome(t);
                }
                "p" => {
                    in_p = true;
                    para.clear();
                    lvl = 0;
                }
                "pPr" => lvl = attr(&attrs, "lvl").and_then(|l| l.parse().ok()).unwrap_or(0),
                "t" if parent == "r" || parent == "fld" => in_t = true,
                "br" => para.push(' '),
                "tbl" => tbls.push(Tbl::default()),
                "tr" => {
                    if let Some(t) = tbls.last_mut() {
                        t.row = vec![];
                    }
                }
                "tc" => {
                    if let Some(t) = tbls.last_mut() {
                        t.cell.clear();
                    }
                }
                "cNvPr" if parent == "nvPicPr" => {
                    s.images += 1;
                    if let Some(d) = attr(&attrs, "descr").filter(|d| !d.trim().is_empty()) {
                        s.described += 1;
                        s.body.push_str(&format!("[image: {}]\n", d.trim()));
                    }
                }
                _ => {}
            }
        }
        Ev::Text(t) => {
            if in_t && in_p {
                para.push_str(&t);
            }
        }
        Ev::End { name } => {
            stack.pop();
            match name.as_str() {
                "t" => in_t = false,
                "p" if in_p => {
                    in_p = false;
                    let text = para.trim().to_string();
                    if let Some(t) = tbls.last_mut() {
                        if !text.is_empty() {
                            if !t.cell.is_empty() {
                                t.cell.push(' ');
                            }
                            t.cell.push_str(&text);
                        }
                    } else if !text.is_empty() && !chrome {
                        if is_title {
                            if !s.title.is_empty() {
                                s.title.push(' ');
                            }
                            s.title.push_str(&text);
                        } else if lvl > 0 {
                            s.body.push_str(&format!("{}- {}\n", "  ".repeat(lvl.min(6) - 1), text));
                        } else {
                            s.body.push_str(&text);
                            s.body.push('\n');
                        }
                    }
                }
                "tc" => {
                    if let Some(t) = tbls.last_mut() {
                        let c = std::mem::take(&mut t.cell);
                        t.row.push(c);
                    }
                }
                "tr" => {
                    if let Some(t) = tbls.last_mut() {
                        let r = std::mem::take(&mut t.row);
                        t.rows.push(r);
                    }
                }
                "tbl" => {
                    if let Some(t) = tbls.pop() {
                        let cols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
                        for (i, r) in t.rows.iter().enumerate() {
                            s.body.push('|');
                            for c in 0..cols {
                                s.body.push_str(&format!(" {} |", r.get(c).map(|x| x.replace('|', "\\|")).unwrap_or_default()));
                            }
                            s.body.push('\n');
                            if i == 0 {
                                s.body.push_str(&format!("{}\n", "|---".repeat(cols) + "|"));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    });
    (s, err.err())
}

pub fn extract(pkg: &Pkg, req: &Request) -> Result<Extract, DocError> {
    let xml = pkg.part("ppt/presentation.xml")?.ok_or_else(|| DocError("the deck has no ppt/presentation.xml".into()))?;
    let rels = pkg.rels("ppt/presentation.xml");
    let mut order = vec![];
    let _ = walk(&xml, pkg.limits.max_nodes, |ev| {
        if let Ev::Start { name, attrs } = ev {
            if name == "sldId" {
                if let Some(r) = attr(&attrs, "r:id").and_then(|id| rels.get(id)) {
                    order.push(r.target.clone());
                }
            }
        }
    });
    if order.is_empty() {
        return Err(DocError("the deck lists no slides".into()));
    }
    let total = order.len();
    // Slides are numbered from 1; `start`/`end` are slide numbers.
    let first = req.start.unwrap_or(1).max(1);
    let last = req.end.unwrap_or(total).min(total);
    if first > total {
        return Err(DocError(format!("the deck has {} slides; start is {}", total, first)));
    }
    let mut text = String::new();
    let mut loss = vec![];
    let (mut images, mut described, mut shown_to) = (0, 0, first.saturating_sub(1));
    let cap = pkg.limits.max_output;
    let mut truncated = false;
    for n in first..=last {
        let path = &order[n - 1];
        let Some(bytes) = pkg.part(path)? else {
            loss.push(format!("slide {} points at a missing part", n));
            continue;
        };
        let (slide, err) = read_slide(&bytes, pkg.limits.max_nodes);
        if let Some(e) = err {
            loss.push(format!("slide {} stops early: {}", n, e));
        }
        images += slide.images;
        described += slide.described;
        let notes_path = pkg.rels(path).into_values().find(|r| r.kind == "notesSlide").map(|r| r.target);
        let notes = match notes_path.and_then(|p| pkg.part(&p).ok().flatten()) {
            Some(b) => {
                let (ns, _) = read_slide(&b, pkg.limits.max_nodes);
                let t = format!("{}{}", if ns.title.is_empty() { String::new() } else { format!("{}\n", ns.title) }, ns.body);
                t.trim().to_string()
            }
            None => String::new(),
        };
        let mut block = format!("## Slide {}{}\n\n{}", n, if slide.title.is_empty() { String::new() } else { format!(": {}", slide.title) }, slide.body);
        if !notes.is_empty() {
            block.push_str(&format!("\nNotes: {}\n", notes.replace('\n', " ")));
        }
        block.push('\n');
        if !text.is_empty() && text.len() + block.len() > cap {
            truncated = true;
            break;
        }
        text.push_str(&block);
        shown_to = n;
    }
    if shown_to < total {
        text.push_str(&format!("(slides {}-{} of {} shown; pass start/end to page)\n", first, shown_to, total));
        truncated = truncated || shown_to < last;
    }
    if images > described {
        loss.push(format!("{} picture(s) without alt text", images - described));
    }
    Ok(Extract { format: Format::Pptx, text, unit: Unit::Slide, total_units: Some(total), truncated, loss, supported: true })
}
