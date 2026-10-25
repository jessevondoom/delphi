//! Text from PDF pages: a small content-stream interpreter that tracks the text and graphics matrices well
//! enough to put line breaks and word spaces in the right places (`specs/014` §4).
//!
//! It reads text in the order it is drawn. It does not reorder columns, interpret rotated text specially, run
//! any action, or read images, annotations and form fields; pages with no text are reported, not guessed at.

use crate::pdf::{Dict, Doc, Lex, Obj, Page, Tok};
use crate::pdffont::{Font, Glyph};
use crate::sniff::Format;
use crate::{DocError, Extract, Limits, Request, Unit};
use std::collections::HashMap;
use std::rc::Rc;

type M = [f64; 6];
const ID: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `a` applied first, then `b`.
fn mul(a: &M, b: &M) -> M {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn translate(tx: f64, ty: f64) -> M {
    [1.0, 0.0, 0.0, 1.0, tx, ty]
}

enum Op {
    Num(f64),
    Str(Vec<u8>),
    Name(String),
    Arr(Vec<Op>),
    Other,
}

struct Last {
    end_x: f64,
    y: f64,
}

struct Interp<'a, 'd> {
    doc: &'a Doc<'d>,
    fonts: &'a mut HashMap<u32, Rc<Font>>,
    out: String,
    images: usize,
    unmapped: usize,
    ctm: M,
    gstack: Vec<M>,
    font: Option<Rc<Font>>,
    size: f64,
    tc: f64,
    tw: f64,
    th: f64,
    tl: f64,
    tm: M,
    tlm: M,
    last: Option<Last>,
    budget: usize,
    cap: usize,
    truncated: bool,
    forms: Vec<u32>,
    max_depth: usize,
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

fn is_ws(c: u8) -> bool {
    matches!(c, 0 | 9 | 10 | 12 | 13 | 32)
}

/// Move past an inline image (`BI ... ID <binary> EI`) so its data is not read as operators.
fn skip_inline_image(lx: &mut Lex) {
    let b = lx.b;
    let mut p = lx.pos;
    let data_start = loop {
        match find(b, b"ID", p) {
            Some(i) if i > 0 && is_ws(b[i - 1]) && b.get(i + 2).is_none_or(|&c| is_ws(c)) => break i + 3,
            Some(i) => p = i + 2,
            None => {
                lx.pos = b.len();
                return;
            }
        }
    };
    let mut p = data_start.min(b.len());
    loop {
        match find(b, b"EI", p) {
            Some(i) if i > 0 && is_ws(b[i - 1]) && b.get(i + 2).is_none_or(|&c| is_ws(c)) => {
                lx.pos = i + 2;
                return;
            }
            Some(i) => p = i + 2,
            None => {
                lx.pos = b.len();
                return;
            }
        }
    }
}

impl Interp<'_, '_> {
    fn run(&mut self, content: &[u8], res: &Dict, depth: usize) {
        let mut lx = Lex::new(content);
        let mut ops: Vec<Op> = vec![];
        loop {
            if self.budget == 0 || self.out.len() > self.cap {
                self.truncated = true;
                return;
            }
            self.budget -= 1;
            match lx.next() {
                Tok::Eof => break,
                Tok::Int(i) => ops.push(Op::Num(i as f64)),
                Tok::Real(r) => ops.push(Op::Num(r)),
                Tok::Str(s) => ops.push(Op::Str(s)),
                Tok::Name(n) => ops.push(Op::Name(n)),
                Tok::ArrOpen => {
                    let mut items = vec![];
                    loop {
                        match lx.next() {
                            Tok::ArrClose | Tok::Eof => break,
                            Tok::Int(i) => items.push(Op::Num(i as f64)),
                            Tok::Real(r) => items.push(Op::Num(r)),
                            Tok::Str(s) => items.push(Op::Str(s)),
                            _ => {}
                        }
                    }
                    ops.push(Op::Arr(items));
                }
                Tok::DictOpen => {
                    let mut nest = 1;
                    while nest > 0 {
                        match lx.next() {
                            Tok::DictOpen => nest += 1,
                            Tok::DictClose => nest -= 1,
                            Tok::Eof => break,
                            _ => {}
                        }
                    }
                    ops.push(Op::Other);
                }
                Tok::ArrClose | Tok::DictClose => {}
                Tok::Kw(op) => {
                    if op == "BI" {
                        skip_inline_image(&mut lx);
                        self.images += 1;
                    } else {
                        self.exec(&op, &ops, res, depth);
                    }
                    ops.clear();
                }
            }
            if ops.len() > 64 {
                ops.drain(..ops.len() - 64);
            }
        }
    }

    fn exec(&mut self, op: &str, ops: &[Op], res: &Dict, depth: usize) {
        let num = |i: usize| match ops.get(i) {
            Some(Op::Num(n)) => Some(*n),
            _ => None,
        };
        match op {
            "q" => {
                if self.gstack.len() < 64 {
                    self.gstack.push(self.ctm);
                }
            }
            "Q" => {
                if let Some(m) = self.gstack.pop() {
                    self.ctm = m;
                }
            }
            "cm" => {
                if let [Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)] = [num(0), num(1), num(2), num(3), num(4), num(5)] {
                    self.ctm = mul(&[a, b, c, d, e, f], &self.ctm);
                }
            }
            "BT" => {
                self.tm = ID;
                self.tlm = ID;
            }
            "Tf" => {
                if let Some(Op::Name(n)) = ops.first() {
                    self.font = self.font_for(res, n);
                }
                if let Some(s) = num(1) {
                    self.size = s;
                }
            }
            "Tc" => self.tc = num(0).unwrap_or(0.0),
            "Tw" => self.tw = num(0).unwrap_or(0.0),
            "Tz" => self.th = num(0).unwrap_or(100.0) / 100.0,
            "TL" => self.tl = num(0).unwrap_or(0.0),
            "Td" | "TD" => {
                if let (Some(tx), Some(ty)) = (num(0), num(1)) {
                    if op == "TD" {
                        self.tl = -ty;
                    }
                    self.tlm = mul(&translate(tx, ty), &self.tlm);
                    self.tm = self.tlm;
                }
            }
            "Tm" => {
                if let [Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)] = [num(0), num(1), num(2), num(3), num(4), num(5)] {
                    self.tm = [a, b, c, d, e, f];
                    self.tlm = self.tm;
                }
            }
            "T*" => self.next_line(),
            "Tj" => {
                if let Some(Op::Str(s)) = ops.first() {
                    self.show(s);
                }
            }
            "'" => {
                self.next_line();
                if let Some(Op::Str(s)) = ops.first() {
                    self.show(s);
                }
            }
            "\"" => {
                self.tw = num(0).unwrap_or(self.tw);
                self.tc = num(1).unwrap_or(self.tc);
                self.next_line();
                if let Some(Op::Str(s)) = ops.get(2) {
                    self.show(s);
                }
            }
            "TJ" => {
                if let Some(Op::Arr(items)) = ops.first() {
                    for it in items {
                        match it {
                            Op::Str(s) => self.show(s),
                            Op::Num(n) => {
                                let adv = -n / 1000.0 * self.size * self.th;
                                self.tm = mul(&translate(adv, 0.0), &self.tm);
                            }
                            _ => {}
                        }
                    }
                }
            }
            "Do" => {
                if let Some(Op::Name(n)) = ops.first() {
                    self.xobject(res, n, depth);
                }
            }
            _ => {}
        }
    }

    fn next_line(&mut self) {
        self.tlm = mul(&translate(0.0, -self.tl), &self.tlm);
        self.tm = self.tlm;
    }

    fn font_for(&mut self, res: &Dict, name: &str) -> Option<Rc<Font>> {
        let fonts = self.doc.lookup(res, "Font");
        let entry = fonts.dict()?.get(name)?.clone();
        if let Obj::Ref(n) = entry {
            if let Some(f) = self.fonts.get(&n) {
                return Some(f.clone());
            }
            let d = self.doc.deref(&entry);
            let f = Rc::new(Font::load(self.doc, d.dict()?));
            if self.fonts.len() < 4096 {
                self.fonts.insert(n, f.clone());
            }
            Some(f)
        } else {
            Some(Rc::new(Font::load(self.doc, entry.dict()?)))
        }
    }

    fn xobject(&mut self, res: &Dict, name: &str, depth: usize) {
        let xo = self.doc.lookup(res, "XObject");
        let Some(entry) = xo.dict().and_then(|d| d.get(name)).cloned() else { return };
        let resolved = self.doc.deref(&entry);
        let Obj::Stream(s) = &*resolved else { return };
        match self.doc.lookup(&s.dict, "Subtype").name() {
            Some("Image") => self.images += 1,
            Some("Form") => {
                let id = if let Obj::Ref(n) = entry { Some(n) } else { None };
                if depth >= self.max_depth || id.is_some_and(|n| self.forms.contains(&n)) {
                    return;
                }
                let Ok((data, _)) = self.doc.decode(s) else { return };
                let m = self.doc.lookup(&s.dict, "Matrix");
                let matrix = match m.arr() {
                    Some(a) if a.len() == 6 => {
                        let v: Vec<f64> = a.iter().map(|o| self.doc.deref(o).num().unwrap_or(0.0)).collect();
                        [v[0], v[1], v[2], v[3], v[4], v[5]]
                    }
                    _ => ID,
                };
                let fres = self.doc.lookup(&s.dict, "Resources").dict().cloned().unwrap_or_else(|| res.clone());
                if let Some(n) = id {
                    self.forms.push(n);
                }
                let (saved_ctm, saved_font, saved_size) = (self.ctm, self.font.clone(), self.size);
                self.ctm = mul(&matrix, &self.ctm);
                self.run(&data, &fres, depth + 1);
                self.ctm = saved_ctm;
                self.font = saved_font;
                self.size = saved_size;
                if id.is_some() {
                    self.forms.pop();
                }
            }
            _ => {}
        }
    }

    fn show(&mut self, bytes: &[u8]) {
        let Some(font) = self.font.clone() else { return };
        for g in font.glyphs(bytes) {
            self.place(&g);
        }
    }

    fn place(&mut self, g: &Glyph) {
        let trm = mul(&self.tm, &self.ctm);
        let (x, y) = (trm[4], trm[5]);
        let size = (self.size * trm[2].hypot(trm[3])).abs().max(0.5);
        if !g.text.is_empty() {
            if let Some(l) = &self.last {
                let dy = (y - l.y).abs();
                if dy > 0.5 * size {
                    if !self.out.is_empty() && !self.out.ends_with('\n') {
                        self.out.push('\n');
                    }
                } else {
                    let gap = x - l.end_x;
                    if (gap > 0.2 * size || gap < -3.0 * size) && !self.out.ends_with([' ', '\n']) {
                        self.out.push(' ');
                    }
                }
            }
            self.out.push_str(&g.text);
        } else if !g.mapped {
            self.unmapped += 1;
        }
        let adv = (g.width / 1000.0 * self.size + self.tc + if g.space { self.tw } else { 0.0 }) * self.th;
        self.tm = mul(&translate(adv, 0.0), &self.tm);
        let t2 = mul(&self.tm, &self.ctm);
        self.last = Some(Last { end_x: t2[4], y });
    }
}

/// Ligatures to letters, soft hyphens and no-break spaces out, trailing blanks off, blank runs collapsed.
fn tidy(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{fb00}' => s.push_str("ff"),
            '\u{fb01}' => s.push_str("fi"),
            '\u{fb02}' => s.push_str("fl"),
            '\u{fb03}' => s.push_str("ffi"),
            '\u{fb04}' => s.push_str("ffl"),
            '\u{ad}' => {}
            '\u{a0}' => s.push(' '),
            c => s.push(c),
        }
    }
    let mut out = String::with_capacity(s.len());
    let mut blank = 0;
    for line in s.lines() {
        let l = line.trim();
        if l.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(l);
    }
    out
}

struct PageText {
    text: String,
    images: usize,
    unmapped: usize,
    truncated: bool,
    problem: Option<String>,
}

fn page_text(doc: &Doc, fonts: &mut HashMap<u32, Rc<Font>>, page: &Page, budget: &mut usize, limits: &Limits) -> PageText {
    let mut content = Vec::new();
    let mut problem = None;
    let mut streams = vec![];
    match doc.lookup(&page.dict, "Contents").as_ref() {
        Obj::Stream(s) => streams.push(s.clone()),
        Obj::Arr(a) => {
            for o in a.iter().take(10_000) {
                if let Obj::Stream(s) = &*doc.deref(o) {
                    streams.push(s.clone());
                }
            }
        }
        _ => {}
    }
    for s in streams {
        match doc.decode(&s) {
            Ok((d, _)) => {
                content.extend_from_slice(&d);
                content.push(b'\n');
            }
            Err(e) => problem = Some(e.0),
        }
        if content.len() > limits.max_expanded {
            problem = Some("the page content is too large".into());
            break;
        }
    }
    let res = page.resources.clone().unwrap_or_default();
    let mut it = Interp {
        doc,
        fonts,
        out: String::new(),
        images: 0,
        unmapped: 0,
        ctm: ID,
        gstack: vec![],
        font: None,
        size: 0.0,
        tc: 0.0,
        tw: 0.0,
        th: 1.0,
        tl: 0.0,
        tm: ID,
        tlm: ID,
        last: None,
        budget: *budget,
        cap: limits.max_output * 2,
        truncated: false,
        forms: vec![],
        max_depth: limits.max_depth,
    };
    it.run(&content, &res, 0);
    *budget = it.budget;
    PageText { text: tidy(&it.out), images: it.images, unmapped: it.unmapped, truncated: it.truncated, problem }
}

/// A PDF text string: UTF-16BE with a byte-order mark, or PDFDocEncoding (taken as Latin-1).
fn pdf_string(b: &[u8]) -> String {
    if b.starts_with(&[0xfe, 0xff]) {
        String::from_utf16_lossy(&b[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>())
    } else {
        b.iter().map(|&c| c as char).collect()
    }
}

fn list(pages: &[usize]) -> String {
    let shown: Vec<String> = pages.iter().take(20).map(|p| p.to_string()).collect();
    if pages.len() > 20 { format!("{} and {} more", shown.join(", "), pages.len() - 20) } else { shown.join(", ") }
}

pub fn extract(bytes: &[u8], req: &Request, limits: &Limits) -> Result<Extract, DocError> {
    let doc = Doc::open(bytes, limits)?;
    let pages = doc.pages();
    let total = pages.len();
    if total == 0 {
        return Err(DocError("the PDF has no readable pages".into()));
    }
    // Pages are numbered from 1; `start`/`end` are page numbers.
    let first = req.start.unwrap_or(1).max(1);
    let last = req.end.unwrap_or(total).min(total);
    if first > total {
        return Err(DocError(format!("the PDF has {} pages; start is {}", total, first)));
    }
    let info = doc.lookup(&doc.trailer, "Info");
    let meta = |k: &str| info.dict().and_then(|d| doc.lookup(d, k).bytes().map(pdf_string)).filter(|s| !s.trim().is_empty());
    let mut head = format!("PDF · {} page{}", total, if total == 1 { "" } else { "s" });
    for (label, key) in [("Title", "Title"), ("Author", "Author")] {
        if let Some(v) = meta(key) {
            head.push_str(&format!(" · {}: {}", label, v.trim().chars().take(120).collect::<String>()));
        }
    }
    let mut text = format!("{}\n\n", head);
    let mut fonts = HashMap::new();
    let mut budget = limits.max_nodes;
    let (mut empty, mut unmapped_pages, mut images) = (vec![], vec![], 0usize);
    let (mut unmapped, mut shown_to, mut truncated) = (0usize, first - 1, false);
    let mut loss = vec![];
    for n in first..=last {
        let pt = page_text(&doc, &mut fonts, &pages[n - 1], &mut budget, limits);
        images += pt.images;
        unmapped += pt.unmapped;
        if pt.unmapped > 0 {
            unmapped_pages.push(n);
        }
        if pt.text.is_empty() {
            empty.push(n);
        }
        if let Some(p) = pt.problem {
            loss.push(format!("page {}: part of the content could not be decoded ({})", n, p));
        }
        if pt.truncated {
            loss.push(format!("page {}: reading stopped at a size limit", n));
        }
        let block = format!("## Page {}\n\n{}\n\n", n, if pt.text.is_empty() { "(no text)" } else { &pt.text });
        if shown_to >= first && text.len() + block.len() > limits.max_output {
            truncated = true;
            break;
        }
        text.push_str(&block);
        shown_to = n;
    }
    if shown_to < total {
        text.push_str(&format!("(pages {}-{} of {} shown; pass start/end to page)\n", first, shown_to, total));
    }
    if doc.rebuilt {
        loss.push("the cross-reference table was damaged and was rebuilt by scanning the file".to_string());
    }
    if !empty.is_empty() {
        loss.push(format!("page(s) {}: no text layer (scanned or image-only pages need OCR, which is not built in)", list(&empty)));
    }
    if unmapped > 0 {
        loss.push(format!("{} character(s) on page(s) {} use fonts with no Unicode map and were skipped", unmapped, list(&unmapped_pages)));
    }
    if images > 0 {
        loss.push(format!("{} image(s) not read", images));
    }
    Ok(Extract { format: Format::Pdf, text, unit: Unit::Page, total_units: Some(total), truncated: truncated || shown_to < last, loss, supported: true })
}
