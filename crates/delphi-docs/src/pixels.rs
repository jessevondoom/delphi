//! Pixels for the reader (`specs/029` §9): any image the knowledge base may hold (PNG, JPEG, GIF), decoded and
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
    } else {
        Err(DocError("not an image this reader draws (PNG, JPEG and GIF are)".into()))
    }
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
