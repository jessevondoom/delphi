//! Pixels for the reader (`specs/029` §9): any image the knowledge base may hold (PNG, JPEG, GIF; WebP since `specs/118` §4), decoded and
//! shrunk to a few dozen cells. Nothing here draws; the console paints the result in half-block cells.

use crate::png::Image;
use crate::DocError;

/// Decode by what the bytes are, not by what the file is called.
pub fn decode(b: &[u8]) -> Result<Image, DocError> {
    if b.starts_with(b"\x89PNG") {
        crate::png::decode(b)
    } else if b.starts_with(&[0xFF, 0xD8]) {
        crate::jpeg::decode(b)
    } else if b.starts_with(b"GIF8") {
        crate::gif::decode(b)
    } else if b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        webp(b)
    } else {
        Err(DocError("not an image this reader draws (PNG, JPEG, GIF and WebP are)".into()))
    }
}

/// The most pixels a WebP may have before anything is allocated for it (the raster readers' limit).
const MAX_WEBP_PIXELS: usize = 24_000_000;

/// Every frame a WebP holds (a `VP8 ` or `VP8L` chunk at the top level or inside an animation frame, `ANMF`) declares a size of its
/// own, which `image-webp` decodes at before it compares it with the canvas. Each is read here first and refused when it is larger
/// than the canvas (`cw` x `ch`), than its animation frame, or than the pixel limit, so a small canvas cannot hide a huge frame.
fn webp_frames(b: &[u8], cw: usize, ch: usize) -> Result<(), DocError> {
    let bad = |why: &str| Err(DocError(format!("the WebP file is refused: {}", why)));
    let u24 = |p: &[u8]| p[0] as usize | (p[1] as usize) << 8 | (p[2] as usize) << 16;
    // (chunks, the largest frame they may hold), walked without recursion past one level of `ANMF`.
    let mut todo: Vec<(&[u8], usize, usize, bool)> = vec![(b.get(12..).unwrap_or(&[]), cw, ch, true)];
    while let Some((mut rest, mw, mh, top)) = todo.pop() {
        while rest.len() >= 8 {
            let kind = &rest[..4];
            let size = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]) as usize;
            let Some(payload) = rest.get(8..8 + size) else { return bad("a chunk is longer than the file") };
            let (fw, fh) = match kind {
                b"VP8 " => {
                    if payload.len() < 10 || payload[3..6] != [0x9d, 0x01, 0x2a] {
                        return bad("a lossy frame has no valid header");
                    }
                    ((u16::from_le_bytes([payload[6], payload[7]]) & 0x3fff) as usize, (u16::from_le_bytes([payload[8], payload[9]]) & 0x3fff) as usize)
                }
                b"VP8L" => {
                    if payload.len() < 5 || payload[0] != 0x2f {
                        return bad("a lossless frame has no valid header");
                    }
                    let bits = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
                    (((bits & 0x3fff) + 1) as usize, (((bits >> 14) & 0x3fff) + 1) as usize)
                }
                b"ANMF" if top => {
                    if payload.len() < 16 {
                        return bad("an animation frame has no valid header");
                    }
                    let (aw, ah) = (u24(&payload[6..9]) + 1, u24(&payload[9..12]) + 1);
                    if aw > mw || ah > mh {
                        return bad("an animation frame is larger than its canvas");
                    }
                    todo.push((&payload[16..], aw, ah, false));
                    (0, 0)
                }
                _ => (0, 0),
            };
            if fw > mw || fh > mh || fw.saturating_mul(fh) > MAX_WEBP_PIXELS {
                return bad("a frame is larger than its canvas");
            }
            rest = &rest[(8 + size + (size & 1)).min(rest.len())..];
        }
    }
    Ok(())
}

/// A WebP (`specs/118` §4 item 1), lossy, lossless or the first frame of an animation, decoded by `image-webp`. Its size is read
/// from the header and checked before the picture is decoded, so a header that claims a huge picture allocates nothing for it.
fn webp(b: &[u8]) -> Result<Image, DocError> {
    let bad = || DocError("the WebP file could not be decoded".into());
    let mut d = image_webp::WebPDecoder::new(std::io::Cursor::new(b)).map_err(|_| bad())?;
    let (w, h) = d.dimensions();
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || w.saturating_mul(h) > MAX_WEBP_PIXELS {
        return Err(DocError(format!("the picture would be {}x{} pixels; the limit is {} pixels in all", w, h, MAX_WEBP_PIXELS)));
    }
    webp_frames(b, w, h)?;
    d.set_memory_limit(MAX_WEBP_PIXELS * 4);
    let alpha = d.has_alpha();
    let mut buf = vec![0u8; d.output_buffer_size().ok_or_else(bad)?];
    d.read_image(&mut buf).map_err(|_| bad())?;
    let rgba = if alpha { buf } else { buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() };
    Ok(Image { width: w, height: h, rgba })
}

/// `img` shrunk to fit inside `max_w` x `max_h` pixels, keeping its proportions and never enlarging it. Each output
/// pixel averages the source pixels it covers (colour weighted by alpha, so a transparent edge does not bleed).
pub fn fit(img: &Image, max_w: usize, max_h: usize) -> Image {
    let (w, h) = (img.width, img.height);
    if w <= max_w && h <= max_h {
        return img.clone();
    }
    let scale = (max_w as f64 / w as f64).min(max_h as f64 / h as f64);
    let (nw, nh) = (((w as f64 * scale).round() as usize).max(1), ((h as f64 * scale).round() as usize).max(1));
    let mut rgba = Vec::with_capacity(nw * nh * 4);
    for y in 0..nh {
        let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1).min(h));
        for x in 0..nw {
            let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1).min(w));
            let (mut c, mut a, mut n) = ([0u64; 3], 0u64, 0u64);
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let p = img.pixel(xx, yy);
                    for k in 0..3 {
                        c[k] += p[k] as u64 * p[3] as u64;
                    }
                    a += p[3] as u64;
                    n += 1;
                }
            }
            match (c[0].checked_div(a), c[1].checked_div(a), c[2].checked_div(a)) {
                (Some(r), Some(g), Some(b)) => rgba.extend_from_slice(&[r as u8, g as u8, b as u8, (a / n) as u8]),
                _ => rgba.extend_from_slice(&[0, 0, 0, 0]), // nothing opaque under this pixel
            }
        }
    }
    Image { width: nw, height: nh, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/images/");
    fn load(name: &str) -> Image {
        decode(&std::fs::read(format!("{DIR}{name}")).unwrap()).unwrap_or_else(|e| panic!("{name}: {}", e.0))
    }

    /// The fixtures are all one 40x24 picture: red grows to the right, green downward, blue alternates in 8-pixel squares.
    fn expected(x: usize, y: usize) -> [i32; 3] {
        [(x * 255 / 39) as i32, (y * 255 / 23) as i32, if (x / 8 + y / 8) % 2 == 1 { 200 } else { 40 }]
    }

    /// The mean absolute error over all pixels and channels: lossless files are exact, JPEG is close.
    fn error(img: &Image, grey: bool) -> f64 {
        assert_eq!((img.width, img.height), (40, 24));
        let mut sum = 0i64;
        for y in 0..24 {
            for x in 0..40 {
                let (p, e) = (img.pixel(x, y), expected(x, y));
                let e = if grey { [(0.299 * e[0] as f64 + 0.587 * e[1] as f64 + 0.114 * e[2] as f64).round() as i32; 3] } else { e };
                sum += (0..3).map(|k| (p[k] as i32 - e[k]).abs() as i64).sum::<i64>();
                assert_eq!(p[3], 255);
            }
        }
        sum as f64 / (40.0 * 24.0 * 3.0)
    }

    #[test]
    fn png_in_every_colour_type_comes_back_as_the_same_picture() {
        assert_eq!(error(&load("grad.png"), false), 0.0);
        assert_eq!(error(&load("rgba.png"), false), 0.0);
        // a 4-bit palette file: compared with what Pillow decodes it to (`pal.ref.ppm`, a 13-byte header then RGB)
        let (pal, reference) = (load("pal.png"), std::fs::read(format!("{DIR}pal.ref.ppm")).unwrap());
        let rgb: Vec<u8> = pal.rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        assert_eq!(&reference[13..], &rgb[..], "palette pixels are exactly the palette's colours");
        assert!(error(&load("grey.png"), true) < 1.5);
    }

    #[test]
    fn jpeg_baseline_subsampled_progressive_and_grey_all_decode_close_to_the_picture() {
        for (name, grey, limit) in [("grad_baseline.jpg", false, 4.0), ("grad_420.jpg", false, 10.0), ("grad_progressive.jpg", false, 10.0), ("grey.jpg", true, 4.0)] {
            let e = error(&load(name), grey);
            assert!(e < limit, "{name}: mean error {e}");
        }
    }

    #[test]
    fn a_progressive_file_decodes_to_what_the_same_picture_does_sequentially() {
        let (a, b) = (load("grad_420.jpg"), load("grad_progressive.jpg"));
        let diff: i64 = a.rgba.iter().zip(&b.rgba).map(|(x, y)| (*x as i64 - *y as i64).abs()).sum();
        assert!(diff as f64 / a.rgba.len() as f64 <= 1.0, "mean difference {}", diff as f64 / a.rgba.len() as f64);
    }

    #[test]
    fn gif_decodes_to_its_palette_colours() {
        let img = load("grad.gif");
        assert!(error(&img, false) < 30.0, "a GIF is 256 colours: close, not exact");
    }

    #[test]
    fn an_interlaced_png_puts_every_pixel_where_it_belongs() {
        // 5x5 image, Adam7, 8-bit grey, value = 10 * y + x; no filtering
        let (w, h) = (5usize, 5usize);
        let passes = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];
        let mut raw = Vec::new();
        for (x0, y0, dx, dy) in passes {
            for y in (y0..h).step_by(dy) {
                if x0 >= w {
                    continue;
                }
                raw.push(0u8);
                raw.extend((x0..w).step_by(dx).map(|x| (10 * y + x) as u8));
            }
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut z, &raw).unwrap();
        let idat = z.finish().unwrap();
        let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut c = crc32fast::Hasher::new();
            c.update(kind);
            c.update(data);
            out.extend_from_slice(&c.finalize().to_be_bytes());
        };
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut png, b"IHDR", &[0, 0, 0, 5, 0, 0, 0, 5, 8, 0, 0, 0, 1]);
        chunk(&mut png, b"IDAT", &idat);
        chunk(&mut png, b"IEND", &[]);
        let img = decode(&png).unwrap();
        for y in 0..h {
            for x in 0..w {
                assert_eq!(img.pixel(x, y)[0], (10 * y + x) as u8, "({x},{y})");
            }
        }
    }

    #[test]
    fn fit_keeps_proportions_never_enlarges_and_averages() {
        let img = load("grad.png");
        let small = fit(&img, 20, 20);
        assert_eq!((small.width, small.height), (20, 12));
        assert_eq!(fit(&img, 100, 100), img, "a picture that already fits is left alone");
        let half = fit(&Image { width: 2, height: 1, rgba: vec![0, 0, 0, 255, 200, 100, 50, 255] }, 1, 1);
        assert_eq!(half.pixel(0, 0), [100, 50, 25, 255]);
        let clear = fit(&Image { width: 2, height: 1, rgba: vec![255, 0, 0, 0, 0, 0, 255, 255] }, 1, 1);
        assert_eq!(&clear.pixel(0, 0)[..3], &[0, 0, 255], "a transparent pixel adds no colour");
    }

    /// A lossless WebP of `img`, written by `image-webp`'s own encoder.
    fn webp_of(img: &Image) -> Vec<u8> {
        let mut out = Vec::new();
        image_webp::WebPEncoder::new(&mut out).encode(&img.rgba, img.width as u32, img.height as u32, image_webp::ColorType::Rgba8).unwrap();
        out
    }

    #[test]
    fn a_lossless_webp_comes_back_as_the_same_picture_and_a_broken_or_huge_one_is_refused() {
        let img = load("rgba.png");
        let w = webp_of(&img);
        assert!(w.starts_with(b"RIFF") && &w[8..12] == b"WEBP");
        assert_eq!(decode(&w).unwrap(), img, "lossless is exact");
        for cut in [12usize, 20, w.len() / 2] {
            assert!(decode(&w[..cut]).is_err(), "cut at {}", cut);
        }
        let mut flipped = w.clone();
        for i in (20..flipped.len()).step_by(5) {
            flipped[i] ^= 0x5A;
        }
        let _ = decode(&flipped); // must not panic
        // A simple-lossless header (VP8L) that claims 16384x16384 pixels: refused before the picture is allocated.
        let mut huge = b"RIFF\0\0\0\0WEBPVP8L\x05\0\0\0\x2f".to_vec();
        huge.extend_from_slice(&(16383u32 | (16383 << 14)).to_le_bytes());
        huge.push(0);
        let n = (huge.len() - 8) as u32;
        huge[4..8].copy_from_slice(&n.to_le_bytes());
        let e = decode(&huge).unwrap_err();
        assert!(e.0.contains("the limit is"), "{}", e.0);
    }

    /// A WebP of `chunks` (fourcc, payload), padded and sized as the format says.
    fn riff(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        for (kind, payload) in chunks {
            body.extend_from_slice(*kind);
            body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            body.extend_from_slice(payload);
            if payload.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    /// The red-team's P2-1: an extended WebP whose canvas is 64x64 holds a lossy frame whose own header says 16000x16000.
    /// `image-webp` would decode the frame at its own size (about a gigabyte) before comparing it with the canvas.
    #[test]
    fn a_frame_larger_than_its_canvas_is_refused_before_it_is_decoded() {
        let vp8x = {
            let mut p = vec![0u8; 10];
            p[4..7].copy_from_slice(&[63, 0, 0]);
            p[7..10].copy_from_slice(&[63, 0, 0]);
            p
        };
        let huge_vp8 = {
            let mut p = vec![0x10, 0x02, 0x00, 0x9d, 0x01, 0x2a];
            p.extend_from_slice(&16000u16.to_le_bytes());
            p.extend_from_slice(&16000u16.to_le_bytes());
            p.extend_from_slice(&[0u8; 32]);
            p
        };
        let e = decode(&riff(&[(b"VP8X", vp8x.clone()), (b"VP8 ", huge_vp8.clone())])).unwrap_err();
        assert_eq!(e.0, "the WebP file is refused: a frame is larger than its canvas");
        // The same inside an animation frame, and an animation frame larger than the canvas.
        let mut anmf = vec![0u8; 16];
        anmf[6..9].copy_from_slice(&[63, 0, 0]);
        anmf[9..12].copy_from_slice(&[63, 0, 0]);
        let mut inner = b"VP8 ".to_vec();
        inner.extend_from_slice(&(huge_vp8.len() as u32).to_le_bytes());
        inner.extend_from_slice(&huge_vp8);
        let mut framed = anmf.clone();
        framed.extend_from_slice(&inner);
        let mut animated = vp8x.clone();
        animated[0] = 0x02;
        assert!(webp_frames(&riff(&[(b"VP8X", animated.clone()), (b"ANMF", framed)]), 64, 64).unwrap_err().0.contains("larger than its canvas"));
        let mut wide = anmf.clone();
        wide[6..9].copy_from_slice(&[0xff, 0x3f, 0]);
        assert!(webp_frames(&riff(&[(b"VP8X", animated), (b"ANMF", wide)]), 64, 64).unwrap_err().0.contains("larger than its canvas"));
        // An honest file passes the check and decodes.
        let img = load("rgba.png");
        let ok = webp_of(&img);
        assert!(webp_frames(&ok, img.width, img.height).is_ok());
        assert_eq!(decode(&ok).unwrap(), img);
    }

    #[test]
    fn hostile_and_broken_files_are_refused_not_trusted() {
        assert!(decode(b"not an image").is_err());
        assert!(decode(b"").is_err());
        for name in ["grad_420.jpg", "grad_progressive.jpg", "grad.gif", "pal.png"] {
            let b = std::fs::read(format!("{DIR}{name}")).unwrap();
            for cut in [3usize, 20, b.len() / 2, b.len() - 1] {
                let _ = decode(&b[..cut]); // must not panic; an error or a partial picture are both fine
            }
            let mut flipped = b.clone();
            for i in (30..flipped.len()).step_by(7) {
                flipped[i] ^= 0x5A;
            }
            let _ = decode(&flipped);
        }
        // a header that claims a huge picture is refused before anything is allocated for it
        let mut huge = std::fs::read(format!("{DIR}grad_baseline.jpg")).unwrap();
        let sof = huge.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        huge[sof + 5..sof + 9].copy_from_slice(&[0x40, 0x00, 0x40, 0x00]);
        assert!(decode(&huge).is_err());
    }
}
