//! PNG files we write (`specs/014` §8.4): 8-bit RGBA, one zlib stream, no interlacing. The encoder uses only
//! `flate2` and `crc32fast`. The decoder reads any PNG (`specs/029` §9: the reader draws images from the knowledge
//! base) and our own output back, so tests compare pixels and not bytes.

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

/// Decode a PNG: every colour type and bit depth (1 to 16), palettes and `tRNS` transparency, and Adam7 interlacing.
/// 16-bit samples keep their high byte. Limits: 16384 pixels a side and 64 MB of pixels.
pub fn decode(b: &[u8]) -> Result<Image, DocError> {
    let bad = |m: &str| DocError(format!("not a PNG this reader handles: {}", m));
    if !b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(bad("no PNG signature"));
    }
    let (mut p, mut hdr, mut idat) = (8, None, Vec::new());
    let (mut palette, mut trns): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
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
                let ok = data.len() == 13 && data[12] <= 1 && data[10] == 0 && data[11] == 0;
                let (depth, color) = (data.get(8).copied().unwrap_or(0), data.get(9).copied().unwrap_or(9));
                let legal = match color {
                    0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
                    3 => matches!(depth, 1 | 2 | 4 | 8),
                    2 | 4 | 6 => matches!(depth, 8 | 16),
                    _ => false,
                };
                if !ok || !legal {
                    return Err(bad("unsupported header"));
                }
                hdr = Some((be32(data, 0).unwrap_or(0), be32(data, 4).unwrap_or(0), depth as usize, color, data[12] == 1));
            }
            b"PLTE" => palette = data.to_vec(),
            b"tRNS" => trns = data.to_vec(),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        p += 12 + len;
    }
    let (w, h, depth, color, interlaced) = hdr.ok_or_else(|| bad("no header"))?;
    if w == 0 || h == 0 || w > 16384 || h > 16384 || w * h * 4 > 64 << 20 {
        return Err(bad("size out of range"));
    }
    if color == 3 && palette.len() < 3 {
        return Err(bad("no palette"));
    }
    let channels = match color {
        0 | 3 => 1,
        4 => 2,
        2 => 3,
        _ => 4,
    };
    let bits = channels * depth;
    let bpp = bits.div_ceil(8); // filter distance in bytes
    // the passes: (x start, y start, x step, y step); a plain image is one pass over everything
    const ADAM7: [(usize, usize, usize, usize); 7] = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];
    let passes: &[(usize, usize, usize, usize)] = if interlaced { &ADAM7 } else { &[(0, 0, 1, 1)] };
    let dims: Vec<(usize, usize)> = passes.iter().map(|&(x0, y0, dx, dy)| (w.saturating_sub(x0).div_ceil(dx), h.saturating_sub(y0).div_ceil(dy))).collect();
    let need: usize = dims.iter().map(|&(pw, ph)| if pw == 0 || ph == 0 { 0 } else { (pw * bits).div_ceil(8) * ph + ph }).sum();
    let mut raw = Vec::with_capacity(need);
    ZlibDecoder::new(&idat[..]).take(need as u64 + 1).read_to_end(&mut raw).map_err(|e| bad(&e.to_string()))?;
    if raw.len() != need {
        return Err(bad("pixel data has the wrong length"));
    }
    let sample = |row: &[u8], i: usize| -> u16 {
        // sample `i` of a row: sub-byte depths pack from the high bits; 16-bit keeps the high byte
        match depth {
            16 => row[i * 2] as u16,
            8 => row[i] as u16,
            d => {
                let bit = i * d;
                ((row[bit / 8] >> (8 - d - bit % 8)) & ((1u8 << d) - 1)) as u16
            }
        }
    };
    let scale = |v: u16| -> u8 { if depth >= 8 { v as u8 } else { (v * 255 / ((1 << depth) - 1)) as u8 } };
    let mut rgba = vec![0u8; w * h * 4];
    let mut at = 0;
    for (pass, &(pw, ph)) in dims.iter().enumerate() {
        if pw == 0 || ph == 0 {
            continue;
        }
        let (x0, y0, dx, dy) = passes[pass];
        let stride = (pw * bits).div_ceil(8);
        let mut prev = vec![0u8; stride];
        for py in 0..ph {
            let ft = raw[at];
            let mut line = raw[at + 1..at + 1 + stride].to_vec();
            at += 1 + stride;
            for i in 0..stride {
                let a = if i >= bpp { line[i - bpp] } else { 0 };
                let up = prev[i];
                let c = if i >= bpp { prev[i - bpp] } else { 0 };
                line[i] = line[i].wrapping_add(match ft {
                    0 => 0,
                    1 => a,
                    2 => up,
                    3 => ((a as u16 + up as u16) / 2) as u8,
                    4 => paeth(a, up, c),
                    _ => return Err(bad("unknown row filter")),
                });
            }
            for px in 0..pw {
                let s = |k: usize| sample(&line, px * channels + k);
                // the raw (unscaled) value of a one-channel pixel, for the tRNS colour key
                let px4: [u8; 4] = match color {
                    0 => {
                        let g = scale(s(0));
                        let key = trns.len() >= 2 && s(0) == (if depth == 16 { trns[0] as u16 } else { u16::from_be_bytes([trns[0], trns[1]]) });
                        [g, g, g, if key { 0 } else { 255 }]
                    }
                    2 => {
                        let key = trns.len() >= 6 && (0..3).all(|k| s(k) == if depth == 16 { trns[k * 2] as u16 } else { u16::from_be_bytes([trns[k * 2], trns[k * 2 + 1]]) });
                        [scale(s(0)), scale(s(1)), scale(s(2)), if key { 0 } else { 255 }]
                    }
                    3 => {
                        let i = s(0) as usize;
                        let rgb = palette.get(i * 3..i * 3 + 3).ok_or_else(|| bad("palette index out of range"))?;
                        [rgb[0], rgb[1], rgb[2], trns.get(i).copied().unwrap_or(255)]
                    }
                    4 => {
                        let g = scale(s(0));
                        [g, g, g, scale(s(1))]
                    }
                    _ => [scale(s(0)), scale(s(1)), scale(s(2)), scale(s(3))],
                };
                let (x, y) = (x0 + px * dx, y0 + py * dy);
                let o = (y * w + x) * 4;
                rgba[o..o + 4].copy_from_slice(&px4);
            }
            prev = line;
        }
    }
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
