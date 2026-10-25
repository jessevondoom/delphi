//! Word documents to Markdown: headings, lists, tables, links, footnotes, comments and tracked changes
//! (`specs/014` §4). Images, text boxes and equations are not read, and the result says how many.

use crate::ooxml::Pkg;
use crate::sniff::Format;
use crate::xml::{attr, walk, Ev};
use crate::{DocError, Extract, Unit};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Body,
    Notes,
    Comments,
}

#[derive(Default)]
struct Tbl {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
}

#[derive(Default)]
struct Counts {
    images: usize,
    described: usize,
    text_boxes: usize,
    equations: usize,
}

struct W<'a> {
    mode: Mode,
    styles: &'a HashMap<String, String>,
    links: &'a HashMap<String, String>,
    out: String,
    stack: Vec<String>,
    skip: Option<usize>,
    p: String,
    in_p: bool,
    lvl: usize,
    list: Option<usize>,
    prev_list: bool,
    collecting: bool,
    marks: Vec<&'static str>,
    link_ends: Vec<Option<String>>,
    tbls: Vec<Tbl>,
    label: Option<String>,
    counts: Counts,
}

fn heading_level(style_id: &str, styles: &HashMap<String, String>) -> usize {
    let name = styles.get(style_id).cloned().unwrap_or_else(|| style_id.to_lowercase());
    if name == "title" {
        return 1;
    }
    name.strip_prefix("heading").map(|r| r.trim()).and_then(|d| d.parse::<usize>().ok()).filter(|&n| n >= 1).map(|n| n.min(6)).unwrap_or(0)
}

impl<'a> W<'a> {
    fn new(mode: Mode, styles: &'a HashMap<String, String>, links: &'a HashMap<String, String>) -> W<'a> {
        W { mode, styles, links, out: String::new(), stack: vec![], skip: None, p: String::new(), in_p: false, lvl: 0, list: None, prev_list: false, collecting: false, marks: vec![], link_ends: vec![], tbls: vec![], label: None, counts: Counts::default() }
    }

    fn in_cell(&self) -> bool {
        !self.tbls.is_empty()
    }

    fn sep(&self) -> &'static str {
        if self.mode == Mode::Body { "\n\n" } else { "\n" }
    }

    fn block(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        let sep = self.sep();
        if let Some(prefix) = self.label.take() {
            self.out.push_str(&prefix);
            self.out.push_str(&text);
            self.out.push('\n');
            return;
        }
        if let Some(l) = self.list {
            if !self.prev_list && !self.out.is_empty() && self.mode == Mode::Body {
                self.out.push('\n');
            }
            self.out.push_str(&format!("{}- {}\n", "  ".repeat(l.min(6)), text));
            self.prev_list = true;
        } else {
            if self.prev_list && self.mode == Mode::Body {
                self.out.push('\n');
            }
            if self.lvl > 0 {
                self.out.push_str(&format!("{} {}{}", "#".repeat(self.lvl), text, sep));
            } else {
                self.out.push_str(&format!("{}{}", text, sep));
            }
            self.prev_list = false;
        }
    }

    fn render_table(t: &Tbl) -> String {
        let cols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if cols == 0 {
            return String::new();
        }
        let cell = |s: &str| s.replace('|', "\\|").replace('\n', " ");
        let mut out = String::new();
        for (i, r) in t.rows.iter().enumerate() {
            out.push('|');
            for c in 0..cols {
                out.push_str(&format!(" {} |", r.get(c).map(|s| cell(s)).unwrap_or_default()));
            }
            out.push('\n');
            if i == 0 {
                out.push_str(&format!("{}\n", "|---".repeat(cols) + "|"));
            }
        }
        out
    }

    fn event(&mut self, ev: Ev) {
        match ev {
            Ev::Text(t) => {
                if self.skip.is_none() && self.collecting {
                    self.p.push_str(&t);
                }
            }
            Ev::Start { name, attrs } => {
                let parent = self.stack.last().cloned().unwrap_or_default();
                self.stack.push(name.clone());
                if self.skip.is_some() {
                    return;
                }
                let val = attr(&attrs, "val").unwrap_or("").to_string();
                match name.as_str() {
                    "txbxContent" => {
                        self.skip = Some(self.stack.len());
                        self.counts.text_boxes += 1;
                    }
                    "Fallback" => self.skip = Some(self.stack.len()),
                    "oMathPara" | "oMath" => {
                        if name == "oMathPara" || parent != "oMathPara" {
                            self.counts.equations += 1;
                            self.p.push_str("[equation]");
                        }
                        self.skip = Some(self.stack.len());
                    }
                    "footnote" | "endnote" | "comment" => {
                        let kind = attr(&attrs, "type");
                        if kind.is_some_and(|k| k != "normal") {
                            self.skip = Some(self.stack.len());
                            return;
                        }
                        let id = attr(&attrs, "id").unwrap_or("?");
                        self.label = Some(if name == "comment" { format!("- **{}** (#{}): ", attr(&attrs, "author").unwrap_or("unknown"), id) } else { format!("[^{}]: ", id) });
                    }
                    "p" if !self.in_p => {
                        self.p.clear();
                        self.in_p = true;
                        self.lvl = 0;
                        self.list = None;
                    }
                    "pStyle" => self.lvl = heading_level(&val, self.styles),
                    "numPr" if parent == "pPr" => self.list = Some(0),
                    "ilvl" if self.list.is_some() => self.list = Some(val.parse().unwrap_or(0)),
                    "t" | "delText" if parent == "r" => self.collecting = true,
                    "tab" if parent == "r" => self.p.push('\t'),
                    "br" | "cr" if parent == "r" => self.p.push(if self.in_cell() { ' ' } else { '\n' }),
                    "noBreakHyphen" if parent == "r" => self.p.push('-'),
                    "ins" | "del" => {
                        // In run or paragraph properties these are empty "the paragraph mark was inserted" flags.
                        if parent == "rPr" || parent == "pPr" {
                            self.marks.push("");
                        } else if name == "ins" {
                            self.p.push_str("{+");
                            self.marks.push("+}");
                        } else {
                            self.p.push_str("{-");
                            self.marks.push("-}");
                        }
                    }
                    "hyperlink" => {
                        let url = attr(&attrs, "r:id").and_then(|id| self.links.get(id)).cloned();
                        if url.is_some() {
                            self.p.push('[');
                        }
                        self.link_ends.push(url);
                    }
                    "footnoteReference" => self.p.push_str(&format!("[^{}]", attr(&attrs, "id").unwrap_or("?"))),
                    "endnoteReference" => self.p.push_str(&format!("[^e{}]", attr(&attrs, "id").unwrap_or("?"))),
                    "commentReference" => self.p.push_str(&format!("[comment {}]", attr(&attrs, "id").unwrap_or("?"))),
                    // DrawingML pictures and legacy VML pictures; a VML `pict` that only wraps a text box is not one.
                    "drawing" | "imagedata" => self.counts.images += 1,
                    "docPr" => {
                        if let Some(d) = attr(&attrs, "descr").filter(|d| !d.trim().is_empty()) {
                            self.counts.described += 1;
                            self.p.push_str(&format!("[image: {}]", d.trim()));
                        }
                    }
                    "tbl" => self.tbls.push(Tbl::default()),
                    "tr" => {
                        if let Some(t) = self.tbls.last_mut() {
                            t.row = vec![];
                        }
                    }
                    "tc" => {
                        if let Some(t) = self.tbls.last_mut() {
                            t.cell.clear();
                        }
                    }
                    _ => {}
                }
            }
            Ev::End { name } => {
                self.stack.pop();
                if let Some(n) = self.skip {
                    if self.stack.len() < n {
                        self.skip = None;
                    }
                    return;
                }
                match name.as_str() {
                    "t" | "delText" => self.collecting = false,
                    "ins" | "del" => {
                        let m = self.marks.pop().unwrap_or("");
                        self.p.push_str(m);
                    }
                    "hyperlink" => {
                        if let Some(Some(url)) = self.link_ends.pop() {
                            self.p.push_str(&format!("]({})", url));
                        }
                    }
                    "p" if self.in_p && self.stack.last().map(|s| s.as_str()) != Some("p") => {
                        self.in_p = false;
                        let text = self.p.trim_end().to_string();
                        self.p.clear();
                        if let Some(t) = self.tbls.last_mut() {
                            if !text.is_empty() {
                                if !t.cell.is_empty() {
                                    t.cell.push(' ');
                                }
                                t.cell.push_str(&text);
                            }
                        } else {
                            self.block(text);
                        }
                    }
                    "tc" => {
                        if let Some(t) = self.tbls.last_mut() {
                            let c = std::mem::take(&mut t.cell);
                            t.row.push(c);
                        }
                    }
                    "tr" => {
                        if let Some(t) = self.tbls.last_mut() {
                            let r = std::mem::take(&mut t.row);
                            t.rows.push(r);
                        }
                    }
                    "tbl" => {
                        if let Some(t) = self.tbls.pop() {
                            let md = W::render_table(&t);
                            if let Some(outer) = self.tbls.last_mut() {
                                // A table inside a cell is flattened into the cell's text.
                                let flat = md.lines().filter(|l| !l.starts_with("|---")).collect::<Vec<_>>().join(" ");
                                outer.cell.push_str(&flat);
                            } else if !md.is_empty() {
                                if self.prev_list {
                                    self.out.push('\n');
                                    self.prev_list = false;
                                }
                                self.out.push_str(&md);
                                self.out.push('\n');
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn styles_of(pkg: &Pkg) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(Some(xml)) = pkg.part("word/styles.xml") else { return map };
    let mut cur: Option<String> = None;
    let _ = walk(&xml, pkg.limits.max_nodes, |ev| match ev {
        Ev::Start { name, attrs } if name == "style" => cur = attr(&attrs, "styleId").map(String::from),
        Ev::Start { name, attrs } if name == "name" => {
            if let (Some(id), Some(v)) = (cur.as_ref(), attr(&attrs, "val")) {
                map.insert(id.clone(), v.to_lowercase());
            }
        }
        _ => {}
    });
    map
}

fn run(xml: &[u8], mode: Mode, pkg: &Pkg, styles: &HashMap<String, String>, links: &HashMap<String, String>) -> (String, Counts, Option<DocError>) {
    let mut w = W::new(mode, styles, links);
    let r = walk(xml, pkg.limits.max_nodes, |ev| w.event(ev));
    (w.out, w.counts, r.err())
}

pub fn extract(pkg: &Pkg) -> Result<Extract, DocError> {
    let doc = pkg.part("word/document.xml")?.ok_or_else(|| DocError("the Word document has no word/document.xml".into()))?;
    let styles = styles_of(pkg);
    let links: HashMap<String, String> = pkg.rels("word/document.xml").into_iter().filter(|(_, r)| r.external && r.kind == "hyperlink").map(|(k, r)| (k, r.target)).collect();
    let mut loss = vec![];
    let (mut text, counts, err) = run(&doc, Mode::Body, pkg, &styles, &links);
    if let Some(e) = err {
        loss.push(format!("the document text stops early: {}", e));
    }
    if let Some(xml) = pkg.part("word/footnotes.xml")? {
        let (notes, _, _) = run(&xml, Mode::Notes, pkg, &styles, &links);
        if !notes.trim().is_empty() {
            text.push_str(&format!("\n## Footnotes\n\n{}", notes));
        }
    }
    if let Some(xml) = pkg.part("word/endnotes.xml")? {
        let (notes, _, _) = run(&xml, Mode::Notes, pkg, &styles, &links);
        if !notes.trim().is_empty() {
            text.push_str(&format!("\n## Endnotes\n\n{}", notes.replace("[^", "[^e")));
        }
    }
    if let Some(xml) = pkg.part("word/comments.xml")? {
        let (c, _, _) = run(&xml, Mode::Comments, pkg, &styles, &links);
        if !c.trim().is_empty() {
            text.push_str(&format!("\n## Comments\n\n{}", c));
        }
    }
    let undescribed = counts.images.saturating_sub(counts.described);
    if undescribed > 0 {
        loss.push(format!("{} image(s) without alt text", undescribed));
    }
    if counts.text_boxes > 0 {
        loss.push(format!("{} text box(es) skipped", counts.text_boxes));
    }
    if counts.equations > 0 {
        loss.push(format!("{} equation(s) shown as [equation]", counts.equations));
    }
    let hf = pkg.parts_under("word/header").len() + pkg.parts_under("word/footer").len();
    if hf > 0 {
        loss.push(format!("{} header/footer part(s) not read", hf));
    }
    Ok(Extract { format: Format::Docx, text: text.trim_end().to_string(), unit: Unit::Line, total_units: None, truncated: false, loss, supported: true })
}
