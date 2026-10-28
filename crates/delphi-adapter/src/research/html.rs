//! A page as markdown (`specs/028` §10.2): headings, paragraphs, lists, links, emphasis and code are kept; scripts, styles, forms and
//! page chrome are dropped. Tolerant of broken HTML and never panics; it is not an HTML parser, only enough of one to store a
//! readable copy of a reference page.

use reqwest::Url;

/// Elements whose whole subtree is dropped.
const SKIP: [&str; 14] = ["script", "style", "noscript", "svg", "head", "form", "nav", "footer", "aside", "iframe", "template", "button", "select", "textarea"];
/// Class or id fragments that mark page chrome rather than content (Wikipedia's edit links, nav boxes and footers, cookie banners).
const CHROME: [&str; 10] = ["navbox", "mw-editsection", "noprint", "printfooter", "catlinks", "sidebar", "cookie", "toc", "mw-jump-link", "vector-"];
const VOID: [&str; 14] = ["br", "hr", "img", "input", "meta", "link", "area", "base", "col", "embed", "source", "track", "wbr", "param"];

pub struct Converted {
    pub title: String,
    pub markdown: String,
}

pub(crate) fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
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

/// The attributes of a start tag as `(name, value)` pairs, names lowercased.
pub(crate) fn attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = tag.as_bytes();
    let mut i = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len());
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let s = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        if s == i {
            break;
        }
        let name = tag[s..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut val = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let vs = i + 1;
                i = vs;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                val = tag[vs..i.min(tag.len())].to_string();
                i += 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                val = tag[vs..i].to_string();
            }
        }
        out.push((name, val));
    }
    out
}

fn is_chrome(a: &[(String, String)]) -> bool {
    a.iter().any(|(k, v)| (k == "class" || k == "id") && CHROME.iter().any(|c| v.to_ascii_lowercase().contains(c)))
}

/// An image's address as it is written in markdown and used everywhere after: parentheses are percent-encoded, so a `)` in a Wikipedia
/// file name cannot end the markdown early.
pub fn image_key(u: &Url) -> String {
    u.as_str().replace('(', "%28").replace(')', "%29")
}

/// An image in converted markdown, `![alt](url)`: where it starts and ends in the text, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageTok {
    pub start: usize,
    pub end: usize,
    pub alt: String,
    pub url: String,
}

/// The images of `md` in order. The converter writes the alt text without brackets or parentheses and the address percent-encoded, so
/// the first `](` after `![` and the next `)` close the token.
pub fn find_images(md: &str) -> Vec<ImageTok> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = md[i..].find("![") {
        let s = i + p;
        let Some(m) = md[s + 2..].find("](") else { break };
        let alt_end = s + 2 + m;
        let Some(c) = md[alt_end + 2..].find(')') else { break };
        let end = alt_end + 2 + c + 1;
        let alt = &md[s + 2..alt_end];
        if alt.contains(['\n', '[', ']']) {
            i = s + 2;
            continue;
        }
        out.push(ImageTok { start: s, end, alt: alt.to_string(), url: md[alt_end + 2..end - 1].to_string() });
        i = end;
    }
    out
}

/// `md` without its images, for text the scout reads and for entries kept as a summary: no picture, no alt text.
pub fn strip_images(md: &str) -> String {
    let (mut out, mut at) = (String::with_capacity(md.len()), 0);
    for t in find_images(md) {
        out.push_str(&md[at..t.start]);
        at = t.end;
    }
    out.push_str(&md[at..]);
    out
}

/// Convert `html` (fetched from `base`) to markdown. Output is capped at `max_chars`, cut at a paragraph boundary. Images are left out.
pub fn to_markdown(html: &str, base: &str, max_chars: usize) -> Converted {
    convert(html, base, max_chars, false)
}

/// As `to_markdown`, with each `<img>` that has an `http(s)` address kept as `![alt](address)` (`specs/028` §11). A link whose only
/// content is an image becomes the image alone, since a picture is drawn only when it stands alone in its paragraph.
pub fn to_markdown_with_images(html: &str, base: &str, max_chars: usize) -> Converted {
    convert(html, base, max_chars, true)
}

fn convert(html: &str, base: &str, max_chars: usize, images: bool) -> Converted {
    let base = Url::parse(base).ok();
    let mut title = String::new();
    let (mut out, mut i) = (String::new(), 0usize);
    // The stack of open elements we care about for closing: (name, closing text).
    let mut stack: Vec<(String, String)> = Vec::new();
    let mut skip: Option<(String, usize)> = None; // (tag, nesting depth) of the subtree being dropped
    let mut in_pre = false;
    let mut link: Option<(String, usize)> = None; // href and where the link text starts in `out`
    let b = html.as_bytes();
    let lower = html.to_ascii_lowercase();

    // `<title>` is inside `<head>`, which is skipped, so read it first.
    if let (Some(s), Some(e)) = (lower.find("<title"), lower.find("</title>")) {
        if let Some(gt) = lower[s..].find('>') {
            if s + gt < e {
                title = decode(html[s + gt + 1..e].trim()).split_whitespace().collect::<Vec<_>>().join(" ");
            }
        }
    }

    while i < b.len() && out.len() < max_chars * 2 {
        if b[i] != b'<' {
            let next = html[i..].find('<').map_or(html.len(), |n| i + n);
            if skip.is_none() {
                let t = decode(&html[i..next]);
                if in_pre {
                    out.push_str(&t);
                } else {
                    let collapsed: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !collapsed.is_empty() {
                        if (t.starts_with(char::is_whitespace) && !out.ends_with([' ', '\n'])) && !out.is_empty() {
                            out.push(' ');
                        }
                        out.push_str(&collapsed);
                        if t.ends_with(char::is_whitespace) {
                            out.push(' ');
                        }
                    }
                }
            }
            i = next;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i..].find("-->").map_or(html.len(), |n| i + n + 3);
            continue;
        }
        let Some(gt) = html[i..].find('>') else { break };
        let inner = &html[i + 1..i + gt];
        i += gt + 1;
        let closing = inner.starts_with('/');
        let name_src = inner.trim_start_matches('/');
        let name: String = name_src.chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        if let Some((sk, depth)) = &mut skip {
            if *sk == name {
                if closing {
                    if *depth == 0 {
                        skip = None;
                    } else {
                        *depth -= 1;
                    }
                } else if !inner.ends_with('/') {
                    *depth += 1;
                }
            }
            continue;
        }
        if closing {
            if let Some(p) = stack.iter().rposition(|(n, _)| *n == name) {
                let (_, text) = stack.remove(p);
                stack.truncate(p);
                if name == "a" {
                    if let Some((href, start)) = link.take() {
                        let label = out[start..].trim().to_string();
                        out.truncate(start);
                        if images && label.starts_with("![") && label.ends_with(')') && find_images(&label).len() == 1 && find_images(&label)[0].end == label.len() {
                            out.push_str(&label); // a picture that was a link: the picture alone
                        } else if !label.is_empty() {
                            out.push_str(&format!("[{}]({})", label, href));
                        }
                    }
                    continue;
                }
                if name == "pre" {
                    in_pre = false;
                }
                out.push_str(&text);
            }
            continue;
        }
        let a = attrs(inner);
        if SKIP.contains(&name.as_str()) || is_chrome(&a) || a.iter().any(|(k, v)| k == "hidden" || (k == "aria-hidden" && v == "true")) {
            if !VOID.contains(&name.as_str()) && !inner.ends_with('/') {
                skip = Some((name, 0));
            }
            continue;
        }
        let block = |out: &mut String| {
            let t = out.trim_end_matches(' ').len();
            out.truncate(t);
            if !out.is_empty() && !out.ends_with("\n\n") {
                out.push_str(if out.ends_with('\n') { "\n" } else { "\n\n" });
            }
        };
        let mut close = String::new();
        match name.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                block(&mut out);
                out.push_str(&"#".repeat(name[1..].parse::<usize>().unwrap_or(2)));
                out.push(' ');
                close = "\n\n".into();
            }
            "p" | "div" | "section" | "article" | "main" | "table" | "ul" | "ol" | "dl" | "figure" | "figcaption" | "tr" | "header" => {
                block(&mut out);
                close = "\n\n".into();
            }
            "li" | "dt" | "dd" => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("- ");
                close = "\n".into();
            }
            "blockquote" => {
                block(&mut out);
                out.push_str("> ");
                close = "\n\n".into();
            }
            "pre" => {
                block(&mut out);
                out.push_str("```\n");
                in_pre = true;
                close = "\n```\n\n".into();
            }
            "code" if !in_pre => {
                out.push('`');
                close = "`".into();
            }
            "strong" | "b" => {
                out.push_str("**");
                close = "**".into();
            }
            "em" | "i" => {
                out.push('*');
                close = "*".into();
            }
            "td" | "th" => {
                if !out.ends_with(['\n', ' ']) {
                    out.push_str(" | ");
                }
            }
            "img" if images => {
                let src = a.iter().find(|(k, _)| k == "src").or_else(|| a.iter().find(|(k, _)| k == "data-src")).map(|(_, v)| decode(v.trim())).unwrap_or_default();
                let abs = base.as_ref().and_then(|bs| bs.join(&src).ok()).filter(|u| matches!(u.scheme(), "http" | "https")).map(|mut u| {
                    u.set_fragment(None);
                    image_key(&u)
                });
                if let Some(u) = abs {
                    let alt: String = decode(a.iter().find(|(k, _)| k == "alt").map(|(_, v)| v.as_str()).unwrap_or(""))
                        .chars()
                        .map(|c| if matches!(c, '[' | ']' | '(' | ')') || c.is_control() { ' ' } else { c })
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    out.push_str(&format!("![{}]({})", alt.chars().take(200).collect::<String>(), u));
                }
            }
            "br" => out.push_str("  \n"),
            "hr" => {
                block(&mut out);
                out.push_str("---\n\n");
            }
            "a" => {
                let href = a.iter().find(|(k, _)| k == "href").map(|(_, v)| decode(v.trim())).unwrap_or_default();
                let abs = base.as_ref().and_then(|bs| bs.join(&href).ok()).filter(|u| matches!(u.scheme(), "http" | "https")).map(|mut u| {
                    u.set_fragment(None);
                    u.to_string()
                });
                if let Some(h) = abs {
                    link = Some((h, out.len()));
                    stack.push((name, String::new()));
                }
                continue;
            }
            _ => {}
        }
        if !VOID.contains(&name.as_str()) && !inner.ends_with('/') {
            stack.push((name, close));
        }
    }
    if let Some((_, start)) = link {
        out.truncate(start); // an unclosed link keeps its text, without the markup
    }
    let mut md = String::new();
    let mut blank = 0;
    for line in out.lines() {
        let l = line.trim_end();
        if l.is_empty() {
            blank += 1;
            if blank <= 1 && !md.is_empty() {
                md.push('\n');
            }
        } else {
            blank = 0;
            md.push_str(l);
            md.push('\n');
        }
    }
    let mut md = md.trim().to_string();
    if md.chars().count() > max_chars {
        let cut: String = md.chars().take(max_chars).collect();
        let at = cut.rfind("\n\n").unwrap_or(cut.len());
        md = format!("{}\n\n[…the page continues at its source]", cut[..at].trim_end());
    }
    Converted { title, markdown: md }
}

/// The readable text of a page without markup, for the scout to read and for checking its quotes against.
pub fn plain_text(markdown: &str) -> String {
    let mut s = String::with_capacity(markdown.len());
    let mut chars = markdown.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '[' => {}
            ']' => {
                if chars.peek() == Some(&'(') {
                    for d in chars.by_ref() {
                        if d == ')' {
                            break;
                        }
                    }
                }
            }
            '*' | '`' | '#' | '>' => {}
            c => s.push(c),
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r##"<!doctype html><html><head><title>Byzantine fault &#8211; Wikipedia</title><style>p{}</style><script>alert(1)</script></head>
<body><nav>Menu <a href="/x">skip</a></nav>
<div class="mw-editsection">[edit]</div>
<h1>Byzantine fault</h1>
<p>A <b>Byzantine</b> fault is <i>any</i> fault with <code>arbitrary</code> behaviour &amp; more.
See <a href="/wiki/Consensus#cite">consensus</a> and <a href="https://example.org/p?a=1&amp;b=2">a paper</a>.</p>
<ul><li>one</li><li>two <a href="javascript:x()">js</a></li></ul>
<pre>fn main() {}
  let x = 1 &lt; 2;</pre>
<blockquote>quoted</blockquote>
<table><tr><td>a</td><td>b</td></tr></table>
<footer>© footer</footer></body></html>"##;

    #[test]
    fn a_page_becomes_readable_markdown_without_its_chrome() {
        let c = to_markdown(PAGE, "https://en.wikipedia.org/wiki/Byzantine_fault", 100_000);
        assert_eq!(c.title, "Byzantine fault – Wikipedia");
        let m = &c.markdown;
        assert!(m.starts_with("# Byzantine fault\n\nA **Byzantine** fault is *any* fault with `arbitrary` behaviour & more."), "{}", m);
        assert!(m.contains("[consensus](https://en.wikipedia.org/wiki/Consensus)"), "{}", m);
        assert!(m.contains("[a paper](https://example.org/p?a=1&b=2)"), "{}", m);
        assert!(m.contains("- one\n- two js"), "a javascript: link keeps its text and loses its target:\n{}", m);
        assert!(m.contains("```\nfn main() {}\n  let x = 1 < 2;\n```"), "{}", m);
        assert!(m.contains("> quoted"));
        for gone in ["alert", "Menu", "skip", "[edit]", "footer", "p{}"] {
            assert!(!m.contains(gone), "{} survived:\n{}", gone, m);
        }
    }

    #[test]
    fn nested_skipped_elements_are_dropped_whole() {
        let m = to_markdown("<div class=\"navbox\"><div>inner</div><div>more</div></div><p>kept</p>", "https://x.org/", 1000).markdown;
        assert_eq!(m, "kept");
    }

    #[test]
    fn broken_html_does_not_panic_or_loop() {
        for h in ["", "<", "<<<>>>", "<a href=\"x", "<p>unclosed <b>bold", "<script>never closed", "&", "&#xZZ; &#99999999999;", "<a><a><a>x", "</p></p>text", "<!-- open"] {
            let _ = to_markdown(h, "https://x.org/", 1000);
        }
        assert_eq!(to_markdown("<p>unclosed <b>bold", "https://x.org/", 1000).markdown, "unclosed **bold");
    }

    #[test]
    fn long_pages_are_cut_at_a_paragraph_and_say_so() {
        let html: String = (0..200).map(|i| format!("<p>paragraph {} {}</p>", i, "word ".repeat(20))).collect();
        let m = to_markdown(&html, "https://x.org/", 1000).markdown;
        assert!(m.chars().count() < 1100 && m.ends_with("[…the page continues at its source]"), "{}", m.len());
    }

    #[test]
    fn images_are_kept_only_when_asked_for_and_a_picture_that_was_a_link_is_the_picture_alone() {
        let html = r#"<p>Before <a href="/wiki/File:A_(b).png"><img alt="A [b] (c)" src="//up.example.org/a_(b).png"></a> after <a href="/x">a <img src="/i.png" alt="icon"> label</a>.</p><img src="data:image/gif;base64,R0lG" alt="inline"><img src="/n.png">"#;
        assert_eq!(to_markdown(html, "https://x.org/wiki/P", 1000).markdown, "Before after [a label](https://x.org/x).");
        let m = to_markdown_with_images(html, "https://x.org/wiki/P", 1000).markdown;
        assert!(m.starts_with("Before ![A b c](https://up.example.org/a_%28b%29.png) after [a ![icon](https://x.org/i.png) label](https://x.org/x)."), "{}", m);
        assert!(m.contains("![](https://x.org/n.png)") && !m.contains("data:"), "{}", m);
        let imgs = find_images(&m);
        assert_eq!(imgs.iter().map(|i| (i.alt.as_str(), i.url.as_str())).collect::<Vec<_>>(), vec![("A b c", "https://up.example.org/a_%28b%29.png"), ("icon", "https://x.org/i.png"), ("", "https://x.org/n.png")]);
        assert_eq!(&m[imgs[0].start..imgs[0].end], "![A b c](https://up.example.org/a_%28b%29.png)");
    }

    #[test]
    fn stripping_images_leaves_the_text_exactly_as_it_was_without_them() {
        let html = r#"<h1>T</h1><p>One <img src="/a.png" alt="x"> two <a href="/p"><img src="/b.png"></a> three.</p><p><img src="/c.png" alt="only"></p>"#;
        let plain = to_markdown(html, "https://x.org/", 1000).markdown;
        let stripped = strip_images(&to_markdown_with_images(html, "https://x.org/", 1000).markdown);
        assert_eq!(plain_text(&stripped), plain_text(&plain));
        assert!(!stripped.contains("!["));
        for hostile in ["![", "![a](", "![a](b", "![a]\n(b)", "![[x]](y)", "text ![a](b) ![c](d)"] {
            let _ = strip_images(hostile);
            let _ = find_images(hostile);
        }
        assert_eq!(strip_images("a ![x](https://u/1.png) b ![y](https://u/2.png) c"), "a  b  c");
    }

    #[test]
    fn plain_text_drops_markup_but_keeps_the_words() {
        let t = plain_text("# Title\n\nA **bold** [link](https://x.org/a) and `code`.\n\n> quoted");
        assert_eq!(t, "Title A bold link and code. quoted");
    }
}
