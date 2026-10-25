//! PDF fonts, as far as reading text needs them: which character a code stands for, and how wide it is
//! (`specs/014` §4). A `ToUnicode` map wins; otherwise a simple font's `/Encoding` (a named base plus
//! `/Differences`) is used. A composite (CID) font with no `ToUnicode` map cannot be read without parsing the
//! font program, so its characters are reported as unmapped instead of guessed.

use crate::pdf::{Dict, Doc, Lex, Obj, Tok};
use std::collections::HashMap;

pub struct Glyph {
    pub text: String,
    /// In thousandths of an em.
    pub width: f64,
    /// A single-byte code 32, which word spacing applies to.
    pub space: bool,
    pub mapped: bool,
}

#[derive(Default)]
pub struct CMap {
    chars: HashMap<u32, String>,
    /// (lo, hi, destination as UTF-16 units; the last unit grows with the offset into the range)
    ranges: Vec<(u32, u32, Vec<u16>)>,
    /// (lo, hi, bytes per code)
    pub codespaces: Vec<(u32, u32, usize)>,
}

fn code_of(b: &[u8]) -> u32 {
    b.iter().take(4).fold(0u32, |a, &x| (a << 8) | x as u32)
}

fn utf16(b: &[u8]) -> Vec<u16> {
    if b.len() == 1 {
        return vec![b[0] as u16];
    }
    b.chunks(2).map(|c| if c.len() == 2 { u16::from_be_bytes([c[0], c[1]]) } else { c[0] as u16 }).collect()
}

const MAX_CMAP_ENTRIES: usize = 200_000;

impl CMap {
    pub fn parse(data: &[u8]) -> CMap {
        let mut cm = CMap::default();
        let mut lx = Lex::new(data);
        loop {
            match lx.next() {
                Tok::Eof => break,
                Tok::Kw(k) => match k.as_str() {
                    "beginbfchar" => loop {
                        match lx.next() {
                            Tok::Str(src) => {
                                if let Tok::Str(dst) = lx.next() {
                                    cm.chars.insert(code_of(&src), String::from_utf16_lossy(&utf16(&dst)));
                                }
                            }
                            Tok::Kw(_) | Tok::Eof => break,
                            _ => {}
                        }
                        if cm.chars.len() > MAX_CMAP_ENTRIES {
                            return cm;
                        }
                    },
                    "beginbfrange" => loop {
                        match lx.next() {
                            Tok::Str(lo) => {
                                let (Tok::Str(hi), third) = (lx.next(), lx.next()) else { break };
                                let (lo, hi) = (code_of(&lo), code_of(&hi));
                                match third {
                                    Tok::Str(dst) => {
                                        if hi >= lo && cm.ranges.len() < MAX_CMAP_ENTRIES {
                                            cm.ranges.push((lo, hi, utf16(&dst)));
                                        }
                                    }
                                    Tok::ArrOpen => {
                                        let mut c = lo;
                                        loop {
                                            match lx.next() {
                                                Tok::Str(d) => {
                                                    if c <= hi && cm.chars.len() < MAX_CMAP_ENTRIES {
                                                        cm.chars.insert(c, String::from_utf16_lossy(&utf16(&d)));
                                                    }
                                                    c = c.saturating_add(1);
                                                }
                                                Tok::ArrClose | Tok::Eof => break,
                                                _ => {}
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            Tok::Kw(_) | Tok::Eof => break,
                            _ => {}
                        }
                    },
                    "begincodespacerange" => loop {
                        match lx.next() {
                            Tok::Str(lo) => {
                                if let Tok::Str(hi) = lx.next() {
                                    cm.codespaces.push((code_of(&lo), code_of(&hi), lo.len().clamp(1, 4)));
                                }
                            }
                            Tok::Kw(_) | Tok::Eof => break,
                            _ => {}
                        }
                    },
                    _ => {}
                },
                _ => {}
            }
        }
        cm
    }

    pub fn get(&self, code: u32) -> Option<String> {
        if let Some(s) = self.chars.get(&code) {
            return Some(s.clone());
        }
        for (lo, hi, dst) in &self.ranges {
            if code >= *lo && code <= *hi {
                let mut u = dst.clone();
                if let Some(last) = u.last_mut() {
                    *last = last.wrapping_add((code - lo) as u16);
                }
                return Some(String::from_utf16_lossy(&u));
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------------------------- encodings

const CP1252_HIGH: &str = "€\u{81}‚ƒ„…†‡ˆ‰Š‹Œ\u{8d}Ž\u{8f}\u{90}‘’“”•–—˜™š›œ\u{9d}žŸ";
const MAC_ROMAN_HIGH: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»… ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";
const ASCII_NAMES: [(&str, char); 34] = [
    ("space", ' '), ("exclam", '!'), ("quotedbl", '"'), ("numbersign", '#'), ("dollar", '$'), ("percent", '%'), ("ampersand", '&'),
    ("quotesingle", '\''), ("parenleft", '('), ("parenright", ')'), ("asterisk", '*'), ("plus", '+'), ("comma", ','), ("hyphen", '-'),
    ("period", '.'), ("slash", '/'), ("colon", ':'), ("semicolon", ';'), ("less", '<'), ("equal", '='), ("greater", '>'), ("question", '?'),
    ("at", '@'), ("bracketleft", '['), ("backslash", '\\'), ("bracketright", ']'), ("asciicircum", '^'), ("underscore", '_'), ("grave", '`'),
    ("braceleft", '{'), ("bar", '|'), ("braceright", '}'), ("asciitilde", '~'), ("nbspace", '\u{a0}'),
];
const DIGITS: [&str; 10] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
/// Names for U+00A0..=U+00FF, in order.
const LATIN1: [&str; 96] = [
    "space", "exclamdown", "cent", "sterling", "currency", "yen", "brokenbar", "section", "dieresis", "copyright", "ordfeminine", "guillemotleft", "logicalnot", "hyphen", "registered", "macron",
    "degree", "plusminus", "twosuperior", "threesuperior", "acute", "mu", "paragraph", "periodcentered", "cedilla", "onesuperior", "ordmasculine", "guillemotright", "onequarter", "onehalf", "threequarters", "questiondown",
    "Agrave", "Aacute", "Acircumflex", "Atilde", "Adieresis", "Aring", "AE", "Ccedilla", "Egrave", "Eacute", "Ecircumflex", "Edieresis", "Igrave", "Iacute", "Icircumflex", "Idieresis",
    "Eth", "Ntilde", "Ograve", "Oacute", "Ocircumflex", "Otilde", "Odieresis", "multiply", "Oslash", "Ugrave", "Uacute", "Ucircumflex", "Udieresis", "Yacute", "Thorn", "germandbls",
    "agrave", "aacute", "acircumflex", "atilde", "adieresis", "aring", "ae", "ccedilla", "egrave", "eacute", "ecircumflex", "edieresis", "igrave", "iacute", "icircumflex", "idieresis",
    "eth", "ntilde", "ograve", "oacute", "ocircumflex", "otilde", "odieresis", "divide", "oslash", "ugrave", "uacute", "ucircumflex", "udieresis", "yacute", "thorn", "ydieresis",
];
const EXTRA: [(&str, &str); 40] = [
    ("quoteleft", "\u{2018}"), ("quoteright", "\u{2019}"), ("quotedblleft", "\u{201c}"), ("quotedblright", "\u{201d}"), ("quotesinglbase", "\u{201a}"), ("quotedblbase", "\u{201e}"),
    ("endash", "\u{2013}"), ("emdash", "\u{2014}"), ("bullet", "\u{2022}"), ("ellipsis", "\u{2026}"), ("dagger", "\u{2020}"), ("daggerdbl", "\u{2021}"), ("perthousand", "\u{2030}"),
    ("guilsinglleft", "\u{2039}"), ("guilsinglright", "\u{203a}"), ("fi", "fi"), ("fl", "fl"), ("ff", "ff"), ("ffi", "ffi"), ("ffl", "ffl"), ("Euro", "\u{20ac}"), ("trademark", "\u{2122}"),
    ("OE", "\u{152}"), ("oe", "\u{153}"), ("Scaron", "\u{160}"), ("scaron", "\u{161}"), ("Zcaron", "\u{17d}"), ("zcaron", "\u{17e}"), ("Ydieresis", "\u{178}"), ("florin", "\u{192}"),
    ("circumflex", "\u{2c6}"), ("tilde", "\u{2dc}"), ("minus", "\u{2212}"), ("fraction", "\u{2044}"), ("dotlessi", "\u{131}"), ("Lslash", "\u{141}"), ("lslash", "\u{142}"), ("arrowright", "\u{2192}"),
    ("summation", "\u{2211}"), ("infinity", "\u{221e}"),
];

/// A glyph name to the text it stands for (a reduced Adobe Glyph List plus `uniXXXX`).
pub fn glyph_text(name: &str) -> Option<String> {
    let name = name.split('.').next().unwrap_or(name);
    if name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic() {
        return Some(name.to_string());
    }
    if let Some(i) = DIGITS.iter().position(|d| *d == name) {
        return Some(i.to_string());
    }
    if let Some((_, c)) = ASCII_NAMES.iter().find(|(n, _)| *n == name) {
        return Some(c.to_string());
    }
    if let Some(i) = LATIN1.iter().position(|n| *n == name) {
        return char::from_u32(0xa0 + i as u32).map(String::from);
    }
    if let Some((_, s)) = EXTRA.iter().find(|(n, _)| *n == name) {
        return Some((*s).to_string());
    }
    let hex = name.strip_prefix("uni").filter(|h| h.len() >= 4 && h.len() % 4 == 0).or_else(|| name.strip_prefix('u').filter(|h| (4..=6).contains(&h.len())));
    hex.and_then(|h| if h.len() > 4 && name.starts_with("uni") { None } else { u32::from_str_radix(h, 16).ok() }).and_then(char::from_u32).map(String::from)
}

fn base_encoding(name: &str) -> Vec<Option<String>> {
    let mut t: Vec<Option<String>> = vec![None; 256];
    for (c, slot) in t.iter_mut().enumerate().take(0x7f).skip(0x20) {
        *slot = Some((c as u8 as char).to_string());
    }
    match name {
        "MacRomanEncoding" => {
            for (i, ch) in MAC_ROMAN_HIGH.chars().enumerate() {
                t[0x80 + i] = Some(ch.to_string());
            }
        }
        "StandardEncoding" => {
            t[0x27] = Some("\u{2019}".into());
            t[0x60] = Some("\u{2018}".into());
        }
        _ => {
            // WinAnsiEncoding, and the default for fonts that name nothing
            for (i, ch) in CP1252_HIGH.chars().enumerate() {
                if !ch.is_control() {
                    t[0x80 + i] = Some(ch.to_string());
                }
            }
            for c in 0xa0..=0xffu32 {
                t[c as usize] = char::from_u32(c).map(String::from);
            }
            t[0x7f] = Some("\u{2022}".into());
        }
    }
    t
}

// ---------------------------------------------------------------------------------------------- fonts

pub struct Font {
    composite: bool,
    to_uni: Option<CMap>,
    enc: Vec<Option<String>>,
    first_char: i64,
    widths: Vec<f64>,
    missing: f64,
    dw: f64,
    cid_widths: HashMap<u32, f64>,
    width_scale: f64,
}

impl Font {
    pub fn load(doc: &Doc, d: &Dict) -> Font {
        let subtype = doc.lookup(d, "Subtype");
        let subtype = subtype.name().unwrap_or("");
        let to_uni = match doc.lookup(d, "ToUnicode").as_ref() {
            Obj::Stream(s) => doc.decode(s).ok().map(|(b, _)| CMap::parse(&b)),
            _ => None,
        };
        let mut f = Font { composite: false, to_uni, enc: vec![], first_char: 0, widths: vec![], missing: 0.0, dw: 1000.0, cid_widths: HashMap::new(), width_scale: 1.0 };
        if subtype == "Type0" {
            f.composite = true;
            let desc = doc.lookup(d, "DescendantFonts");
            if let Some(dd) = desc.arr().and_then(|a| a.first()).map(|o| doc.deref(o)).and_then(|o| o.dict().cloned()) {
                f.dw = doc.lookup(&dd, "DW").num().unwrap_or(1000.0);
                if let Some(w) = doc.lookup(&dd, "W").arr() {
                    let w: Vec<Obj> = w.iter().map(|o| (*doc.deref(o)).clone()).collect();
                    let mut i = 0;
                    while i < w.len() && f.cid_widths.len() < 100_000 {
                        let Some(first) = w[i].int() else { break };
                        match w.get(i + 1) {
                            Some(Obj::Arr(a)) => {
                                for (k, x) in a.iter().enumerate() {
                                    if let Some(x) = doc.deref(x).num() {
                                        f.cid_widths.insert(first as u32 + k as u32, x);
                                    }
                                }
                                i += 2;
                            }
                            Some(last) => {
                                let (Some(last), Some(wd)) = (last.int(), w.get(i + 2).and_then(|o| o.num())) else { break };
                                for c in first..=last.min(first + 65_535) {
                                    f.cid_widths.insert(c as u32, wd);
                                }
                                i += 3;
                            }
                            None => break,
                        }
                    }
                }
            }
            return f;
        }
        // Simple font: build the code table.
        let enc_obj = doc.lookup(d, "Encoding");
        let (base, diffs) = match enc_obj.as_ref() {
            Obj::Name(n) => (n.clone(), None),
            Obj::Dict(ed) => (doc.lookup(ed, "BaseEncoding").name().unwrap_or("").to_string(), doc.lookup(ed, "Differences").arr().map(|a| a.iter().map(|o| (*doc.deref(o)).clone()).collect::<Vec<_>>())),
            _ => (String::new(), None),
        };
        f.enc = base_encoding(&base);
        if let Some(diffs) = diffs {
            let mut code = 0usize;
            for o in diffs {
                match o {
                    Obj::Int(n) => code = n.max(0) as usize,
                    Obj::Name(n) => {
                        if code < 256 {
                            f.enc[code] = glyph_text(&n);
                        }
                        code += 1;
                    }
                    _ => {}
                }
            }
        }
        f.first_char = doc.lookup(d, "FirstChar").int().unwrap_or(0);
        if let Some(w) = doc.lookup(d, "Widths").arr() {
            f.widths = w.iter().take(256).map(|o| doc.deref(o).num().unwrap_or(0.0)).collect();
        }
        let fd = doc.lookup(d, "FontDescriptor");
        f.missing = fd.dict().and_then(|fd| doc.lookup(fd, "MissingWidth").num()).unwrap_or(0.0);
        if subtype == "Type3" {
            // glyph-space widths: scale by the font matrix to thousandths of an em
            f.width_scale = doc.lookup(d, "FontMatrix").arr().and_then(|m| m.first()).and_then(|o| doc.deref(o).num()).unwrap_or(0.001) * 1000.0;
        }
        if f.widths.is_empty() && f.missing == 0.0 {
            let base_font = doc.lookup(d, "BaseFont");
            f.missing = if base_font.name().is_some_and(|n| n.contains("Courier")) { 600.0 } else { 500.0 };
        }
        f
    }

    /// The codes of `s` as glyphs: the text each stands for and how far it advances.
    pub fn glyphs(&self, s: &[u8]) -> Vec<Glyph> {
        let mut out = Vec::with_capacity(s.len());
        if !self.composite {
            for &c in s {
                let text = self.to_uni.as_ref().and_then(|m| m.get(c as u32)).or_else(|| self.enc.get(c as usize).cloned().flatten());
                let idx = c as i64 - self.first_char;
                let width = if idx >= 0 && (idx as usize) < self.widths.len() { self.widths[idx as usize] } else { self.missing } * self.width_scale;
                out.push(Glyph { mapped: text.is_some(), text: text.unwrap_or_default(), width, space: c == 32 });
            }
            return out;
        }
        let spaces = self.to_uni.as_ref().map(|m| &m.codespaces[..]).unwrap_or(&[]);
        let mut i = 0;
        while i < s.len() {
            let n = spaces.iter().find(|(lo, hi, n)| i + n <= s.len() && (*lo..=*hi).contains(&code_of(&s[i..i + n]))).map(|(_, _, n)| *n).unwrap_or(2).min(s.len() - i);
            let code = code_of(&s[i..i + n]);
            i += n;
            let text = self.to_uni.as_ref().and_then(|m| m.get(code));
            let width = self.cid_widths.get(&code).copied().unwrap_or(self.dw);
            out.push(Glyph { mapped: text.is_some(), text: text.unwrap_or_default(), width, space: false });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_names_map_to_text() {
        assert_eq!(glyph_text("A").as_deref(), Some("A"));
        assert_eq!(glyph_text("seven").as_deref(), Some("7"));
        assert_eq!(glyph_text("parenleft").as_deref(), Some("("));
        assert_eq!(glyph_text("eacute").as_deref(), Some("é"));
        assert_eq!(glyph_text("Adieresis").as_deref(), Some("Ä"));
        assert_eq!(glyph_text("ydieresis").as_deref(), Some("ÿ"));
        assert_eq!(glyph_text("Euro").as_deref(), Some("€"));
        assert_eq!(glyph_text("fi").as_deref(), Some("fi"));
        assert_eq!(glyph_text("uni20AC").as_deref(), Some("€"));
        assert_eq!(glyph_text("a.sc").as_deref(), Some("a"));
        assert_eq!(glyph_text("g123"), None);
    }

    #[test]
    fn base_encodings_differ_where_they_should() {
        let win = base_encoding("WinAnsiEncoding");
        let mac = base_encoding("MacRomanEncoding");
        assert_eq!(win[0xe9].as_deref(), Some("é"));
        assert_eq!(win[0x93].as_deref(), Some("\u{201c}"));
        assert_eq!(win[0x81], None);
        assert_eq!(mac[0x8e].as_deref(), Some("é"));
        assert_eq!(mac[0xd2].as_deref(), Some("\u{201c}"));
        assert_eq!(mac.iter().skip(0x80).filter(|c| c.is_some()).count(), 128);
    }

    #[test]
    fn a_tounicode_map_covers_chars_ranges_arrays_and_codespaces() {
        let cm = CMap::parse(
            b"/CIDInit begincmap 1 begincodespacerange <0000> <FFFF> endcodespacerange 2 beginbfchar <0003> <0020> <0004> <00660069> endbfchar \
              2 beginbfrange <0010> <0012> <0041> <0020> <0021> [<0058> <0059>] endbfrange endcmap",
        );
        assert_eq!(cm.get(3).as_deref(), Some(" "));
        assert_eq!(cm.get(4).as_deref(), Some("fi"));
        assert_eq!((cm.get(0x10).as_deref(), cm.get(0x12).as_deref(), cm.get(0x13)), (Some("A"), Some("C"), None));
        assert_eq!((cm.get(0x20).as_deref(), cm.get(0x21).as_deref()), (Some("X"), Some("Y")));
        assert_eq!(cm.codespaces, vec![(0, 0xffff, 2)]);
    }

    #[test]
    fn hostile_cmaps_are_bounded() {
        let mut s = b"beginbfrange <0000> <FFFFFFFF> <0041> endbfrange beginbfchar ".to_vec();
        s.extend_from_slice(b"<00");
        let cm = CMap::parse(&s);
        let _ = cm.get(5);
        let _ = CMap::parse(b"beginbfrange <01> <02> [ <03> ");
        let _ = CMap::parse(&[0xff; 100]);
    }
}
