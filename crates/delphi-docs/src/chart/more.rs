//! The other chart forms (`specs/014` §8.1): stacked bars (parts of a total), donut (parts of a whole), heatmap
//! (a grid shaded in one hue), box plot (the spread within groups) and sparkline (the shape of a trend). They
//! share the header, footer, palette and mark rules of the main renderer and draw only their own plot area.

use super::render::{column_path, line, text, trunc, tw, Fmt, PAD};
use super::scale::{f, linear_ticks};
use super::{ChartSpec, Kind, XAxis};

pub(super) fn handles(k: Kind) -> bool {
    matches!(k, Kind::Stacked | Kind::Donut | Kind::Heatmap | Kind::Box | Kind::Sparkline)
}

/// The `q` quantile (0 to 1) of sorted values, by linear interpolation between the nearest ranks.
pub(super) fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

pub(super) struct Out {
    pub body: String,
    /// The marks could not carry the values, so the renderer adds a values table.
    pub table: bool,
    /// Where the content ends, when that is above the bottom of the plot area (the chart is cut to fit).
    pub end: Option<f64>,
}

fn cats(spec: &ChartSpec) -> &[String] {
    match &spec.x {
        XAxis::Categories(c) => c,
        _ => &[],
    }
}

/// Plot area `top..bottom`; `relief` is true when a series needs its values shown (light-mode low-contrast slots).
pub(super) fn draw(spec: &ChartSpec, fmt: &Fmt, w: f64, top: f64, bottom: f64, relief: bool) -> Out {
    match spec.kind {
        Kind::Stacked => stacked(spec, fmt, w, top, bottom, relief),
        Kind::Donut => donut(spec, fmt, w, top, bottom),
        Kind::Heatmap => heatmap(spec, fmt, w, top, bottom),
        Kind::Box => boxes(spec, fmt, w, top, bottom),
        _ => sparkline(spec, fmt, w),
    }
}

fn stacked(spec: &ChartSpec, fmt: &Fmt, w: f64, top: f64, bottom: f64, relief: bool) -> Out {
    let cats = cats(spec);
    let n = cats.len();
    let totals: Vec<f64> = (0..n).map(|i| spec.series.iter().map(|s| s.values[i].unwrap_or(0.0).max(0.0)).sum()).collect();
    let (lo, hi, ticks) = linear_ticks(0.0, totals.iter().cloned().fold(0.0, f64::max), 5);
    let tick_w = ticks.iter().map(|t| tw(&fmt.tick(*t), 11.0)).fold(0.0, f64::max);
    let (x0, x1) = (PAD + tick_w + 10.0, w - PAD);
    let py = |v: f64| bottom - (v - lo) / (hi - lo) * (bottom - top);
    let mut body = String::new();
    for t in &ticks {
        line(&mut body, "g", x0, py(*t), x1, py(*t));
        text(&mut body, x0 - 8.0, py(*t) + 4.0, "t3", 11.0, "end", &fmt.tick(*t));
    }
    line(&mut body, "ax", x0, py(0.0), x1, py(0.0));
    let band = (x1 - x0) / n as f64;
    let thick = (band * 0.7).clamp(3.0, 24.0);
    let widest = cats.iter().map(|c| tw(c, 11.0)).fold(0.0, f64::max);
    let step = (1..=n).find(|s| band * *s as f64 >= widest + 8.0).unwrap_or(n);
    for (i, cat) in cats.iter().enumerate() {
        let bx = x0 + band * i as f64 + (band - thick) / 2.0;
        let present: Vec<usize> = (0..spec.series.len()).filter(|j| spec.series[*j].values[i].unwrap_or(0.0) > 0.0).collect();
        let mut acc = 0.0;
        for (pos, j) in present.iter().enumerate() {
            let v = spec.series[*j].values[i].unwrap_or(0.0);
            // a 2px surface gap above the segment below, and a rounded end on the topmost segment only
            let y_from = py(acc) - if pos > 0 { 2.0 } else { 0.0 };
            let y_to = py(acc + v);
            acc += v;
            if y_from - y_to < 1.0 {
                continue;
            }
            if pos + 1 == present.len() {
                body.push_str(&format!("<path class=\"f{}\" d=\"{}\"/>\n", j + 1, column_path(bx, thick, y_from, y_to)));
            } else {
                body.push_str(&format!("<rect class=\"f{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n", j + 1, f(bx), f(y_to), f(thick), f(y_from - y_to)));
            }
        }
        let label = fmt.val(totals[i]);
        if n <= 30 && tw(&label, 11.0) <= band + 2.0 {
            text(&mut body, bx + thick / 2.0, py(totals[i]) - 6.0, "t2", 11.0, "middle", &label);
        }
        if i % step == 0 {
            text(&mut body, x0 + band * (i as f64 + 0.5), bottom + 16.0, "t3", 11.0, "middle", &trunc(cat, band * step as f64 - 6.0, 11.0));
        }
    }
    Out { body, table: relief, end: None }
}

fn point(cx: f64, cy: f64, r: f64, a: f64) -> (f64, f64) {
    (cx + r * a.cos(), cy + r * a.sin())
}

fn donut(spec: &ChartSpec, fmt: &Fmt, w: f64, top: f64, bottom: f64) -> Out {
    let labels = cats(spec);
    let vals: Vec<f64> = spec.series[0].values.iter().map(|v| v.unwrap_or(0.0).max(0.0)).collect();
    let total: f64 = vals.iter().sum();
    let side_by_side = w >= 600.0;
    let legend_h = labels.len() as f64 * 22.0;
    let avail_h = bottom - top + 24.0 - if side_by_side { 0.0 } else { legend_h + 12.0 };
    let r_out = (avail_h / 2.0).min(if side_by_side { w * 0.2 } else { w * 0.3 }).max(30.0);
    let r_in = r_out * 0.6;
    let (cx, cy) = if side_by_side { (PAD + r_out + 24.0, (top + bottom) / 2.0 + 8.0) } else { (w / 2.0, top + r_out + 4.0) };
    let mut body = String::new();
    let mut a0 = -std::f64::consts::FRAC_PI_2;
    for (j, v) in vals.iter().enumerate() {
        let sweep = v / total * std::f64::consts::TAU;
        if sweep <= 1e-9 {
            continue;
        }
        let d = if sweep >= std::f64::consts::TAU - 1e-6 {
            format!("M{},{} A{r},{r} 0 1 1 {},{} A{r},{r} 0 1 1 {},{} M{},{} A{q},{q} 0 1 0 {},{} A{q},{q} 0 1 0 {},{} Z", f(cx - r_out), f(cy), f(cx + r_out), f(cy), f(cx - r_out), f(cy), f(cx - r_in), f(cy), f(cx + r_in), f(cy), f(cx - r_in), f(cy), r = f(r_out), q = f(r_in))
        } else {
            let a1 = a0 + sweep;
            let (o0, o1, i1, i0) = (point(cx, cy, r_out, a0), point(cx, cy, r_out, a1), point(cx, cy, r_in, a1), point(cx, cy, r_in, a0));
            let large = if sweep > std::f64::consts::PI { 1 } else { 0 };
            format!("M{},{} A{r},{r} 0 {l} 1 {},{} L{},{} A{q},{q} 0 {l} 0 {},{} Z", f(o0.0), f(o0.1), f(o1.0), f(o1.1), f(i1.0), f(i1.1), f(i0.0), f(i0.1), r = f(r_out), q = f(r_in), l = large)
        };
        body.push_str(&format!("<path class=\"f{} sl\" fill-rule=\"evenodd\" d=\"{}\"/>\n", j + 1, d));
        a0 += sweep;
    }
    text(&mut body, cx, cy + 4.0, "t1 b", 20.0, "middle", &trunc(&fmt.val(total), r_in * 1.7, 20.0));
    text(&mut body, cx, cy + 22.0, "t3", 11.0, "middle", "Total");
    // legend: the part, its value and its share; the colour key is never the only way to tell parts apart
    let (lx, right, mut ly) = if side_by_side { (cx + r_out + 40.0, w - PAD, cy - legend_h / 2.0 + 14.0) } else { (PAD, w - PAD, cy + r_out + 28.0) };
    let share_w = 44.0;
    let value_w = labels.iter().zip(&vals).map(|(_, v)| tw(&fmt.val(*v), 12.0)).fold(0.0, f64::max) + 12.0;
    for (j, (label, v)) in labels.iter().zip(&vals).enumerate() {
        body.push_str(&format!("<rect class=\"f{}\" x=\"{}\" y=\"{}\" width=\"10\" height=\"10\" rx=\"2\"/>\n", j + 1, f(lx), f(ly - 9.0)));
        text(&mut body, lx + 18.0, ly, "t2", 12.0, "start", &trunc(label, right - share_w - value_w - lx - 24.0, 12.0));
        text(&mut body, right - share_w, ly, "t2", 12.0, "end", &fmt.val(*v));
        let share = v / total * 100.0;
        text(&mut body, right, ly, "t3", 12.0, "end", &if share > 0.0 && share < 1.0 { "<1%".to_string() } else { format!("{:.0}%", share) });
        ly += 22.0;
    }
    Out { body, table: false, end: None }
}

fn heatmap(spec: &ChartSpec, fmt: &Fmt, w: f64, top: f64, bottom: f64) -> Out {
    let cols = cats(spec);
    let (n, k) = (cols.len(), spec.series.len());
    let all: Vec<f64> = spec.series.iter().flat_map(|s| s.values.iter().flatten().copied()).collect();
    let (lo, hi) = (all.iter().cloned().fold(f64::INFINITY, f64::min), all.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    let label_w = spec.series.iter().map(|s| tw(&s.name, 11.0)).fold(0.0, f64::max).min(w * 0.3);
    let (x0, x1) = (PAD + label_w + 10.0, w - PAD);
    let grid_bottom = bottom - 20.0;
    let cell_w = (x1 - x0) / n as f64;
    let cell_h = ((grid_bottom - top) / k as f64).min(34.0);
    let mut body = String::new();
    for (r, s) in spec.series.iter().enumerate() {
        let y = top + r as f64 * cell_h;
        text(&mut body, PAD + label_w, y + cell_h / 2.0 + 4.0, "t2", 11.0, "end", &trunc(&s.name, label_w, 11.0));
        for (c, v) in s.values.iter().enumerate() {
            let x = x0 + c as f64 * cell_w;
            let Some(v) = v else {
                body.push_str(&format!("<rect class=\"g\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\"/>\n", f(x + 0.5), f(y + 0.5), f(cell_w - 1.0), f(cell_h - 1.0)));
                continue;
            };
            let t = if hi > lo { (v - lo) / (hi - lo) } else { 0.5 };
            let step = ((t * 5.0).floor() as usize).min(4);
            body.push_str(&format!("<rect class=\"h{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\"/>\n", step, f(x + 0.5), f(y + 0.5), f(cell_w - 1.0), f(cell_h - 1.0)));
            let label = fmt.val(*v);
            if cell_h >= 16.0 && tw(&label, 11.0) + 6.0 <= cell_w {
                text(&mut body, x + cell_w / 2.0, y + cell_h / 2.0 + 4.0, &format!("ht{}", step), 11.0, "middle", &label);
            }
        }
    }
    let widest = cols.iter().map(|c| tw(c, 11.0)).fold(0.0, f64::max);
    let every = (1..=n).find(|s| cell_w * *s as f64 >= widest + 8.0).unwrap_or(n);
    for (c, label) in cols.iter().enumerate().step_by(every) {
        text(&mut body, x0 + (c as f64 + 0.5) * cell_w, top + k as f64 * cell_h + 14.0, "t3", 11.0, "middle", &trunc(label, cell_w * every as f64 - 6.0, 11.0));
    }
    // the scale: five steps, faint to strong, with the lowest and highest value
    let (lo_s, hi_s) = (fmt.val(lo), fmt.val(hi));
    let sy = top + k as f64 * cell_h + 26.0;
    text(&mut body, x0, sy + 9.0, "t3", 11.0, "start", &lo_s);
    let sx = x0 + tw(&lo_s, 11.0) + 8.0;
    for i in 0..5 {
        body.push_str(&format!("<rect class=\"h{}\" x=\"{}\" y=\"{}\" width=\"26\" height=\"10\"/>\n", i, f(sx + i as f64 * 26.0), f(sy)));
    }
    text(&mut body, sx + 5.0 * 26.0 + 8.0, sy + 9.0, "t3", 11.0, "start", &hi_s);
    Out { body, table: false, end: Some(sy + 14.0) }
}

fn boxes(spec: &ChartSpec, fmt: &Fmt, w: f64, top: f64, bottom: f64) -> Out {
    let cats = cats(spec);
    let n = cats.len();
    let all: Vec<f64> = spec.groups.iter().flatten().copied().collect();
    let (lo, hi, ticks) = linear_ticks(all.iter().cloned().fold(f64::INFINITY, f64::min), all.iter().cloned().fold(f64::NEG_INFINITY, f64::max), 5);
    let tick_w = ticks.iter().map(|t| tw(&fmt.tick(*t), 11.0)).fold(0.0, f64::max);
    let (x0, x1) = (PAD + tick_w + 10.0, w - PAD);
    let py = |v: f64| bottom - (v - lo) / (hi - lo) * (bottom - top);
    let mut body = String::new();
    for t in &ticks {
        line(&mut body, "g", x0, py(*t), x1, py(*t));
        text(&mut body, x0 - 8.0, py(*t) + 4.0, "t3", 11.0, "end", &fmt.tick(*t));
    }
    line(&mut body, "ax", x0, bottom, x1, bottom);
    let band = (x1 - x0) / n as f64;
    let bw = (band * 0.5).clamp(8.0, 40.0);
    let widest = cats.iter().map(|c| tw(c, 11.0)).fold(0.0, f64::max);
    let step = (1..=n).find(|s| band * *s as f64 >= widest + 8.0).unwrap_or(n);
    for (i, g) in spec.groups.iter().enumerate() {
        let (q1, med, q3) = (quantile(g, 0.25), quantile(g, 0.5), quantile(g, 0.75));
        let iqr = q3 - q1;
        let (fence_lo, fence_hi) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
        let whisker_lo = g.iter().copied().find(|v| *v >= fence_lo).unwrap_or(q1);
        let whisker_hi = g.iter().rev().copied().find(|v| *v <= fence_hi).unwrap_or(q3);
        let cx = x0 + band * (i as f64 + 0.5);
        let bx = cx - bw / 2.0;
        body.push_str(&format!("<path class=\"wh\" d=\"M{c},{a} L{c},{b} M{c},{d} L{c},{e} M{l},{b} L{r},{b} M{l},{e} L{r},{e}\"/>\n", c = f(cx), a = f(py(q3)), b = f(py(whisker_hi)), d = f(py(q1)), e = f(py(whisker_lo)), l = f(cx - bw / 4.0), r = f(cx + bw / 4.0)));
        body.push_str(&format!("<rect class=\"bx\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\"/>\n", f(bx), f(py(q3)), f(bw), f((py(q1) - py(q3)).max(1.0))));
        body.push_str(&format!("<path class=\"k1\" d=\"M{},{} L{},{}\"/>\n", f(bx), f(py(med)), f(bx + bw), f(py(med))));
        for v in g.iter().filter(|v| **v < fence_lo || **v > fence_hi).take(40) {
            body.push_str(&format!("<circle class=\"d1\" cx=\"{}\" cy=\"{}\" r=\"4\"/>\n", f(cx), f(py(*v))));
        }
        let label = fmt.val(med);
        if (band - bw) / 2.0 - 6.0 >= tw(&label, 11.0) {
            text(&mut body, bx + bw + 6.0, py(med) + 4.0, "t2", 11.0, "start", &label);
        }
        if i % step == 0 {
            text(&mut body, cx, bottom + 16.0, "t3", 11.0, "middle", &trunc(&cats[i], band * step as f64 - 6.0, 11.0));
        }
    }
    Out { body, table: false, end: None }
}

fn sparkline(spec: &ChartSpec, fmt: &Fmt, w: f64) -> Out {
    let mut body = String::new();
    let s = &spec.series[0];
    let n = s.values.len();
    let end_label = s.values.iter().rev().flatten().next().map(|v| fmt.val(*v)).unwrap_or_default();
    let label_w = tw(&end_label, 12.0);
    text(&mut body, PAD, 20.0, "t1 b", 13.0, "start", &trunc(&spec.title, w - 2.0 * PAD, 13.0));
    let y_top = if let Some(sub) = &spec.subtitle {
        text(&mut body, PAD, 36.0, "t3", 11.0, "start", &trunc(sub, w - 2.0 * PAD, 11.0));
        46.0
    } else {
        36.0
    };
    let y_bot = 76.0;
    let (x0, x1) = (PAD + 3.0, w - PAD - label_w - 16.0);
    let xs: Vec<f64> = match &spec.x {
        XAxis::Categories(_) => (0..n).map(|i| i as f64).collect(),
        XAxis::Numbers(v) | XAxis::Dates(v) => v.clone(),
    };
    let (xlo, xhi) = (xs.iter().cloned().fold(f64::INFINITY, f64::min), xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    let vals: Vec<f64> = s.values.iter().flatten().copied().collect();
    let (vlo, vhi) = (vals.iter().cloned().fold(f64::INFINITY, f64::min), vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    let pad = if vhi > vlo { (vhi - vlo) * 0.08 } else { 1.0 };
    let (vlo, vhi) = (vlo - pad, vhi + pad);
    let px = |x: f64| if xhi > xlo { x0 + (x - xlo) / (xhi - xlo) * (x1 - x0) } else { (x0 + x1) / 2.0 };
    let py = |v: f64| y_bot - (v - vlo) / (vhi - vlo) * (y_bot - y_top);
    let mut d = String::new();
    let mut pen_up = true;
    let mut last = None;
    for (i, v) in s.values.iter().enumerate() {
        match v {
            Some(v) => {
                d.push_str(&format!("{}{},{} ", if pen_up { "M" } else { "L" }, f(px(xs[i])), f(py(*v))));
                pen_up = false;
                last = Some((px(xs[i]), py(*v)));
            }
            None => pen_up = true,
        }
    }
    body.push_str(&format!("<path class=\"k1\" d=\"{}\"/>\n", d.trim_end()));
    if let Some((x, y)) = last {
        body.push_str(&format!("<circle class=\"d1\" cx=\"{}\" cy=\"{}\" r=\"4\"/>\n", f(x), f(y)));
        text(&mut body, x + 10.0, y + 4.0, "t2", 12.0, "start", &end_label);
    }
    Out { body, table: false, end: None }
}

/// The heatmap's step fills and their label colours, as CSS variables for one mode.
pub(super) fn mode_vars(light: bool) -> String {
    let mut s = String::new();
    for (i, (fill, text)) in super::palette::heat(light).iter().enumerate() {
        s.push_str(&format!("--h{i}:{fill};--ht{i}:{text};"));
    }
    s
}
