//! Image files, as metadata only (`specs/014` §4): format, size, colour depth, frames and a few text fields.
//! The pixels are not read here; images reach a model through its vision input, not through this tool.

use crate::archive::human;
use crate::sniff::Format;
use crate::{DocError, Extract, Unit};

fn be32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn le16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn be16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn clean(b: &[u8]) -> String {
    String::from_utf8_lossy(b).chars().map(|c| if c.is_control() { ' ' } else { c }).take(120).collect::<String>().trim().to_string()
}

fn png(b: &[u8]) -> Option<Vec<String>> {
    let (w, h) = (be32(b, 16)?, be32(b, 20)?);
    let depth = *b.get(24)?;
    let color = match *b.get(25)? {
        0 => "grayscale",
        2 => "RGB",
        3 => "palette",
        4 => "grayscale+alpha",
        6 => "RGBA",
        _ => "unknown colour type",
    };
    let mut out = vec![format!("{}×{} pixels · {}-bit {}{}", w, h, depth, color, if *b.get(28)? == 1 { " · interlaced" } else { "" })];
    let mut p = 8;
    let (mut texts, mut animated) = (0, false);
    while p + 8 <= b.len() && texts < 6 {
        let len = be32(b, p)? as usize;
        let kind = b.get(p + 4..p + 8)?;
        let data = b.get(p + 8..(p + 8).saturating_add(len).min(b.len()))?;
        match kind {
            b"acTL" => animated = true,
            b"tEXt" => {
                if let Some(z) = data.iter().position(|&c| c == 0) {
                    let key = clean(&data[..z]);
                    if matches!(key.as_str(), "Title" | "Author" | "Description" | "Comment" | "Software" | "Copyright" | "Source") {
                        out.push(format!("{}: {}", key, clean(&data[z + 1..])));
                        texts += 1;
                    }
                }
            }
            b"IEND" => break,
            _ => {}
        }
        p = p.checked_add(12)?.checked_add(len)?;
    }
    if animated {
        out.push("animated (APNG)".into());
    }
    Some(out)
}

fn jpeg(b: &[u8]) -> Option<Vec<String>> {
    let mut p = 2;
    let (mut out, mut exif, mut comment) = (vec![], false, None);
    let mut size = None;
    for _ in 0..200 {
        while b.get(p) != Some(&0xff) {
            p += 1;
            if p >= b.len() {
                return finish_jpeg(size, exif, comment, out);
            }
        }
        while b.get(p) == Some(&0xff) {
            p += 1;
        }
        let m = *b.get(p)?;
        p += 1;
        if m == 0xd8 || (0xd0..=0xd7).contains(&m) || m == 0x01 {
            continue;
        }
        let len = be16(b, p)? as usize;
        let seg = b.get(p + 2..(p + len).min(b.len()))?;
        match m {
            0xc0..=0xcf if !matches!(m, 0xc4 | 0xc8 | 0xcc) => {
                size = Some((be16(seg, 3)?, be16(seg, 1)?, seg.first().copied()?, seg.get(5).copied()?, m == 0xc2 || m == 0xc6 || m == 0xca));
                break;
            }
            0xe1 if seg.starts_with(b"Exif") => exif = true,
            0xfe => comment = Some(clean(seg)),
            0xda => break,
            _ => {}
        }
        p += len;
    }
    out.shrink_to_fit();
    finish_jpeg(size, exif, comment, out)
}

fn finish_jpeg(size: Option<(u16, u16, u8, u8, bool)>, exif: bool, comment: Option<String>, mut out: Vec<String>) -> Option<Vec<String>> {
    let (w, h, bits, comps, progressive) = size?;
    out.push(format!("{}×{} pixels · {}-bit, {} component{}{}", w, h, bits, comps, if comps == 1 { "" } else { "s" }, if progressive { " · progressive" } else { "" }));
    if exif {
        out.push("has EXIF metadata (camera, date, location may be present; not read)".into());
    }
    if let Some(c) = comment.filter(|c| !c.is_empty()) {
        out.push(format!("Comment: {}", c));
    }
    Some(out)
}

fn gif(b: &[u8]) -> Option<Vec<String>> {
    let (w, h) = (le16(b, 6)?, le16(b, 8)?);
    let flags = *b.get(10)?;
    let mut p = 13 + if flags & 0x80 != 0 { 3 * (1usize << ((flags & 7) + 1)) } else { 0 };
    let mut frames = 0;
    for _ in 0..100_000 {
        match *b.get(p)? {
            0x21 => {
                p += 2;
                loop {
                    let n = *b.get(p)? as usize;
                    p += 1 + n;
                    if n == 0 {
                        break;
                    }
                }
            }
            0x2c => {
                frames += 1;
                let f = *b.get(p + 9)?;
                p += 10 + if f & 0x80 != 0 { 3 * (1usize << ((f & 7) + 1)) } else { 0 } + 1;
                loop {
                    let n = *b.get(p)? as usize;
                    p += 1 + n;
                    if n == 0 {
                        break;
                    }
                }
            }
            _ => break,
        }
    }
    Some(vec![format!("{}×{} pixels · {} frame{}{}", w, h, frames, if frames == 1 { "" } else { "s" }, if frames > 1 { " · animated" } else { "" })])
}

fn webp(b: &[u8]) -> Option<Vec<String>> {
    let kind = b.get(12..16)?;
    let (w, h, note) = match kind {
        b"VP8 " => ((le16(b, 26)? & 0x3fff) as u32, (le16(b, 28)? & 0x3fff) as u32, "lossy"),
        b"VP8L" => {
            let (b1, b2, b3, b4) = (*b.get(22)? as u32, *b.get(23)? as u32, *b.get(24)? as u32, *b.get(25)? as u32);
            (1 + (b1 | (b2 & 0x3f) << 8), 1 + ((b2 >> 6) | b3 << 2 | (b4 & 0xf) << 10), "lossless")
        }
        b"VP8X" => {
            let d = |o: usize| (*b.get(o)? as u32 | (*b.get(o + 1)? as u32) << 8 | (*b.get(o + 2)? as u32) << 16).checked_add(1);
            (d(24)?, d(27)?, if b.get(20)? & 0x02 != 0 { "extended, animated" } else { "extended" })
        }
        _ => return None,
    };
    Some(vec![format!("{}×{} pixels · {}", w, h, note)])
}

pub fn extract(bytes: &[u8], format: Format) -> Result<Extract, DocError> {
    let name = format.name();
    let info = match format {
        Format::Png => png(bytes),
        Format::Jpeg => jpeg(bytes),
        Format::Gif => gif(bytes),
        _ => webp(bytes),
    };
    let mut text = format!("{} · {}", name, human(bytes.len() as u64));
    let mut loss = vec!["pixel content is not read here; a model sees images through its vision input".to_string()];
    match info {
        Some(lines) => {
            for l in lines {
                text.push_str(&format!("\n{}", l));
            }
        }
        None => loss.push("the image header is damaged or truncated; size and depth are unknown".to_string()),
    }
    Ok(Extract { format, text, unit: Unit::Line, total_units: None, truncated: false, loss, supported: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(b: &[u8], f: Format) -> String {
        extract(b, f).unwrap().text
    }

    #[test]
    fn png_size_depth_and_text_fields() {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.extend_from_slice(&13u32.to_be_bytes());
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&[0, 0, 7, 128, 0, 0, 4, 56, 8, 6, 0, 0, 0]);
        b.extend_from_slice(&[0; 4]);
        let t = b"Software\0Delphi";
        b.extend_from_slice(&(t.len() as u32).to_be_bytes());
        b.extend_from_slice(b"tEXt");
        b.extend_from_slice(t);
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(b"IEND\0\0\0\0");
        let t = ex(&b, Format::Png);
        assert!(t.contains("1920×1080 pixels · 8-bit RGBA") && t.contains("Software: Delphi"), "{}", t);
    }

    #[test]
    fn jpeg_gif_and_webp_headers() {
        let j = [&[0xff, 0xd8, 0xff, 0xe1, 0, 8][..], b"Exif\0\0", &[0xff, 0xc0, 0, 11, 8, 0x02, 0x58, 0x03, 0x20, 3, 0, 0, 0], &[0xff, 0xda, 0, 2]].concat();
        let t = ex(&j, Format::Jpeg);
        assert!(t.contains("800×600 pixels · 8-bit, 3 components") && t.contains("EXIF"), "{}", t);
        let g = [&b"GIF89a"[..], &[10, 0, 20, 0, 0x80, 0, 0], &[0; 6], &[0x2c, 0, 0, 0, 0, 10, 0, 20, 0, 0, 2, 1, 0, 0], &[0x2c, 0, 0, 0, 0, 10, 0, 20, 0, 0, 2, 1, 0, 0], &[0x3b]].concat();
        assert!(ex(&g, Format::Gif).contains("10×20 pixels · 2 frames · animated"), "{}", ex(&g, Format::Gif));
        let w = [&b"RIFF\0\0\0\0WEBPVP8X"[..], &[10, 0, 0, 0, 0x02, 0, 0, 0], &[0x7f, 0x07, 0, 0x37, 0x04, 0]].concat();
        assert!(ex(&w, Format::Webp).contains("1920×1080 pixels · extended, animated"), "{}", ex(&w, Format::Webp));
    }

    #[test]
    fn truncated_and_hostile_headers_are_reported_not_panicked_on() {
        for f in [Format::Png, Format::Jpeg, Format::Gif, Format::Webp] {
            let e = extract(b"x", f).unwrap();
            assert!(e.loss.iter().any(|l| l.contains("damaged or truncated")), "{:?}", f);
        }
        let mut x: u64 = 0x3141_5926_5358_9793;
        for n in 0..3000usize {
            let b: Vec<u8> = (0..n % 120)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    x as u8
                })
                .collect();
            for (magic, f) in [(&b"\x89PNG\r\n\x1a\n"[..], Format::Png), (&[0xff, 0xd8][..], Format::Jpeg), (&b"GIF89a"[..], Format::Gif), (&b"RIFF\0\0\0\0WEBP"[..], Format::Webp)] {
                let _ = extract(&[magic, &b].concat(), f);
            }
        }
    }
}
