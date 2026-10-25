//! A streaming XML tokenizer for reading office documents (`specs/014` §3, §6).
//!
//! Deliberately small and deliberately blind to the parts of XML that are attack surface: a DOCTYPE is
//! skipped, never interpreted; entities are only the five built-ins and numeric references, so a "billion
//! laughs" definition is inert; there are no external references. Names are reduced to their local part
//! (`w:p` is `p`). Nesting depth and the number of nodes are capped. It is lenient about end-tag names, because
//! the readers only care about structure they can trust from well-formed producers, and a damaged file should
//! still yield what came before the damage.

use crate::DocError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ev {
    Start { name: String, attrs: Vec<(String, String)> },
    End { name: String },
    Text(String),
}

/// The value of attribute `key`: an exact match on the name as written (`r:id`, `id`) first, then on the
/// local name, so `w:val` is found as `val` while `id` and `r:id` on the same element stay distinct.
pub fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == key).or_else(|| attrs.iter().find(|(k, _)| local(k) == key)).map(|(_, v)| v.as_str())
}

const MAX_DEPTH: usize = 256;

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// The five predefined entities and numeric references. Anything else stays as written.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// Stream the events of `xml` to `f`. Returns `Err` if the document is malformed or over a limit; events
/// delivered before that point are valid, so callers keep what they have and report the damage.
pub fn walk(xml: &[u8], max_nodes: usize, mut f: impl FnMut(Ev)) -> Result<(), DocError> {
    if xml.starts_with(&[0xff, 0xfe]) || xml.starts_with(&[0xfe, 0xff]) {
        return Err(DocError("UTF-16 XML is not supported".into()));
    }
    let xml = xml.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(xml);
    let text = String::from_utf8_lossy(xml);
    let s = text.as_ref();
    let b = s.as_bytes();
    let bad = |what: &str| DocError(format!("malformed XML ({})", what));
    let mut i = 0usize;
    let mut depth = 0usize;
    let mut nodes = 0usize;
    while i < b.len() {
        if b[i] != b'<' {
            let end = b[i..].iter().position(|&c| c == b'<').map(|p| i + p).unwrap_or(b.len());
            nodes += 1;
            if nodes > max_nodes {
                return Err(DocError("the XML has more nodes than the reader allows".into()));
            }
            f(Ev::Text(unescape(&s[i..end])));
            i = end;
            continue;
        }
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").ok_or_else(|| bad("unterminated comment"))? + 3;
        } else if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").ok_or_else(|| bad("unterminated CDATA"))?;
            nodes += 1;
            if nodes > max_nodes {
                return Err(DocError("the XML has more nodes than the reader allows".into()));
            }
            f(Ev::Text(rest[9..end].to_string()));
            i += end + 3;
        } else if rest.starts_with("<?") {
            i += rest.find("?>").ok_or_else(|| bad("unterminated processing instruction"))? + 2;
        } else if rest.starts_with("<!") {
            // DOCTYPE or other declaration: skipped to the matching '>', honouring an internal subset in [...].
            let mut brackets = 0i32;
            let mut j = 2;
            let rb = rest.as_bytes();
            loop {
                match rb.get(j) {
                    None => return Err(bad("unterminated declaration")),
                    Some(b'[') => brackets += 1,
                    Some(b']') => brackets -= 1,
                    Some(b'>') if brackets <= 0 => break,
                    _ => {}
                }
                j += 1;
            }
            i += j + 1;
        } else if rest.starts_with("</") {
            let end = rest.find('>').ok_or_else(|| bad("unterminated end tag"))?;
            let name = local(rest[2..end].trim());
            depth = depth.saturating_sub(1);
            f(Ev::End { name: name.to_string() });
            i += end + 1;
        } else {
            let rb = rest.as_bytes();
            let mut j = 1;
            while j < rb.len() && !is_ws(rb[j]) && rb[j] != b'/' && rb[j] != b'>' {
                j += 1;
            }
            let name = local(&rest[1..j]).to_string();
            let mut attrs = vec![];
            let mut self_close = false;
            loop {
                while j < rb.len() && is_ws(rb[j]) {
                    j += 1;
                }
                match rb.get(j) {
                    None => return Err(bad("unterminated tag")),
                    Some(b'>') => {
                        j += 1;
                        break;
                    }
                    Some(b'/') => {
                        self_close = true;
                        j += 1;
                    }
                    Some(_) => {
                        let ns = j;
                        while j < rb.len() && !is_ws(rb[j]) && !matches!(rb[j], b'=' | b'/' | b'>') {
                            j += 1;
                        }
                        let key = rest[ns..j].to_string();
                        while j < rb.len() && is_ws(rb[j]) {
                            j += 1;
                        }
                        let mut val = String::new();
                        if rb.get(j) == Some(&b'=') {
                            j += 1;
                            while j < rb.len() && is_ws(rb[j]) {
                                j += 1;
                            }
                            match rb.get(j) {
                                Some(&q @ (b'"' | b'\'')) => {
                                    let vs = j + 1;
                                    let ve = rb[vs..].iter().position(|&c| c == q).map(|p| vs + p).ok_or_else(|| bad("unterminated attribute"))?;
                                    val = unescape(&rest[vs..ve]);
                                    j = ve + 1;
                                }
                                Some(_) => {
                                    let vs = j;
                                    while j < rb.len() && !is_ws(rb[j]) && rb[j] != b'>' {
                                        j += 1;
                                    }
                                    val = unescape(&rest[vs..j]);
                                }
                                None => return Err(bad("unterminated attribute")),
                            }
                        }
                        // The first occurrence wins, as a duplicate attribute is not well-formed.
                        if !attrs.iter().any(|(k, _): &(String, String)| *k == key) {
                            attrs.push((key, val));
                        }
                    }
                }
            }
            nodes += 1;
            if nodes > max_nodes {
                return Err(DocError("the XML has more nodes than the reader allows".into()));
            }
            depth += 1;
            if depth > MAX_DEPTH {
                return Err(DocError(format!("the XML is nested more than {} deep", MAX_DEPTH)));
            }
            f(Ev::Start { name: name.clone(), attrs });
            if self_close {
                depth -= 1;
                f(Ev::End { name });
            }
            i += j;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(x: &str) -> (Vec<String>, Result<(), DocError>) {
        let mut v = vec![];
        let r = walk(x.as_bytes(), 1_000_000, |e| {
            v.push(match e {
                Ev::Start { name, attrs } => format!("<{}{}>", name, attrs.iter().map(|(k, v)| format!(" {}={}", k, v)).collect::<String>()),
                Ev::End { name } => format!("</{}>", name),
                Ev::Text(t) => format!("'{}'", t),
            })
        });
        (v, r)
    }

    #[test]
    fn elements_attributes_text_and_namespaces() {
        let (v, r) = events(r#"<?xml version="1.0"?><w:p w:val="a &amp; b" x='1'><w:t xml:space="preserve"> hi </w:t><br/></w:p>"#);
        assert!(r.is_ok());
        assert_eq!(v, ["<p w:val=a & b x=1>", "<t xml:space=preserve>", "' hi '", "</t>", "<br>", "</br>", "</p>"]);
        let mut found = None;
        let _ = walk(br#"<sldId id="256" r:id="rId1"/>"#, 10, |e| {
            if let Ev::Start { attrs, .. } = e {
                found = Some((attr(&attrs, "id").map(String::from), attr(&attrs, "r:id").map(String::from)));
            }
        });
        assert_eq!(found, Some((Some("256".into()), Some("rId1".into()))), "id and r:id stay distinct");
    }

    #[test]
    fn entities_cdata_comments_and_references() {
        let (v, _) = events("<a>&lt;&#65;&#x42;&bogus;&amp;<!-- gone --><![CDATA[<raw & text>]]></a>");
        assert_eq!(v, ["<a>", "'<AB&bogus;&'", "'<raw & text>'", "</a>"]);
    }

    #[test]
    fn a_doctype_with_entity_definitions_is_inert() {
        let (v, r) = events(r#"<!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;&lol;">]><a>&lol2;</a>"#);
        assert!(r.is_ok());
        assert_eq!(v, ["<a>", "'&lol2;'", "</a>"], "entities defined in the DTD are never expanded");
    }

    #[test]
    fn nesting_and_node_limits_stop_the_walk_after_valid_events() {
        let deep = "<a>".repeat(300);
        let (v, r) = events(&deep);
        assert!(r.unwrap_err().0.contains("nested more than"));
        assert_eq!(v.len(), MAX_DEPTH);
        let many = "<a/>".repeat(100);
        let mut n = 0;
        let r = walk(many.as_bytes(), 10, |_| n += 1);
        assert!(r.unwrap_err().0.contains("more nodes"));
        assert!(n <= 20);
    }

    #[test]
    fn damage_is_reported_after_the_events_before_it() {
        for bad in ["<a><b", "<a x=\"1", "<a><!-- x", "<a><![CDATA[x", "<a></a", "<!DOCTYPE a [<!ENTITY"] {
            let (_, r) = events(bad);
            assert!(r.is_err(), "{}", bad);
        }
        let (v, r) = events("<a><b>text</b><c");
        assert!(r.is_err());
        assert_eq!(&v[..4], ["<a>", "<b>", "'text'", "</b>"]);
    }

    #[test]
    fn utf16_and_junk_do_not_panic() {
        assert!(walk(&[0xff, 0xfe, b'<', 0], 100, |_| {}).is_err());
        let mut x: u64 = 0x1234_5678_9abc_def1;
        for n in 0..3000usize {
            let buf: Vec<u8> = (0..n % 200)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    const ALPHABET: &[u8] = b"<>/=\"'&;! ?-[]ab\n";
                    ALPHABET[(x as usize) % ALPHABET.len()]
                })
                .collect();
            let _ = walk(&buf, 1000, |_| {});
        }
    }
}
