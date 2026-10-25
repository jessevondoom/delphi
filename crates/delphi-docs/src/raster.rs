//! A scanline rasterizer for the small SVG subset Delphi's own writers produce (`specs/014` §8.4): rectangles
//! (rounded or not), lines, circles, paths of `M L Q A Z`, translated groups, and text. It exists so a chart or
//! diagram has one scene (the SVG) and PNG is that scene drawn again, not a second drawing routine to keep in step.
//!
//! - Fills are anti-aliased: four sub-scanlines per pixel with exact horizontal coverage, non-zero or even-odd.
//! - A stroke is the union of a quad per segment and a disc per vertex, taken as one coverage mask so adjacent
//!   pieces leave no seams.
//! - Text is the built-in 5x7 bitmap font at a whole-number scale; it is plain on purpose.
//! - Colours come from the class names the writers use, resolved against the palette for the chosen mode, so a
//!   PNG needs no CSS engine. An element it does not recognise draws nothing.

use crate::chart::palette::{heat, GRID_DARK, GRID_LIGHT, AXIS_DARK, AXIS_LIGHT, INK_DARK, INK_LIGHT, SERIES_DARK, SERIES_LIGHT, SURFACE_DARK, SURFACE_LIGHT};
use crate::font;
use crate::png::{self, Image};
use crate::xml::{attr, walk, Ev};
use crate::DocError;

const SS: usize = 4;
/// Most pixels a picture may have: a 2x chart is 1.2 million.
const MAX_PIXELS: usize = 24_000_000;

type P = (f64, f64);
type Rgb = [u8; 3];

fn rgb(hex: &str) -> Rgb {
    let (r, g, b) = crate::chart::palette::rgb(hex);
    [r, g, b]
}

struct Mask {
    x0: i64,
    y0: i64,
    w: usize,
    h: usize,
    a: Vec<f32>,
}

impl Mask {
    fn new(x0: i64, y0: i64, w: usize, h: usize) -> Mask {
        Mask { x0, y0, w, h, a: vec![0.0; w * h] }
    }
}

fn edges(contours: &[Vec<P>]) -> Vec<(P, P)> {
    let mut e = vec![];
    for c in contours {
        for i in 0..c.len() {
            let (p, q) = (c[i], c[(i + 1) % c.len()]);
            if p.1 != q.1 {
                e.push((p, q));
            }
        }
    }
    e
}

/// Add the coverage of the polygons to the mask: summed for a fill, or by maximum when pieces of one stroke overlap.
fn rasterize(mask: &mut Mask, contours: &[Vec<P>], evenodd: bool, max_mode: bool) {
    let es = edges(contours);
    if es.is_empty() {
        return;
    }
    let (mut ymin, mut ymax, mut xmin, mut xmax) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for c in contours {
        for p in c {
            ymin = ymin.min(p.1);
            ymax = ymax.max(p.1);
            xmin = xmin.min(p.0);
            xmax = xmax.max(p.0);
        }
    }
    if !(ymin.is_finite() && ymax.is_finite() && xmin.is_finite() && xmax.is_finite()) {
        return;
    }
    let ya = (ymin.floor() as i64).max(mask.y0);
    let yb = (ymax.ceil() as i64).min(mask.y0 + mask.h as i64);
    let xa = (xmin.floor() as i64).max(mask.x0);
    let xb = (xmax.ceil() as i64).min(mask.x0 + mask.w as i64);
    if ya >= yb || xa >= xb {
        return;
    }
    let bw = (xb - xa) as usize;
    let mut row = vec![0f32; bw];
    let mut xs: Vec<(f64, i32)> = Vec::with_capacity(16);
    let wgt = 1.0 / SS as f32;
    for y in ya..yb {
        row.iter_mut().for_each(|v| *v = 0.0);
        for k in 0..SS {
            let sy = y as f64 + (k as f64 + 0.5) / SS as f64;
            xs.clear();
            for (p, q) in &es {
                if (p.1 <= sy && sy < q.1) || (q.1 <= sy && sy < p.1) {
                    xs.push((p.0 + (sy - p.1) / (q.1 - p.1) * (q.0 - p.0), if q.1 > p.1 { 1 } else { -1 }));
                }
            }
            xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let (mut wind, mut inside, mut prev) = (0i32, false, 0.0f64);
            for (x, d) in &xs {
                if inside {
                    span(&mut row, prev - xa as f64, *x - xa as f64, wgt);
                }
                wind += d;
                inside = if evenodd { !inside } else { wind != 0 };
                prev = *x;
            }
        }
        let base = (y - mask.y0) as usize * mask.w;
        for (i, v) in row.iter().enumerate() {
            if *v > 0.0 {
                let at = base + (xa - mask.x0) as usize + i;
                mask.a[at] = if max_mode { mask.a[at].max(v.min(1.0)) } else { (mask.a[at] + v).min(1.0) };
            }
        }
    }
}

fn span(row: &mut [f32], a: f64, b: f64, w: f32) {
    let (a, b) = (a.max(0.0), b.min(row.len() as f64));
    if b <= a {
        return;
    }
    let (ia, ib) = (a.floor() as usize, b.floor() as usize);
    if ia == ib {
        row[ia] += (b - a) as f32 * w;
        return;
    }
    row[ia] += (ia as f64 + 1.0 - a) as f32 * w;
    for v in row.iter_mut().take(ib).skip(ia + 1) {
        *v += w;
    }
    if ib < row.len() {
        row[ib] += (b - ib as f64) as f32 * w;
    }
}

/// Where and how one piece of text is drawn: its anchor point (the baseline, in pixels), glyph scale and colour.
#[derive(Clone, Copy)]
struct TextAt<'a> {
    x: f64,
    baseline: f64,
    scale: usize,
    anchor: &'a str,
    color: Rgb,
    bold: bool,
}

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Canvas {
    fn blend(&mut self, mask: &Mask, c: Rgb, opacity: f32) {
        for my in 0..mask.h {
            let y = mask.y0 + my as i64;
            if y < 0 || y >= self.h as i64 {
                continue;
            }
            for mx in 0..mask.w {
                let a = mask.a[my * mask.w + mx] * opacity;
                if a <= 0.0 {
                    continue;
                }
                let x = mask.x0 + mx as i64;
                if x < 0 || x >= self.w as i64 {
                    continue;
                }
                let i = (y as usize * self.w + x as usize) * 4;
                for (k, ch) in c.iter().enumerate() {
                    self.px[i + k] = (self.px[i + k] as f32 + (*ch as f32 - self.px[i + k] as f32) * a).round() as u8;
                }
            }
        }
    }

    fn mask_for(&self, contours: &[Vec<P>], pad: f64) -> Option<Mask> {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for c in contours {
            for p in c {
                x0 = x0.min(p.0);
                y0 = y0.min(p.1);
                x1 = x1.max(p.0);
                y1 = y1.max(p.1);
            }
        }
        if !(x0.is_finite() && y1.is_finite()) {
            return None;
        }
        let (bx0, by0) = (((x0 - pad).floor() as i64).max(0), ((y0 - pad).floor() as i64).max(0));
        let (bx1, by1) = (((x1 + pad).ceil() as i64).min(self.w as i64), ((y1 + pad).ceil() as i64).min(self.h as i64));
        if bx1 <= bx0 || by1 <= by0 {
            return None;
        }
        Some(Mask::new(bx0, by0, (bx1 - bx0) as usize, (by1 - by0) as usize))
    }

    fn fill(&mut self, contours: &[Vec<P>], evenodd: bool, c: Rgb, opacity: f32) {
        if let Some(mut m) = self.mask_for(contours, 1.0) {
            rasterize(&mut m, contours, evenodd, false);
            self.blend(&m, c, opacity);
        }
    }

    /// Stroke a polyline (closed or not) with round joins and caps.
    fn stroke(&mut self, pts: &[P], closed: bool, width: f64, c: Rgb) {
        if pts.is_empty() || width <= 0.0 {
            return;
        }
        let hw = width / 2.0;
        let mut pieces: Vec<Vec<P>> = vec![];
        let n = pts.len();
        let segs = if closed { n } else { n.saturating_sub(1) };
        for i in 0..segs {
            let (p, q) = (pts[i], pts[(i + 1) % n]);
            let (dx, dy) = (q.0 - p.0, q.1 - p.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-9 {
                continue;
            }
            let (nx, ny) = (-dy / len * hw, dx / len * hw);
            pieces.push(vec![(p.0 + nx, p.1 + ny), (q.0 + nx, q.1 + ny), (q.0 - nx, q.1 - ny), (p.0 - nx, p.1 - ny)]);
        }
        let sides = if hw < 2.0 { 8 } else { 16 };
        for p in pts {
            pieces.push((0..sides).map(|k| {
                let t = k as f64 / sides as f64 * std::f64::consts::TAU;
                (p.0 + hw * t.cos(), p.1 + hw * t.sin())
            }).collect());
        }
        let all: Vec<Vec<P>> = pieces.iter().flatten().map(|p| vec![*p]).collect();
        if let Some(mut m) = self.mask_for(&all, 1.0) {
            for piece in &pieces {
                rasterize(&mut m, std::slice::from_ref(piece), false, true);
            }
            self.blend(&m, c, 1.0);
        }
    }

    fn text(&mut self, s: &str, at: &TextAt) {
        let TextAt { x, baseline, scale, anchor, color: c, bold } = *at;
        let glyphs = font::glyphs(s);
        let width = (glyphs.len() * font::ADVANCE).saturating_sub(1) * scale;
        let left = match anchor {
            "middle" => x - width as f64 / 2.0,
            "end" => x - width as f64,
            _ => x,
        }
        .round() as i64;
        let top = baseline.round() as i64 - (font::CAP_ROWS * scale) as i64;
        let passes = if bold { 2 } else { 1 };
        for pass in 0..passes {
            for (gi, cols) in glyphs.iter().enumerate() {
                for (ci, col) in cols.iter().enumerate() {
                    for row in 0..8 {
                        if col >> row & 1 == 0 {
                            continue;
                        }
                        let (px, py) = (left + ((gi * font::ADVANCE + ci) * scale) as i64 + pass, top + (row * scale) as i64);
                        for dy in 0..scale as i64 {
                            for dx in 0..scale as i64 {
                                let (x, y) = (px + dx, py + dy);
                                if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
                                    let i = (y as usize * self.w + x as usize) * 4;
                                    self.px[i..i + 3].copy_from_slice(&c);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---- paths

fn numbers(s: &str) -> Vec<f64> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut prev = ' ';
    for ch in s.chars().chain(std::iter::once(' ')) {
        let splits = ch.is_whitespace() || ch == ',' || (ch == '-' && !cur.is_empty() && prev != 'e');
        if splits && !cur.is_empty() {
            if let Ok(v) = cur.parse::<f64>() {
                out.push(v);
            }
            cur.clear();
        }
        if !(ch.is_whitespace() || ch == ',') {
            cur.push(ch);
        }
        prev = ch;
    }
    out
}

fn arc_points(from: P, rx: f64, ry: f64, large: bool, sweep: bool, to: P, out: &mut Vec<P>) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-9 || ry < 1e-9 || (from.0 == to.0 && from.1 == to.1) {
        out.push(to);
        return;
    }
    let (dx, dy) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
    let lambda = dx * dx / (rx * rx) + dy * dy / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * dy * dy - ry * ry * dx * dx;
    let den = rx * rx * dy * dy + ry * ry * dx * dx;
    let coef = (num / den).max(0.0).sqrt() * if large == sweep { -1.0 } else { 1.0 };
    let (cxp, cyp) = (coef * rx * dy / ry, -coef * ry * dx / rx);
    let (cx, cy) = (cxp + (from.0 + to.0) / 2.0, cyp + (from.1 + to.1) / 2.0);
    let t1 = ((dy - cyp) / ry).atan2((dx - cxp) / rx);
    let t2 = ((-dy - cyp) / ry).atan2((-dx - cxp) / rx);
    let mut dt = t2 - t1;
    if sweep && dt < 0.0 {
        dt += std::f64::consts::TAU;
    } else if !sweep && dt > 0.0 {
        dt -= std::f64::consts::TAU;
    }
    let steps = ((dt.abs() / (std::f64::consts::PI / 90.0)).ceil() as usize).clamp(2, 240);
    for i in 1..=steps {
        let t = t1 + dt * i as f64 / steps as f64;
        out.push((cx + rx * t.cos(), cy + ry * t.sin()));
    }
}

/// The subpaths of an SVG path: (points in user space, closed).
fn flatten(d: &str) -> Vec<(Vec<P>, bool)> {
    let mut out: Vec<(Vec<P>, bool)> = vec![];
    let mut cur: Vec<P> = vec![];
    let mut at = (0.0, 0.0);
    let mut start = (0.0, 0.0);
    let bytes: Vec<char> = d.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if !c.is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < bytes.len() && !(bytes[j].is_ascii_alphabetic() && bytes[j] != 'e') {
            j += 1;
        }
        let args = numbers(&bytes[i + 1..j].iter().collect::<String>());
        match c {
            'M' if args.len() >= 2 => {
                if cur.len() > 1 {
                    out.push((std::mem::take(&mut cur), false));
                }
                cur.clear();
                at = (args[0], args[1]);
                start = at;
                cur.push(at);
                for p in args[2..].chunks_exact(2) {
                    at = (p[0], p[1]);
                    cur.push(at);
                }
            }
            'L' => {
                for p in args.chunks_exact(2) {
                    at = (p[0], p[1]);
                    cur.push(at);
                }
            }
            'Q' => {
                for q in args.chunks_exact(4) {
                    let (c1, e) = ((q[0], q[1]), (q[2], q[3]));
                    for s in 1..=10 {
                        let t = s as f64 / 10.0;
                        let u = 1.0 - t;
                        cur.push((u * u * at.0 + 2.0 * u * t * c1.0 + t * t * e.0, u * u * at.1 + 2.0 * u * t * c1.1 + t * t * e.1));
                    }
                    at = e;
                }
            }
            'A' => {
                for a in args.chunks_exact(7) {
                    let to = (a[5], a[6]);
                    arc_points(at, a[0], a[1], a[3] != 0.0, a[4] != 0.0, to, &mut cur);
                    at = to;
                }
            }
            'Z' | 'z' => {
                if cur.len() > 1 {
                    out.push((std::mem::take(&mut cur), true));
                }
                cur.clear();
                at = start;
                cur.push(at);
            }
            _ => {}
        }
        i = j;
    }
    if cur.len() > 1 {
        out.push((cur, false));
    }
    out
}

fn rect_contour(x: f64, y: f64, w: f64, h: f64, r: f64) -> Vec<P> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r < 0.01 {
        return vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    }
    let mut out = vec![];
    for (cx, cy, a0) in [(x + w - r, y + r, -90.0f64), (x + w - r, y + h - r, 0.0), (x + r, y + h - r, 90.0), (x + r, y + r, 180.0)] {
        for s in 0..=6 {
            let t = (a0 + 90.0 * s as f64 / 6.0).to_radians();
            out.push((cx + r * t.cos(), cy + r * t.sin()));
        }
    }
    out
}

// ---- style

struct Theme {
    surface: Rgb,
    ink: [Rgb; 3],
    grid: Rgb,
    axis: Rgb,
    series: [Rgb; 4],
    heat: [(Rgb, Rgb); 5],
}

impl Theme {
    fn new(dark: bool) -> Theme {
        let (surface, ink, grid, axis, series) = if dark { (SURFACE_DARK, INK_DARK, GRID_DARK, AXIS_DARK, SERIES_DARK) } else { (SURFACE_LIGHT, INK_LIGHT, GRID_LIGHT, AXIS_LIGHT, SERIES_LIGHT) };
        let h = heat(!dark);
        Theme {
            surface: rgb(surface),
            ink: [rgb(ink[0]), rgb(ink[1]), rgb(ink[2])],
            grid: rgb(grid),
            axis: rgb(axis),
            series: [rgb(series[0]), rgb(series[1]), rgb(series[2]), rgb(series[3])],
            heat: [(rgb(&h[0].0), rgb(h[0].1)), (rgb(&h[1].0), rgb(h[1].1)), (rgb(&h[2].0), rgb(h[2].1)), (rgb(&h[3].0), rgb(h[3].1)), (rgb(&h[4].0), rgb(h[4].1))],
        }
    }
}

#[derive(Clone, Default)]
struct Style {
    fill: Option<Rgb>,
    fill_opacity: Option<f32>,
    stroke: Option<Rgb>,
    stroke_w: f64,
    text: Option<Rgb>,
    bold: bool,
    hidden: bool,
}

fn style_of(classes: &str, th: &Theme) -> Style {
    let mut s = Style::default();
    for c in classes.split_whitespace() {
        let digit = c.chars().last().and_then(|d| d.to_digit(10)).map(|d| d as usize);
        match (c.trim_end_matches(|ch: char| ch.is_ascii_digit()), digit) {
            ("surface", _) => s.fill = Some(th.surface),
            ("g", _) => {
                s.stroke = Some(th.grid);
                s.stroke_w = 1.0;
            }
            ("ax", _) => {
                s.stroke = Some(th.axis);
                s.stroke_w = 1.0;
            }
            ("ref", _) => {
                s.stroke = Some(th.ink[2]);
                s.stroke_w = 1.0;
            }
            ("t", Some(n @ 1..=3)) => s.text = Some(th.ink[n - 1]),
            ("b", _) => s.bold = true,
            ("f", Some(n @ 1..=4)) => s.fill = Some(th.series[n - 1]),
            ("k", Some(n @ 1..=4)) => {
                s.stroke = Some(th.series[n - 1]);
                s.stroke_w = 2.0;
                s.fill = None;
            }
            ("a", Some(n @ 1..=4)) => {
                s.fill = Some(th.series[n - 1]);
                s.fill_opacity = Some(0.1);
            }
            ("d", Some(n @ 1..=4)) => {
                s.fill = Some(th.series[n - 1]);
                s.stroke = Some(th.surface);
                s.stroke_w = 2.0;
            }
            ("sl", _) => {
                s.stroke = Some(th.surface);
                s.stroke_w = 2.0;
            }
            ("bx", _) => {
                s.fill = Some(th.series[0]);
                s.fill_opacity = Some(0.18);
                s.stroke = Some(th.series[0]);
                s.stroke_w = 2.0;
            }
            ("wh", _) => {
                s.stroke = Some(th.series[0]);
                s.stroke_w = 1.5;
                s.fill = None;
            }
            ("h", Some(n @ 0..=4)) => s.fill = Some(th.heat[n].0),
            ("ht", Some(n @ 0..=4)) => s.text = Some(th.heat[n].1),
            ("hit", _) | ("brush", _) => s.hidden = true,
            _ => {}
        }
    }
    s
}

fn glyph_scale(font_size: f64) -> usize {
    match font_size {
        s if s < 14.0 => 2,
        s if s < 20.0 => 3,
        _ => 4,
    }
}

fn f64_attr(attrs: &[(String, String)], k: &str) -> f64 {
    attr(attrs, k).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0)
}

/// Draw an SVG of the supported subset. `dark` picks the palette unless the root says `data-theme`.
pub fn svg_to_image(svg: &str, dark: bool, scale: u32) -> Result<Image, DocError> {
    let scale = scale.clamp(1, 4) as f64;
    let mut canvas: Option<Canvas> = None;
    let mut th = Theme::new(dark);
    let mut groups: Vec<P> = vec![(0.0, 0.0)];
    let mut offset = (0.0, 0.0);
    let (mut in_text, mut text_buf, mut text_at) = (false, String::new(), (0.0, 0.0, 2usize, "start".to_string(), Style::default()));
    let mut error: Option<DocError> = None;
    let r = walk(svg.as_bytes(), 200_000, |ev| {
        if error.is_some() {
            return;
        }
        match ev {
            Ev::Start { name, attrs } => {
                let hidden = attr(&attrs, "hidden").is_some();
                if name == "svg" && canvas.is_none() {
                    let (vw, vh) = match attr(&attrs, "viewBox").map(|v| v.split_whitespace().filter_map(|n| n.parse::<f64>().ok()).collect::<Vec<_>>()) {
                        Some(v) if v.len() == 4 => (v[2], v[3]),
                        _ => (f64_attr(&attrs, "width"), f64_attr(&attrs, "height")),
                    };
                    let (w, h) = ((vw * scale).ceil() as usize, (vh * scale).ceil() as usize);
                    if w == 0 || h == 0 || w * h > MAX_PIXELS {
                        error = Some(DocError(format!("the picture would be {}x{} pixels; the limit is {} pixels in all", w, h, MAX_PIXELS)));
                        return;
                    }
                    match attr(&attrs, "data-theme") {
                        Some("dark") => th = Theme::new(true),
                        Some("light") => th = Theme::new(false),
                        _ => {}
                    }
                    canvas = Some(Canvas { w, h, px: [th.surface[0], th.surface[1], th.surface[2], 255].repeat(w * h) });
                    return;
                }
                let Some(cv) = canvas.as_mut() else { return };
                if name == "g" {
                    let t = attr(&attrs, "transform").and_then(|t| t.strip_prefix("translate(")).map(|t| t.trim_end_matches(')')).map(numbers).filter(|n| !n.is_empty());
                    let d = t.map(|n| (n[0] * scale, n.get(1).copied().unwrap_or(0.0) * scale)).unwrap_or((0.0, 0.0));
                    groups.push(d);
                    offset = (offset.0 + d.0, offset.1 + d.1);
                    return;
                }
                let st = style_of(attr(&attrs, "class").unwrap_or(""), &th);
                if hidden || st.hidden {
                    return;
                }
                let tf = |p: P| (p.0 * scale + offset.0, p.1 * scale + offset.1);
                let paint = |cv: &mut Canvas, contours: Vec<Vec<P>>, closed: bool, evenodd: bool| {
                    if let Some(c) = st.fill {
                        cv.fill(&contours, evenodd, c, st.fill_opacity.unwrap_or(1.0));
                    }
                    if let (Some(c), true) = (st.stroke, st.stroke_w > 0.0) {
                        for pts in &contours {
                            cv.stroke(pts, closed, st.stroke_w * scale, c);
                        }
                    }
                };
                match name.as_str() {
                    "rect" => {
                        let (x, y, w, h, r) = (f64_attr(&attrs, "x"), f64_attr(&attrs, "y"), f64_attr(&attrs, "width"), f64_attr(&attrs, "height"), f64_attr(&attrs, "rx"));
                        if w > 0.0 && h > 0.0 {
                            let c: Vec<P> = rect_contour(x, y, w, h, r).into_iter().map(tf).collect();
                            paint(cv, vec![c], true, false);
                        }
                    }
                    "line" => {
                        let (a, b) = (tf((f64_attr(&attrs, "x1"), f64_attr(&attrs, "y1"))), tf((f64_attr(&attrs, "x2"), f64_attr(&attrs, "y2"))));
                        if let (Some(c), true) = (st.stroke, st.stroke_w > 0.0) {
                            cv.stroke(&[a, b], false, st.stroke_w * scale, c);
                        }
                    }
                    "circle" => {
                        let (cx, cy, r) = (f64_attr(&attrs, "cx"), f64_attr(&attrs, "cy"), f64_attr(&attrs, "r"));
                        let n = ((r * scale * 2.0) as usize).clamp(12, 120);
                        let c: Vec<P> = (0..n).map(|k| {
                            let t = k as f64 / n as f64 * std::f64::consts::TAU;
                            tf((cx + r * t.cos(), cy + r * t.sin()))
                        }).collect();
                        paint(cv, vec![c], true, false);
                    }
                    "path" => {
                        let subs = flatten(attr(&attrs, "d").unwrap_or(""));
                        let evenodd = attr(&attrs, "fill-rule") == Some("evenodd");
                        let contours: Vec<Vec<P>> = subs.iter().map(|s| s.0.iter().map(|p| tf(*p)).collect()).collect();
                        if let Some(c) = st.fill {
                            // an open subpath is filled as if closed, as SVG does
                            cv.fill(&contours, evenodd, c, st.fill_opacity.unwrap_or(1.0));
                        }
                        if let (Some(c), true) = (st.stroke, st.stroke_w > 0.0) {
                            for (pts, s) in contours.iter().zip(&subs) {
                                cv.stroke(pts, s.1, st.stroke_w * scale, c);
                            }
                        }
                    }
                    "text" => {
                        in_text = true;
                        text_buf.clear();
                        let size = attr(&attrs, "font-size").and_then(|v| v.parse::<f64>().ok()).unwrap_or(11.0);
                        let (x, y) = tf((f64_attr(&attrs, "x"), f64_attr(&attrs, "y")));
                        text_at = (x, y, glyph_scale(size), attr(&attrs, "text-anchor").unwrap_or("start").to_string(), st);
                    }
                    _ => {}
                }
            }
            Ev::Text(t) => {
                if in_text {
                    text_buf.push_str(&t);
                }
            }
            Ev::End { name } => {
                if name == "g" && groups.len() > 1 {
                    if let Some(d) = groups.pop() {
                        offset = (offset.0 - d.0, offset.1 - d.1);
                    }
                } else if name == "text" && in_text {
                    in_text = false;
                    if let Some(cv) = canvas.as_mut() {
                        let (x, y, gs, anchor, st) = &text_at;
                        cv.text(text_buf.trim(), &TextAt { x: *x, baseline: *y, scale: *gs, anchor, color: st.text.unwrap_or(th.ink[0]), bold: st.bold });
                    }
                }
            }
        }
    });
    if let Some(e) = error {
        return Err(e);
    }
    r?;
    let cv = canvas.ok_or_else(|| DocError("there is no <svg> element to draw".into()))?;
    Ok(Image { width: cv.w, height: cv.h, rgba: cv.px })
}

/// Draw an SVG and encode it as a PNG, with `text` as its tEXt chunks.
pub fn svg_to_png(svg: &str, dark: bool, scale: u32, text: &[(&str, &str)]) -> Result<Vec<u8>, DocError> {
    Ok(png::encode(&svg_to_image(svg, dark, scale)?, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(img: &Image, x: usize, y: usize) -> [u8; 3] {
        let p = img.pixel(x, y);
        [p[0], p[1], p[2]]
    }

    fn doc(body: &str) -> String {
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 40 30\" width=\"40\" height=\"30\"><rect class=\"surface\" width=\"40\" height=\"30\"/>{}</svg>", body)
    }

    #[test]
    fn the_canvas_is_the_viewbox_at_scale_and_starts_as_the_surface() {
        let img = svg_to_image(&doc(""), false, 2).unwrap();
        assert_eq!((img.width, img.height), (80, 60));
        assert_eq!(px(&img, 3, 3), rgb(SURFACE_LIGHT));
        let dark = svg_to_image(&doc(""), true, 1).unwrap();
        assert_eq!(px(&dark, 3, 3), rgb(SURFACE_DARK));
    }

    #[test]
    fn a_rectangle_fills_exactly_and_its_edge_is_anti_aliased() {
        let img = svg_to_image(&doc("<rect class=\"f1\" x=\"10\" y=\"10\" width=\"10.5\" height=\"10\"/>"), false, 1).unwrap();
        let blue = rgb(SERIES_LIGHT[0]);
        assert_eq!(px(&img, 12, 15), blue);
        assert_eq!(px(&img, 25, 15), rgb(SURFACE_LIGHT), "outside is untouched");
        let edge = px(&img, 20, 15);
        assert!(edge != blue && edge != rgb(SURFACE_LIGHT), "half a pixel of coverage blends: {:?}", edge);
        // about half way between surface and blue
        let mid = (blue[0] as i32 + rgb(SURFACE_LIGHT)[0] as i32) / 2;
        assert!((edge[0] as i32 - mid).abs() <= 6, "{:?} vs {}", edge, mid);
    }

    #[test]
    fn a_circle_covers_its_centre_and_a_donut_arc_path_leaves_a_hole() {
        let c = svg_to_image(&doc("<circle class=\"f2\" cx=\"20\" cy=\"15\" r=\"8\"/>"), false, 2).unwrap();
        assert_eq!(px(&c, 40, 30), rgb(SERIES_LIGHT[1]));
        assert_eq!(px(&c, 2, 2), rgb(SURFACE_LIGHT));
        // a ring: outer and inner arcs in opposite directions, even-odd
        let ring = "<path class=\"f1\" fill-rule=\"evenodd\" d=\"M12,15 A8,8 0 1 1 28,15 A8,8 0 1 1 12,15 M16,15 A4,4 0 1 0 24,15 A4,4 0 1 0 16,15 Z\"/>";
        let r = svg_to_image(&doc(ring), false, 2).unwrap();
        assert_eq!(px(&r, 40, 30), rgb(SURFACE_LIGHT), "the middle is a hole");
        assert_eq!(px(&r, 40, 18), rgb(SERIES_LIGHT[0]), "the band is filled");
    }

    #[test]
    fn rounded_data_ends_curve_away_from_the_corner() {
        // a column as the chart writes it: square at the base (y 25), rounded at the top (y 5)
        let d = "M10,25 L10,9 Q10,5 14,5 L16,5 Q20,5 20,9 L20,25 Z";
        let img = svg_to_image(&doc(&format!("<path class=\"f1\" d=\"{}\"/>", d)), false, 4).unwrap();
        let surface = rgb(SURFACE_LIGHT);
        assert_eq!(px(&img, 41, 21), surface, "the top-left corner is cut away");
        assert_eq!(px(&img, 41, 98), rgb(SERIES_LIGHT[0]), "the base corner is square");
        assert_eq!(px(&img, 60, 40), rgb(SERIES_LIGHT[0]));
    }

    #[test]
    fn strokes_join_without_seams_and_a_two_pixel_line_is_two_pixels() {
        let img = svg_to_image(&doc("<path class=\"k1\" d=\"M5,15 L20,15 L35,15\"/>"), false, 1).unwrap();
        let blue = rgb(SERIES_LIGHT[0]);
        for x in 6..34 {
            assert_eq!(px(&img, x, 14), blue, "x={}", x);
            assert_eq!(px(&img, x, 15), blue);
        }
        assert_eq!(px(&img, 20, 14), blue, "no seam at the join");
        assert_eq!(px(&img, 20, 12), rgb(SURFACE_LIGHT));
    }

    #[test]
    fn text_is_drawn_from_the_bitmap_font_with_anchor_and_colour() {
        let svg = doc("<text x=\"4\" y=\"14\" class=\"t1\" font-size=\"11\" text-anchor=\"start\">Hi</text><text x=\"36\" y=\"26\" class=\"t3\" font-size=\"11\" text-anchor=\"end\">1.5</text>");
        let img = svg_to_image(&svg, false, 2).unwrap();
        let ink = rgb(INK_LIGHT[0]);
        let drawn = (0..img.width).flat_map(|x| (0..img.height).map(move |y| (x, y))).filter(|(x, y)| px(&img, *x, *y) == ink).count();
        assert!(drawn > 40, "{} ink pixels", drawn);
        let muted = rgb(INK_LIGHT[2]);
        let right = (0..img.height).flat_map(|y| (0..img.width).map(move |x| (x, y))).filter(|(x, y)| px(&img, *x, *y) == muted).map(|(x, _)| x).max().unwrap();
        assert!((68..=72).contains(&right), "right-aligned text ends at its anchor: {}", right);
        // the 'H' of Hi: a vertical stem two glyph-pixels wide starting at x = 8
        assert_eq!(px(&img, 8, 20), ink);
    }

    #[test]
    fn translated_groups_and_hidden_things_are_honoured_and_unknown_elements_draw_nothing() {
        let svg = doc("<g transform=\"translate(10,5)\"><rect class=\"f1\" x=\"0\" y=\"0\" width=\"5\" height=\"5\"/></g><rect class=\"f2\" x=\"30\" y=\"20\" width=\"5\" height=\"5\"/><rect class=\"hit\" x=\"0\" y=\"0\" width=\"40\" height=\"30\"/><rect class=\"f3\" hidden x=\"0\" y=\"20\" width=\"5\" height=\"5\"/><image href=\"x\"/><foo class=\"f4\"/>");
        let img = svg_to_image(&svg, false, 1).unwrap();
        assert_eq!(px(&img, 12, 7), rgb(SERIES_LIGHT[0]));
        assert_eq!(px(&img, 32, 22), rgb(SERIES_LIGHT[1]), "the group's offset ended with the group");
        assert_eq!(px(&img, 2, 22), rgb(SURFACE_LIGHT));
        assert_eq!(px(&img, 2, 2), rgb(SURFACE_LIGHT));
    }

    #[test]
    fn hostile_input_is_refused_or_harmless() {
        assert!(svg_to_image("not xml at all", false, 2).is_err());
        assert!(svg_to_image("<svg viewBox=\"0 0 100000 100000\"></svg>", false, 2).unwrap_err().0.contains("limit"));
        assert!(svg_to_image("<svg viewBox=\"0 0 0 0\"></svg>", false, 2).is_err());
        let nasty = doc("<path class=\"f1\" d=\"M1e999,5 L-1e999,NaN A0,0 0 1 1 5,5 Q Z Z M\"/><path class=\"k1\" d=\"M5,5\"/><circle class=\"f1\" r=\"1e12\"/>");
        assert!(svg_to_image(&nasty, false, 1).is_ok());
    }

    #[test]
    fn the_same_svg_is_the_same_png() {
        let svg = doc("<path class=\"k2\" d=\"M2,3 L30,25\"/><text x=\"3\" y=\"10\" class=\"t2\" font-size=\"11\">x</text>");
        assert_eq!(svg_to_png(&svg, false, 2, &[]).unwrap(), svg_to_png(&svg, false, 2, &[]).unwrap());
    }
}
