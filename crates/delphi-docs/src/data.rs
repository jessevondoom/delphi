//! JSON, YAML and TOML (`specs/014` §4): small files are shown in full (JSON pretty-printed), large ones as a
//! size-aware outline. `member` is a path into the data (`users.0.name`) to read just that part.

use crate::sniff::Format;
use crate::{DocError, Extract, Limits, Request, Unit};

/// A format-neutral tree, so one renderer serves all three formats.
#[derive(Debug, Clone)]
pub enum Node {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Node>),
    Map(Vec<(String, Node)>),
}

fn from_json(v: &serde_json::Value) -> Node {
    use serde_json::Value as V;
    match v {
        V::Null => Node::Null,
        V::Bool(b) => Node::Bool(*b),
        V::Number(n) => Node::Num(n.to_string()),
        V::String(s) => Node::Str(s.clone()),
        V::Array(a) => Node::Arr(a.iter().map(from_json).collect()),
        V::Object(o) => Node::Map(o.iter().map(|(k, v)| (k.clone(), from_json(v))).collect()),
    }
}

fn from_yaml(v: &serde_yaml::Value) -> Node {
    use serde_yaml::Value as V;
    match v {
        V::Null => Node::Null,
        V::Bool(b) => Node::Bool(*b),
        V::Number(n) => Node::Num(n.to_string()),
        V::String(s) => Node::Str(s.clone()),
        V::Sequence(a) => Node::Arr(a.iter().map(from_yaml).collect()),
        V::Mapping(m) => Node::Map(
            m.iter()
                .map(|(k, v)| {
                    let key = match k {
                        V::String(s) => s.clone(),
                        other => serde_yaml::to_string(other).unwrap_or_default().trim().to_string(),
                    };
                    (key, from_yaml(v))
                })
                .collect(),
        ),
        V::Tagged(t) => from_yaml(&t.value),
    }
}

fn from_toml(v: &toml::Value) -> Node {
    use toml::Value as V;
    match v {
        V::String(s) => Node::Str(s.clone()),
        V::Integer(i) => Node::Num(i.to_string()),
        V::Float(f) => Node::Num(f.to_string()),
        V::Boolean(b) => Node::Bool(*b),
        V::Datetime(d) => Node::Str(d.to_string()),
        V::Array(a) => Node::Arr(a.iter().map(from_toml).collect()),
        V::Table(t) => Node::Map(t.iter().map(|(k, v)| (k.clone(), from_toml(v))).collect()),
    }
}

const STR_PREVIEW: usize = 80;
const SMALL: usize = 24 * 1024;
const MAX_DEPTH: usize = 3;

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn preview(s: &str) -> String {
    let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if flat.chars().count() > STR_PREVIEW {
        let cut: String = flat.chars().take(STR_PREVIEW).collect();
        format!("{}…\"", quote(&cut).trim_end_matches('"'))
    } else {
        quote(&flat)
    }
}

fn kind(n: &Node) -> String {
    match n {
        Node::Null => "null".into(),
        Node::Bool(_) => "bool".into(),
        Node::Num(_) => "number".into(),
        Node::Str(_) => "string".into(),
        Node::Arr(a) => format!("array[{}]", a.len()),
        Node::Map(m) => format!("object{{{} keys}}", m.len()),
    }
}

fn scalar(n: &Node) -> Option<String> {
    match n {
        Node::Null => Some("null".into()),
        Node::Bool(b) => Some(b.to_string()),
        Node::Num(s) => Some(s.clone()),
        Node::Str(s) => Some(preview(s)),
        _ => None,
    }
}

/// Full pretty-printed JSON of a node.
fn pretty(n: &Node, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    match n {
        Node::Arr(a) if !a.is_empty() => {
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                out.push_str(&format!("{}  ", pad));
                pretty(x, indent + 1, out);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            out.push_str(&format!("{}]", pad));
        }
        Node::Map(m) if !m.is_empty() => {
            out.push_str("{\n");
            for (i, (k, x)) in m.iter().enumerate() {
                out.push_str(&format!("{}  {}: ", pad, quote(k)));
                pretty(x, indent + 1, out);
                out.push_str(if i + 1 < m.len() { ",\n" } else { "\n" });
            }
            out.push_str(&format!("{}}}", pad));
        }
        Node::Arr(_) => out.push_str("[]"),
        Node::Map(_) => out.push_str("{}"),
        Node::Str(s) => out.push_str(&quote(s)),
        other => out.push_str(&scalar(other).unwrap_or_default()),
    }
}

fn outline(n: &Node, label: &str, depth: usize, lines: &mut Vec<String>, budget: &mut usize) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    let pad = "  ".repeat(depth);
    match n {
        Node::Map(m) => {
            if depth >= MAX_DEPTH {
                let keys: Vec<&str> = m.iter().take(8).map(|(k, _)| k.as_str()).collect();
                lines.push(format!("{}{}object, {} keys: {}{}", pad, label, m.len(), keys.join(", "), if m.len() > 8 { ", …" } else { "" }));
                return;
            }
            lines.push(format!("{}{}object, {} key{}", pad, label, m.len(), if m.len() == 1 { "" } else { "s" }));
            for (k, v) in m.iter().take(40) {
                outline(v, &format!("{}: ", k), depth + 1, lines, budget);
            }
            if m.len() > 40 {
                lines.push(format!("{}  … {} more keys", pad, m.len() - 40));
            }
        }
        Node::Arr(a) => {
            lines.push(format!("{}{}array, {} item{}", pad, label, a.len(), if a.len() == 1 { "" } else { "s" }));
            if depth >= MAX_DEPTH {
                return;
            }
            for (i, v) in a.iter().take(2).enumerate() {
                outline(v, &format!("[{}] ", i), depth + 1, lines, budget);
            }
            if a.len() > 2 {
                // all-objects arrays: say which keys the items carry
                if a.iter().take(50).all(|x| matches!(x, Node::Map(_))) {
                    let mut keys: Vec<&str> = vec![];
                    for x in a.iter().take(50) {
                        if let Node::Map(m) = x {
                            for (k, _) in m {
                                if !keys.contains(&k.as_str()) && keys.len() < 12 {
                                    keys.push(k);
                                }
                            }
                        }
                    }
                    lines.push(format!("{}  … {} more; item keys seen: {}", pad, a.len() - 2, keys.join(", ")));
                } else {
                    lines.push(format!("{}  … {} more", pad, a.len() - 2));
                }
            }
        }
        scalar_node => lines.push(format!("{}{}{}", pad, label, scalar(scalar_node).unwrap_or_default())),
    }
}

/// Follow `a.b.0.c` into the tree.
fn at<'a>(root: &'a Node, path: &str) -> Result<&'a Node, DocError> {
    let mut cur = root;
    let mut walked = String::new();
    for seg in path.split(['.', '/']).filter(|s| !s.is_empty()) {
        let seg = seg.trim_start_matches('[').trim_end_matches(']');
        cur = match cur {
            Node::Map(m) => m.iter().find(|(k, _)| k == seg).map(|(_, v)| v).ok_or_else(|| {
                let keys: Vec<&str> = m.iter().take(20).map(|(k, _)| k.as_str()).collect();
                DocError(format!("no key `{}` at `{}`; keys here: {}", seg, walked, keys.join(", ")))
            })?,
            Node::Arr(a) => seg
                .parse::<usize>()
                .ok()
                .and_then(|i| a.get(i))
                .ok_or_else(|| DocError(format!("`{}` is not an index into the {}-item array at `{}`", seg, a.len(), walked)))?,
            other => return Err(DocError(format!("`{}` is a {}; it has nothing at `{}`", walked, kind(other), seg))),
        };
        walked.push_str(&format!(".{}", seg));
    }
    Ok(cur)
}

pub fn extract(bytes: &[u8], format: Format, req: &Request, limits: &Limits) -> Result<Extract, DocError> {
    let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let text = String::from_utf8_lossy(body);
    let name = format.name();
    let parsed: Result<Node, String> = match format {
        Format::Json => serde_json::from_str::<serde_json::Value>(&text).map(|v| from_json(&v)).map_err(|e| e.to_string()),
        Format::Yaml => serde_yaml::from_str::<serde_yaml::Value>(&text).map(|v| from_yaml(&v)).map_err(|e| e.to_string()),
        _ => text.parse::<toml::Table>().map(|t| from_toml(&toml::Value::Table(t))).map_err(|e| e.to_string().lines().next().unwrap_or("").to_string()),
    };
    let root = match parsed {
        Ok(n) => n,
        Err(e) => {
            // not valid after all: show it as written and say why
            let mut shown = text.into_owned();
            let truncated = shown.len() > limits.max_output;
            if truncated {
                let mut n = limits.max_output;
                while !shown.is_char_boundary(n) {
                    n -= 1;
                }
                shown.truncate(n);
            }
            return Ok(Extract { format, text: shown, unit: Unit::Line, total_units: None, truncated, loss: vec![format!("not valid {} ({}); shown as written", name, e)], supported: true });
        }
    };
    let (node, label) = match &req.member {
        Some(p) => (at(&root, p)?, format!(" at `{}`", p)),
        None => (&root, String::new()),
    };
    let mut full = String::new();
    pretty(node, 0, &mut full);
    let small = full.len() <= SMALL;
    let mut out;
    let mut loss = vec![];
    if small && (format == Format::Json || req.member.is_some()) {
        out = format!("{}{}, shown in full as JSON\n{}", name, label, full);
    } else if small {
        out = String::from_utf8_lossy(body).into_owned();
        if let Some(p) = &req.member {
            out = format!("{} at `{}`\n{}", name, p, full);
        }
    } else {
        let mut lines = vec![];
        let mut budget = limits.max_output / 60;
        outline(node, "", 0, &mut lines, &mut budget);
        out = format!("{}{}: {} bytes, {}. Outline (pass `member`, a path such as `users.0.name`, to read part of it):\n{}", name, label, bytes.len(), kind(node), lines.join("\n"));
        loss.push("large file shown as an outline: strings cut at 80 characters, arrays show their first 2 items".to_string());
    }
    if format == Format::Json || format == Format::Toml {
        // serde keeps map keys sorted unless told otherwise
        if matches!(node, Node::Map(_)) && !small {
            loss.push("keys are listed in sorted order, not file order".to_string());
        }
    }
    let truncated = out.len() > limits.max_output;
    if truncated {
        let mut n = limits.max_output;
        while !out.is_char_boundary(n) {
            n -= 1;
        }
        out.truncate(n);
    }
    Ok(Extract { format, text: out, unit: Unit::Line, total_units: None, truncated, loss, supported: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(s: &str, f: Format, member: Option<&str>) -> Result<Extract, DocError> {
        extract(s.as_bytes(), f, &Request { member: member.map(String::from), ..Request::default() }, &Limits::default())
    }

    #[test]
    fn small_json_is_pretty_printed_and_paths_drill_in() {
        let e = ex(r#"{"b":[1,2,{"c":"x"}],"a":null,"s":"é\n"}"#, Format::Json, None).unwrap();
        assert!(e.text.contains("shown in full as JSON\n{\n  \"a\": null,\n  \"b\": [\n    1,") && e.text.contains("\"s\": \"é\\n\""), "{}", e.text);
        let p = ex(r#"{"b":[1,2,{"c":"x"}]}"#, Format::Json, Some("b.2.c")).unwrap();
        assert!(p.text.contains("at `b.2.c`") && p.text.ends_with("\"x\""), "{}", p.text);
        let e = ex(r#"{"b":[1]}"#, Format::Json, Some("b.7")).unwrap_err();
        assert!(e.0.contains("not an index into the 1-item array"), "{}", e);
        let e = ex(r#"{"b":1,"c":2}"#, Format::Json, Some("zzz")).unwrap_err();
        assert!(e.0.contains("no key `zzz`") && e.0.contains("keys here: b, c"), "{}", e);
        assert!(ex(r#"{"b":1}"#, Format::Json, Some("b.x")).unwrap_err().0.contains("is a number"));
    }

    #[test]
    fn a_large_document_is_an_outline_that_names_its_shape() {
        let items: Vec<String> = (0..2000).map(|i| format!(r#"{{"id":{},"name":"user number {}","tags":["a","b"],"profile":{{"bio":"{}"}}}}"#, i, i, "x".repeat(200))).collect();
        let s = format!(r#"{{"users":[{}],"meta":{{"count":2000,"source":"test"}}}}"#, items.join(","));
        let e = ex(&s, Format::Json, None).unwrap();
        assert!(e.text.contains("Outline") && e.text.contains("users: array, 2000 items") && e.text.contains("… 1998 more; item keys seen: id, name, profile, tags"), "{}", &e.text[..600.min(e.text.len())]);
        assert!(e.text.contains("meta: object, 2 keys") && e.text.contains("count: 2000"), "{}", e.text);
        assert!(!e.text.contains(&"x".repeat(100)), "long strings are cut");
        assert!(e.text.len() < 4000, "{}", e.text.len());
        let sub = ex(&s, Format::Json, Some("users.1999.name")).unwrap();
        assert!(sub.text.contains("user number 1999"), "{}", sub.text);
    }

    #[test]
    fn yaml_and_toml_read_through_the_same_view() {
        let y = ex("name: delphi\nitems:\n  - a\n  - b: 1\n", Format::Yaml, None).unwrap();
        assert!(y.text.contains("name: delphi"), "small YAML is shown as written: {}", y.text);
        let p = ex("name: delphi\nitems:\n  - a\n  - b: 1\n", Format::Yaml, Some("items.1.b")).unwrap();
        assert!(p.text.ends_with('1'), "{}", p.text);
        let t = ex("[package]\nname = \"x\"\n[[bin]]\nname = \"a\"\n", Format::Toml, Some("bin.0.name")).unwrap();
        assert!(t.text.ends_with("\"a\""), "{}", t.text);
    }

    #[test]
    fn invalid_data_is_shown_as_written_with_the_reason() {
        let e = ex("{\"a\": ", Format::Json, None).unwrap();
        assert!(e.text.starts_with("{\"a\":") && e.loss[0].contains("not valid JSON"), "{:?}", e.loss);
        assert!(ex("a: [1, 2", Format::Yaml, None).unwrap().loss[0].contains("not valid YAML"));
        assert!(ex("a = ", Format::Toml, None).unwrap().loss[0].contains("not valid TOML"));
    }

    #[test]
    fn deeply_nested_and_hostile_inputs_are_survivable() {
        let deep = format!("{}1{}", "[".repeat(100), "]".repeat(100));
        let _ = ex(&deep, Format::Json, None);
        let _ = ex(&"[".repeat(100_000), Format::Json, None);
        let _ = ex(&"a:\n  - ".repeat(2000), Format::Yaml, None);
        // "billion laughs": nine levels of nine references each would expand to 9^9 items
        let mut bomb = String::from("a0: &a0 [1,2,3,4,5,6,7,8,9]\n");
        for i in 1..9 {
            bomb.push_str(&format!("a{}: &a{} [{}]\n", i, i, vec![format!("*a{}", i - 1); 9].join(",")));
        }
        let started = std::time::Instant::now();
        let e = ex(&bomb, Format::Yaml, None).unwrap();
        assert!(started.elapsed().as_secs() < 20, "alias expansion must be cut off quickly");
        assert!(e.text.len() <= Limits::default().max_output + 100);
    }
}
