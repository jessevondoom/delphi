//! PNG files we write (`specs/014` §8.4): 8-bit RGBA, one zlib stream, no interlacing. The encoder uses only
//! `flate2` and `crc32fast`. A small decoder (8-bit RGB and RGBA, every filter) reads our own output back, so tests
//! compare pixels and not bytes.

use crate::DocError;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{Read, Write};

/// Pixels, row by row, four bytes each.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width + x) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut h = crc32fast::Hasher::new();
    h.update(kind);
    h.update(data);
    out.extend_from_slice(&h.finalize().to_be_bytes());
}

fn text_chunk(out: &mut Vec<u8>, key: &str, value: &str) {
    // tEXt is Latin-1; anything else becomes `?`, and control characters become spaces
    let latin = |s: &str| -> Vec<u8> { s.chars().map(|c| if c.is_control() { b' ' } else if (c as u32) < 256 { c as u8 } else { b'?' }).collect() };
    let mut data = latin(key);
    data.push(0);
    data.extend(latin(value).into_iter().take(1000));
    chunk(out, b"tEXt", &data);
}

/// Encode with `text` as (keyword, value) pairs; each is a tEXt chunk, which `read_doc` shows for known keywords.
pub fn encode(img: &Image, text: &[(&str, &str)]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(img.width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(img.height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    for (k, v) in text {
        text_chunk(&mut out, k, v);
    }
    // filter type 1 (Sub) on every row: flat colour runs become zeros, which deflate well
    let stride = img.width * 4;
    let mut raw = Vec::with_capacity((stride + 1) * img.height);
    for y in 0..img.height {
        raw.push(1);
        let row = &img.rgba[y * stride..(y + 1) * stride];
        for (i, b) in row.iter().enumerate() {
            raw.push(b.wrapping_sub(if i >= 4 { row[i - 4] } else { 0 }));
        }
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::new(6));
    z.write_all(&raw).expect("writing to a Vec cannot fail");
    chunk(&mut out, b"IDAT", &z.finish().expect("finishing a Vec cannot fail"));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn be32(b: &[u8], o: usize) -> Option<usize> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (pa, pb, pc) = ((b as i32 - c as i32).abs(), (a as i32 - c as i32).abs(), (a as i32 + b as i32 - 2 * c as i32).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Decode an 8-bit RGB or RGBA PNG without interlacing. Limits: 16384 pixels a side and 64 MB of pixels.
pub fn decode(b: &[u8]) -> Result<Image, DocError> {
    let bad = |m: &str| DocError(format!("not a PNG this reader handles: {}", m));
    if !b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(bad("no PNG signature"));
    }
    let (mut p, mut dims, mut idat) = (8, None, Vec::new());
    while p + 12 <= b.len() {
        let len = be32(b, p).ok_or_else(|| bad("truncated"))?;
        let kind = &b[p + 4..p + 8];
        let data = b.get(p + 8..p + 8 + len).ok_or_else(|| bad("truncated chunk"))?;
        let crc = be32(b, p + 8 + len).ok_or_else(|| bad("truncated"))? as u32;
        let mut h = crc32fast::Hasher::new();
        h.update(kind);
        h.update(data);
        if h.finalize() != crc {
            return Err(bad("a chunk fails its checksum"));
        }
        match kind {
            b"IHDR" => {
                if data.len() != 13 || data[8] != 8 || !matches!(data[9], 2 | 6) || data[12] != 0 {
                    return Err(bad("only 8-bit RGB and RGBA, not interlaced"));
                }
                dims = Some((be32(data, 0).unwrap_or(0), be32(data, 4).unwrap_or(0), if data[9] == 6 { 4 } else { 3 }));
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        p += 12 + len;
    }
    let (w, h, bpp) = dims.ok_or_else(|| bad("no header"))?;
    if w == 0 || h == 0 || w > 16384 || h > 16384 || w * h * 4 > 64 << 20 {
        return Err(bad("size out of range"));
    }
    let mut raw = Vec::with_capacity((w * bpp + 1) * h);
    ZlibDecoder::new(&idat[..]).take(((w * bpp + 1) * h + 1) as u64).read_to_end(&mut raw).map_err(|e| bad(&e.to_string()))?;
    if raw.len() != (w * bpp + 1) * h {
        return Err(bad("pixel data has the wrong length"));
    }
    let stride = w * bpp;
    let mut px = vec![0u8; stride * h];
    for y in 0..h {
        let (ft, line) = (raw[y * (stride + 1)], &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)]);
        for i in 0..stride {
            let a = if i >= bpp { px[y * stride + i - bpp] } else { 0 };
            let up = if y > 0 { px[(y - 1) * stride + i] } else { 0 };
            let c = if y > 0 && i >= bpp { px[(y - 1) * stride + i - bpp] } else { 0 };
            px[y * stride + i] = line[i].wrapping_add(match ft {
                0 => 0,
                1 => a,
                2 => up,
                3 => ((a as u16 + up as u16) / 2) as u8,
                4 => paeth(a, up, c),
                _ => return Err(bad("unknown row filter")),
            });
        }
    }
    let rgba = if bpp == 4 { px } else { px.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect() };
    Ok(Image { width: w, height: h, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: usize, h: usize) -> Image {
        Image { width: w, height: h, rgba: (0..w * h).flat_map(|i| [(i % w * 255 / w) as u8, (i / w * 255 / h) as u8, ((i * 7) % 256) as u8, 255]).collect() }
    }

    #[test]
    fn encoding_then_decoding_gives_the_same_pixels() {
        let img = gradient(37, 23);
        let bytes = encode(&img, &[("Title", "T"), ("Comment", "written by a test")]);
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.ends_with(&[0xAE, 0x42, 0x60, 0x82]), "signature and IEND");
        assert_eq!(decode(&bytes).unwrap(), img);
    }

    #[test]
    fn the_file_is_a_valid_png_to_our_own_image_reader_and_carries_its_text() {
        let bytes = encode(&gradient(10, 6), &[("Title", "A chart"), ("Comment", "delphi artifact: test")]);
        let e = crate::extract(&bytes, "x.png", &crate::Request::default(), &crate::Limits::default()).unwrap();
        assert!(e.text.contains("10×6 pixels") && e.text.contains("8-bit RGBA") && e.text.contains("Title: A chart") && e.text.contains("Comment: delphi artifact: test"), "{}", e.text);
    }

    #[test]
    fn flat_images_compress_to_a_small_file() {
        let img = Image { width: 1440, height: 800, rgba: [252, 252, 251, 255].repeat(1440 * 800) };
        assert!(encode(&img, &[]).len() < 8_000);
    }

    #[test]
    fn corrupt_and_hostile_files_are_refused_not_trusted() {
        let good = encode(&gradient(8, 8), &[]);
        let mut flipped = good.clone();
        flipped[40] ^= 0xFF;
        assert!(decode(&flipped).unwrap_err().0.contains("checksum"));
        assert!(decode(&good[..good.len() - 20]).is_err());
        assert!(decode(b"not a png").is_err());
        let mut huge = good.clone();
        huge[16..20].copy_from_slice(&0xFFFFFFFFu32.to_be_bytes());
        assert!(decode(&huge).is_err(), "a lying header is refused (by checksum or size)");
    }

    #[test]
    fn text_is_latin1_and_cannot_break_the_chunk() {
        let bytes = encode(&gradient(2, 2), &[("Title", "line\nbreak — and ünï")]);
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("Title") && decode(&bytes).is_ok());
    }
}
