//! RTF to plain text (`specs/014` §4): control words for paragraphs, tabs, quotes and Unicode; hex escapes
//! through Windows-1252; font tables, style sheets, pictures and other destinations skipped. Formatting is lost.

use crate::pdffont::CP1252_HIGH;
use crate::sniff::Format;
use crate::{DocError, Extract, Limits, Unit};

const SKIP: [&str; 30] = [
    "fonttbl", "colortbl", "stylesheet", "info", "pict", "object", "header", "footer", "headerl", "headerr", "headerf", "footerl", "footerr", "footerf", "footnote", "fldinst", "listtable", "listoverridetable", "rsidtbl",
    "generator", "xmlnstbl", "themedata", "colorschememapping", "latentstyles", "datastore", "operator", "author", "panose", "nonshppict", "pgdsctbl",
];

fn cp1252(b: u8) -> char {
    match b {
        0x80..=0x9f => CP1252_HIGH.chars().nth((b - 0x80) as usize).filter(|c| !c.is_control()).unwrap_or('\u{fffd}'),
        b => b as char,
    }
}

#[derive(Clone)]
struct Group {
    skip: bool,
    uc: usize,
}

pub fn to_text(input: &[u8]) -> String {
    let mut out = String::new();
    let mut stack = vec![Group { skip: false, uc: 1 }];
    let mut overflow = 0usize;
    let mut i = 0;
    let mut skip_chars = 0usize;
    while i < input.len() {
        let c = input[i];
        i += 1;
        let skipping = stack.last().is_some_and(|g| g.skip);
        match c {
            b'{' => {
                if stack.len() < 256 {
                    let top = stack.last().cloned().unwrap_or(Group { skip: false, uc: 1 });
                    stack.push(top);
                } else {
                    overflow += 1;
                }
            }
            b'}' => {
                if overflow > 0 {
                    overflow -= 1;
                } else if stack.len() > 1 {
                    stack.pop();
                }
            }
            b'\\' => {
                let Some(&n) = input.get(i) else { break };
                i += 1;
                match n {
                    b'\\' | b'{' | b'}' => {
                        if skip_chars > 0 {
                            skip_chars -= 1;
                        } else if !skipping {
                            out.push(n as char);
                        }
                    }
                    b'\'' => {
                        let hex = input.get(i..i + 2).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
                        i = (i + 2).min(input.len());
                        if skip_chars > 0 {
                            skip_chars -= 1;
                        } else if let (Some(b), false) = (hex, skipping) {
                            out.push(cp1252(b));
                        }
                    }
                    b'*' => {
                        if let Some(g) = stack.last_mut() {
                            g.skip = true;
                        }
                    }
                    b'~' if !skipping => out.push('\u{a0}'),
                    b'_' if !skipping => out.push('-'),
                    b'\n' | b'\r' if !skipping => out.push('\n'),
                    b if b.is_ascii_alphabetic() => {
                        let s = i - 1;
                        while i < input.len() && input[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        let word = String::from_utf8_lossy(&input[s..i]).into_owned();
                        let ns = i;
                        if input.get(i) == Some(&b'-') {
                            i += 1;
                        }
                        while i < input.len() && input[i].is_ascii_digit() {
                            i += 1;
                        }
                        let param: Option<i64> = std::str::from_utf8(&input[ns..i]).ok().and_then(|p| p.parse().ok());
                        if input.get(i) == Some(&b' ') {
                            i += 1;
                        }
                        if SKIP.contains(&word.as_str()) {
                            if let Some(g) = stack.last_mut() {
                                g.skip = true;
                            }
                            continue;
                        }
                        if skipping {
                            continue;
                        }
                        match word.as_str() {
                            "par" | "line" | "sect" | "page" => out.push('\n'),
                            "tab" => out.push('\t'),
                            "cell" | "nestcell" => out.push_str(" | "),
                            "row" | "nestrow" => out.push('\n'),
                            "emdash" => out.push('—'),
                            "endash" => out.push('–'),
                            "bullet" => out.push('•'),
                            "lquote" => out.push('‘'),
                            "rquote" => out.push('’'),
                            "ldblquote" => out.push('“'),
                            "rdblquote" => out.push('”'),
                            "uc" => {
                                if let Some(g) = stack.last_mut() {
                                    g.uc = param.unwrap_or(1).clamp(0, 8) as usize;
                                }
                            }
                            "u" => {
                                if let Some(p) = param {
                                    let cp = if p < 0 { p + 65_536 } else { p } as u32;
                                    out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                                    skip_chars = stack.last().map(|g| g.uc).unwrap_or(1);
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            b'\r' | b'\n' => {}
            _ => {
                if skip_chars > 0 {
                    skip_chars -= 1;
                } else if !skipping {
                    out.push(cp1252(c));
                }
            }
        }
    }
    let mut tidy = String::with_capacity(out.len());
    let mut blank = 0;
    for line in out.lines() {
        let l = line.trim();
        if l.is_empty() {
            blank += 1;
            continue;
        }
        if !tidy.is_empty() {
            tidy.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        tidy.push_str(l);
    }
    tidy
}

pub fn extract(bytes: &[u8], limits: &Limits) -> Result<Extract, DocError> {
    let mut text = to_text(bytes);
    let mut loss = vec!["formatting, pictures, headers, footers and footnotes are not read".to_string()];
    let truncated = text.len() > limits.max_output;
    if truncated {
        let mut n = limits.max_output;
        while !text.is_char_boundary(n) {
            n -= 1;
        }
        text.truncate(n);
        loss.push("output cut at the size limit".to_string());
    }
    Ok(Extract { format: Format::Rtf, text, unit: Unit::Line, total_units: None, truncated, loss, supported: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_paragraphs_escapes_and_unicode() {
        let rtf = br"{\rtf1\ansi\deff0{\fonttbl{\f0 Times;}}{\colortbl;\red0\green0\blue0;}{\*\generator Word;}\pard Hello \b bold\b0  world\par Second \'e9t\'e9 \u8364?{\i  it}\tab end\par}";
        assert_eq!(to_text(rtf), "Hello bold world\nSecond été € it\tend");
        assert_eq!(to_text(br"{\rtf1 \uc0\u233 x}"), "éx", "uc0 means no fallback character follows");
        assert_eq!(to_text(br"{\rtf1 \u233?x}"), "éx", "the fallback character after a Unicode escape is skipped");
    }

    #[test]
    fn destinations_and_unbalanced_groups() {
        assert_eq!(to_text(br"{\rtf1 {\info{\title Secret}}visible{\pict binary \bin}}} more {{{"), "visible more");
        assert_eq!(to_text(br"{\rtf1 a\\b\{c\}\~d}"), "a\\b{c}\u{a0}d");
        assert_eq!(to_text(br"{\rtf1 \trowd a\cell b\cell\row c}"), "a | b |\nc");
    }

    #[test]
    fn junk_and_deep_nesting_are_bounded() {
        let deep = format!("{}x{}", "{".repeat(100_000), "}".repeat(100_000));
        let _ = to_text(deep.as_bytes());
        let mut x: u64 = 0x1111_2222_3333_4444;
        for n in 0..1500usize {
            let b: Vec<u8> = (0..n % 200)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    br"{}\'u-0123 abc*".to_vec()[(x as usize) % 14]
                })
                .collect();
            let _ = to_text(&b);
        }
    }
}
