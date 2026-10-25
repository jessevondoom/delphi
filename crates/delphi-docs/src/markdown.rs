//! Markdown to HTML for generated pages (`specs/014` §7.3 rule 5). Text from an agent goes in as text, never
//! as markup: raw HTML is not recognised, so `<script>` is shown as the characters it is. Links are the one
//! thing worth saying plainly: a page may not reach out, so only in-page `#anchors` are clickable, and any other
//! link is printed as its text followed by the address in parentheses. Images are not embedded.
//!
//! Supported: headings (shifted down one level, because the page title is the `h1`), paragraphs with hard
//! breaks, emphasis, strong, code spans, fenced code, block quotes, nested bullet and numbered lists, GFM
//! tables, and rules.

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// An id usable in `href="#id"` and `id="…"`: letters, digits, `-` and `_`.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation()
}

/// Nesting limits. Agent-written text can be hostile; a stack overflow aborts the whole process, so every
/// recursive path stops at a fixed depth and shows the rest as plain text.
const MAX_INLINE_DEPTH: usize = 6;
const MAX_QUOTE_DEPTH: usize = 8;
const MAX_LIST_DEPTH: usize = 12;

/// Inline Markdown: code, strong, emphasis, links and escapes. Everything else is escaped text.
pub fn inline(src: &str) -> String {
    inline_at(src, 0)
}

fn inline_at(src: &str, depth: usize) -> String {
    if depth > MAX_INLINE_DEPTH {
        return escape(src);
    }
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() + 16);
    let mut i = 0;
    let find = |from: usize, pat: &[char]| -> Option<usize> { (from..b.len().saturating_sub(pat.len() - 1)).find(|&j| b[j..j + pat.len()] == *pat) };
    while i < b.len() {
        let c = b[i];
        match c {
            '\\' if i + 1 < b.len() && is_punct(b[i + 1]) => {
                out.push_str(&escape(&b[i + 1].to_string()));
                i += 2;
            }
            '`' => {
                let n = b[i..].iter().take_while(|&&x| x == '`').count();
                let fence: Vec<char> = vec!['`'; n];
                match find(i + n, &fence) {
                    Some(end) => {
                        let code: String = b[i + n..end].iter().collect();
                        out.push_str(&format!("<code>{}</code>", escape(code.trim())));
                        i = end + n;
                    }
                    None => {
                        out.push_str(&"`".repeat(n));
                        i += n;
                    }
                }
            }
            '*' | '_' => {
                let double = b.get(i + 1) == Some(&c);
                let n = if double { 2 } else { 1 };
                let prev_word = i > 0 && b[i - 1].is_alphanumeric();
                let next_space = b.get(i + n).is_none_or(|x| x.is_whitespace());
                let marker: Vec<char> = vec![c; n];
                let closing = if next_space || (c == '_' && prev_word) { None } else { find(i + n, &marker).filter(|&e| e > i + n && !b[e - 1].is_whitespace()) };
                match closing {
                    Some(end) => {
                        let inner: String = b[i + n..end].iter().collect();
                        let tag = if double { "strong" } else { "em" };
                        out.push_str(&format!("<{t}>{}</{t}>", inline_at(&inner, depth + 1), t = tag));
                        i = end + n;
                    }
                    None => {
                        out.push(c);
                        i += 1;
                    }
                }
            }
            '!' | '[' => {
                let image = c == '!' && b.get(i + 1) == Some(&'[');
                let open = if image { i + 1 } else { i };
                if c == '!' && !image {
                    out.push('!');
                    i += 1;
                    continue;
                }
                let text_end = (open + 1..b.len()).find(|&j| b[j] == ']');
                // the address ends at the `)` that balances the opening one, so `f(x)` inside a URL survives
                let close_of = |from: usize| {
                    let mut nest = 0usize;
                    (from..b.len()).find(|&j| match b[j] {
                        '(' => {
                            nest += 1;
                            false
                        }
                        ')' if nest == 0 => true,
                        ')' => {
                            nest -= 1;
                            false
                        }
                        _ => false,
                    })
                };
                let link = text_end.filter(|&e| b.get(e + 1) == Some(&'(')).and_then(|e| close_of(e + 2).map(|close| (e, close)));
                match link {
                    Some((e, close)) => {
                        let text: String = b[open + 1..e].iter().collect();
                        let url: String = b[e + 2..close].iter().collect::<String>().trim().to_string();
                        if image {
                            out.push_str(&format!("[image: {}]", escape(&text)));
                        } else if let Some(anchor) = url.strip_prefix('#') {
                            out.push_str(&format!("<a href=\"#{}\">{}</a>", slug(anchor), inline_at(&text, depth + 1)));
                        } else {
                            out.push_str(&format!("{} (<span class=\"url\">{}</span>)", inline_at(&text, depth + 1), escape(&url)));
                        }
                        i = close + 1;
                    }
                    None => {
                        out.push_str(&escape(&c.to_string()));
                        i += 1;
                    }
                }
            }
            c => {
                out.push_str(&escape(&c.to_string()));
                i += 1;
            }
        }
    }
    out
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count() + line.chars().take_while(|c| *c == '\t').count() * 4
}

/// (ordered, rest) when a line starts a list item.
fn list_item(line: &str) -> Option<(bool, &str)> {
    let t = line.trim_start();
    for m in ["- ", "* ", "+ "] {
        if let Some(r) = t.strip_prefix(m) {
            return Some((false, r));
        }
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if (1..=9).contains(&digits) {
        if let Some(r) = t[digits..].strip_prefix(". ").or_else(|| t[digits..].strip_prefix(") ")) {
            return Some((true, r));
        }
    }
    None
}

fn is_rule(line: &str) -> bool {
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

fn table_cells(line: &str) -> Vec<String> {
    let t = line.trim().trim_start_matches('|').trim_end_matches('|');
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_table_sep(line: &str) -> bool {
    let t = line.trim();
    t.contains('-') && t.contains('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// Render nested list items starting at `lines[*i]`, all at indentation `base` or deeper.
fn render_list(lines: &[&str], i: &mut usize, out: &mut String, depth: usize) {
    let base = indent_of(lines[*i]);
    let ordered = list_item(lines[*i]).map(|(o, _)| o).unwrap_or(false);
    out.push_str(if ordered { "<ol>" } else { "<ul>" });
    while *i < lines.len() {
        let l = lines[*i];
        let Some((o, text)) = list_item(l) else { break };
        let ind = indent_of(l);
        if ind < base || (ind == base && o != ordered) {
            break;
        }
        if ind > base {
            // a deeper item belongs to the previous one; it was consumed there
            break;
        }
        out.push_str(&format!("<li>{}", inline(text)));
        *i += 1;
        while *i < lines.len() {
            let n = lines[*i];
            if n.trim().is_empty() {
                break;
            }
            match list_item(n) {
                Some(_) if indent_of(n) > base && depth < MAX_LIST_DEPTH => render_list(lines, i, out, depth + 1),
                Some((_, text)) if indent_of(n) > base => {
                    // nested too deep: the item is shown as a continuation of its parent
                    out.push(' ');
                    out.push_str(&inline(text));
                    *i += 1;
                }
                Some(_) => break,
                None if indent_of(n) > base => {
                    out.push(' ');
                    out.push_str(&inline(n.trim()));
                    *i += 1;
                }
                None => break,
            }
        }
        out.push_str("</li>");
    }
    out.push_str(if ordered { "</ol>" } else { "</ul>" });
}

/// Render Markdown to HTML. Raw HTML in the source is shown as text.
pub fn to_html(src: &str) -> String {
    html_at(src, 0)
}

fn html_at(src: &str, depth: usize) -> String {
    let text = src.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::with_capacity(text.len() * 2);
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        if t.is_empty() {
            i += 1;
        } else if let Some(fence) = t.strip_prefix("```").or_else(|| t.strip_prefix("~~~")) {
            let marker = &t[..3];
            let lang: String = fence.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '+').take(20).collect();
            let mut code = String::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with(marker) {
                code.push_str(lines[i]);
                code.push('\n');
                i += 1;
            }
            i += 1;
            let class = if lang.is_empty() { String::new() } else { format!(" class=\"language-{}\"", lang) };
            out.push_str(&format!("<pre><code{}>{}</code></pre>\n", class, escape(code.trim_end_matches('\n'))));
        } else if t.starts_with('#') && t.chars().take_while(|c| *c == '#').count() <= 6 && t.trim_start_matches('#').starts_with(' ') {
            let level = t.chars().take_while(|c| *c == '#').count();
            let body = t.trim_start_matches('#').trim().trim_end_matches('#').trim();
            let tag = (level + 1).min(6);
            out.push_str(&format!("<h{t} id=\"{}\">{}</h{t}>\n", slug(body), inline(body), t = tag));
            i += 1;
        } else if is_rule(t) && !list_item(line).is_some() {
            out.push_str("<hr>\n");
            i += 1;
        } else if t.starts_with('>') {
            let mut inner = String::new();
            while i < lines.len() && lines[i].trim().starts_with('>') {
                inner.push_str(lines[i].trim().trim_start_matches('>').trim_start());
                inner.push('\n');
                i += 1;
            }
            if depth < MAX_QUOTE_DEPTH {
                out.push_str(&format!("<blockquote>{}</blockquote>\n", html_at(&inner, depth + 1)));
            } else {
                out.push_str(&format!("<blockquote><p>{}</p></blockquote>\n", escape(inner.trim())));
            }
        } else if list_item(line).is_some() {
            render_list(&lines, &mut i, &mut out, 0);
            out.push('\n');
        } else if t.contains('|') && i + 1 < lines.len() && is_table_sep(lines[i + 1]) {
            let head = table_cells(t);
            i += 2;
            out.push_str("<div class=\"table-wrap\"><table><thead><tr>");
            for h in &head {
                out.push_str(&format!("<th scope=\"col\">{}</th>", inline(h)));
            }
            out.push_str("</tr></thead><tbody>");
            while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
                let cells = table_cells(lines[i]);
                out.push_str("<tr>");
                for c in 0..head.len() {
                    out.push_str(&format!("<td>{}</td>", inline(cells.get(c).map(|s| s.as_str()).unwrap_or(""))));
                }
                out.push_str("</tr>");
                i += 1;
            }
            out.push_str("</tbody></table></div>\n");
        } else {
            // a paragraph runs to the next blank line or block start
            let mut para: Vec<String> = vec![];
            while i < lines.len() {
                let l = lines[i];
                let lt = l.trim();
                if lt.is_empty() || lt.starts_with("```") || lt.starts_with("~~~") || lt.starts_with('>') || list_item(l).is_some() || (lt.starts_with('#') && lt.trim_start_matches('#').starts_with(' ')) || is_rule(lt) {
                    break;
                }
                if lt.contains('|') && i + 1 < lines.len() && is_table_sep(lines[i + 1]) {
                    break;
                }
                let hard = l.ends_with("  ") || lt.ends_with('\\');
                para.push(format!("{}{}", inline(lt.trim_end_matches('\\')), if hard { "<br>" } else { "" }));
                i += 1;
            }
            if para.is_empty() {
                // a line that starts something we do not parse; show it rather than loop
                para.push(inline(t));
                i += 1;
            }
            out.push_str(&format!("<p>{}</p>\n", para.join("\n")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_html_and_scripts_are_text() {
        let h = to_html("<script>alert(1)</script>\n\n<img src=x onerror=alert(1)> & \"quoted\"");
        assert!(!h.contains("<script") && !h.contains("<img") && h.contains("&lt;script&gt;alert(1)&lt;/script&gt;"), "{}", h);
        assert!(h.contains("&lt;img src=x onerror=alert(1)&gt; &amp; &quot;quoted&quot;"), "{}", h);
        assert_eq!(inline("a < b > c & d"), "a &lt; b &gt; c &amp; d");
    }

    #[test]
    fn links_never_become_clickable_except_in_page_anchors() {
        assert_eq!(inline("[docs](https://example.com/x?a=1&b=2)"), "docs (<span class=\"url\">https://example.com/x?a=1&amp;b=2</span>)");
        assert_eq!(inline("[click](javascript:alert(1))"), "click (<span class=\"url\">javascript:alert(1)</span>)");
        assert_eq!(inline("[data](data:text/html,<b>)"), "data (<span class=\"url\">data:text/html,&lt;b&gt;</span>)");
        assert_eq!(inline("[jump](#My Section!)"), "<a href=\"#my-section\">jump</a>");
        assert_eq!(inline("![a chart](https://e.com/c.png)"), "[image: a chart]");
        assert!(!to_html("[x](https://e.com) <https://e.com>").contains("href=\"http"));
    }

    #[test]
    fn inline_formatting_and_escapes() {
        assert_eq!(inline("**bold** and *em* and `code <b>`"), "<strong>bold</strong> and <em>em</em> and <code>code &lt;b&gt;</code>");
        assert_eq!(inline("snake_case_name and 2 * 3 * 4"), "snake_case_name and 2 * 3 * 4");
        assert_eq!(inline(r"\*not em\* \[x\]"), "*not em* [x]");
        assert_eq!(inline("unclosed **bold and `code"), "unclosed **bold and `code");
    }

    #[test]
    fn blocks() {
        let md = "# Title\n\nPara one\nstill one.  \nhard break\n\n## Sub\n\n- a\n  - nested\n- b\n\n1. x\n2. y\n\n> quote\n> more\n\n```rust\nlet a = 1 < 2;\n```\n\n| H1 | H2 |\n|---|:-:|\n| 1 | **2** |\n| 3 |\n\n---\n";
        let h = to_html(md);
        assert!(h.contains("<h2 id=\"title\">Title</h2>") && h.contains("<h3 id=\"sub\">Sub</h3>"), "{}", h);
        assert!(h.contains("<p>Para one\nstill one.<br>\nhard break</p>"), "{}", h);
        assert!(h.contains("<ul><li>a<ul><li>nested</li></ul></li><li>b</li></ul>") && h.contains("<ol><li>x</li><li>y</li></ol>"), "{}", h);
        assert!(h.contains("<blockquote><p>quote\nmore</p>\n</blockquote>") && h.contains("<pre><code class=\"language-rust\">let a = 1 &lt; 2;</code></pre>"), "{}", h);
        assert!(h.contains("<th scope=\"col\">H1</th>") && h.contains("<td><strong>2</strong></td>") && h.contains("<td>3</td><td></td>") && h.ends_with("<hr>\n"), "{}", h);
    }

    #[test]
    fn hostile_and_degenerate_input_terminates() {
        for s in ["", "#", "# ", "####### seven", "|", "| a |\n|", "- ", "1. ", ">", "```", "[", "](", "[](", "**", "`", "\\", "![", "[a](b", "* * *", "- - -"] {
            let _ = to_html(s);
        }
        let _ = to_html(&"- a\n".repeat(5000));
        let _ = to_html(&format!("{}x", "  - n\n".repeat(2000)));
        let _ = to_html(&"> ".repeat(2000));
        let _ = to_html(&"[a](".repeat(3000));
        let mut x: u64 = 0x2468_ace0_1357_9bdf;
        for n in 0..2000usize {
            let s: String = (0..n % 120)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    ["*", "_", "`", "[", "]", "(", ")", "#", "-", ">", "|", "\n", " ", "a", "1", ".", "!", "<", "\\", "~"][(x as usize) % 20]
                })
                .collect();
            let _ = to_html(&s);
        }
    }
}
