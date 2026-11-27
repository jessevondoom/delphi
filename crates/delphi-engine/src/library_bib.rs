//! Bibliography export (`specs/055` §3.2.1, §11 SK2): APA, MLA, Chicago, plain text, and the two interchange forms
//! reference managers read, CSL-JSON and BibTeX.
//!
//! Built from the CSL fields of a source's record alone, deterministically. The styles are compact renderings of the
//! common cases (author, date, title, container, publisher, DOI or URL), not a full CSL processor; CSL-JSON is the lossless
//! form for anyone who wants an exact style. Only citation fields are written: no Delphi field, no capture text, no excerpt.
//! A private source is left out of every export (`specs/055` §10.1) and is only counted, so the operator can see why.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Apa,
    Mla,
    Chicago,
    Plain,
    CslJson,
    BibTex,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        match s {
            "apa" => Some(Format::Apa),
            "mla" => Some(Format::Mla),
            "chicago" => Some(Format::Chicago),
            "plain" => Some(Format::Plain),
            "csl-json" => Some(Format::CslJson),
            "bibtex" => Some(Format::BibTex),
            _ => None,
        }
    }
}

const CSL_FIELDS: [&str; 22] = ["type", "title", "author", "editor", "translator", "container-title", "publisher", "publisher-place", "issued", "volume", "issue", "page", "edition", "URL", "DOI", "ISBN", "ISSN", "language", "abstract", "note", "collection-number", "accessed"];

pub struct Export {
    pub text: String,
    /// Sources left out because they are private.
    pub withheld: usize,
}

/// The CSL-JSON item of one record: the standard fields only, with `id` and `accessed` (the retrieval date) added.
pub fn csl_item(id: &str, meta: &Value) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), json!(id));
    for k in CSL_FIELDS {
        if !meta[k].is_null() {
            o.insert(k.into(), meta[k].clone());
        }
    }
    if let Some(d) = meta["retrieved"].as_str().and_then(|r| parts(&json!(r))) {
        o.insert("accessed".into(), json!({"date-parts": [d]}));
    }
    Value::Object(o)
}

/// Export `ids` (all sources when empty) in `format`, with no person to confirm a labelled citation. Unknown ids are an error; a
/// source whose label refuses, and one whose label asks (`internal`), are withheld and counted. See [`export_with`].
pub fn export(lib: &Library, ids: &[String], format: Format) -> Result<Export, String> {
    export_with(lib, ids, format, crate::egress::Confirm::Nobody)
}

/// The bibliography sink (`specs/141` §2.4, its row): each source's label is computed from the records as they are now and decided at
/// `Sink::Bibliography`. A refusing mark (private, confidential, library private, or a record that cannot be read) withholds the
/// citation. An `internal` citation is sent only when the operator confirms, once for the export, and the export then begins with
/// the visible line (`% Handling: …` in BibTeX); CSL-JSON has no place for the line, so a labelled citation is withheld there. A
/// held source's citation is sent, its free-text fields cut (§2.4 item 7).
pub fn export_with(lib: &Library, ids: &[String], format: Format, confirm: crate::egress::Confirm<'_>) -> Result<Export, String> {
    let all = lib.list();
    let chosen: Vec<&(String, Value)> = if ids.is_empty() {
        all.iter().collect()
    } else {
        ids.iter().map(|i| all.iter().find(|(id, _)| id == i).ok_or_else(|| format!("no source `{}`", i))).collect::<Result<_, _>>()?
    };
    let labeller = crate::label::Labeller::new(Some(lib), lib.workspace());
    let decided: Vec<(&(String, Value), crate::label::Label, crate::label::Kind)> = chosen
        .into_iter()
        .map(|c| {
            let l = labeller.source(&c.0).label;
            let k = crate::egress::check_label(&l, crate::egress::Sink::Bibliography).kind;
            (c, l, k)
        })
        .collect();
    use crate::label::Kind;
    let labelled: Vec<&(&(String, Value), crate::label::Label, Kind)> = decided.iter().filter(|(_, _, k)| *k == Kind::SendLabelled).collect();
    let joined = labelled.iter().fold(crate::label::Label::none(), |acc, (_, l, _)| acc.join(l));
    let line = crate::label::visible_line(&joined);
    let labelled_ok = !labelled.is_empty()
        && format != Format::CslJson
        && match confirm {
            crate::egress::Confirm::Terminal(ask) => ask(&format!("{} ({})", line.clone().unwrap_or_default(), crate::egress::check_label(&joined, crate::egress::Sink::Bibliography).reason)),
            crate::egress::Confirm::Card | crate::egress::Confirm::Nobody => false,
        };
    let sendable = |k: Kind| k == Kind::Send || k == Kind::Quotations || (k == Kind::SendLabelled && labelled_ok);
    let chosen: Vec<&(String, Value)> = decided.iter().filter(|(_, _, k)| sendable(*k)).map(|(c, _, _)| *c).collect();
    // The shared egress check (`specs/098` §12 item 16): a private source, and a record that cannot be read, never leave through an export.
    let leaves = |m: &Value| crate::egress::check(m, Channel::Export).is_ok();
    let withheld = decided.len() - chosen.iter().filter(|(_, m)| leaves(m)).count();
    let mut out = render(lib, chosen.into_iter().filter(|(_, m)| leaves(m)).collect(), format);
    if labelled_ok {
        if let Some(l) = line {
            out = match format {
                Format::BibTex => format!("% {}\n\n{out}", l.trim_start_matches("> ")),
                _ => format!("{l}\n\n{out}"),
            };
        }
    }
    Ok(Export { text: out, withheld })
}

/// The citations of `ids` as a page of the library shows them in its own bibliography (`library_page_check`). Not a sink: nothing here
/// leaves the machine, and the page is checked at its own sink when it does. A private source is left out, a held one's free-text
/// fields cut, as at the bibliography sink.
pub(crate) fn for_page(lib: &Library, ids: &[String], format: Format) -> Result<Export, String> {
    let all = lib.list();
    let chosen: Vec<&(String, Value)> = ids.iter().map(|i| all.iter().find(|(id, _)| id == i).ok_or_else(|| format!("no source `{}`", i))).collect::<Result<_, _>>()?;
    let leaves = |m: &Value| crate::egress::check(m, Channel::Export).is_ok();
    let withheld = chosen.iter().filter(|(_, m)| !leaves(m)).count();
    Ok(Export { text: render(lib, chosen.into_iter().filter(|(_, m)| leaves(m)).collect(), format), withheld })
}

/// The citations of `chosen` in `format`. A held source's citation is sent; its text is not (`specs/141` §2.4 item 7, the guard of
/// `specs/142` §2.5 item 4): every free-text field of its record that is sent is cut to the bound if it carries a run of the text.
fn render(lib: &Library, chosen: Vec<&(String, Value)>, format: Format) -> String {
    let held = lib.held();
    let cut: Vec<(String, Value)> = chosen
        .into_iter()
        .map(|(id, m)| {
            let mut m = m.clone();
            // `specs/142` §2.6 item 8: a source's summary is what a bibliography has of a source held as a pointer; it is its
            // `abstract` where the record has none, shown as written by a model.
            if m["abstract"].is_null() {
                if let Some(s) = crate::source_summary::summary_text(lib, id) {
                    m["abstract"] = json!(format!("{}: {}", crate::source_summary::SHOWN, s));
                }
            }
            if crate::library::is_held(&m) {
                for k in ["title", "container-title", "attribution_text", "abstract", "note"] {
                    if let Some(t) = m[k].as_str().map(String::from) {
                        m[k] = json!(held.cut_field(id, &t));
                    }
                }
            }
            (id.clone(), m)
        })
        .collect();
    let mut shown: Vec<&(String, Value)> = cut.iter().collect();
    shown.sort_by_key(|(id, m)| (sort_key(m), id.clone()));
    match format {
        Format::CslJson => serde_json::to_string_pretty(&shown.iter().map(|(id, m)| csl_item(id, m)).collect::<Vec<_>>()).unwrap_or_default() + "\n",
        Format::BibTex => shown.iter().map(|(id, m)| bibtex(id, m)).collect::<Vec<_>>().join("\n"),
        _ => shown.iter().map(|(_, m)| format!("{}\n", render_one(m, format))).collect(),
    }
}

fn sort_key(m: &Value) -> String {
    names(&m["author"]).into_iter().next().map(|n| n.family_or_literal().to_lowercase()).unwrap_or_else(|| m["title"].as_str().unwrap_or("").to_lowercase())
}

fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn text(m: &Value, k: &str) -> Option<String> {
    m[k].as_str().map(clean).filter(|s| !s.is_empty()).or_else(|| m[k].as_u64().map(|n| n.to_string()))
}

#[derive(Clone)]
struct Name {
    family: String,
    given: String,
    literal: String,
}

impl Name {
    fn family_or_literal(&self) -> &str {
        if self.family.is_empty() { &self.literal } else { &self.family }
    }

    fn initials(&self) -> String {
        self.given.split_whitespace().map(|p| p.split('-').map(|q| q.chars().next().map(|c| format!("{}.", c)).unwrap_or_default()).collect::<Vec<_>>().join("-")).collect::<Vec<_>>().join(" ")
    }
}

fn names(v: &Value) -> Vec<Name> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|n| Name { family: clean(n["family"].as_str().unwrap_or("")), given: clean(n["given"].as_str().unwrap_or("")), literal: clean(n["literal"].as_str().unwrap_or("")) })
                .filter(|n| !n.family.is_empty() || !n.given.is_empty() || !n.literal.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// `[year, month, day]` as far as the record states them.
fn parts(v: &Value) -> Option<Vec<u32>> {
    if let Some(p) = v["date-parts"][0].as_array() {
        let p: Vec<u32> = p.iter().filter_map(|x| x.as_u64().or_else(|| x.as_str()?.parse().ok()).map(|n| n as u32)).collect();
        return (!p.is_empty()).then_some(p);
    }
    let s = v.as_str().or_else(|| v["raw"].as_str())?;
    let mut out = Vec::new();
    for (i, seg) in s.get(..10.min(s.len()))?.split('-').enumerate() {
        match seg.parse::<u32>() {
            Ok(n) if i == 0 && seg.len() == 4 => out.push(n),
            Ok(n) if i > 0 && !out.is_empty() => out.push(n),
            _ => break,
        }
    }
    (!out.is_empty()).then_some(out)
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn month(n: u32, short: bool) -> String {
    let m = MONTHS.get((n as usize).wrapping_sub(1)).copied().unwrap_or("");
    if short && m.len() > 4 { format!("{}.", &m[..3]) } else { m.to_string() }
}

fn link(m: &Value) -> Option<String> {
    text(m, "DOI").map(|d| format!("https://doi.org/{}", d.trim_start_matches("https://doi.org/"))).or_else(|| text(m, "URL"))
}

/// End a fragment with a full stop unless it ends in terminal punctuation.
fn stop(s: &str) -> String {
    if s.ends_with(['.', '?', '!']) { s.to_string() } else { format!("{}.", s) }
}

fn render_one(m: &Value, f: Format) -> String {
    let au = names(&m["author"]);
    let title = text(m, "title").unwrap_or_else(|| "Untitled".into());
    let container = text(m, "container-title");
    let publisher = text(m, "publisher");
    let date = parts(&m["issued"]);
    let year = date.as_ref().map(|d| d[0].to_string());
    let article = matches!(m["type"].as_str(), Some("article-journal") | Some("article") | Some("chapter") | Some("entry-encyclopedia") | Some("webpage") | Some("article-newspaper") | Some("article-magazine"));
    let mut out: Vec<String> = Vec::new();
    match f {
        Format::Apa => {
            let who = match au.len() {
                0 => None,
                1 => Some(apa_name(&au[0])),
                n if n <= 20 => Some(format!("{}, & {}", au[..n - 1].iter().map(apa_name).collect::<Vec<_>>().join(", "), apa_name(&au[n - 1]))),
                _ => Some(format!("{}, ... {}", au[..19].iter().map(apa_name).collect::<Vec<_>>().join(", "), apa_name(&au[au.len() - 1]))),
            };
            let when = format!("({})", year.clone().unwrap_or_else(|| "n.d.".into()));
            match &who {
                Some(w) => out.push(format!("{} {}", stop(w), stop(&when))),
                None => {
                    out.push(stop(&title));
                    out.push(stop(&when));
                }
            }
            if who.is_some() {
                out.push(stop(&title));
            }
            if let Some(c) = &container {
                let mut c = c.clone();
                if let Some(v) = text(m, "volume") {
                    c = format!("{}, {}", c, v);
                    if let Some(i) = text(m, "issue") {
                        c = format!("{}({})", c, i);
                    }
                }
                if let Some(p) = text(m, "page") {
                    c = format!("{}, {}", c, p);
                }
                out.push(stop(&c));
            }
            if let Some(p) = publisher.filter(|p| Some(p) != container.as_ref()) {
                out.push(stop(&p));
            }
            if let Some(l) = link(m) {
                out.push(l);
            }
        }
        Format::Mla => {
            let who = match au.len() {
                0 => None,
                1 => Some(full_name(&au[0], true)),
                2 => Some(format!("{}, and {}", full_name(&au[0], true), full_name(&au[1], false))),
                _ => Some(format!("{}, et al", full_name(&au[0], true))),
            };
            if let Some(w) = who {
                out.push(stop(&w));
            }
            out.push(if article { format!("\u{201c}{}.\u{201d}", title.trim_end_matches('.')) } else { stop(&title) });
            let mut tail: Vec<String> = Vec::new();
            tail.extend(container.clone());
            tail.extend(text(m, "volume").map(|v| format!("vol. {}", v)));
            tail.extend(text(m, "issue").map(|v| format!("no. {}", v)));
            tail.extend(publisher.filter(|p| Some(p) != container.as_ref()));
            if let Some(d) = &date {
                tail.push(match d.len() {
                    1 => d[0].to_string(),
                    2 => format!("{} {}", month(d[1], true), d[0]),
                    _ => format!("{} {} {}", d[2], month(d[1], true), d[0]),
                });
            }
            tail.extend(text(m, "page").map(|p| format!("pp. {}", p)));
            tail.extend(link(m));
            if !tail.is_empty() {
                out.push(stop(&tail.join(", ")));
            }
        }
        Format::Chicago => {
            let who = match au.len() {
                0 => None,
                1 => Some(full_name(&au[0], true)),
                2 => Some(format!("{}, and {}", full_name(&au[0], true), full_name(&au[1], false))),
                n if n <= 10 => Some(format!("{}, and {}", std::iter::once(full_name(&au[0], true)).chain(au[1..n - 1].iter().map(|a| full_name(a, false))).collect::<Vec<_>>().join(", "), full_name(&au[n - 1], false))),
                _ => Some(format!("{}, et al", full_name(&au[0], true))),
            };
            if let Some(w) = who {
                out.push(stop(&w));
            }
            out.push(if article { format!("\u{201c}{}.\u{201d}", title.trim_end_matches('.')) } else { stop(&title) });
            if let Some(c) = &container {
                let mut c = c.clone();
                if let Some(v) = text(m, "volume") {
                    c = format!("{} {}", c, v);
                    if let Some(i) = text(m, "issue") {
                        c = format!("{}, no. {}", c, i);
                    }
                }
                out.push(stop(&c));
            }
            let imprint: Vec<String> = [publisher.filter(|p| Some(p) != container.as_ref()), year.clone()].into_iter().flatten().collect();
            if !imprint.is_empty() {
                out.push(stop(&imprint.join(", ")));
            }
            if let Some(l) = link(m) {
                out.push(l);
            }
        }
        Format::Plain => {
            if !au.is_empty() {
                out.push(stop(&au.iter().map(|a| full_name(a, false)).collect::<Vec<_>>().join(", ")));
            }
            out.push(stop(&title));
            out.extend(container.map(|c| stop(&c)));
            out.extend(publisher.map(|p| stop(&p)));
            if let Some(d) = &date {
                out.push(stop(&match d.len() {
                    1 => d[0].to_string(),
                    2 => format!("{} {}", month(d[1], false), d[0]),
                    _ => format!("{} {} {}", d[2], month(d[1], false), d[0]),
                }));
            }
            out.extend(link(m));
            if let Some(r) = m["retrieved"].as_str().and_then(|r| parts(&json!(r))).filter(|_| m["URL"].is_string()) {
                out.push(format!("(retrieved {})", r.iter().map(u32::to_string).map(|x| if x.len() == 1 { format!("0{}", x) } else { x }).collect::<Vec<_>>().join("-")));
            }
        }
        Format::CslJson | Format::BibTex => {}
    }
    out.join(" ")
}

fn apa_name(n: &Name) -> String {
    if n.family.is_empty() { n.literal.clone() } else if n.given.is_empty() { n.family.clone() } else { format!("{}, {}", n.family, n.initials()) }
}

fn full_name(n: &Name, inverted: bool) -> String {
    if n.family.is_empty() {
        n.literal.clone()
    } else if n.given.is_empty() {
        n.family.clone()
    } else if inverted {
        format!("{}, {}", n.family, n.given)
    } else {
        format!("{} {}", n.given, n.family)
    }
}

fn tex(s: &str) -> String {
    let mut o = String::new();
    for c in clean(s).chars() {
        match c {
            '\\' => o.push_str("\\textbackslash{}"),
            '{' | '}' | '&' | '%' | '$' | '#' | '_' => {
                o.push('\\');
                o.push(c);
            }
            '^' => o.push_str("\\^{}"),
            '~' => o.push_str("\\~{}"),
            c => o.push(c),
        }
    }
    o
}

fn bibtex(id: &str, m: &Value) -> String {
    let kind = match m["type"].as_str() {
        Some("article-journal") => "article",
        Some("book") => "book",
        Some("chapter") => "incollection",
        Some("report") => "techreport",
        Some("thesis") => "phdthesis",
        _ => "misc",
    };
    let mut f: Vec<(&str, String)> = Vec::new();
    let au = names(&m["author"]);
    if !au.is_empty() {
        // a literal (an organisation) is braced so BibTeX does not split it
        f.push(("author", au.iter().map(|n| if n.family.is_empty() { format!("{{{}}}", tex(&n.literal)) } else { format!("{}, {}", tex(&n.family), tex(&n.given)) }).collect::<Vec<_>>().join(" and ")));
    }
    f.push(("title", tex(&text(m, "title").unwrap_or_default())));
    let container = text(m, "container-title");
    if let Some(c) = &container {
        f.push((if kind == "article" { "journal" } else { "booktitle" }, tex(c)));
    }
    if let Some(d) = parts(&m["issued"]) {
        f.push(("year", d[0].to_string()));
        if d.len() > 1 {
            f.push(("month", d[1].to_string()));
        }
    }
    for (bk, ck) in [("volume", "volume"), ("number", "issue"), ("pages", "page"), ("publisher", "publisher"), ("edition", "edition"), ("doi", "DOI"), ("isbn", "ISBN"), ("issn", "ISSN"), ("url", "URL")] {
        if let Some(v) = text(m, ck) {
            f.push((bk, if bk == "url" || bk == "doi" { clean(&v) } else { tex(&v) }));
        }
    }
    let key: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    format!("@{}{{{},\n{}\n}}\n", kind, key, f.iter().map(|(k, v)| format!("  {} = {{{}}}", k, v)).collect::<Vec<_>>().join(",\n"))
}
