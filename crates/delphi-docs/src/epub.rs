//! EPUB books (`specs/014` §4): the spine's chapters, in reading order, converted from XHTML to Markdown.
//! Styles, images and fonts are not read. A chapter is a page of the result: `start`/`end` are chapter numbers.

use crate::ooxml::{resolve, Pkg};
use crate::sniff::Format;
use crate::xml::{attr, walk, Ev};
use crate::{DocError, Extract, Request, Unit};
use std::collections::HashMap;

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let Some(v) = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn extract(pkg: &Pkg, req: &Request) -> Result<Extract, DocError> {
    let container = pkg.part("META-INF/container.xml")?.ok_or_else(|| DocError("the EPUB has no META-INF/container.xml".into()))?;
    let mut opf_path = None;
    let _ = walk(&container, pkg.limits.max_nodes, |ev| {
        if let Ev::Start { name, attrs } = ev {
            if name == "rootfile" && opf_path.is_none() {
                opf_path = attr(&attrs, "full-path").map(percent_decode);
            }
        }
    });
    let opf_path = opf_path.ok_or_else(|| DocError("the EPUB names no package document".into()))?;
    let opf = pkg.part(&opf_path)?.ok_or_else(|| DocError(format!("the package document `{}` is missing", opf_path)))?;
    let (mut manifest, mut spine): (HashMap<String, (String, String)>, Vec<String>) = (HashMap::new(), vec![]);
    let (mut title, mut author) = (String::new(), String::new());
    let mut capture: Option<&'static str> = None;
    let _ = walk(&opf, pkg.limits.max_nodes, |ev| match ev {
        Ev::Start { name, attrs } => match name.as_str() {
            "item" => {
                if let (Some(id), Some(href)) = (attr(&attrs, "id"), attr(&attrs, "href")) {
                    manifest.insert(id.to_string(), (resolve(&opf_path, &percent_decode(href)), attr(&attrs, "media-type").unwrap_or("").to_string()));
                }
            }
            "itemref" => {
                if let Some(id) = attr(&attrs, "idref") {
                    spine.push(id.to_string());
                }
            }
            "title" if title.is_empty() => capture = Some("title"),
            "creator" if author.is_empty() => capture = Some("creator"),
            _ => {}
        },
        Ev::Text(t) => match capture {
            Some("title") => title.push_str(&t),
            Some("creator") => author.push_str(&t),
            _ => {}
        },
        Ev::End { .. } => capture = None,
    });
    let chapters: Vec<String> = spine.iter().filter_map(|id| manifest.get(id)).filter(|(_, mt)| mt.contains("html")).map(|(p, _)| p.clone()).collect();
    if chapters.is_empty() {
        return Err(DocError("the EPUB's reading order lists no chapters".into()));
    }
    let total = chapters.len();
    let first = req.start.unwrap_or(1).max(1);
    let last = req.end.unwrap_or(total).min(total);
    if first > total {
        return Err(DocError(format!("the book has {} chapters; start is {}", total, first)));
    }
    let mut head = format!("EPUB · {} chapter{}", total, if total == 1 { "" } else { "s" });
    if !title.trim().is_empty() {
        head.push_str(&format!(" · {}", title.trim()));
    }
    if !author.trim().is_empty() {
        head.push_str(&format!(" · {}", author.trim()));
    }
    let mut text = format!("{}\n\n", head);
    let mut loss = vec![];
    let mut shown_to = first - 1;
    for n in first..=last {
        let path = &chapters[n - 1];
        let Some(bytes) = pkg.part(path)? else {
            loss.push(format!("chapter {} (`{}`) is missing from the book", n, path));
            continue;
        };
        let (md, l) = crate::html::to_markdown(&String::from_utf8_lossy(&bytes), pkg.limits.max_nodes);
        loss.extend(l.into_iter().filter(|x| !x.contains("without alt text")));
        let block = format!("## Chapter {}\n\n{}\n\n", n, md);
        if shown_to >= first && text.len() + block.len() > pkg.limits.max_output {
            break;
        }
        text.push_str(&block);
        shown_to = n;
    }
    if shown_to < total {
        text.push_str(&format!("(chapters {}-{} of {} shown; pass start/end to page)\n", first, shown_to, total));
    }
    loss.push("images, styles and fonts are not read".to_string());
    Ok(Extract { format: Format::Epub, text, unit: Unit::Page, total_units: Some(total), truncated: shown_to < last, loss, supported: true })
}
