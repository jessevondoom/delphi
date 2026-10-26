//! The strict frontmatter subset parser (spec 012 §2, §6 steps 4 and 7; plan risk R1).
//!
//! A record's frontmatter is attacker-controlled text, and general YAML is far too large a language to trust
//! with it: anchors and aliases (bombs), tags (`!!python/...` in other stacks), merge keys, duplicate keys
//! with parser-dependent winners, multi-document streams, folded scalars, implicit typing ("no" is a boolean,
//! "010" is octal). So this is **not YAML**. It is a small language that happens to be valid YAML for the
//! documents it accepts, and everything else fails closed:
//!
//! * `key: value`, keys `[a-z][a-z0-9_]{0,31}`, exactly one space after the colon, indentation two spaces
//!   per level (never tabs), one map per indentation block;
//! * a value is a **double-quoted string** (only `\"` and `\\` escapes), a **plain scalar** (a conservative
//!   character set: no `# , [ ] { } & * ! | > % @ " ' \``, no `: `), or a **one-line flow list** of those;
//! * a key with nothing after the colon opens a nested **map** or a **block list of maps** (`- key: v`), to
//!   at most [`MAX_DEPTH`] levels. Nothing else nests: there are no flow maps, no lists of lists.
//! * no comments, blank lines, trailing spaces, CR, `---` or `...` inside the block, so there is one
//!   spelling of every document and no second document to smuggle.
//!
//! **No implicit typing.** The parser never turns a scalar into a number, boolean or null; it keeps the text
//! and whether it was quoted, and the schema decides what each field means ([`Scalar::as_int`], and so on).
//!
//! The text is also screened as a whole before tokenizing ([`check_text`]): valid UTF-8 (overlong forms and
//! surrogates are not UTF-8), NFC, no control characters except `\n`, no bidi controls, no invisible
//! formatting characters, no Unicode tag characters. That covers the body too.
//!
//! The parser counts its own invocations ([`parse_count`]) so the verify-before-parse test can prove it was
//! never called for a record that failed spec 012 §6 steps 1-3.

use crate::limits::*;
use std::cell::Cell;

thread_local! {
    static PARSES: Cell<usize> = const { Cell::new(0) };
}

/// How many times [`parse`] has been entered on this thread. Test instrumentation for spec 012 test 12.
pub fn parse_count() -> usize {
    PARSES.with(Cell::get)
}

/// Why the frontmatter was refused. The text is a fixed string, never any of the attacker's bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FmError(pub &'static str);

type Res<T> = std::result::Result<T, FmError>;

fn err<T>(why: &'static str) -> Res<T> {
    Err(FmError(why))
}

/// Characters that render as nothing or reorder text, so a human reviewer and a machine would disagree
/// about what a string says. Spec 012 §6 step 7 names the bidi controls and `U+E0000`-`U+E007F`; the rest
/// are the same class of trick (zero-width and invisible formatting, variation-selector tags, noncharacters).
pub fn is_forbidden_char(c: char) -> bool {
    let u = c as u32;
    matches!(u,
        0x00AD | 0x034F | 0x061C | 0x115F | 0x1160 | 0x17B4 | 0x17B5 | 0x180E
        | 0x200B..=0x200F | 0x2028 | 0x2029 | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F
        | 0x3164 | 0xFDD0..=0xFDEF | 0xFEFF | 0xFFA0 | 0xFFF9..=0xFFFB
        | 0xE0000..=0xE007F | 0xE0100..=0xE01EF)
        || (u & 0xFFFE) == 0xFFFE
}

/// Whole-text screen: no control characters but `\n`, nothing from [`is_forbidden_char`], NFC.
pub fn check_text(s: &str) -> Res<()> {
    for c in s.chars() {
        if c == '\n' {
            continue;
        }
        if c.is_control() {
            return err("control character");
        }
        if is_forbidden_char(c) {
            return err("bidi, invisible or tag character");
        }
    }
    if !unicode_normalization::is_nfc(s) {
        return err("text is not NFC");
    }
    Ok(())
}

/// A scalar exactly as written. `quoted` matters: only an unquoted `null` is null, only unquoted digits are
/// an integer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scalar {
    pub text: String,
    pub quoted: bool,
}

impl Scalar {
    pub fn is_null(&self) -> bool {
        !self.quoted && self.text == "null"
    }

    /// A string value. An unquoted `null` is not a string; spell it `"null"` to mean the word.
    pub fn as_str(&self) -> Option<&str> {
        if self.is_null() { None } else { Some(&self.text) }
    }

    /// A canonical decimal integer: digits only, no sign, no leading zero, at most 15 digits, unquoted.
    pub fn as_int(&self) -> Option<u64> {
        let t = self.text.as_str();
        if self.quoted || t.is_empty() || t.len() > 15 || !t.bytes().all(|b| b.is_ascii_digit()) || (t.len() > 1 && t.starts_with('0')) {
            return None;
        }
        t.parse().ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Scalar(Scalar),
    /// A one-line flow list of scalars.
    List(Vec<Scalar>),
    Map(Map),
    /// A block list of maps (`pointers`, `evidence`).
    Seq(Vec<Map>),
}

/// An ordered map with unique keys. Schema code `take`s the keys it knows and then calls [`Map::finish`],
/// so a key nobody asked for is an error rather than an ignored smuggling channel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Map {
    pub entries: Vec<(String, Value)>,
}

impl Map {
    pub fn take(&mut self, key: &str) -> Option<Value> {
        let at = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(at).1)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    /// Ok only if every key has been taken.
    pub fn finish(self) -> Res<()> {
        if self.entries.is_empty() { Ok(()) } else { err("unknown key") }
    }

    /// The first key anywhere in this tree (any depth, any list element) that is in `names`.
    pub fn find_key_deep(&self, names: &[&str]) -> Option<String> {
        for (k, v) in &self.entries {
            if names.contains(&k.as_str()) {
                return Some(k.clone());
            }
            let hit = match v {
                Value::Map(m) => m.find_key_deep(names),
                Value::Seq(items) => items.iter().find_map(|m| m.find_key_deep(names)),
                _ => None,
            };
            if hit.is_some() {
                return hit;
            }
        }
        None
    }
}

/// A schema-level refusal: which field (always one of our own schema names, never attacker text) and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bad {
    pub field: &'static str,
    pub why: &'static str,
}

pub fn bad(field: &'static str, why: &'static str) -> Bad {
    Bad { field, why }
}

/// Typed accessors. Each takes the key out of the map (so [`Map::finish`] can find the leftovers) and
/// refuses the wrong type; an absent optional key is `None`, a present key of the wrong type is an error,
/// and `null` is only accepted where [`Map::opt_nullable_str`] says so.
impl Map {
    fn scalar(&mut self, f: &'static str) -> Result<Option<Scalar>, Bad> {
        match self.take(f) {
            None => Ok(None),
            Some(Value::Scalar(s)) => Ok(Some(s)),
            Some(_) => Err(bad(f, "expected a single value")),
        }
    }

    pub fn req_str(&mut self, f: &'static str) -> Result<String, Bad> {
        self.opt_str(f)?.ok_or(bad(f, "missing"))
    }

    pub fn opt_str(&mut self, f: &'static str) -> Result<Option<String>, Bad> {
        match self.scalar(f)? {
            None => Ok(None),
            Some(s) => s.as_str().map(|t| Some(t.to_string())).ok_or(bad(f, "null is not allowed here")),
        }
    }

    /// Absent or `null` is `None`.
    pub fn opt_nullable_str(&mut self, f: &'static str) -> Result<Option<String>, Bad> {
        match self.scalar(f)? {
            None => Ok(None),
            Some(s) if s.is_null() => Ok(None),
            Some(s) => Ok(Some(s.text)),
        }
    }

    pub fn req_int(&mut self, f: &'static str) -> Result<u64, Bad> {
        self.opt_int(f)?.ok_or(bad(f, "missing"))
    }

    pub fn opt_int(&mut self, f: &'static str) -> Result<Option<u64>, Bad> {
        match self.scalar(f)? {
            None => Ok(None),
            Some(s) => s.as_int().map(Some).ok_or(bad(f, "not a canonical integer")),
        }
    }

    pub fn req_list(&mut self, f: &'static str) -> Result<Vec<String>, Bad> {
        self.opt_list(f)?.ok_or(bad(f, "missing"))
    }

    pub fn opt_list(&mut self, f: &'static str) -> Result<Option<Vec<String>>, Bad> {
        match self.take(f) {
            None => Ok(None),
            Some(Value::List(items)) => items.into_iter().map(|s| s.as_str().map(str::to_string).ok_or(bad(f, "null in a list"))).collect::<Result<_, _>>().map(Some),
            Some(_) => Err(bad(f, "expected a [list]")),
        }
    }

    pub fn req_map(&mut self, f: &'static str) -> Result<Map, Bad> {
        self.opt_map(f)?.ok_or(bad(f, "missing"))
    }

    pub fn opt_map(&mut self, f: &'static str) -> Result<Option<Map>, Bad> {
        match self.take(f) {
            None => Ok(None),
            Some(Value::Map(m)) => Ok(Some(m)),
            Some(_) => Err(bad(f, "expected a nested map")),
        }
    }

    /// A block list of maps. `[]` is the one spelling of the empty list.
    pub fn opt_seq(&mut self, f: &'static str) -> Result<Option<Vec<Map>>, Bad> {
        match self.take(f) {
            None => Ok(None),
            Some(Value::Seq(items)) => Ok(Some(items)),
            Some(Value::List(items)) if items.is_empty() => Ok(Some(Vec::new())),
            Some(_) => Err(bad(f, "expected a list of maps")),
        }
    }
}

/// Look `s` up in a closed enum table. Exact, case-sensitive.
pub fn pick<T: Copy>(s: &str, table: &[(&str, T)]) -> Option<T> {
    table.iter().find(|(n, _)| *n == s).map(|(_, v)| *v)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub front: Map,
    /// Everything after the closing `---` line, verbatim. Data, never parsed, never instructions.
    pub body: String,
}

struct Line<'a> {
    indent: usize,
    text: &'a str,
}

/// Split a record file into its frontmatter map and body. Total over arbitrary bytes: returns an error, never
/// panics, and recursion is bounded by [`MAX_DEPTH`].
pub fn parse(bytes: &[u8]) -> Res<Document> {
    PARSES.with(|c| c.set(c.get() + 1));
    let text = std::str::from_utf8(bytes).map_err(|_| FmError("not valid UTF-8"))?;
    check_text(text)?;
    let rest = text.strip_prefix("---\n").ok_or(FmError("does not open with a --- line"))?;
    let mut raw_lines = Vec::new();
    let mut consumed = 0;
    let mut closed = false;
    for raw in rest.split_inclusive('\n') {
        // A last line with no newline can't be the closing delimiter: the publisher always ends it with LF.
        let Some(line) = raw.strip_suffix('\n') else { break };
        consumed += raw.len();
        if line == "---" {
            closed = true;
            break;
        }
        if raw_lines.len() >= MAX_FRONTMATTER_LINES {
            return err("too many frontmatter lines");
        }
        raw_lines.push(line);
    }
    if !closed {
        return err("frontmatter is not closed by a --- line");
    }
    let mut lines = Vec::with_capacity(raw_lines.len());
    for l in raw_lines {
        if l.len() > MAX_LINE_BYTES {
            return err("line too long");
        }
        let indent = l.len() - l.trim_start_matches(' ').len();
        let text = &l[indent..];
        if text.is_empty() {
            return err("blank line");
        }
        if text.ends_with(' ') {
            return err("trailing space");
        }
        lines.push(Line { indent, text });
    }
    let mut p = Parser { lines, pos: 0 };
    let front = p.map(0, 0)?;
    if p.pos != p.lines.len() {
        return err("unparsed lines");
    }
    Ok(Document { front, body: rest[consumed..].to_string() })
}

struct Parser<'a> {
    lines: Vec<Line<'a>>,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn map(&mut self, indent: usize, depth: usize) -> Res<Map> {
        if depth >= MAX_DEPTH {
            return err("nesting too deep");
        }
        let mut map = Map::default();
        while self.pos < self.lines.len() {
            let (li, text) = (self.lines[self.pos].indent, self.lines[self.pos].text);
            if li < indent {
                break;
            }
            if li > indent {
                return err("unexpected indentation");
            }
            let (key, rest) = split_key(text)?;
            if map.contains(key) {
                return err("duplicate key");
            }
            if map.entries.len() >= MAX_KEYS_PER_MAP {
                return err("too many keys");
            }
            self.pos += 1;
            let value = match rest {
                Some(r) => inline(r)?,
                None => {
                    let child = self.lines.get(self.pos).filter(|l| l.indent == indent + 2).ok_or(FmError("key with no value"))?;
                    if child.text.starts_with("- ") {
                        self.seq(indent + 2, depth + 1)?
                    } else {
                        Value::Map(self.map(indent + 2, depth + 1)?)
                    }
                }
            };
            map.entries.push((key.to_string(), value));
        }
        Ok(map)
    }

    fn seq(&mut self, indent: usize, depth: usize) -> Res<Value> {
        if depth >= MAX_DEPTH {
            return err("nesting too deep");
        }
        let mut items = Vec::new();
        while self.pos < self.lines.len() && self.lines[self.pos].indent == indent && self.lines[self.pos].text.starts_with("- ") {
            if items.len() >= MAX_SEQ_ITEMS {
                return err("too many list items");
            }
            // The dash line is the item map's first key line, two columns deeper than the dash.
            let rest = &self.lines[self.pos].text[2..];
            self.lines[self.pos] = Line { indent: indent + 2, text: rest };
            items.push(self.map(indent + 2, depth + 1)?);
        }
        Ok(Value::Seq(items))
    }
}

fn split_key(text: &str) -> Res<(&str, Option<&str>)> {
    let colon = text.find(':').ok_or(FmError("line is not key: value"))?;
    let key = &text[..colon];
    let mut chars = key.chars();
    let ok = key.len() <= MAX_KEY_LEN && chars.next().is_some_and(|c| c.is_ascii_lowercase()) && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !ok {
        return err("bad key");
    }
    let after = &text[colon + 1..];
    if after.is_empty() {
        return Ok((key, None));
    }
    let value = after.strip_prefix(' ').ok_or(FmError("no space after the colon"))?;
    if value.is_empty() || value.starts_with(' ') {
        return err("extra space after the colon");
    }
    Ok((key, Some(value)))
}

fn inline(s: &str) -> Res<Value> {
    if s.starts_with('"') {
        let (text, used) = quoted(s)?;
        if used != s.len() {
            return err("text after a quoted string");
        }
        Ok(Value::Scalar(Scalar { text, quoted: true }))
    } else if s.starts_with('[') {
        flow_list(s)
    } else {
        Ok(Value::Scalar(plain(s)?))
    }
}

/// A double-quoted string at the start of `s`; returns it and the bytes consumed including both quotes.
fn quoted(s: &str) -> Res<(String, usize)> {
    let mut out = String::new();
    let mut it = s.char_indices().skip(1);
    while let Some((i, c)) = it.next() {
        match c {
            '"' => return Ok((out, i + 1)),
            '\\' => match it.next() {
                Some((_, e @ ('"' | '\\'))) => out.push(e),
                _ => return err("unsupported escape"),
            },
            c => out.push(c),
        }
    }
    err("unterminated string")
}

fn plain(s: &str) -> Res<Scalar> {
    // Anchors (&), aliases (*), tags (!), block scalars (| >), directives (%), reserved (@ `), quotes, flow
    // and comment syntax (# , [ ] { }) are all refused anywhere in a plain scalar, not just at the start.
    const FORBIDDEN: &str = "\"'`#,[]{}&*!|>%@";
    if s.is_empty() || s.starts_with([' ', '-', '?', ':']) || s.ends_with(' ') || s.contains(|c| FORBIDDEN.contains(c)) || s.contains(": ") || s.ends_with(':') {
        return err("not a plain scalar");
    }
    Ok(Scalar { text: s.to_string(), quoted: false })
}

fn flow_list(s: &str) -> Res<Value> {
    let inner = s.strip_prefix('[').and_then(|t| t.strip_suffix(']')).ok_or(FmError("unterminated list"))?;
    let mut items = Vec::new();
    if inner.is_empty() {
        return Ok(Value::List(items));
    }
    let mut rest = inner;
    loop {
        if items.len() >= MAX_LIST_ITEMS {
            return err("too many list items");
        }
        let (item, tail) = if rest.starts_with('"') {
            let (text, used) = quoted(rest)?;
            (Scalar { text, quoted: true }, &rest[used..])
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            (plain(&rest[..end])?, &rest[end..])
        };
        items.push(item);
        if tail.is_empty() {
            return Ok(Value::List(items));
        }
        // One comma, then at most one space; an empty item or trailing comma fails at the next iteration.
        let after = tail.strip_prefix(',').ok_or(FmError("junk after a list item"))?;
        rest = after.strip_prefix(' ').unwrap_or(after);
        if rest.is_empty() || rest.starts_with(' ') {
            return err("empty list item");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(front: &str) -> Vec<u8> {
        format!("---\n{front}---\n").into_bytes()
    }

    fn bad(front: &str) {
        assert!(parse(&doc(front)).is_err(), "should be refused: {front:?}");
    }

    #[test]
    fn parses_scalars_lists_maps_and_block_lists() {
        let d = parse(&doc("a: x y\nb: \"q: [1], #2\"\nc: [one, \"two, 2\"]\nd:\n  e: 5\n  f: []\ng:\n  - h: 1\n    i:\n      j: k\n  - h: 2\n")).unwrap();
        let mut m = d.front;
        assert_eq!(m.take("a"), Some(Value::Scalar(Scalar { text: "x y".into(), quoted: false })));
        let Some(Value::Scalar(b)) = m.take("b") else { panic!() };
        assert_eq!(b.text, "q: [1], #2");
        let Some(Value::List(c)) = m.take("c") else { panic!() };
        assert_eq!(c.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["one", "two, 2"]);
        let Some(Value::Map(d)) = m.take("d") else { panic!() };
        assert_eq!(d.entries.len(), 2);
        let Some(Value::Seq(g)) = m.take("g") else { panic!() };
        assert_eq!(g.len(), 2);
        assert_eq!(parse(&doc("a: 1\n")).unwrap().body, "");
    }

    #[test]
    fn body_is_everything_after_the_closing_line_and_is_never_parsed() {
        let d = parse(b"---\na: 1\n---\ntier: 1\n---\nb: 2\n").unwrap();
        assert_eq!(d.body, "tier: 1\n---\nb: 2\n");
        assert_eq!(d.front.entries.len(), 1, "a second document in the body adds no keys");
    }

    #[test]
    fn yaml_features_are_refused() {
        for b in [
            "a: &x 1\n", "a: *x\n", "a: !!str 1\n", "a: !tag 1\n", "<<: *x\n", "a: [*x, *x]\n", "a: |\n", "a: >\n", "a: {b: 1}\n", "a: 'single'\n",
            "a: 1 # comment\n", "# comment\na: 1\n", "a: 1\na: 2\n", "a:\n  b: 1\n  b: 2\n", "a: @x\n", "a: %x\n", "a: `x`\n", "? a\n", "- a\n",
        ] {
            bad(b);
        }
    }

    #[test]
    fn layout_is_canonical() {
        for b in [
            "a:1\n", "a:  1\n", "a: 1 \n", "\na: 1\n", "a: 1\n\nb: 2\n", "a: 1\r\n", " a: 1\n", "A: 1\n", "a-b: 1\n", "a b: 1\n", "a:\n", "a:\n b: 1\n", "a:\n    b: 1\n",
            "a:\n  b: 1\n c: 2\n", "a: [1,, 2]\n", "a: [1, 2,]\n", "a: [ 1]\n", "a: [1 ,2]\n", "a: [1,  2]\n", "a: [1\n", "a: \"x\" y\n", "a: \"x\n", "a: \"\\n\"\n", "a: \"\\u0041\"\n",
            "...\n", "a:\n  - b: 1\n  c: 2\n", "a:\n  -\n", "a:\n  - b\n",
        ] {
            bad(b);
        }
        assert!(parse(b"a: 1\n").is_err(), "no opening delimiter");
        assert!(parse(b"---\na: 1\n").is_err(), "no closing delimiter");
        assert!(parse(b"---\na: 1\n---").is_err(), "closing delimiter without a newline");
        assert!(parse(b"\xEF\xBB\xBF---\na: 1\n---\n").is_err(), "a BOM is not a --- line");
    }

    #[test]
    fn nesting_and_size_are_bounded() {
        let mut deep = String::new();
        for i in 0..8 {
            deep.push_str(&format!("{}k:\n", "  ".repeat(i)));
        }
        deep.push_str(&format!("{}k: 1\n", "  ".repeat(8)));
        bad(&deep);
        let ok_depth = "a:\n  b:\n    c:\n      d: 1\n";
        assert!(parse(&doc(ok_depth)).is_ok(), "four levels is the limit");
        bad("a:\n  b:\n    c:\n      d:\n        e: 1\n");
        let long = format!("a: {}\n", "x".repeat(MAX_LINE_BYTES - 3));
        assert!(parse(&doc(&long)).is_ok());
        bad(&format!("a: {}\n", "x".repeat(MAX_LINE_BYTES - 2)));
        let lines_ok: String = (0..MAX_KEYS_PER_MAP).map(|i| format!("k{i}: 1\n")).collect();
        assert!(parse(&doc(&lines_ok)).is_ok());
        let lines_over: String = (0..=MAX_KEYS_PER_MAP).map(|i| format!("k{i}: 1\n")).collect();
        bad(&lines_over);
        // Four maps of 39 keys plus their headers is exactly the line cap; one more key is over it.
        let block = |m: usize| format!("m{m}:\n{}", (0..39).map(|i| format!("  k{i}: 1\n")).collect::<String>());
        let at_cap: String = (0..4).map(block).collect();
        assert_eq!(at_cap.lines().count(), MAX_FRONTMATTER_LINES);
        assert!(parse(&doc(&at_cap)).is_ok());
        bad(&format!("{at_cap}z: 1\n"));
        let items_ok: Vec<String> = (0..MAX_LIST_ITEMS).map(|i| i.to_string()).collect();
        assert!(parse(&doc(&format!("a: [{}]\n", items_ok.join(",")))).is_ok());
        bad(&format!("a: [{},1]\n", items_ok.join(",")));
    }

    #[test]
    fn text_hygiene_is_enforced_everywhere() {
        for c in ['\u{202E}', '\u{2066}', '\u{200B}', '\u{FEFF}', '\u{E0041}', '\u{E007F}', '\u{E0100}', '\u{0007}', '\u{0085}', '\t', '\r', '\u{0}', '\u{FFFF}'] {
            assert!(parse(format!("---\na: x{c}y\n---\n").as_bytes()).is_err(), "frontmatter {c:?}");
            assert!(parse(format!("---\na: 1\n---\nbody {c} text\n").as_bytes()).is_err(), "body {c:?}");
            assert!(parse(format!("---\na: \"x{c}y\"\n---\n").as_bytes()).is_err(), "quoted {c:?}");
        }
        // The plain neighbours of the forbidden ranges are fine, so the ranges are not over-wide.
        assert!(parse("---\na: x\u{E007F}\n---\n".as_bytes()).is_err());
        assert!(parse("---\na: caf\u{e9} \u{1F600} \u{FE0F}\n---\n".as_bytes()).is_ok());
        assert!(parse("---\na: 1\n---\nline one\nline two\n".as_bytes()).is_ok());
    }

    #[test]
    fn non_nfc_text_is_refused_and_the_nfd_form_really_differs() {
        let nfc = "caf\u{e9}";
        let nfd = "cafe\u{301}";
        assert_ne!(nfc, nfd);
        assert!(parse(format!("---\na: {nfc}\n---\n").as_bytes()).is_ok());
        assert!(parse(format!("---\na: {nfd}\n---\n").as_bytes()).is_err());
        assert!(parse(format!("---\na: 1\n---\n{nfd}\n").as_bytes()).is_err(), "the body is screened too");
    }

    #[test]
    fn invalid_utf8_including_overlong_forms_is_refused() {
        // 0xC0 0xAF is an overlong '/'; 0xED 0xA0 0x80 is a surrogate; 0xF4 0x90.. is past U+10FFFF.
        for bad_bytes in [&b"\xC0\xAF"[..], b"\xED\xA0\x80", b"\xF4\x90\x80\x80", b"\xE0\x80\xAF", b"\xFF"] {
            let mut v = b"---\na: x".to_vec();
            v.extend_from_slice(bad_bytes);
            v.extend_from_slice(b"\n---\n");
            assert!(parse(&v).is_err(), "{bad_bytes:?}");
        }
    }

    #[test]
    fn the_parser_counts_its_calls() {
        let before = parse_count();
        let _ = parse(b"junk");
        let _ = parse(b"---\na: 1\n---\n");
        assert_eq!(parse_count(), before + 2);
    }

    #[test]
    fn null_and_integers_are_typed_by_the_schema_not_the_parser() {
        let Some(Value::Scalar(n)) = parse(&doc("a: null\n")).unwrap().front.take("a") else { panic!() };
        assert!(n.is_null() && n.as_str().is_none());
        let Some(Value::Scalar(q)) = parse(&doc("a: \"null\"\n")).unwrap().front.take("a") else { panic!() };
        assert_eq!(q.as_str(), Some("null"));
        for (t, want) in [("0", Some(0)), ("7", Some(7)), ("007", None), ("-1", None), ("+1", None), ("1.0", None), ("0x1", None), ("1e3", None), ("1234567890123456", None)] {
            assert_eq!(Scalar { text: t.into(), quoted: false }.as_int(), want, "{t}");
        }
        assert_eq!(Scalar { text: "7".into(), quoted: true }.as_int(), None, "a quoted number is a string");
    }
}
