//! A GIF decoder for the reader (`specs/029` §9): the first frame of a GIF87a or GIF89a file, with its transparency,
//! interlacing and local or global palette. Later frames of an animation are ignored. Pure and dependency-free;
//! limits: 16384 pixels a side and 25 million pixels.

use crate::png::Image;
use crate::DocError;

fn bad(m: &str) -> DocError {
    DocError(format!("not a GIF this reader handles: {}", m))
}

fn le16(b: &[u8], o: usize) -> Result<usize, DocError> {
    Ok(u16::from_le_bytes(b.get(o..o + 2).ok_or_else(|| bad("truncated"))?.try_into().unwrap()) as usize)
}

/// The data sub-blocks that start at `p`, joined, and the offset after them.
fn blocks(b: &[u8], mut p: usize) -> Result<(Vec<u8>, usize), DocError> {
    let mut out = Vec::new();
    loop {
        let n = *b.get(p).ok_or_else(|| bad("truncated"))? as usize;
        p += 1;
        if n == 0 {
            return Ok((out, p));
        }
        out.extend_from_slice(b.get(p..p + n).ok_or_else(|| bad("truncated"))?);
        p += n;
    }
}

/// Variable-width LZW as GIF uses it: codes grow from `min + 1` bits to 12, a clear code resets the table. A damaged
/// stream ends the output early; the caller pads the rest.
fn lzw(data: &[u8], min: u32, want: usize) -> Vec<u8> {
    let (clear, end) = (1usize << min, (1usize << min) + 1);
    let mut table: Vec<(u16, u8)> = Vec::new(); // (prefix code, byte); entry i is code clear + 2 + i
    let mut out = Vec::with_capacity(want);
    let (mut width, mut prev, mut buf, mut nbits, mut pos) = (min + 1, None::<usize>, 0u32, 0u32, 0usize);
    // append the string for `code`; returns its first byte
    let expand = |table: &[(u16, u8)], mut code: usize, out: &mut Vec<u8>| -> u8 {
        let start = out.len();
        while code >= clear + 2 {
            let (p, b) = table[code - clear - 2];
            out.push(b);
            code = p as usize;
        }
        out.push(code as u8);
        out[start..].reverse();
        out[start]
    };
    while out.len() < want {
        while nbits < width {
            let Some(&byte) = data.get(pos) else { return out };
            buf |= (byte as u32) << nbits;
            nbits += 8;
            pos += 1;
        }
        let code = (buf & ((1 << width) - 1)) as usize;
        buf >>= width;
        nbits -= width;
        if code == clear {
            table.clear();
            (width, prev) = (min + 1, None);
            continue;
        }
        if code == end {
            break;
        }
        let known = clear + 2 + table.len();
        match prev {
            None if code < clear => out.push(code as u8),
            None => return out,
            Some(p) => {
                if code > known {
                    return out;
                }
                let first = if code < known {
                    expand(&table, code, &mut out)
                } else {
                    // the code being defined right now: the previous string plus its own first byte
                    let f = expand(&table, p, &mut out);
                    out.push(f);
                    f
                };
                if table.len() < 4096 - clear - 2 {
                    table.push((p as u16, first));
                }
            }
        }
        prev = Some(code);
        if clear + 2 + table.len() >= (1 << width) && width < 12 {
            width += 1;
        }
    }
    out
}

pub fn decode(b: &[u8]) -> Result<Image, DocError> {
    if !(b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) {
        return Err(bad("no GIF signature"));
    }
    let (w, h, flags) = (le16(b, 6)?, le16(b, 8)?, *b.get(10).ok_or_else(|| bad("truncated"))?);
    if w == 0 || h == 0 || w > 16384 || h > 16384 || w * h > 25_000_000 {
        return Err(bad("size out of range"));
    }
    let mut p = 13;
    let mut global: &[u8] = &[];
    if flags & 0x80 != 0 {
        let n = 3 << ((flags & 7) + 1);
        global = b.get(p..p + n).ok_or_else(|| bad("truncated"))?;
        p += n;
    }
    let mut transparent: Option<u8> = None;
    loop {
        match *b.get(p).ok_or_else(|| bad("no image"))? {
            0x21 => {
                let label = *b.get(p + 1).ok_or_else(|| bad("truncated"))?;
                let (data, next) = blocks(b, p + 2)?;
                if label == 0xF9 && data.len() >= 4 && data[0] & 1 == 1 {
                    transparent = Some(data[3]);
                }
                p = next;
            }
            0x2C => break,
            _ => return Err(bad("no image")),
        }
    }
    let (left, top, iw, ih) = (le16(b, p + 1)?, le16(b, p + 3)?, le16(b, p + 5)?, le16(b, p + 7)?);
    let iflags = *b.get(p + 9).ok_or_else(|| bad("truncated"))?;
    p += 10;
    let mut table = global;
    if iflags & 0x80 != 0 {
        let n = 3 << ((iflags & 7) + 1);
        table = b.get(p..p + n).ok_or_else(|| bad("truncated"))?;
        p += n;
    }
    if table.is_empty() {
        return Err(bad("no palette"));
    }
    if iw * ih > 25_000_000 {
        return Err(bad("size out of range"));
    }
    let min = *b.get(p).ok_or_else(|| bad("truncated"))? as u32;
    if !(2..=8).contains(&min) {
        return Err(bad("bad code size"));
    }
    let (data, _) = blocks(b, p + 1)?;
    let mut idx = lzw(&data, min, iw * ih);
    idx.resize(iw * ih, 0);
    // interlaced rows arrive in four passes; the n-th stored row belongs at `row_of[n]`
    let row_of: Vec<usize> = if iflags & 0x40 != 0 { [(0, 8), (4, 8), (2, 4), (1, 2)].iter().flat_map(|&(s, st)| (s..ih).step_by(st)).collect() } else { (0..ih).collect() };
    let mut rgba = vec![0u8; w * h * 4];
    for (n, &y) in row_of.iter().enumerate() {
        for x in 0..iw {
            let (px, py) = (left + x, top + y);
            if px >= w || py >= h {
                continue;
            }
            let i = idx[n * iw + x];
            let c = table.get(i as usize * 3..i as usize * 3 + 3).unwrap_or(&[0, 0, 0]);
            let o = (py * w + px) * 4;
            rgba[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], if Some(i) == transparent { 0 } else { 255 }]);
        }
    }
    Ok(Image { width: w, height: h, rgba })
}
