//! The PDF document model: a lexer, an object parser, the cross-reference data (tables, streams, object
//! streams, and a rebuild-by-scan when they are damaged), stream filters, and the page tree (`specs/014` §4, §6).
//!
//! Everything is bounded: objects parsed, nesting, decoded bytes, and the number of pages. References are
//! followed with visited sets, so a cyclic page tree or a stream whose `/Length` points at itself ends cleanly.
//! Nothing in a PDF is executed: JavaScript, launch actions and embedded files are never looked at.

use crate::archive::member_cap;
use crate::{DocError, Limits};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::rc::Rc;

pub type Dict = HashMap<String, Obj>;

#[derive(Debug, Clone)]
pub enum Obj {
    Null,
    /// `true` or `false`; no reader branches on the value yet.
    Bool,
    Int(i64),
    Real(f64),
    Str(Vec<u8>),
    Name(String),
    Arr(Vec<Obj>),
    Dict(Dict),
    Stream(Rc<Stream>),
    Ref(u32),
}

#[derive(Debug)]
pub struct Stream {
    pub dict: Dict,
    pub start: usize,
    pub len: usize,
}

impl Obj {
    pub fn num(&self) -> Option<f64> {
        match self {
            Obj::Int(i) => Some(*i as f64),
            Obj::Real(r) => Some(*r),
            _ => None,
        }
    }
    pub fn int(&self) -> Option<i64> {
        match self {
            Obj::Int(i) => Some(*i),
            Obj::Real(r) => Some(*r as i64),
            _ => None,
        }
    }
    pub fn name(&self) -> Option<&str> {
        match self {
            Obj::Name(n) => Some(n),
            _ => None,
        }
    }
    pub fn dict(&self) -> Option<&Dict> {
        match self {
            Obj::Dict(d) => Some(d),
            Obj::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }
    pub fn arr(&self) -> Option<&[Obj]> {
        match self {
            Obj::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Obj::Str(s) => Some(s),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------- lexer

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Real(f64),
    Name(String),
    Str(Vec<u8>),
    ArrOpen,
    ArrClose,
    DictOpen,
    DictClose,
    Kw(String),
    Eof,
}

pub struct Lex<'a> {
    pub b: &'a [u8],
    pub pos: usize,
}

fn is_ws(c: u8) -> bool {
    matches!(c, 0 | 9 | 10 | 12 | 13 | 32)
}
fn is_delim(c: u8) -> bool {
    matches!(c, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}
fn hexval(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

impl<'a> Lex<'a> {
    pub fn new(b: &'a [u8]) -> Lex<'a> {
        Lex { b, pos: 0 }
    }

    pub fn skip_ws(&mut self) {
        loop {
            while self.pos < self.b.len() && is_ws(self.b[self.pos]) {
                self.pos += 1;
            }
            if self.pos < self.b.len() && self.b[self.pos] == b'%' {
                while self.pos < self.b.len() && self.b[self.pos] != b'\n' && self.b[self.pos] != b'\r' {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn literal_string(&mut self) -> Vec<u8> {
        // self.pos is just past '('
        let mut out = vec![];
        let mut depth = 1;
        while self.pos < self.b.len() {
            let c = self.b[self.pos];
            self.pos += 1;
            match c {
                b'(' => {
                    depth += 1;
                    out.push(c);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(c);
                }
                b'\\' => {
                    let Some(&e) = self.b.get(self.pos) else { break };
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'0'..=b'7' => {
                            let mut v = (e - b'0') as u32;
                            for _ in 0..2 {
                                match self.b.get(self.pos) {
                                    Some(&d @ b'0'..=b'7') => {
                                        v = v * 8 + (d - b'0') as u32;
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(v as u8);
                        }
                        b'\r' => {
                            if self.b.get(self.pos) == Some(&b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        other => out.push(other),
                    }
                }
                _ => out.push(c),
            }
        }
        out
    }

    fn hex_string(&mut self) -> Vec<u8> {
        // self.pos is just past '<'
        let mut out = vec![];
        let mut hi: Option<u8> = None;
        while self.pos < self.b.len() {
            let c = self.b[self.pos];
            self.pos += 1;
            if c == b'>' {
                break;
            }
            if let Some(v) = hexval(c) {
                match hi.take() {
                    Some(h) => out.push(h * 16 + v),
                    None => hi = Some(v),
                }
            }
        }
        if let Some(h) = hi {
            out.push(h * 16);
        }
        out
    }

    pub fn next(&mut self) -> Tok {
        self.skip_ws();
        let Some(&c) = self.b.get(self.pos) else { return Tok::Eof };
        match c {
            b'/' => {
                self.pos += 1;
                let s = self.pos;
                while self.pos < self.b.len() && !is_ws(self.b[self.pos]) && !is_delim(self.b[self.pos]) {
                    self.pos += 1;
                }
                let raw = &self.b[s..self.pos];
                let mut out = Vec::with_capacity(raw.len());
                let mut i = 0;
                while i < raw.len() {
                    if raw[i] == b'#' && hexval(raw.get(i + 1).copied().unwrap_or(0)).is_some() && raw.get(i + 2).and_then(|&d| hexval(d)).is_some() {
                        out.push(hexval(raw[i + 1]).unwrap_or(0) * 16 + hexval(raw[i + 2]).unwrap_or(0));
                        i += 3;
                    } else {
                        out.push(raw[i]);
                        i += 1;
                    }
                }
                Tok::Name(String::from_utf8_lossy(&out).into_owned())
            }
            b'(' => {
                self.pos += 1;
                Tok::Str(self.literal_string())
            }
            b'<' => {
                if self.b.get(self.pos + 1) == Some(&b'<') {
                    self.pos += 2;
                    Tok::DictOpen
                } else {
                    self.pos += 1;
                    Tok::Str(self.hex_string())
                }
            }
            b'>' => {
                if self.b.get(self.pos + 1) == Some(&b'>') {
                    self.pos += 2;
                    Tok::DictClose
                } else {
                    self.pos += 1;
                    Tok::Kw(">".into())
                }
            }
            b'[' => {
                self.pos += 1;
                Tok::ArrOpen
            }
            b']' => {
                self.pos += 1;
                Tok::ArrClose
            }
            b'{' | b'}' | b')' => {
                self.pos += 1;
                Tok::Kw((c as char).to_string())
            }
            _ => {
                let s = self.pos;
                while self.pos < self.b.len() && !is_ws(self.b[self.pos]) && !is_delim(self.b[self.pos]) {
                    self.pos += 1;
                }
                let t = String::from_utf8_lossy(&self.b[s..self.pos]).into_owned();
                if let Ok(i) = t.parse::<i64>() {
                    Tok::Int(i)
                } else if t.bytes().any(|b| b.is_ascii_digit()) && t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.')) {
                    t.parse::<f64>().map(Tok::Real).unwrap_or(Tok::Kw(t))
                } else {
                    Tok::Kw(t)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- parser

/// Parse one object whose first token is `tok`. `budget` is the remaining object allowance.
pub fn parse_obj(lx: &mut Lex, tok: Tok, depth: usize, budget: &mut usize) -> Option<Obj> {
    if *budget == 0 || depth > 64 {
        return None;
    }
    *budget -= 1;
    Some(match tok {
        Tok::Int(i) => {
            let save = lx.pos;
            if i >= 0 {
                if let Tok::Int(g) = lx.next() {
                    if g >= 0 {
                        if let Tok::Kw(k) = lx.next() {
                            if k == "R" {
                                return Some(Obj::Ref(i as u32));
                            }
                        }
                    }
                }
            }
            lx.pos = save;
            Obj::Int(i)
        }
        Tok::Real(r) => Obj::Real(r),
        Tok::Str(s) => Obj::Str(s),
        Tok::Name(n) => Obj::Name(n),
        Tok::ArrOpen => {
            let mut v = vec![];
            loop {
                match lx.next() {
                    Tok::ArrClose | Tok::Eof => break,
                    t => v.push(parse_obj(lx, t, depth + 1, budget)?),
                }
            }
            Obj::Arr(v)
        }
        Tok::DictOpen => {
            let mut d = Dict::new();
            loop {
                match lx.next() {
                    Tok::DictClose | Tok::Eof => break,
                    Tok::Name(k) => {
                        let t = lx.next();
                        if t == Tok::DictClose {
                            break;
                        }
                        let v = parse_obj(lx, t, depth + 1, budget)?;
                        d.entry(k).or_insert(v);
                    }
                    _ => {}
                }
            }
            Obj::Dict(d)
        }
        Tok::Kw(k) => match k.as_str() {
            "true" | "false" => Obj::Bool,
            "null" => Obj::Null,
            _ => return None,
        },
        Tok::ArrClose | Tok::DictClose | Tok::Eof => return None,
    })
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

// ---------------------------------------------------------------------------------------------- document

#[derive(Debug, Clone, Copy)]
enum X {
    Offset(usize),
    InStream { stm: u32, idx: u32 },
}

/// The objects inside an object stream: (object number, the bytes of that object).
type ObjStm = Rc<Vec<(u32, Vec<u8>)>>;

pub struct Doc<'a> {
    pub data: &'a [u8],
    pub limits: &'a Limits,
    xref: HashMap<u32, X>,
    pub trailer: Dict,
    cache: RefCell<HashMap<u32, Rc<Obj>>>,
    objstm: RefCell<HashMap<u32, ObjStm>>,
    budget: Cell<usize>,
    decoded: Cell<usize>,
    depth: Cell<usize>,
    /// The cross-reference data was damaged and rebuilt by scanning the file.
    pub rebuilt: bool,
}

pub struct Page {
    pub dict: Dict,
    pub resources: Option<Dict>,
}

const MAX_PAGES: usize = 100_000;

impl<'a> Doc<'a> {
    pub fn open(data: &'a [u8], limits: &'a Limits) -> Result<Doc<'a>, DocError> {
        if find(&data[..data.len().min(1024)], b"%PDF-", 0).is_none() {
            return Err(DocError("not a PDF (no %PDF- header)".into()));
        }
        let mut d = Doc { data, limits, xref: HashMap::new(), trailer: Dict::new(), cache: RefCell::new(HashMap::new()), objstm: RefCell::new(HashMap::new()), budget: Cell::new(limits.max_nodes), decoded: Cell::new(0), depth: Cell::new(0), rebuilt: false };
        let ok = d.load_xref().is_some() && d.root_ok();
        if !ok {
            d.rebuild();
            d.rebuilt = true;
        }
        if d.trailer.contains_key("Encrypt") {
            return Err(DocError("this PDF is encrypted; encrypted PDFs are not read".into()));
        }
        Ok(d)
    }

    fn root_ok(&self) -> bool {
        let root = self.lookup(&self.trailer, "Root");
        root.dict().is_some_and(|r| r.contains_key("Pages"))
    }

    // ---- cross-reference data

    fn load_xref(&mut self) -> Option<()> {
        let tail_start = self.data.len().saturating_sub(2048);
        let sx = self.data[tail_start..].windows(9).rposition(|w| w == b"startxref")? + tail_start;
        let mut lx = Lex::new(self.data);
        lx.pos = sx + 9;
        let Tok::Int(off) = lx.next() else { return None };
        let mut next = Some(off as usize);
        let mut seen = HashSet::new();
        let mut first = true;
        while let Some(off) = next.take() {
            if off >= self.data.len() || !seen.insert(off) || seen.len() > 1000 {
                return if first { None } else { Some(()) };
            }
            let (entries, trailer) = self.read_xref_section(off)?;
            for (n, e) in entries {
                self.xref.entry(n).or_insert(e);
            }
            // Hybrid files: a table whose trailer also names an xref stream with the compressed objects.
            if let Some(xs) = trailer.get("XRefStm").and_then(|o| o.int()) {
                if let Some((e2, _)) = self.read_xref_section(xs as usize) {
                    for (n, e) in e2 {
                        self.xref.entry(n).or_insert(e);
                    }
                }
            }
            next = trailer.get("Prev").and_then(|o| o.int()).filter(|&p| p >= 0).map(|p| p as usize);
            if first {
                self.trailer = trailer;
                first = false;
            } else {
                for (k, v) in trailer {
                    self.trailer.entry(k).or_insert(v);
                }
            }
        }
        Some(())
    }

    fn read_xref_section(&self, off: usize) -> Option<(Vec<(u32, X)>, Dict)> {
        let mut lx = Lex::new(self.data);
        lx.pos = off;
        lx.skip_ws();
        if self.data[lx.pos..].starts_with(b"xref") {
            lx.pos += 4;
            let mut out = vec![];
            loop {
                match lx.next() {
                    Tok::Int(first) => {
                        let Tok::Int(count) = lx.next() else { return None };
                        if count < 0 || first < 0 || out.len() as u64 + count as u64 > self.limits.max_nodes as u64 {
                            return None;
                        }
                        for i in 0..count {
                            let (Tok::Int(o), Tok::Int(_g), Tok::Kw(kind)) = (lx.next(), lx.next(), lx.next()) else { return None };
                            if kind == "n" && o > 0 {
                                out.push(((first + i) as u32, X::Offset(o as usize)));
                            }
                        }
                    }
                    Tok::Kw(k) if k == "trailer" => break,
                    _ => return None,
                }
            }
            let t = lx.next();
            let mut budget = 100_000;
            let d = match parse_obj(&mut lx, t, 0, &mut budget)? {
                Obj::Dict(d) => d,
                _ => return None,
            };
            Some((out, d))
        } else {
            let (_, obj) = self.parse_indirect_at(off)?;
            let Obj::Stream(s) = obj else { return None };
            if s.dict.get("Type").and_then(|o| o.name()) != Some("XRef") {
                return None;
            }
            let data = self.decode(&s).ok()?.0;
            let w: Vec<usize> = s.dict.get("W")?.arr()?.iter().filter_map(|o| o.int()).map(|v| v.clamp(0, 8) as usize).collect();
            if w.len() != 3 || w.iter().sum::<usize>() == 0 {
                return None;
            }
            let size = s.dict.get("Size").and_then(|o| o.int()).unwrap_or(0).max(0);
            let index: Vec<i64> = match s.dict.get("Index").and_then(|o| o.arr()) {
                Some(a) => a.iter().filter_map(|o| o.int()).collect(),
                None => vec![0, size],
            };
            let rec = w[0] + w[1] + w[2];
            let mut pos = 0;
            let mut out = vec![];
            for pair in index.chunks(2) {
                let [first, count] = pair else { break };
                for i in 0..*count {
                    if pos + rec > data.len() || out.len() > self.limits.max_nodes {
                        break;
                    }
                    let field = |from: usize, n: usize| data[from..from + n].iter().fold(0u64, |a, &b| (a << 8) | b as u64);
                    let t = if w[0] == 0 { 1 } else { field(pos, w[0]) };
                    let f2 = field(pos + w[0], w[1]);
                    let f3 = field(pos + w[0] + w[1], w[2]);
                    pos += rec;
                    let num = (*first + i) as u32;
                    match t {
                        1 => out.push((num, X::Offset(f2 as usize))),
                        2 => out.push((num, X::InStream { stm: f2 as u32, idx: f3 as u32 })),
                        _ => {}
                    }
                }
            }
            Some((out, s.dict.clone()))
        }
    }

    /// Rebuild the object table by scanning for `N G obj`, and find the trailer.
    fn rebuild(&mut self) {
        self.xref.clear();
        self.cache.borrow_mut().clear();
        self.objstm.borrow_mut().clear();
        let b = self.data;
        let mut i = 0;
        while let Some(p) = find(b, b"obj", i) {
            i = p + 3;
            let after_ok = b.get(p + 3).is_none_or(|&c| is_ws(c) || is_delim(c));
            if !after_ok || p == 0 || !is_ws(b[p - 1]) {
                continue;
            }
            // walk back over " gen " and "num"
            let mut q = p - 1;
            while q > 0 && is_ws(b[q]) {
                q -= 1;
            }
            let gen_end = q + 1;
            while q > 0 && b[q].is_ascii_digit() {
                q -= 1;
            }
            if gen_end == q + 1 || !is_ws(b[q]) {
                continue;
            }
            while q > 0 && is_ws(b[q]) {
                q -= 1;
            }
            let num_end = q + 1;
            while q > 0 && b[q].is_ascii_digit() {
                q -= 1;
            }
            let num_start = if b[q].is_ascii_digit() { q } else { q + 1 };
            if num_start >= num_end || num_end - num_start > 10 {
                continue;
            }
            if let Ok(n) = std::str::from_utf8(&b[num_start..num_end]).unwrap_or("").parse::<u32>() {
                self.xref.insert(n, X::Offset(num_start));
            }
            if self.xref.len() > self.limits.max_nodes {
                break;
            }
        }
        // trailers
        let mut trailer = Dict::new();
        let mut i = 0;
        while let Some(p) = find(b, b"trailer", i) {
            i = p + 7;
            let mut lx = Lex::new(b);
            lx.pos = i;
            let t = lx.next();
            let mut budget = 100_000;
            if let Some(Obj::Dict(d)) = parse_obj(&mut lx, t, 0, &mut budget) {
                if d.contains_key("Root") {
                    trailer = d;
                }
            }
        }
        // objects of interest: object streams (their contents), xref streams (a Root), catalogs
        let mut nums: Vec<(u32, usize)> = self.xref.iter().filter_map(|(n, x)| if let X::Offset(o) = x { Some((*n, *o)) } else { None }).collect();
        nums.sort_by_key(|&(_, o)| o);
        let mut catalog = None;
        for (n, off) in nums {
            let Some((_, obj)) = self.parse_indirect_at(off) else { continue };
            let Some(d) = obj.dict() else { continue };
            match d.get("Type").and_then(|o| o.name()) {
                Some("Catalog") => catalog = Some(n),
                Some("XRef") if !trailer.contains_key("Root") && d.contains_key("Root") => {
                    trailer = d.clone();
                }
                Some("ObjStm") => {
                    if let Some(items) = self.load_objstm(n) {
                        for (idx, (m, _)) in items.iter().enumerate() {
                            self.xref.entry(*m).or_insert(X::InStream { stm: n, idx: idx as u32 });
                        }
                    }
                }
                _ => {}
            }
        }
        if !trailer.contains_key("Root") {
            if let Some(c) = catalog {
                trailer.insert("Root".into(), Obj::Ref(c));
            }
        }
        self.trailer = trailer;
    }

    // ---- objects

    /// `N G obj <object> [stream ... endstream]` at `off`. Returns the object number and the object.
    fn parse_indirect_at(&self, off: usize) -> Option<(u32, Obj)> {
        if off >= self.data.len() {
            return None;
        }
        let mut lx = Lex::new(self.data);
        lx.pos = off;
        let (Tok::Int(num), Tok::Int(_), Tok::Kw(k)) = (lx.next(), lx.next(), lx.next()) else { return None };
        if k != "obj" || num < 0 {
            return None;
        }
        let mut budget = self.budget.get();
        let t = lx.next();
        let obj = parse_obj(&mut lx, t, 0, &mut budget);
        self.budget.set(budget);
        let obj = obj?;
        if let Obj::Dict(d) = &obj {
            if lx.next() == Tok::Kw("stream".into()) {
                let mut s = lx.pos;
                if self.data.get(s) == Some(&b'\r') {
                    s += 1;
                }
                if self.data.get(s) == Some(&b'\n') {
                    s += 1;
                }
                let declared = d.get("Length").and_then(|l| match l {
                    Obj::Ref(n) => self.get(*n).and_then(|o| o.int()),
                    o => o.int(),
                });
                let valid = declared.filter(|&l| l >= 0).map(|l| l as usize).filter(|&l| s.checked_add(l).is_some_and(|e| e <= self.data.len() && self.endstream_follows(e)));
                let len = match valid {
                    Some(l) => l,
                    None => {
                        let e = find(self.data, b"endstream", s).unwrap_or(self.data.len());
                        let mut e2 = e;
                        if e2 > s && self.data[e2 - 1] == b'\n' {
                            e2 -= 1;
                        }
                        if e2 > s && self.data[e2 - 1] == b'\r' {
                            e2 -= 1;
                        }
                        e2 - s
                    }
                };
                return Some((num as u32, Obj::Stream(Rc::new(Stream { dict: d.clone(), start: s, len }))));
            }
        }
        Some((num as u32, obj))
    }

    fn endstream_follows(&self, mut e: usize) -> bool {
        while e < self.data.len() && is_ws(self.data[e]) {
            e += 1;
        }
        self.data[e.min(self.data.len())..].starts_with(b"endstream")
    }

    fn load_objstm(&self, stm: u32) -> Option<ObjStm> {
        if let Some(c) = self.objstm.borrow().get(&stm) {
            return Some(c.clone());
        }
        let obj = self.get(stm)?;
        let Obj::Stream(s) = &*obj else { return None };
        let (data, _) = self.decode(s).ok()?;
        let n = s.dict.get("N")?.int()?.clamp(0, self.limits.max_nodes as i64) as usize;
        let first = s.dict.get("First")?.int()?.max(0) as usize;
        let mut lx = Lex::new(&data);
        let mut heads = Vec::with_capacity(n.min(65_536));
        for _ in 0..n {
            let (Tok::Int(num), Tok::Int(off)) = (lx.next(), lx.next()) else { break };
            if num < 0 || off < 0 {
                break;
            }
            heads.push((num as u32, off as usize));
        }
        let mut items = Vec::with_capacity(heads.len());
        for (k, (num, off)) in heads.iter().enumerate() {
            let start = first.saturating_add(*off).min(data.len());
            let end = heads.get(k + 1).map(|h| first.saturating_add(h.1)).unwrap_or(data.len()).clamp(start, data.len());
            items.push((*num, data[start..end].to_vec()));
        }
        let rc = Rc::new(items);
        self.objstm.borrow_mut().insert(stm, rc.clone());
        Some(rc)
    }

    /// Object number `n`, parsed on first use.
    pub fn get(&self, n: u32) -> Option<Rc<Obj>> {
        if let Some(o) = self.cache.borrow().get(&n) {
            return Some(o.clone());
        }
        if self.depth.get() > 24 {
            return None;
        }
        self.depth.set(self.depth.get() + 1);
        let found = match self.xref.get(&n).copied() {
            Some(X::Offset(off)) => self.parse_indirect_at(off).filter(|(m, _)| *m == n).map(|(_, o)| o),
            Some(X::InStream { stm, idx }) => self.load_objstm(stm).and_then(|items| {
                let (m, bytes) = items.get(idx as usize)?;
                if *m != n {
                    return None;
                }
                let mut lx = Lex::new(bytes);
                let mut budget = self.budget.get();
                let t = lx.next();
                let o = parse_obj(&mut lx, t, 0, &mut budget);
                self.budget.set(budget);
                o
            }),
            None => None,
        };
        self.depth.set(self.depth.get() - 1);
        let rc = Rc::new(found?);
        if self.cache.borrow().len() < self.limits.max_nodes {
            self.cache.borrow_mut().insert(n, rc.clone());
        }
        Some(rc)
    }

    /// Follow references until a direct object (bounded). Missing objects are `Null`.
    pub fn deref(&self, o: &Obj) -> Rc<Obj> {
        let mut cur = match o {
            Obj::Ref(n) => self.get(*n).unwrap_or_else(|| Rc::new(Obj::Null)),
            other => return Rc::new(other.clone()),
        };
        for _ in 0..8 {
            match &*cur {
                Obj::Ref(n) => cur = self.get(*n).unwrap_or_else(|| Rc::new(Obj::Null)),
                _ => break,
            }
        }
        cur
    }

    /// `d[key]`, dereferenced; `Null` if absent.
    pub fn lookup(&self, d: &Dict, key: &str) -> Rc<Obj> {
        match d.get(key) {
            Some(o) => self.deref(o),
            None => Rc::new(Obj::Null),
        }
    }

    // ---- streams

    /// The decoded bytes of a stream, and whether decoding hit damage (a truncated or corrupt tail).
    pub fn decode(&self, s: &Stream) -> Result<(Vec<u8>, bool), DocError> {
        let end = s.start.checked_add(s.len).filter(|&e| e <= self.data.len()).ok_or_else(|| DocError("a stream runs past the end of the file".into()))?;
        let mut data = self.data[s.start..end].to_vec();
        let mut damaged = false;
        let filters: Vec<String> = match self.lookup(&s.dict, "Filter").as_ref() {
            Obj::Name(n) => vec![n.clone()],
            Obj::Arr(a) => a.iter().filter_map(|o| self.deref(o).name().map(String::from)).collect(),
            _ => vec![],
        };
        let parms: Vec<Option<Dict>> = match self.lookup(&s.dict, "DecodeParms").as_ref() {
            Obj::Dict(d) => vec![Some(d.clone())],
            Obj::Arr(a) => a.iter().map(|o| self.deref(o).dict().cloned()).collect(),
            _ => vec![],
        };
        for (i, f) in filters.iter().enumerate() {
            let p = parms.get(i).cloned().flatten();
            let cap = member_cap(data.len() as u64, self.limits);
            data = match f.as_str() {
                "FlateDecode" | "Fl" => {
                    let (out, d) = inflate(&data, cap)?;
                    damaged |= d;
                    self.predict(out, p.as_ref())
                }
                "LZWDecode" | "LZW" => {
                    let early = p.as_ref().and_then(|d| d.get("EarlyChange")).and_then(|o| o.int()).unwrap_or(1) != 0;
                    let out = lzw(&data, early, cap)?;
                    self.predict(out, p.as_ref())
                }
                "ASCII85Decode" | "A85" => ascii85(&data, cap)?,
                "ASCIIHexDecode" | "AHx" => ascii_hex(&data),
                "RunLengthDecode" | "RL" => run_length(&data, cap)?,
                "Crypt" => data,
                other => return Err(DocError(format!("stream filter {} is not supported", other))),
            };
            let total = self.decoded.get().saturating_add(data.len());
            self.decoded.set(total);
            if total > self.limits.max_expanded {
                return Err(DocError("the PDF's streams expand past the total limit (decompression-bomb guard)".into()));
            }
        }
        Ok((data, damaged))
    }

    fn predict(&self, data: Vec<u8>, parms: Option<&Dict>) -> Vec<u8> {
        let Some(p) = parms else { return data };
        let get = |k: &str, d: i64| p.get(k).and_then(|o| self.deref(o).int()).unwrap_or(d);
        let predictor = get("Predictor", 1);
        if predictor < 2 {
            return data;
        }
        let colors = get("Colors", 1).clamp(1, 32) as usize;
        let bpc = get("BitsPerComponent", 8).clamp(1, 16) as usize;
        let columns = get("Columns", 1).clamp(1, 1 << 20) as usize;
        let bpp = (colors * bpc).div_ceil(8).max(1);
        let rowlen = (colors * bpc * columns).div_ceil(8);
        if predictor == 2 {
            // TIFF predictor, 8 bits per component only
            if bpc != 8 {
                return data;
            }
            let mut out = data;
            for row in out.chunks_mut(rowlen) {
                for i in bpp..row.len() {
                    row[i] = row[i].wrapping_add(row[i - bpp]);
                }
            }
            return out;
        }
        // PNG predictors: each row is prefixed by a filter byte
        let mut out = Vec::with_capacity(data.len());
        let mut prev = vec![0u8; rowlen];
        for chunk in data.chunks(rowlen + 1) {
            let (ft, row) = (chunk[0], &chunk[1..]);
            let mut cur = row.to_vec();
            cur.resize(rowlen, 0);
            for i in 0..rowlen {
                let a = if i >= bpp { cur[i - bpp] } else { 0 };
                let b = prev[i];
                let c = if i >= bpp { prev[i - bpp] } else { 0 };
                cur[i] = cur[i].wrapping_add(match ft {
                    1 => a,
                    2 => b,
                    3 => ((a as u16 + b as u16) / 2) as u8,
                    4 => {
                        let (pa, pb, pc) = ((b as i32 - c as i32).abs(), (a as i32 - c as i32).abs(), (a as i32 + b as i32 - 2 * c as i32).abs());
                        if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                    }
                    _ => 0,
                });
            }
            out.extend_from_slice(&cur[..row.len().min(rowlen)]);
            prev = cur;
        }
        out
    }

    // ---- pages

    /// The pages in order, with inherited resources. A damaged tree falls back to every `/Type /Page` object.
    pub fn pages(&self) -> Vec<Page> {
        let mut out = vec![];
        let root = self.lookup(&self.trailer, "Root");
        let start = root.dict().and_then(|r| r.get("Pages")).cloned();
        let mut seen = HashSet::new();
        let mut stack: Vec<(Obj, Option<Dict>, usize)> = start.into_iter().map(|p| (p, None, 0)).collect();
        while let Some((node, inherited, depth)) = stack.pop() {
            if depth > 64 || out.len() >= MAX_PAGES {
                continue;
            }
            if let Obj::Ref(n) = node {
                if !seen.insert(n) {
                    continue;
                }
            }
            let resolved = self.deref(&node);
            let Some(d) = resolved.dict() else { continue };
            let res = match self.lookup(d, "Resources").dict() {
                Some(r) => Some(r.clone()),
                None => inherited,
            };
            match self.lookup(d, "Kids").as_ref() {
                Obj::Arr(kids) => {
                    for k in kids.iter().rev() {
                        stack.push((k.clone(), res.clone(), depth + 1));
                    }
                }
                _ => out.push(Page { dict: d.clone(), resources: res }),
            }
        }
        if out.is_empty() {
            let mut nums: Vec<u32> = self.xref.keys().copied().collect();
            nums.sort_unstable();
            for n in nums {
                if out.len() >= MAX_PAGES {
                    break;
                }
                if let Some(o) = self.get(n) {
                    if let Some(d) = o.dict() {
                        if d.get("Type").and_then(|t| t.name()) == Some("Page") {
                            let res = self.lookup(d, "Resources").dict().cloned();
                            out.push(Page { dict: d.clone(), resources: res });
                        }
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------- filters

fn inflate(data: &[u8], cap: usize) -> Result<(Vec<u8>, bool), DocError> {
    let bomb = || DocError(format!("a stream expands past {} bytes (decompression-bomb guard)", cap));
    let mut out = Vec::new();
    let r = flate2::read::ZlibDecoder::new(data).take(cap as u64 + 1).read_to_end(&mut out);
    if out.len() > cap {
        return Err(bomb());
    }
    match r {
        Ok(_) => Ok((out, false)),
        Err(_) if !out.is_empty() => Ok((out, true)),
        Err(_) => {
            // No zlib header, or a bad one: try raw deflate after the two header bytes.
            let mut raw = Vec::new();
            let r2 = flate2::read::DeflateDecoder::new(data.get(2..).unwrap_or(&[])).take(cap as u64 + 1).read_to_end(&mut raw);
            if raw.len() > cap {
                return Err(bomb());
            }
            if raw.is_empty() {
                return Err(DocError("a Flate stream is corrupt".into()));
            }
            Ok((raw, r2.is_err()))
        }
    }
}

fn ascii_hex(data: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    let mut hi = None;
    for &c in data {
        if c == b'>' {
            break;
        }
        if let Some(v) = hexval(c) {
            match hi.take() {
                Some(h) => out.push(h * 16 + v),
                None => hi = Some(v),
            }
        }
    }
    if let Some(h) = hi {
        out.push(h * 16);
    }
    out
}

fn ascii85(data: &[u8], cap: usize) -> Result<Vec<u8>, DocError> {
    let mut out = vec![];
    let (mut group, mut n) = ([0u32; 5], 0usize);
    let data = data.strip_prefix(b"<~").unwrap_or(data);
    for &c in data {
        match c {
            b'~' => break,
            b'z' if n == 0 => out.extend_from_slice(&[0, 0, 0, 0]),
            b'!'..=b'u' => {
                group[n] = (c - b'!') as u32;
                n += 1;
                if n == 5 {
                    let v = group.iter().fold(0u32, |a, &d| a.wrapping_mul(85).wrapping_add(d));
                    out.extend_from_slice(&v.to_be_bytes());
                    n = 0;
                }
            }
            _ => {}
        }
        if out.len() > cap {
            return Err(DocError("an ASCII85 stream is too large".into()));
        }
    }
    if n > 1 {
        for g in group.iter_mut().skip(n) {
            *g = 84;
        }
        let v = group.iter().fold(0u32, |a, &d| a.wrapping_mul(85).wrapping_add(d));
        out.extend_from_slice(&v.to_be_bytes()[..n - 1]);
    }
    Ok(out)
}

fn run_length(data: &[u8], cap: usize) -> Result<Vec<u8>, DocError> {
    let mut out = vec![];
    let mut i = 0;
    while i < data.len() {
        let l = data[i] as usize;
        i += 1;
        if l == 128 {
            break;
        }
        if l < 128 {
            let end = (i + l + 1).min(data.len());
            out.extend_from_slice(&data[i..end]);
            i = end;
        } else if let Some(&b) = data.get(i) {
            out.extend(std::iter::repeat_n(b, 257 - l));
            i += 1;
        }
        if out.len() > cap {
            return Err(DocError("a RunLength stream is too large".into()));
        }
    }
    Ok(out)
}

fn lzw(data: &[u8], early: bool, cap: usize) -> Result<Vec<u8>, DocError> {
    let mut out: Vec<u8> = vec![];
    let mut table: Vec<Vec<u8>> = (0..=255u8).map(|b| vec![b]).chain([vec![], vec![]]).collect();
    let (mut bits, mut nbits, mut width) = (0u32, 0u32, 9u32);
    let mut prev: Option<Vec<u8>> = None;
    for &byte in data {
        bits = (bits << 8) | byte as u32;
        nbits += 8;
        while nbits >= width {
            let code = ((bits >> (nbits - width)) & ((1 << width) - 1)) as usize;
            nbits -= width;
            match code {
                256 => {
                    table.truncate(258);
                    width = 9;
                    prev = None;
                    continue;
                }
                257 => return Ok(out),
                _ => {}
            }
            let entry = if code < table.len() {
                table[code].clone()
            } else if let Some(p) = &prev {
                let mut e = p.clone();
                e.push(p[0]);
                e
            } else {
                return Ok(out);
            };
            out.extend_from_slice(&entry);
            if let Some(p) = prev.take() {
                let mut e = p;
                e.push(entry[0]);
                table.push(e);
            }
            let size = table.len() + usize::from(early);
            width = if size >= 2048 { 12 } else if size >= 1024 { 11 } else if size >= 512 { 10 } else { 9 };
            prev = Some(entry);
            if out.len() > cap {
                return Err(DocError("an LZW stream is too large".into()));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_match_the_published_examples() {
        // PDF 32000-1, 7.4.4.2: this LZW stream decodes to "-----A---B".
        let enc = [0x80, 0x0b, 0x60, 0x50, 0x22, 0x0c, 0x0c, 0x85, 0x01];
        assert_eq!(lzw(&enc, true, 1 << 20).unwrap(), b"-----A---B");
        assert_eq!(ascii85(b"9jqo^~>", 1 << 20).unwrap(), b"Man ");
        assert_eq!(ascii85(b"<~z~>", 1 << 20).unwrap(), [0, 0, 0, 0]);
        assert_eq!(ascii_hex(b"48 65 6C6c 6F7>"), b"Hello\x70");
        assert_eq!(run_length(&[2, b'a', b'b', b'c', 254, b'x', 128], 1 << 20).unwrap(), b"abcxxx");
    }

    #[test]
    fn filters_are_bounded() {
        assert!(run_length(&[129, 0, 129, 0, 129, 0], 1000).is_ok());
        assert!(run_length(&[129, 0].repeat(100), 100).is_err());
        assert!(ascii85(&b"z".repeat(100), 100).is_err());
        assert!(inflate(&[0x78, 0x9c, 0xff, 0xff], 100).is_err());
    }

    #[test]
    fn the_lexer_reads_names_numbers_and_strings_leniently() {
        let mut lx = Lex::new(b"/A#42C 12 -3.5 .5 (a(b)c\\)) <4 8> [1 2] << /K 1 >> true 1.2.3 %comment\n end");
        assert_eq!(lx.next(), Tok::Name("ABC".into()));
        assert_eq!(lx.next(), Tok::Int(12));
        assert_eq!(lx.next(), Tok::Real(-3.5));
        assert_eq!(lx.next(), Tok::Real(0.5));
        assert_eq!(lx.next(), Tok::Str(b"a(b)c)".to_vec()));
        assert_eq!(lx.next(), Tok::Str(vec![0x48]));
        assert_eq!(lx.next(), Tok::ArrOpen);
        assert_eq!((lx.next(), lx.next(), lx.next()), (Tok::Int(1), Tok::Int(2), Tok::ArrClose));
        assert_eq!((lx.next(), lx.next(), lx.next(), lx.next()), (Tok::DictOpen, Tok::Name("K".into()), Tok::Int(1), Tok::DictClose));
        assert_eq!(lx.next(), Tok::Kw("true".into()));
        assert_eq!(lx.next(), Tok::Kw("1.2.3".into()));
        assert_eq!(lx.next(), Tok::Kw("end".into()));
        assert_eq!(lx.next(), Tok::Eof);
    }
}
