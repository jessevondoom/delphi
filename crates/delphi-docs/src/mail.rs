//! Email (`.eml`, `.mbox`) and calendar (`.ics`) files as readable text (`specs/014` §4): headers, the text
//! body (HTML bodies converted), and a list of attachments. Attachments are listed, never opened or run.
//!
//! The raw message is handled as Latin-1 (one char per byte) so structure can be parsed without losing the
//! bytes; each body part is turned back into bytes and decoded by its declared charset.

use crate::archive::human;
use crate::sniff::Format;
use crate::{DocError, Extract, Limits, Request, Unit};

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&x| x as char).collect()
}

fn to_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

fn utf8_or_latin1(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => latin1(b),
    }
}

fn decode_charset(b: &[u8], charset: &str) -> String {
    match charset.trim_matches('"').to_ascii_lowercase().as_str() {
        "iso-8859-1" | "latin1" | "us-ascii" | "ascii" => latin1(b),
        "windows-1252" | "cp1252" => b.iter().map(|&x| if (0x80..0xa0).contains(&x) { crate::pdffont::CP1252_HIGH.chars().nth((x - 0x80) as usize).filter(|c| !c.is_control()).unwrap_or('\u{fffd}') } else { x as char }).collect(),
        _ => String::from_utf8_lossy(b).into_owned(),
    }
}

fn b64(s: &str) -> Vec<u8> {
    let mut out = vec![];
    let (mut acc, mut n) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => continue,
        } as u32;
        acc = (acc << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
            acc &= (1 << n) - 1;
        }
    }
    out
}

fn qp(b: &[u8], header: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'=' => {
                if b.get(i + 1) == Some(&b'\r') && b.get(i + 2) == Some(&b'\n') {
                    i += 3;
                } else if b.get(i + 1) == Some(&b'\n') {
                    i += 2;
                } else if let Some(v) = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b'=');
                    i += 1;
                }
            }
            b'_' if header => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// RFC 2047 encoded words in a header value.
fn decode_words(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let (before, tail) = rest.split_at(start);
        let parsed = (|| {
            let t = &tail[2..];
            let (charset, t) = t.split_once('?')?;
            let (enc, t) = t.split_once('?')?;
            let (text, after) = t.split_once("?=")?;
            let bytes = match enc.to_ascii_lowercase().as_str() {
                "b" => b64(text),
                "q" => qp(text.as_bytes(), true),
                _ => return None,
            };
            Some((decode_charset(&bytes, charset), after))
        })();
        match parsed {
            Some((w, after)) => {
                // whitespace between adjacent encoded words is not part of the text
                if !(last_was_word && before.trim().is_empty()) {
                    out.push_str(before);
                }
                out.push_str(&w);
                rest = after;
                last_was_word = true;
            }
            None => {
                out.push_str(before);
                out.push_str("=?");
                rest = &tail[2..];
                last_was_word = false;
            }
        }
    }
    out.push_str(rest);
    out
}

fn split_head_body(s: &str) -> (&str, &str) {
    for sep in ["\r\n\r\n", "\n\n"] {
        if let Some(i) = s.find(sep) {
            return (&s[..i], &s[i + sep.len()..]);
        }
    }
    (s, "")
}

fn headers(head: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for line in head.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = out.last_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
        } else if let Some((k, v)) = line.split_once(':') {
            out.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    out
}

fn header<'a>(hs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    hs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// A parameter of a header such as `text/plain; charset="utf-8"; boundary=abc`.
fn param(value: &str, key: &str) -> Option<String> {
    for p in value.split(';').skip(1) {
        let (k, v) = p.split_once('=')?;
        if k.trim().eq_ignore_ascii_case(key) {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

#[derive(Default)]
struct Body {
    plain: Vec<String>,
    html: Vec<String>,
    attachments: Vec<String>,
    nested: usize,
}

fn walk_part(raw: &str, depth: usize, body: &mut Body, limits: &Limits, parts: &mut usize) {
    *parts += 1;
    if depth > 8 || *parts > 500 {
        return;
    }
    let (head, content) = split_head_body(raw);
    let hs = headers(head);
    let ct = header(&hs, "content-type").unwrap_or("text/plain");
    let mime = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let disp = header(&hs, "content-disposition").unwrap_or("");
    if let (true, Some(boundary)) = (mime.starts_with("multipart/"), param(ct, "boundary")) {
        let delim = format!("--{}", boundary);
        let mut segs = content.split(delim.as_str());
        segs.next(); // preamble
        for seg in segs {
            if seg.starts_with("--") {
                break;
            }
            walk_part(seg.trim_start_matches(['\r', '\n']), depth + 1, body, limits, parts);
        }
        return;
    }
    if mime == "message/rfc822" {
        body.nested += 1;
        let (nh, nb) = split_head_body(content);
        let nested_headers = headers(nh);
        let subject = header(&nested_headers, "subject").map(decode_words).unwrap_or_default();
        body.plain.push(format!("--- attached message: {} ---", subject));
        walk_part(&format!("{}\n\n{}", nh, nb), depth + 1, body, limits, parts);
        return;
    }
    let cte = header(&hs, "content-transfer-encoding").unwrap_or("7bit").to_ascii_lowercase();
    let bytes = match cte.trim() {
        "base64" => b64(content),
        "quoted-printable" => qp(content.as_bytes(), false),
        _ => to_bytes(content),
    };
    let filename = param(disp, "filename").or_else(|| param(ct, "name")).map(|n| decode_words(&n));
    let attached = disp.trim_start().to_ascii_lowercase().starts_with("attachment") || (filename.is_some() && !mime.starts_with("text/"));
    if attached || !(mime.starts_with("text/")) {
        body.attachments.push(format!("{} ({}, {})", filename.unwrap_or_else(|| "unnamed".into()), mime, human(bytes.len() as u64)));
        return;
    }
    let charset = param(ct, "charset").unwrap_or_else(|| "utf-8".into());
    let text = if charset.eq_ignore_ascii_case("utf-8") && cte.trim() != "base64" && cte.trim() != "quoted-printable" { utf8_or_latin1(&bytes) } else { decode_charset(&bytes, &charset) };
    match mime.as_str() {
        "text/html" => body.html.push(crate::html::to_markdown(&text, limits.max_nodes).0),
        "text/calendar" => body.plain.push(format!("[calendar invitation]\n{}", ics_text(&text))),
        _ => body.plain.push(text.trim_end().to_string()),
    }
}

fn render_message(raw: &str, limits: &Limits) -> (String, Vec<String>) {
    let (head, _) = split_head_body(raw);
    let hs = headers(head);
    let mut out = String::new();
    for (k, label) in [("from", "From"), ("to", "To"), ("cc", "Cc"), ("date", "Date"), ("subject", "Subject")] {
        if let Some(v) = header(&hs, k) {
            out.push_str(&format!("{}: {}\n", label, utf8_or_latin1_str(&decode_words(v))));
        }
    }
    let mut body = Body::default();
    let mut parts = 0;
    walk_part(raw, 0, &mut body, limits, &mut parts);
    out.push('\n');
    let mut loss = vec![];
    if !body.plain.is_empty() {
        out.push_str(&body.plain.join("\n\n"));
        if !body.html.is_empty() {
            loss.push("an HTML version of the body exists; the plain-text version is shown".to_string());
        }
    } else if !body.html.is_empty() {
        out.push_str(&body.html.join("\n\n"));
        loss.push("the message has only an HTML body; it was converted to text".to_string());
    } else {
        out.push_str("(no text body)");
    }
    if !body.attachments.is_empty() {
        out.push_str(&format!("\n\nAttachments ({}), listed and not opened:\n", body.attachments.len()));
        for a in body.attachments.iter().take(50) {
            out.push_str(&format!("- {}\n", a));
        }
    }
    (out.trim_end().to_string(), loss)
}

/// A header value that was raw 8-bit UTF-8 (read as Latin-1) is turned back into text.
fn utf8_or_latin1_str(s: &str) -> String {
    if s.chars().all(|c| (c as u32) < 256) { utf8_or_latin1(&to_bytes(s)) } else { s.to_string() }
}

fn cap(mut text: String, limits: &Limits) -> (String, bool) {
    let cut = text.len() > limits.max_output;
    if cut {
        let mut n = limits.max_output;
        while !text.is_char_boundary(n) {
            n -= 1;
        }
        text.truncate(n);
    }
    (text, cut)
}

pub fn eml(bytes: &[u8], limits: &Limits) -> Result<Extract, DocError> {
    let (text, loss) = render_message(&latin1(bytes), limits);
    let (text, truncated) = cap(text, limits);
    Ok(Extract { format: Format::Email, text, unit: Unit::Line, total_units: None, truncated, loss, supported: true })
}

/// Message boundaries in an mbox: lines starting `From ` at the start or after a blank line.
fn mbox_split(s: &str) -> Vec<&str> {
    let mut starts = vec![];
    let mut prev_blank = true;
    let mut pos = 0;
    for line in s.split_inclusive('\n') {
        if prev_blank && line.starts_with("From ") {
            starts.push(pos);
        }
        prev_blank = line.trim_end_matches(['\r', '\n']).is_empty();
        pos += line.len();
    }
    let mut out = vec![];
    for (i, &st) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(s.len());
        let msg = &s[st..end];
        // drop the "From " envelope line
        out.push(msg.split_once('\n').map(|(_, rest)| rest).unwrap_or(""));
    }
    out
}

pub fn mbox(bytes: &[u8], req: &Request, limits: &Limits) -> Result<Extract, DocError> {
    let raw = latin1(bytes);
    let msgs = mbox_split(&raw);
    if let Some(m) = &req.member {
        let n: usize = m.trim_start_matches('#').parse().map_err(|_| DocError(format!("`{}` is not a message number; use #N from the listing", m)))?;
        let msg = msgs.get(n).ok_or_else(|| DocError(format!("no message #{}; the mailbox has {}", n, msgs.len())))?;
        let (text, loss) = render_message(msg, limits);
        let (text, truncated) = cap(format!("message #{}\n{}", n, text), limits);
        return Ok(Extract { format: Format::Mbox, text, unit: Unit::Member, total_units: Some(msgs.len()), truncated, loss, supported: true });
    }
    let first = req.start.unwrap_or(0);
    let last = req.end.map(|e| e + 1).unwrap_or(first + 100).min(first + 100).min(msgs.len());
    let mut out = format!("mailbox · {} message{}\n", msgs.len(), if msgs.len() == 1 { "" } else { "s" });
    for (i, m) in msgs.iter().enumerate().take(last).skip(first) {
        let hs = headers(split_head_body(m).0);
        let get = |k: &str| utf8_or_latin1_str(&decode_words(header(&hs, k).unwrap_or(""))).chars().take(70).collect::<String>();
        out.push_str(&format!("#{:<4} {} | {} | {}\n", i, get("date"), get("from"), get("subject")));
    }
    if last < msgs.len() {
        out.push_str(&format!("(messages #{}-#{} of {} shown; pass start/end to page, or member #N to read one)\n", first, last.saturating_sub(1), msgs.len()));
    } else if !msgs.is_empty() {
        out.push_str("(pass member #N to read one message)\n");
    }
    let (text, truncated) = cap(out, limits);
    Ok(Extract { format: Format::Mbox, text, unit: Unit::Member, total_units: Some(msgs.len()), truncated: truncated || last < msgs.len(), loss: vec![], supported: true })
}

// ---------------------------------------------------------------------------------------------- iCalendar

fn ics_unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\N", "\n").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

/// `20261025T140000Z` as `2026-10-25 14:00 UTC`; dates alone as `2026-10-25`.
fn ics_when(prop: &str, value: &str) -> String {
    let v = value.trim();
    let tz = prop.split(';').find_map(|p| p.strip_prefix("TZID="));
    let digits = |r: std::ops::Range<usize>| v.get(r);
    match (digits(0..4), digits(4..6), digits(6..8)) {
        (Some(y), Some(m), Some(d)) if y.chars().all(|c| c.is_ascii_digit()) => {
            let mut s = format!("{}-{}-{}", y, m, d);
            if let (Some(h), Some(mi)) = (digits(9..11), digits(11..13)) {
                s.push_str(&format!(" {}:{}", h, mi));
                if v.ends_with('Z') {
                    s.push_str(" UTC");
                } else if let Some(tz) = tz {
                    s.push_str(&format!(" ({})", tz));
                }
            }
            s
        }
        _ => v.to_string(),
    }
}

fn ics_text(text: &str) -> String {
    // unfold continuation lines
    let mut lines: Vec<String> = vec![];
    for l in text.lines() {
        if l.starts_with([' ', '\t']) {
            if let Some(last) = lines.last_mut() {
                last.push_str(&l[1..]);
                continue;
            }
        }
        lines.push(l.to_string());
    }
    let mut out = String::new();
    let mut cur: Option<(String, Vec<(String, String)>)> = None;
    let mut count = 0;
    for l in lines {
        let Some((prop, value)) = l.split_once(':') else { continue };
        let name = prop.split(';').next().unwrap_or("").to_ascii_uppercase();
        match name.as_str() {
            "BEGIN" if matches!(value.trim(), "VEVENT" | "VTODO" | "VJOURNAL") => cur = Some((value.trim().to_string(), vec![])),
            "END" if matches!(value.trim(), "VEVENT" | "VTODO" | "VJOURNAL") => {
                if let Some((kind, props)) = cur.take() {
                    count += 1;
                    if count > 500 {
                        out.push_str("(more than 500 entries; the rest are not shown)\n");
                        break;
                    }
                    let get = |k: &str| props.iter().find(|(n, _)| n.split(';').next().unwrap_or("") == k).map(|(n, v)| (n.clone(), ics_unescape(v)));
                    let title = get("SUMMARY").map(|(_, v)| v).unwrap_or_else(|| "(no title)".into());
                    out.push_str(&format!("## {}{}\n", title, if kind == "VEVENT" { "" } else { " (task)" }));
                    if let Some((p, v)) = get("DTSTART") {
                        let end = get("DTEND").or_else(|| get("DUE")).map(|(p, v)| format!(" to {}", ics_when(&p, &v))).unwrap_or_default();
                        out.push_str(&format!("When: {}{}\n", ics_when(&p, &v), end));
                    }
                    for (key, label) in [("LOCATION", "Where"), ("STATUS", "Status"), ("RRULE", "Repeats"), ("ORGANIZER", "Organizer")] {
                        if let Some((_, v)) = get(key) {
                            out.push_str(&format!("{}: {}\n", label, v.trim_start_matches("mailto:")));
                        }
                    }
                    let attendees: Vec<String> = props
                        .iter()
                        .filter(|(n, _)| n.split(';').next() == Some("ATTENDEE"))
                        .map(|(n, v)| n.split(';').find_map(|p| p.strip_prefix("CN=")).map(|c| c.trim_matches('"').to_string()).unwrap_or_else(|| v.trim_start_matches("mailto:").to_string()))
                        .collect();
                    if !attendees.is_empty() {
                        out.push_str(&format!("Attendees: {}\n", attendees.join(", ")));
                    }
                    if let Some((_, d)) = get("DESCRIPTION") {
                        out.push_str(&format!("{}\n", d.trim()));
                    }
                    out.push('\n');
                }
            }
            _ => {
                if let Some((_, props)) = cur.as_mut() {
                    props.push((prop.to_string(), value.to_string()));
                }
            }
        }
    }
    out.trim_end().to_string()
}

pub fn ics(bytes: &[u8], limits: &Limits) -> Result<Extract, DocError> {
    let text = ics_text(&String::from_utf8_lossy(bytes));
    let (text, truncated) = cap(if text.is_empty() { "(no events or tasks)".to_string() } else { text }, limits);
    Ok(Extract { format: Format::Calendar, text, unit: Unit::Line, total_units: None, truncated, loss: vec![], supported: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(s: &str) -> Extract {
        eml(s.as_bytes(), &Limits::default()).unwrap()
    }

    #[test]
    fn headers_encoded_words_and_a_plain_body() {
        let e = read("From: =?UTF-8?B?SsO8cmdlbg==?= <j@example.com>\nTo: a@x.com,\n b@x.com\nSubject: =?iso-8859-1?Q?Caf=E9_menu?= =?utf-8?Q?_ok?=\nDate: Sat, 24 Oct 2026 10:00:00 +0000\n\nHello,\nbody line\n");
        assert!(e.text.starts_with("From: Jürgen <j@example.com>\nTo: a@x.com, b@x.com\nDate: Sat, 24 Oct 2026 10:00:00 +0000\nSubject: Café menu ok\n\nHello,\nbody line"), "{}", e.text);
    }

    #[test]
    fn multipart_prefers_text_lists_attachments_and_decodes_encodings() {
        let m = "Subject: x\nContent-Type: multipart/mixed; boundary=\"B1\"\n\n--B1\nContent-Type: multipart/alternative; boundary=B2\n\n--B2\nContent-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: quoted-printable\n\nCaf=C3=A9 =\nline\n--B2\nContent-Type: text/html\n\n<p>html version</p>\n--B2--\n--B1\nContent-Type: application/pdf; name=\"report.pdf\"\nContent-Disposition: attachment; filename=\"report.pdf\"\nContent-Transfer-Encoding: base64\n\nSGVsbG8gd29ybGQ=\n--B1--\n";
        let e = read(m);
        assert!(e.text.contains("\nCafé line") && !e.text.contains("html version"), "{}", e.text);
        assert!(e.text.contains("Attachments (1), listed and not opened:\n- report.pdf (application/pdf, 11 B)"), "{}", e.text);
        assert!(e.loss.iter().any(|l| l.contains("HTML version")), "{:?}", e.loss);
    }

    #[test]
    fn html_only_mail_is_converted_and_attached_messages_are_read() {
        let e = read("Subject: h\nContent-Type: text/html; charset=utf-8\n\n<h1>Hi</h1><p>there <a href=\"https://e.com\">link</a></p><script>x</script>");
        assert!(e.text.contains("# Hi") && e.text.contains("[link](https://e.com)") && e.loss.iter().any(|l| l.contains("only an HTML body")), "{}", e.text);
        let n = read("Content-Type: multipart/mixed; boundary=Z\n\n--Z\nContent-Type: text/plain\n\nouter\n--Z\nContent-Type: message/rfc822\n\nSubject: inner one\nContent-Type: text/plain\n\ninner body\n--Z--\n");
        assert!(n.text.contains("outer") && n.text.contains("--- attached message: inner one ---") && n.text.contains("inner body"), "{}", n.text);
    }

    #[test]
    fn an_mbox_lists_and_reads_messages() {
        let mb = "From a@x Sat Oct 24 10:00:00 2026\nFrom: A <a@x.com>\nSubject: First\nDate: d1\n\nbody one\n\nFrom b@x Sat Oct 24 11:00:00 2026\nFrom: B <b@x.com>\nSubject: Second\nDate: d2\n\nbody two\nFrom: not a boundary\n";
        let l = mbox(mb.as_bytes(), &Request::default(), &Limits::default()).unwrap();
        assert_eq!(l.total_units, Some(2));
        assert!(l.text.contains("#0    d1 | A <a@x.com> | First") && l.text.contains("#1    d2 | B <b@x.com> | Second"), "{}", l.text);
        let m = mbox(mb.as_bytes(), &Request { member: Some("#1".into()), ..Request::default() }, &Limits::default()).unwrap();
        assert!(m.text.contains("message #1") && m.text.contains("body two"), "{}", m.text);
        assert!(mbox(mb.as_bytes(), &Request { member: Some("#9".into()), ..Request::default() }, &Limits::default()).unwrap_err().0.contains("no message #9"));
    }

    #[test]
    fn a_calendar_lists_events_with_times_places_and_people() {
        let c = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:Design review\nDTSTART:20261025T140000Z\nDTEND:20261025T150000Z\nLOCATION:Room 4\\, North\nORGANIZER;CN=Dana:mailto:dana@x.com\nATTENDEE;CN=\"Lee\":mailto:lee@x.com\nATTENDEE:mailto:sam@x.com\nRRULE:FREQ=WEEKLY\nDESCRIPTION:Line one\\nLine\n two\nEND:VEVENT\nBEGIN:VEVENT\nSUMMARY:All day\nDTSTART;VALUE=DATE:20261101\nEND:VEVENT\nEND:VCALENDAR\n";
        let e = ics(c.as_bytes(), &Limits::default()).unwrap();
        assert!(e.text.contains("## Design review\nWhen: 2026-10-25 14:00 UTC to 2026-10-25 15:00 UTC\nWhere: Room 4, North"), "{}", e.text);
        assert!(e.text.contains("Organizer: dana@x.com") || e.text.contains("Organizer: mailto"), "{}", e.text);
        assert!(e.text.contains("Attendees: Lee, sam@x.com") && e.text.contains("Repeats: FREQ=WEEKLY") && e.text.contains("Line one\nLinetwo"), "{}", e.text);
        assert!(e.text.contains("## All day\nWhen: 2026-11-01"), "{}", e.text);
    }

    #[test]
    fn hostile_mail_is_bounded() {
        let deep = "Content-Type: multipart/mixed; boundary=a\n\n".to_string() + &"--a\nContent-Type: multipart/mixed; boundary=a\n\n".repeat(50);
        let _ = read(&deep);
        let _ = read("Content-Type: multipart/mixed; boundary=\n\n--\n--");
        let _ = read(&format!("Subject: {}\n\n", "=?utf-8?B?".repeat(1000)));
        let mut x: u64 = 0x7777_8888_9999_aaaa;
        for n in 0..1500usize {
            let s: String = (0..n % 200)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    ["\n", "--", "a", ":", "=", "?", "B", "Q", ";", "boundary=", "multipart/", "text/", "base64", " ", "="][(x as usize) % 15]
                })
                .collect();
            let _ = read(&s);
            let _ = mbox(s.as_bytes(), &Request::default(), &Limits::default());
            let _ = ics(s.as_bytes(), &Limits::default());
        }
    }
}
