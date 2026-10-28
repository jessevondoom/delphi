//! A JPEG decoder (`specs/029` §9: the reader draws images from the knowledge base): Huffman-coded baseline,
//! extended-sequential and progressive files, 8 bits a sample, one component (grey) or three (YCbCr, or RGB when the
//! Adobe marker says so). Arithmetic coding, 12-bit samples, lossless and four-component (CMYK) files are refused.
//! Every scan, whatever its kind, fills one array of coefficients per component; the transform and the colour
//! conversion run once at the end. The upsampling is nearest-neighbour: the reader shrinks the picture to a few
//! dozen cells wide, so nothing finer would show. Pure and dependency-free; limits: 16384 pixels a side, 25 million pixels.

use crate::png::Image;
use crate::DocError;

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

fn bad(m: &str) -> DocError {
    DocError(format!("not a JPEG this reader handles: {}", m))
}

/// A canonical Huffman table, decoded a bit at a time (the files we draw are small; speed is not the point).
#[derive(Clone, Default)]
struct Huff {
    mincode: [i32; 17],
    maxcode: [i32; 17],
    valptr: [usize; 17],
    vals: Vec<u8>,
}

impl Huff {
    fn new(counts: &[u8], vals: &[u8]) -> Huff {
        let mut h = Huff { vals: vals.to_vec(), ..Default::default() };
        let (mut code, mut k) = (0i32, 0usize);
        for l in 1..=16 {
            let n = counts[l - 1] as i32;
            h.valptr[l] = k;
            h.mincode[l] = code;
            code += n;
            k += n as usize;
            h.maxcode[l] = if n > 0 { code - 1 } else { -1 };
            code <<= 1;
        }
        h
    }
}

/// The entropy-coded bytes of a scan: `FF 00` is a data byte, any other `FF xx` is a marker that ends the data
/// (the reader then supplies zero bits, so a truncated file ends in grey, not in an error).
struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    buf: u32,
    n: u32,
    marker: bool,
}

impl Bits<'_> {
    fn bit(&mut self) -> u32 {
        if self.n == 0 {
            let mut byte = 0u8;
            if !self.marker {
                match self.d.get(self.pos) {
                    Some(0xFF) => match self.d.get(self.pos + 1) {
                        Some(0) => {
                            byte = 0xFF;
                            self.pos += 2;
                        }
                        _ => self.marker = true,
                    },
                    Some(&b) => {
                        byte = b;
                        self.pos += 1;
                    }
                    None => self.marker = true,
                }
            }
            self.buf = byte as u32;
            self.n = 8;
        }
        self.n -= 1;
        (self.buf >> self.n) & 1
    }

    fn bits(&mut self, n: u32) -> i32 {
        (0..n).fold(0, |v, _| (v << 1) | self.bit() as i32)
    }

    /// A signed value of `s` bits as JPEG codes it (the high bit clear means negative).
    fn extend(&mut self, s: u32) -> i32 {
        if s == 0 {
            return 0;
        }
        let v = self.bits(s);
        if v < 1 << (s - 1) { v - (1 << s) + 1 } else { v }
    }

    fn decode(&mut self, h: &Huff) -> Result<u8, DocError> {
        let mut code = 0i32;
        for l in 1..=16 {
            code = (code << 1) | self.bit() as i32;
            if h.maxcode[l] >= 0 && code <= h.maxcode[l] && code >= h.mincode[l] {
                return h.vals.get(h.valptr[l] + (code - h.mincode[l]) as usize).copied().ok_or_else(|| bad("bad Huffman code"));
            }
        }
        Err(bad("bad Huffman code"))
    }

    /// Past a restart interval: drop the partial byte and move over the `FF D0..D7` marker.
    fn restart(&mut self) {
        self.n = 0;
        self.marker = false;
        while self.pos + 1 < self.d.len() {
            if self.d[self.pos] == 0xFF && (0xD0..=0xD7).contains(&self.d[self.pos + 1]) {
                self.pos += 2;
                return;
            }
            if self.d[self.pos] == 0xFF && self.d[self.pos + 1] != 0 && self.d[self.pos + 1] != 0xFF {
                return; // a marker that is not a restart: leave it for the segment reader
            }
            self.pos += 1;
        }
    }
}

struct Comp {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    /// blocks across and down, padded to whole MCUs
    bw: usize,
    bh: usize,
    coef: Vec<i16>,
    dc_pred: i32,
}

struct Scan {
    comps: Vec<(usize, usize, usize)>, // (component, dc table, ac table)
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
}

fn be16(b: &[u8], o: usize) -> Result<usize, DocError> {
    Ok(u16::from_be_bytes(b.get(o..o + 2).ok_or_else(|| bad("truncated"))?.try_into().unwrap()) as usize)
}

pub fn decode(b: &[u8]) -> Result<Image, DocError> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return Err(bad("no JPEG signature"));
    }
    let mut p = 2;
    let mut qt = [[0u16; 64]; 4];
    let (mut dc, mut ac): (Vec<Huff>, Vec<Huff>) = (vec![Huff::default(); 4], vec![Huff::default(); 4]);
    let (mut w, mut h, mut progressive) = (0usize, 0usize, false);
    let mut comps: Vec<Comp> = Vec::new();
    let (mut hmax, mut vmax) = (1, 1);
    let (mut restart, mut adobe, mut scans) = (0usize, None, 0);
    while p + 4 <= b.len() {
        if b[p] != 0xFF {
            p += 1;
            continue;
        }
        let m = b[p + 1];
        if m == 0xFF || m == 0 || (0xD0..=0xD7).contains(&m) || m == 0x01 {
            p += if m == 0xFF { 1 } else { 2 };
            continue;
        }
        if m == 0xD9 {
            break;
        }
        let len = be16(b, p + 2)?;
        let seg = b.get(p + 4..p + 2 + len).ok_or_else(|| bad("truncated segment"))?;
        p += 2 + len;
        match m {
            0xC0..=0xC2 => {
                if !comps.is_empty() {
                    return Err(bad("two frames"));
                }
                progressive = m == 0xC2;
                if seg.len() < 6 || seg[0] != 8 {
                    return Err(bad("only 8-bit samples"));
                }
                (h, w) = (be16(seg, 1)?, be16(seg, 3)?);
                let n = seg[5] as usize;
                if w == 0 || h == 0 || w > 16384 || h > 16384 || w * h > 25_000_000 {
                    return Err(bad("size out of range"));
                }
                if !matches!(n, 1 | 3) || seg.len() < 6 + n * 3 {
                    return Err(bad("only grey and three-component colour"));
                }
                for i in 0..n {
                    let s = &seg[6 + i * 3..9 + i * 3];
                    let (hh, vv) = ((s[1] >> 4) as usize, (s[1] & 15) as usize);
                    if !(1..=4).contains(&hh) || !(1..=4).contains(&vv) || s[2] > 3 {
                        return Err(bad("bad component"));
                    }
                    comps.push(Comp { id: s[0], h: hh, v: vv, tq: s[2] as usize, bw: 0, bh: 0, coef: Vec::new(), dc_pred: 0 });
                }
                hmax = comps.iter().map(|c| c.h).max().unwrap();
                vmax = comps.iter().map(|c| c.v).max().unwrap();
                let (mx, my) = (w.div_ceil(8 * hmax), h.div_ceil(8 * vmax));
                for c in &mut comps {
                    (c.bw, c.bh) = (mx * c.h, my * c.v);
                    c.coef = vec![0i16; c.bw * c.bh * 64];
                }
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return Err(bad("lossless, hierarchical and arithmetic-coded files are not supported")),
            0xC4 => {
                let mut o = 0;
                while o + 17 <= seg.len() {
                    let (class, id) = (seg[o] >> 4, (seg[o] & 15) as usize);
                    let counts = &seg[o + 1..o + 17];
                    let total: usize = counts.iter().map(|&c| c as usize).sum();
                    let vals = seg.get(o + 17..o + 17 + total).ok_or_else(|| bad("truncated table"))?;
                    if id > 3 || class > 1 {
                        return Err(bad("bad Huffman table"));
                    }
                    let t = Huff::new(counts, vals);
                    if class == 0 { dc[id] = t } else { ac[id] = t }
                    o += 17 + total;
                }
            }
            0xDB => {
                let mut o = 0;
                while o < seg.len() {
                    let (prec, id) = (seg[o] >> 4, (seg[o] & 15) as usize);
                    if id > 3 {
                        return Err(bad("bad quantisation table"));
                    }
                    o += 1;
                    for i in 0..64 {
                        let v = if prec == 0 { *seg.get(o + i).ok_or_else(|| bad("truncated table"))? as u16 } else { be16(seg, o + i * 2)? as u16 };
                        qt[id][ZIGZAG[i]] = v;
                    }
                    o += if prec == 0 { 64 } else { 128 };
                }
            }
            0xDD => restart = be16(seg, 0)?,
            0xEE if seg.starts_with(b"Adobe") && seg.len() >= 12 => adobe = Some(seg[11]),
            0xDA => {
                if comps.is_empty() {
                    return Err(bad("a scan before the frame"));
                }
                let ns = *seg.first().ok_or_else(|| bad("truncated scan"))? as usize;
                if ns == 0 || ns > comps.len() || seg.len() < 1 + ns * 2 + 3 {
                    return Err(bad("bad scan header"));
                }
                let mut sc = Scan { comps: Vec::new(), ss: seg[1 + ns * 2] as usize, se: seg[2 + ns * 2] as usize, ah: (seg[3 + ns * 2] >> 4) as u32, al: (seg[3 + ns * 2] & 15) as u32 };
                for i in 0..ns {
                    let ci = comps.iter().position(|c| c.id == seg[1 + i * 2]).ok_or_else(|| bad("scan names an unknown component"))?;
                    let (td, ta) = ((seg[2 + i * 2] >> 4) as usize, (seg[2 + i * 2] & 15) as usize);
                    if td > 3 || ta > 3 {
                        return Err(bad("bad table number"));
                    }
                    sc.comps.push((ci, td, ta));
                }
                if !progressive {
                    (sc.ss, sc.se, sc.ah, sc.al) = (0, 63, 0, 0);
                } else if sc.se > 63 || sc.ss > sc.se || sc.al > 13 || (sc.ss == 0 && sc.se != 0) || (sc.ss > 0 && ns != 1) {
                    return Err(bad("bad progressive scan"));
                }
                let mut r = Bits { d: b, pos: p, buf: 0, n: 0, marker: false };
                scan(&mut r, &mut comps, &sc, &dc, &ac, progressive, restart, (w, h, hmax, vmax))?;
                p = r.pos;
                scans += 1;
            }
            _ => {}
        }
    }
    if comps.is_empty() || scans == 0 {
        return Err(bad("no image data"));
    }
    Ok(finish(&comps, &qt, (w, h, hmax, vmax), adobe))
}

#[allow(clippy::too_many_arguments)]
fn scan(r: &mut Bits, comps: &mut [Comp], sc: &Scan, dc: &[Huff], ac: &[Huff], progressive: bool, restart: usize, (w, h, hmax, vmax): (usize, usize, usize, usize)) -> Result<(), DocError> {
    for c in comps.iter_mut() {
        c.dc_pred = 0;
    }
    let mut eobrun = 0u32;
    // the units of this scan: MCUs for several components, plain blocks of the component for one
    let single = sc.comps.len() == 1;
    let (across, down) = if single {
        let c = &comps[sc.comps[0].0];
        ((w * c.h).div_ceil(hmax).div_ceil(8), (h * c.v).div_ceil(vmax).div_ceil(8))
    } else {
        (w.div_ceil(8 * hmax), h.div_ceil(8 * vmax))
    };
    let mut count = 0usize;
    for uy in 0..down {
        for ux in 0..across {
            if restart > 0 && count > 0 && count % restart == 0 {
                r.restart();
                for c in comps.iter_mut() {
                    c.dc_pred = 0;
                }
                eobrun = 0;
            }
            count += 1;
            for &(ci, td, ta) in &sc.comps {
                let (bh_, bv_) = if single { (1, 1) } else { (comps[ci].h, comps[ci].v) };
                for by in 0..bv_ {
                    for bx in 0..bh_ {
                        let c = &mut comps[ci];
                        let (x, y) = (ux * bh_ + bx, uy * bv_ + by);
                        if x >= c.bw || y >= c.bh {
                            continue;
                        }
                        let o = (y * c.bw + x) * 64;
                        let (blk, pred) = (&mut c.coef[o..o + 64], &mut c.dc_pred);
                        if !progressive {
                            baseline(r, blk, pred, &dc[td], &ac[ta])?;
                        } else if sc.ss == 0 {
                            if sc.ah == 0 {
                                let t = r.decode(&dc[td])? as u32;
                                *pred += r.extend(t.min(16));
                                blk[0] = (*pred << sc.al) as i16;
                            } else if r.bit() == 1 {
                                blk[0] |= 1 << sc.al;
                            }
                        } else if sc.ah == 0 {
                            ac_first(r, blk, &ac[ta], sc, &mut eobrun)?;
                        } else {
                            ac_refine(r, blk, &ac[ta], sc, &mut eobrun)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn baseline(r: &mut Bits, blk: &mut [i16], pred: &mut i32, dc: &Huff, ac: &Huff) -> Result<(), DocError> {
    let t = r.decode(dc)? as u32;
    *pred += r.extend(t.min(16));
    blk[0] = *pred as i16;
    let mut k = 1;
    while k < 64 {
        let rs = r.decode(ac)?;
        let (run, s) = ((rs >> 4) as usize, (rs & 15) as u32);
        if s == 0 {
            if run == 15 {
                k += 16;
                continue;
            }
            break;
        }
        k += run;
        if k > 63 {
            break;
        }
        blk[ZIGZAG[k]] = r.extend(s) as i16;
        k += 1;
    }
    Ok(())
}

fn ac_first(r: &mut Bits, blk: &mut [i16], ac: &Huff, sc: &Scan, eobrun: &mut u32) -> Result<(), DocError> {
    if *eobrun > 0 {
        *eobrun -= 1;
        return Ok(());
    }
    let mut k = sc.ss;
    while k <= sc.se {
        let rs = r.decode(ac)?;
        let (run, s) = ((rs >> 4) as u32, (rs & 15) as u32);
        if s == 0 {
            if run < 15 {
                *eobrun = (1 << run) - 1 + if run > 0 { r.bits(run) as u32 } else { 0 };
                break;
            }
            k += 16;
            continue;
        }
        k += run as usize;
        if k > 63 {
            break;
        }
        blk[ZIGZAG[k]] = (r.extend(s) * (1 << sc.al)) as i16;
        k += 1;
    }
    Ok(())
}

fn ac_refine(r: &mut Bits, blk: &mut [i16], ac: &Huff, sc: &Scan, eobrun: &mut u32) -> Result<(), DocError> {
    let (p1, m1) = (1i16 << sc.al, -1i16 << sc.al);
    let mut k = sc.ss;
    // a nonzero coefficient being refined: one more bit, added in its own direction when not already set
    let refine = |r: &mut Bits, v: &mut i16| {
        if r.bit() == 1 && *v & p1 == 0 {
            *v += if *v >= 0 { p1 } else { m1 };
        }
    };
    if *eobrun == 0 {
        while k <= sc.se {
            let rs = r.decode(ac)?;
            let (mut run, s) = ((rs >> 4) as u32, (rs & 15) as u32);
            let mut value = 0i16;
            if s == 0 {
                if run < 15 {
                    *eobrun = (1 << run) + if run > 0 { r.bits(run) as u32 } else { 0 };
                    break;
                }
            } else {
                value = if r.bit() == 1 { p1 } else { m1 };
            }
            while k <= sc.se {
                let v = &mut blk[ZIGZAG[k]];
                if *v != 0 {
                    refine(r, v);
                } else if run == 0 {
                    if value != 0 {
                        *v = value;
                    }
                    k += 1;
                    break;
                } else {
                    run -= 1;
                }
                k += 1;
            }
        }
    }
    if *eobrun > 0 {
        while k <= sc.se {
            let v = &mut blk[ZIGZAG[k]];
            if *v != 0 {
                refine(r, v);
            }
            k += 1;
        }
        *eobrun -= 1;
    }
    Ok(())
}

/// The inverse transform of one block, dequantised, into 8x8 samples.
fn idct(coef: &[i16], q: &[u16; 64], cos: &[[f32; 8]; 8], out: &mut [u8], stride: usize) {
    let mut tmp = [0f32; 64];
    for v in 0..8 {
        for x in 0..8 {
            tmp[v * 8 + x] = (0..8).map(|u| cos[x][u] * coef[v * 8 + u] as f32 * q[v * 8 + u] as f32).sum();
        }
    }
    for x in 0..8 {
        for y in 0..8 {
            let s: f32 = (0..8).map(|v| cos[y][v] * tmp[v * 8 + x]).sum();
            out[y * stride + x] = (s + 128.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

fn finish(comps: &[Comp], qt: &[[u16; 64]; 4], (w, h, hmax, vmax): (usize, usize, usize, usize), adobe: Option<u8>) -> Image {
    // cos[x][u] = C(u)/2 * cos((2x+1) u pi / 16)
    let mut cos = [[0f32; 8]; 8];
    for (x, row) in cos.iter_mut().enumerate() {
        for (u, c) in row.iter_mut().enumerate() {
            *c = (if u == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 }) / 2.0 * (((2 * x + 1) * u) as f32 * std::f32::consts::PI / 16.0).cos();
        }
    }
    let planes: Vec<(Vec<u8>, usize)> = comps
        .iter()
        .map(|c| {
            let stride = c.bw * 8;
            let mut plane = vec![0u8; stride * c.bh * 8];
            for by in 0..c.bh {
                for bx in 0..c.bw {
                    let o = (by * c.bw + bx) * 64;
                    idct(&c.coef[o..o + 64], &qt[c.tq], &cos, &mut plane[by * 8 * stride + bx * 8..], stride);
                }
            }
            (plane, stride)
        })
        .collect();
    let at = |i: usize, x: usize, y: usize| -> f32 {
        let (plane, stride) = &planes[i];
        plane[(y * comps[i].v / vmax) * stride + x * comps[i].h / hmax] as f32
    };
    let rgb_direct = adobe == Some(0);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let px = if comps.len() == 1 {
                let g = at(0, x, y) as u8;
                [g, g, g]
            } else if rgb_direct {
                [at(0, x, y) as u8, at(1, x, y) as u8, at(2, x, y) as u8]
            } else {
                let (yy, cb, cr) = (at(0, x, y), at(1, x, y) - 128.0, at(2, x, y) - 128.0);
                let f = |v: f32| v.round().clamp(0.0, 255.0) as u8;
                [f(yy + 1.402 * cr), f(yy - 0.344136 * cb - 0.714136 * cr), f(yy + 1.772 * cb)]
            };
            rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    Image { width: w, height: h, rgba }
}
