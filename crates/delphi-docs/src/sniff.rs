//! Format detection by magic bytes. The file name is only a hint, used to tell text formats apart and to
//! break ties; a `.xlsx` that is not a zip is not read as one.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pdf,
    /// zip, and the zip-based formats (docx, xlsx, pptx, odf, epub, jar) until the listing says which.
    Zip,
    Tar,
    Gzip,
    Sqlite,
    Png,
    Jpeg,
    Gif,
    Webp,
    /// OLE2 compound files: legacy `.doc`, `.xls`, `.ppt`.
    LegacyOffice,
    Rtf,
    Html,
    Xml,
    Json,
    Yaml,
    Toml,
    Csv,
    Tsv,
    Notebook,
    Email,
    Calendar,
    Markdown,
    /// UTF-8 text with no more specific format.
    Text,
    /// Not text and not recognised.
    Unknown,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Pdf => "PDF",
            Format::Zip => "zip archive",
            Format::Tar => "tar archive",
            Format::Gzip => "gzip",
            Format::Sqlite => "SQLite database",
            Format::Png => "PNG image",
            Format::Jpeg => "JPEG image",
            Format::Gif => "GIF image",
            Format::Webp => "WebP image",
            Format::LegacyOffice => "legacy Office file (.doc/.xls/.ppt)",
            Format::Rtf => "RTF",
            Format::Html => "HTML",
            Format::Xml => "XML",
            Format::Json => "JSON",
            Format::Yaml => "YAML",
            Format::Toml => "TOML",
            Format::Csv => "CSV",
            Format::Tsv => "TSV",
            Format::Notebook => "Jupyter notebook",
            Format::Email => "email",
            Format::Calendar => "iCalendar",
            Format::Markdown => "Markdown",
            Format::Text => "text",
            Format::Unknown => "unrecognised binary",
        }
    }
}

impl Format {
    /// Text that `read_file` shows line by line.
    pub fn is_text(self) -> bool {
        matches!(self, Format::Rtf | Format::Html | Format::Xml | Format::Json | Format::Yaml | Format::Toml | Format::Csv | Format::Tsv | Format::Notebook | Format::Email | Format::Calendar | Format::Markdown | Format::Text)
    }
}

fn ext_of(name: &str) -> String {
    let file = name.rsplit('/').next().unwrap_or(name);
    match file.rsplit_once('.') {
        Some((stem, e)) if !stem.is_empty() => e.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// Is `head` text? UTF-8 (a multi-byte character cut at the end of the sample is allowed), no NUL, and no
/// control bytes other than tab, newline, carriage return, form feed and escape.
fn looks_like_text(head: &[u8]) -> bool {
    let valid = match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && e.valid_up_to() + 4 > head.len(),
    };
    valid && !head.iter().any(|&b| b == 0 || (b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0c | 0x1b)))
}

fn starts_with_ci(hay: &[u8], needle: &[u8]) -> bool {
    hay.len() >= needle.len() && hay[..needle.len()].eq_ignore_ascii_case(needle)
}

/// Detect the format of `bytes` (a prefix of at least a few KB is enough).
pub fn detect(bytes: &[u8], name: &str) -> Format {
    let b = bytes;
    if b.starts_with(b"%PDF-") {
        return Format::Pdf;
    }
    if b.starts_with(b"PK\x03\x04") || b.starts_with(b"PK\x05\x06") {
        return Format::Zip;
    }
    if b.starts_with(&[0x1f, 0x8b]) {
        return Format::Gzip;
    }
    if b.starts_with(b"SQLite format 3\0") {
        return Format::Sqlite;
    }
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Format::Png;
    }
    if b.starts_with(&[0xff, 0xd8, 0xff]) {
        return Format::Jpeg;
    }
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return Format::Gif;
    }
    if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        return Format::Webp;
    }
    if b.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        return Format::LegacyOffice;
    }
    if b.len() >= 262 && &b[257..262] == b"ustar" {
        return Format::Tar;
    }
    if !looks_like_text(&b[..b.len().min(8192)]) {
        return Format::Unknown;
    }
    let body = b.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(b);
    let trimmed: &[u8] = {
        let n = body.iter().take_while(|c| c.is_ascii_whitespace()).count();
        &body[n..]
    };
    if trimmed.starts_with(b"{\\rtf") {
        return Format::Rtf;
    }
    match ext_of(name).as_str() {
        "csv" => return Format::Csv,
        "tsv" | "tab" => return Format::Tsv,
        "json" => return Format::Json,
        "ipynb" => return Format::Notebook,
        "yaml" | "yml" => return Format::Yaml,
        "toml" => return Format::Toml,
        "md" | "markdown" => return Format::Markdown,
        "eml" => return Format::Email,
        "ics" => return Format::Calendar,
        "html" | "htm" | "xhtml" => return Format::Html,
        "xml" | "svg" => return Format::Xml,
        _ => {}
    }
    if starts_with_ci(trimmed, b"<!doctype html") || starts_with_ci(trimmed, b"<html") {
        return Format::Html;
    }
    if trimmed.starts_with(b"<?xml") {
        return Format::Xml;
    }
    Format::Text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_bytes_decide_before_the_name_does() {
        assert_eq!(detect(b"%PDF-1.7\n%\xe2\xe3", "notes.txt"), Format::Pdf);
        assert_eq!(detect(b"PK\x03\x04rest", "report.pdf"), Format::Zip);
        assert_eq!(detect(&[0x1f, 0x8b, 8, 0], "a.tar.gz"), Format::Gzip);
        assert_eq!(detect(b"SQLite format 3\0more", "data.csv"), Format::Sqlite);
        assert_eq!(detect(b"\x89PNG\r\n\x1a\nxx", "x.txt"), Format::Png);
        assert_eq!(detect(&[0xff, 0xd8, 0xff, 0xe0], "x"), Format::Jpeg);
        assert_eq!(detect(b"GIF89a....", "x"), Format::Gif);
        assert_eq!(detect(b"RIFF\x00\x00\x00\x00WEBPVP8 ", "x"), Format::Webp);
        assert_eq!(detect(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1, 0], "old.xls"), Format::LegacyOffice);
    }

    #[test]
    fn tar_is_found_by_the_ustar_field() {
        let mut h = vec![0u8; 512];
        h[257..262].copy_from_slice(b"ustar");
        assert_eq!(detect(&h, "x.tar"), Format::Tar);
    }

    #[test]
    fn a_renamed_file_is_read_as_what_it_is() {
        assert_eq!(detect(b"PK\x03\x04", "data.xlsx"), Format::Zip);
        assert_eq!(detect(b"just words\n", "data.xlsx"), Format::Text);
        assert_eq!(detect(&[0, 1, 2, 3, 4], "notes.md"), Format::Unknown);
    }

    #[test]
    fn text_formats_are_told_apart_by_name_then_by_content() {
        assert_eq!(detect(b"a,b\n1,2\n", "t.csv"), Format::Csv);
        assert_eq!(detect(b"a\tb\n", "t.tsv"), Format::Tsv);
        assert_eq!(detect(b"{}", "t.json"), Format::Json);
        assert_eq!(detect(b"# hi", "README.md"), Format::Markdown);
        assert_eq!(detect(b"  <!DOCTYPE HTML><p>", "page"), Format::Html);
        assert_eq!(detect(b"<?xml version=\"1.0\"?><a/>", "f"), Format::Xml);
        assert_eq!(detect(b"{\\rtf1\\ansi hi}", "f"), Format::Rtf);
        assert_eq!(detect("\u{feff}a,b".as_bytes(), "x.csv"), Format::Csv);
        assert_eq!(detect(b"plain", ".gitignore"), Format::Text);
    }

    #[test]
    fn a_sample_cut_inside_a_character_is_still_text() {
        let s = "héllo wörld ✓".as_bytes();
        assert_eq!(detect(&s[..s.len() - 1], "x"), Format::Text);
        assert_eq!(detect(&[b'a', 0xff, b'b'], "x"), Format::Unknown);
    }

    #[test]
    fn hostile_or_truncated_input_never_panics() {
        let mut x: u64 = 0x9e3779b97f4a7c15;
        for n in 0..2000usize {
            let len = n % 600;
            let buf: Vec<u8> = (0..len)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    (x >> 24) as u8
                })
                .collect();
            let _ = detect(&buf, "f.bin");
        }
        for magic in [&b"%PDF-"[..], b"PK\x03\x04", b"RIFF", b"SQLite format 3", b"\x89PNG"] {
            for cut in 0..magic.len() {
                let _ = detect(&magic[..cut], "f");
            }
        }
    }
}
