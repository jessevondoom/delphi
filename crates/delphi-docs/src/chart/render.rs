//! Drawing a checked [`ChartSpec`] as a self-contained SVG (`specs/014` §8.2). The look comes from fixed mark
//! specs (thin marks, a 4px rounded data end on a square baseline, 2px lines with an end dot, areas at 10%,
//! hairline solid grid) and two spacers (a 2px surface gap between touching fills, a 2px surface ring on dots).
//! Text is always set in the ink tokens, never in a series colour. Light and dark come from CSS variables in the
//! file, so the same SVG reads on either page.

use super::palette::*;
use super::scale::*;
use super::{ChartSpec, Kind, Series, Size, Theme, XAxis};
use crate::markdown::escape;

const FONT: &str = "system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif";
const PAD: f64 = 16.0;

/// Estimated width of `s` in `px`-sized system type. Deliberately a little wide: a label that is judged to fit
/// then fits with room to spare.
fn tw(s: &str, px: f64) -> f64 {
    s.chars().map(|c| if c.is_ascii_digit() { 0.58 } else if c.is_uppercase() { 0.68 } else if matches!(c, 'i' | 'l' | 'I' | '.' | ',' | ':' | ';' | '\'' | '|' | '!' | ' ' | 'j' | 't' | 'f' | 'r') { 0.40 } else if c.is_ascii() { 0.58 } else { 1.0 }).sum::<f64>() * px
}

fn trunc(s: &str, max_w: f64, px: f64) -> String {
    if tw(s, px) <= max_w {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        if tw(&format!("{}{}…", out, c), px) > max_w {
            break;
        }
        out.push(c);
    }
    format!("{}…", out.trim_end())
}

fn vars(light: bool) -> String {
    let (surface, ink, series, grid, axis) = if light { (SURFACE_LIGHT, INK_LIGHT, SERIES_LIGHT, GRID_LIGHT, AXIS_LIGHT) } else { (SURFACE_DARK, INK_DARK, SERIES_DARK, GRID_DARK, AXIS_DARK) };
    format!(
        "color-scheme:{};--surface:{};--ink:{};--ink2:{};--muted:{};--grid:{};--axis:{};--s1:{};--s2:{};--s3:{};--s4:{};",
        if light { "light" } else { "dark" },
        surface,
        ink[0],
        ink[1],
        ink[2],
        grid,
        axis,
        series[0],
        series[1],
        series[2],
        series[3]
    )
}

fn style() -> String {
    let mut s = String::new();
    s.push_str(&format!(".viz-root{{{}font-family:{};}}\n", vars(true), FONT));
    s.push_str(&format!("@media (prefers-color-scheme: dark){{.viz-root:where(:root:not([data-theme=\"light\"])){{{d}}}:root:where(:not([data-theme=\"light\"])) .viz-root:where(:not([data-theme=\"light\"])){{{d}}}}}\n", d = vars(false)));
    s.push_str(&format!(".viz-root[data-theme=\"dark\"],:root[data-theme=\"dark\"] .viz-root{{{}}}\n", vars(false)));
    s.push_str(".surface{fill:var(--surface)}.g{stroke:var(--grid);stroke-width:1;fill:none}.ax{stroke:var(--axis);stroke-width:1;fill:none}.ref{stroke:var(--muted);stroke-width:1;fill:none}\n");
    s.push_str(".t1{fill:var(--ink)}.t2{fill:var(--ink2)}.t3{fill:var(--muted)}.b{font-weight:600}\n");
    for i in 1..=4 {
        s.push_str(&format!(".f{i}{{fill:var(--s{i})}}.k{i}{{stroke:var(--s{i});fill:none;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}}.a{i}{{fill:var(--s{i});fill-opacity:.1}}.d{i}{{fill:var(--s{i});stroke:var(--surface);stroke-width:2}}\n", i = i));
    }
    s.push_str("@media (forced-colors: active){.surface{fill:Canvas}.t1,.t2,.t3{fill:CanvasText}.g,.ax,.ref{stroke:GrayText}.f1,.f2,.f3,.f4,.d1,.d2,.d3,.d4{fill:CanvasText}.k1,.k2,.k3,.k4{stroke:CanvasText}}\n");
    s
}

struct Fmt<'a>(&'a ChartSpec);

impl Fmt<'_> {
    fn val(&self, v: f64) -> String {
        self.0.y_fmt.full(v)
    }
    fn tick(&self, v: f64) -> String {
        self.0.y_fmt.axis(v)
    }
    fn xnum(&self, v: f64) -> String {
        if v.fract() == 0.0 && (1800.0..=2200.0).contains(&v) { format!("{}", v as i64) } else { NumFmt::default().axis(v) }
    }
}

fn text(out: &mut String, x: f64, y: f64, class: &str, size: f64, anchor: &str, s: &str) {
    out.push_str(&format!("<text x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"{}\" text-anchor=\"{}\">{}</text>\n", f(x), f(y), class, f(size), anchor, escape(s)));
}

fn line(out: &mut String, class: &str, x1: f64, y1: f64, x2: f64, y2: f64) {
    out.push_str(&format!("<line class=\"{}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n", class, f(x1), f(y1), f(x2), f(y2)));
}

/// A column from `y0` (the baseline) to `y1` (the data end): the data end is rounded, the baseline square.
fn column_path(x: f64, w: f64, y0: f64, y1: f64) -> String {
    let h = (y0 - y1).abs();
    let r = 4f64.min(w / 2.0).min(h);
    let s = if y1 < y0 { 1.0 } else { -1.0 }; // 1: grows upward
    format!(
        "M{x0},{b} L{x0},{e} Q{x0},{y} {xr},{y} L{xl},{y} Q{x1},{y} {x1},{e} L{x1},{b} Z",
        x0 = f(x),
        x1 = f(x + w),
        xr = f(x + r),
        xl = f(x + w - r),
        b = f(y0),
        y = f(y1),
        e = f(y1 + s * r)
    )
}

/// A bar from `x0` (the baseline) to `x1` (the data end), `h` thick starting at `y`.
fn row_path(x0: f64, x1: f64, y: f64, h: f64) -> String {
    let w = (x1 - x0).abs();
    let r = 4f64.min(h / 2.0).min(w);
    let s = if x1 > x0 { 1.0 } else { -1.0 };
    format!("M{b},{y} L{e},{y} Q{x},{y} {x},{yr} L{x},{yl} Q{x},{y2} {e},{y2} L{b},{y2} Z", b = f(x0), x = f(x1), e = f(x1 - s * r), y = f(y), y2 = f(y + h), yr = f(y + r), yl = f(y + h - r))
}

struct Axis {
    lo: f64,
    hi: f64,
    ticks: Vec<f64>,
    log: bool,
    p0: f64, // pixel at lo
    p1: f64, // pixel at hi
}

impl Axis {
    fn at(&self, v: f64) -> f64 {
        let t = if self.log { (v.max(self.lo).ln() - self.lo.ln()) / (self.hi.ln() - self.lo.ln()) } else { (v - self.lo) / (self.hi - self.lo) };
        self.p0 + t * (self.p1 - self.p0)
    }
}

fn value_axis(spec: &ChartSpec, p0: f64, p1: f64) -> Axis {
    let vals: Vec<f64> = spec.series.iter().flat_map(|s| s.values.iter().flatten().copied()).collect();
    let (mut lo, mut hi) = (vals.iter().cloned().fold(f64::INFINITY, f64::min), vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    if spec.log_y {
        let (l, h, t) = log_ticks(lo, hi);
        return Axis { lo: l, hi: h, ticks: t, log: true, p0, p1 };
    }
    if matches!(spec.kind, Kind::Bar | Kind::HBar | Kind::Area | Kind::Hist) && !spec.truncated {
        lo = lo.min(0.0);
        hi = hi.max(0.0);
    }
    let (l, h, t) = linear_ticks(lo, hi, 5);
    Axis { lo: l, hi: h, ticks: t, log: false, p0, p1 }
}

struct Legend {
    rows: usize,
}

fn legend(out: &mut String, spec: &ChartSpec, y: f64, width: f64, line_key: bool, dots: bool) -> Legend {
    let (mut x, mut row) = (PAD, 0usize);
    for (i, s) in spec.series.iter().enumerate() {
        let w = tw(&s.name, 12.0) + 26.0;
        if x + w > width - PAD && x > PAD {
            x = PAD;
            row += 1;
        }
        let cy = y + row as f64 * 18.0;
        if dots {
            out.push_str(&format!("<circle class=\"d{n}\" cx=\"{m}\" cy=\"{c}\" r=\"5\"/>\n", n = i + 1, m = f(x + 5.0), c = f(cy - 4.0)));
            text(out, x + 16.0, cy, "t2", 12.0, "start", &s.name);
            x += w;
        } else if line_key {
            out.push_str(&format!("<path class=\"k{n}\" d=\"M{a},{c} L{b},{c}\"/><circle class=\"d{n}\" cx=\"{m}\" cy=\"{c}\" r=\"4\"/>\n", n = i + 1, a = f(x), b = f(x + 18.0), m = f(x + 18.0), c = f(cy - 4.0)));
            text(out, x + 28.0, cy, "t2", 12.0, "start", &s.name);
            x += w + 14.0;
        } else {
            out.push_str(&format!("<rect class=\"f{}\" x=\"{}\" y=\"{}\" width=\"10\" height=\"10\" rx=\"2\"/>\n", i + 1, f(x), f(cy - 9.0)));
            text(out, x + 16.0, cy, "t2", 12.0, "start", &s.name);
            x += w;
        }
    }
    Legend { rows: row + 1 }
}

/// Values for the accessible description: what a screen-reader user, or anyone who cannot see the marks, needs.
fn describe(spec: &ChartSpec, fmt: &Fmt) -> String {
    let mut d = format!("{} chart. ", match spec.kind {
        Kind::Bar => "Bar",
        Kind::HBar => "Horizontal bar",
        Kind::Line => "Line",
        Kind::Area => "Area",
        Kind::Scatter => "Scatter",
        Kind::Hist => "Histogram",
    });
    let xl = |i: usize| match &spec.x {
        XAxis::Categories(c) => c[i].clone(),
        XAxis::Numbers(n) => fmt.xnum(n[i]),
        XAxis::Dates(n) => date_label(n[i]),
    };
    for s in &spec.series {
        let pts: Vec<(usize, f64)> = s.values.iter().enumerate().filter_map(|(i, v)| v.map(|v| (i, v))).collect();
        if pts.is_empty() {
            continue;
        }
        if spec.kind == Kind::Hist {
            d.push_str(&format!("Distribution of {} ({} values, minimum {}, maximum {}). ", s.name, pts.len(), fmt.val(pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min)), fmt.val(pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max))));
            continue;
        }
        let (lo, hi) = (pts.iter().cloned().fold(pts[0], |a, b| if b.1 < a.1 { b } else { a }), pts.iter().cloned().fold(pts[0], |a, b| if b.1 > a.1 { b } else { a }));
        d.push_str(&format!("{}: minimum {} at {}, maximum {} at {}.", s.name, fmt.val(lo.1), xl(lo.0), fmt.val(hi.1), xl(hi.0)));
        if matches!(spec.kind, Kind::Line | Kind::Area) && pts.len() > 1 {
            let (a, b) = (pts[0], pts[pts.len() - 1]);
            if a.1 != 0.0 {
                let ch = (b.1 - a.1) / a.1.abs() * 100.0;
                d.push_str(&format!(" From {} to {} it {} {:.0}%.", fmt.val(a.1), fmt.val(b.1), if ch >= 0.0 { "rises" } else { "falls" }, ch.abs()));
            }
        }
        d.push(' ');
    }
    if spec.kind != Kind::Scatter && spec.kind != Kind::Hist && spec.x.len() * spec.series.len() <= 60 {
        d.push_str("Values: ");
        let parts: Vec<String> = (0..spec.x.len()).map(|i| format!("{} {}", xl(i), spec.series.iter().map(|s| s.values[i].map(|v| fmt.val(v)).unwrap_or_else(|| "none".into())).collect::<Vec<_>>().join(" / "))).collect();
        d.push_str(&parts.join("; "));
        d.push('.');
    }
    d.trim().to_string()
}

/// Rows for the values table that stands in for labels when they cannot be shown on the marks.
fn table_rows(spec: &ChartSpec, fmt: &Fmt) -> Vec<Vec<String>> {
    let mut rows = vec![];
    match (&spec.x, spec.kind) {
        (XAxis::Categories(c), Kind::Bar | Kind::HBar) => {
            let mut head = vec![spec.x_name.clone()];
            head.extend(spec.series.iter().map(|s| s.name.clone()));
            rows.push(head);
            for (i, label) in c.iter().enumerate() {
                let mut r = vec![label.clone()];
                r.extend(spec.series.iter().map(|s| s.values[i].map(|v| fmt.val(v)).unwrap_or_default()));
                rows.push(r);
            }
        }
        _ => {
            rows.push(vec!["Series".into(), "First".into(), "Last".into(), "Min".into(), "Max".into()]);
            for s in &spec.series {
                let v: Vec<f64> = s.values.iter().flatten().copied().collect();
                rows.push(vec![s.name.clone(), v.first().map(|x| fmt.val(*x)).unwrap_or_default(), v.last().map(|x| fmt.val(*x)).unwrap_or_default(), fmt.val(v.iter().cloned().fold(f64::INFINITY, f64::min)), fmt.val(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max))]);
            }
        }
    }
    rows
}

fn table(out: &mut String, rows: &[Vec<String>], y: f64, width: f64, spec: &ChartSpec) -> f64 {
    let cols = rows[0].len();
    let series_rows = rows[0][0] == "Series";
    let first_w = rows.iter().map(|r| tw(&r[0], 11.0)).fold(0.0, f64::max).min(width * 0.35) + 16.0 + if series_rows { 14.0 } else { 0.0 };
    let rest = ((width - 2.0 * PAD - first_w) / (cols.max(2) - 1) as f64).clamp(56.0, 96.0);
    for (ri, r) in rows.iter().enumerate() {
        let cy = y + ri as f64 * 17.0;
        if ri == 0 {
            line(out, "g", PAD, cy + 5.0, PAD + first_w + rest * (cols - 1) as f64, cy + 5.0);
        }
        for (ci, cell) in r.iter().enumerate() {
            let (x, anchor) = if ci == 0 { (PAD + if series_rows && ri > 0 { 14.0 } else { 0.0 }, "start") } else { (PAD + first_w + rest * ci as f64 - 8.0, "end") };
            if series_rows && ri > 0 && ci == 0 {
                out.push_str(&format!("<circle class=\"d{}\" cx=\"{}\" cy=\"{}\" r=\"4\"/>\n", ri, f(PAD + 4.0), f(cy - 4.0)));
            }
            if ri == 0 && ci > 0 && spec.kind != Kind::Line && spec.kind != Kind::Area {
                out.push_str(&format!("<rect class=\"f{}\" x=\"{}\" y=\"{}\" width=\"8\" height=\"8\" rx=\"2\"/>\n", ci, f(x - tw(cell, 11.0) - 12.0), f(cy - 8.0)));
            }
            text(out, x, cy, if ri == 0 { "t3" } else { "t2" }, 11.0, anchor, &trunc(cell, if ci == 0 { first_w - 8.0 } else { rest - 8.0 }, 11.0));
        }
    }
    rows.len() as f64 * 17.0 + 6.0
}

/// The chart as an SVG document. `comment` is the in-file provenance note (sanitised by the caller).
pub fn render_svg(spec: &ChartSpec, comment: Option<&str>) -> String {
    let fmt = Fmt(spec);
    let (w, base_h) = match spec.size {
        Size::Wide => (720.0, 400.0),
        Size::Square => (480.0, 480.0),
        Size::Tall => (480.0, 640.0),
    };
    let k = spec.series.len();
    let multi = k >= 2;
    let needs_relief = k >= 3; // slots 3 and 4 are under 3:1 on the light surface

    // --- header
    let mut head = String::new();
    let mut y = PAD;
    text(&mut head, PAD, y + 16.0, "t1 b", 18.0, "start", &trunc(&spec.title, w - 2.0 * PAD, 18.0));
    y += 24.0;
    let mut sub: Vec<String> = vec![];
    if let Some(s) = &spec.subtitle {
        sub.push(s.clone());
    }
    if spec.log_y {
        sub.push("Log scale".into());
    }
    if spec.truncated {
        sub.push("Axis does not start at zero".into());
    }
    if !sub.is_empty() {
        text(&mut head, PAD, y + 12.0, "t2", 13.0, "start", &trunc(&sub.join(" · "), w - 2.0 * PAD, 13.0));
        y += 20.0;
    }
    if multi {
        let l = legend(&mut head, spec, y + 14.0, w, spec.kind == Kind::Line, spec.kind == Kind::Scatter);
        y += 8.0 + l.rows as f64 * 18.0;
    }
    let plot_top = y + 10.0;

    // --- footer lines
    let mut foot: Vec<String> = vec![];
    if let Some(s) = &spec.source {
        foot.push(format!("Source: {}", s));
    }
    foot.extend(spec.notes.iter().cloned());

    // --- body (needs the plot box, which needs the label widths)
    let ticks_text = |a: &Axis| a.ticks.iter().map(|t| fmt.tick(*t)).collect::<Vec<_>>();
    let mut body = String::new();
    let mut extra_h = 0.0;
    let foot_h = foot.len() as f64 * 15.0 + if foot.is_empty() { 0.0 } else { 6.0 };
    let x_axis_h = 24.0;

    // the values table, when marks cannot carry the values
    // assigned at the end of each of the three drawing branches below, once the marks are placed
    #[allow(clippy::needless_late_init)]
    let want_table: bool;
    let plot_bottom = base_h - PAD - foot_h - x_axis_h;

    let horizontal = spec.kind == Kind::HBar;
    let cats: Vec<String> = match &spec.x {
        XAxis::Categories(c) => c.clone(),
        _ => vec![],
    };

    if horizontal {
        // categories down the left; values along the bottom
        let label_w = cats.iter().map(|c| tw(c, 11.0)).fold(0.0, f64::max).min(w * 0.38);
        let (x0, x1) = (PAD + label_w + 10.0, w - PAD - 36.0);
        let ax = value_axis(spec, x0, x1);
        let n = cats.len();
        let band = (plot_bottom - plot_top) / n as f64;
        let thick = ((band * 0.7 - 2.0 * (k as f64 - 1.0)) / k as f64).clamp(3.0, 24.0);
        let group = k as f64 * thick + (k as f64 - 1.0) * 2.0;
        for (t, label) in ax.ticks.iter().zip(ticks_text(&ax)) {
            let px = ax.at(*t);
            line(&mut body, "g", px, plot_top, px, plot_bottom);
            text(&mut body, px, plot_bottom + 16.0, "t3", 11.0, "middle", &label);
        }
        let zero = ax.at(0f64.clamp(ax.lo, ax.hi));
        line(&mut body, "ax", zero, plot_top, zero, plot_bottom);
        let mut all_fit = true;
        let mut labels = String::new();
        for (i, c) in cats.iter().enumerate() {
            let cy = plot_top + band * i as f64 + (band - group) / 2.0;
            text(&mut body, PAD + label_w, plot_top + band * (i as f64 + 0.5) + 4.0, "t2", 11.0, "end", &trunc(c, label_w, 11.0));
            for (j, s) in spec.series.iter().enumerate() {
                let Some(v) = s.values[i] else { continue };
                let by = cy + j as f64 * (thick + 2.0);
                let end = ax.at(v);
                if (end - zero).abs() > 0.5 {
                    body.push_str(&format!("<path class=\"f{}\" d=\"{}\"/>\n", j + 1, row_path(zero, end, by, thick)));
                }
                let lbl = fmt.val(v);
                let room = if v >= 0.0 { x1 + 36.0 - end - 6.0 } else { end - x0 };
                if tw(&lbl, 11.0) <= room && (k == 1 || thick >= 11.0) {
                    text(&mut labels, if v >= 0.0 { end + 6.0 } else { end - 6.0 }, by + thick / 2.0 + 4.0, "t2", 11.0, if v >= 0.0 { "start" } else { "end" }, &lbl);
                } else {
                    all_fit = false;
                }
            }
        }
        body.push_str(&labels);
        want_table = needs_relief && !all_fit;
    } else if matches!(spec.kind, Kind::Bar | Kind::Hist) {
        let hist = spec.kind == Kind::Hist;
        let (bars, edges): (Vec<f64>, Vec<f64>) = if hist { histogram(&spec.series[0], spec.bins) } else { (vec![], vec![]) };
        let spec_for_axis;
        let sp: &ChartSpec = if hist {
            spec_for_axis = ChartSpec { series: vec![Series { name: "Count".into(), values: bars.iter().map(|b| Some(*b)).collect() }], ..spec.clone() };
            &spec_for_axis
        } else {
            spec
        };
        let tick_w = value_axis(sp, 0.0, 1.0).ticks.iter().map(|t| tw(&fmt.tick(*t), 11.0)).fold(0.0, f64::max);
        let (x0, x1) = (PAD + tick_w + 10.0, w - PAD);
        let ax = value_axis(sp, plot_bottom, plot_top);
        for (t, label) in ax.ticks.iter().zip(ticks_text(&ax)) {
            let py = ax.at(*t);
            line(&mut body, "g", x0, py, x1, py);
            text(&mut body, x0 - 8.0, py + 4.0, "t3", 11.0, "end", &label);
        }
        let zero = ax.at(0f64.clamp(ax.lo, ax.hi));
        line(&mut body, "ax", x0, zero, x1, zero);
        let n = if hist { bars.len() } else { cats.len() };
        let band = (x1 - x0) / n as f64;
        let (thick, group) = if hist { (band - 2.0, band - 2.0) } else {
            let t = ((band * 0.7 - 2.0 * (k as f64 - 1.0)) / k as f64).clamp(3.0, 24.0);
            (t, k as f64 * t + (k as f64 - 1.0) * 2.0)
        };
        let mut all_fit = true;
        let mut labels = String::new();
        let step = if hist { 1 } else { (1..=n).find(|s| band * *s as f64 >= cats.iter().map(|c| tw(c, 11.0)).fold(0.0, f64::max) + 8.0).unwrap_or(n) };
        for i in 0..n {
            let gx = x0 + band * i as f64 + (band - group) / 2.0;
            if hist {
                let vals = [Some(bars[i])];
                for (j, v) in vals.iter().enumerate() {
                    let Some(v) = v else { continue };
                    if *v > 0.0 {
                        body.push_str(&format!("<path class=\"f1\" d=\"{}\"/>\n", column_path(gx + j as f64, thick, zero, ax.at(*v))));
                    }
                }
                if i % 2 == 0 || n <= 12 {
                    let _ = edges.get(i);
                }
            } else {
                for (j, s) in spec.series.iter().enumerate() {
                    let Some(v) = s.values[i] else { continue };
                    let bx = gx + j as f64 * (thick + 2.0);
                    let end = ax.at(v);
                    if (end - zero).abs() > 0.5 {
                        body.push_str(&format!("<path class=\"f{}\" d=\"{}\"/>\n", j + 1, column_path(bx, thick, zero, end)));
                    }
                    let lbl = fmt.val(v);
                    let fits = tw(&lbl, 11.0) <= if k == 1 { band - 2.0 } else { thick + 4.0 };
                    if fits && n * k <= 30 {
                        text(&mut labels, bx + thick / 2.0, if v >= 0.0 { end - 6.0 } else { end + 14.0 }, "t2", 11.0, "middle", &lbl);
                    } else {
                        all_fit = false;
                    }
                }
            }
            if !hist && i % step == 0 {
                text(&mut body, x0 + band * (i as f64 + 0.5), plot_bottom + 16.0, "t3", 11.0, "middle", &trunc(&cats[i], band * step as f64 - 6.0, 11.0));
            }
        }
        if hist {
            // x axis: bin edges at nice values
            let (lo, hi) = (edges[0], edges[edges.len() - 1]);
            let (_, _, xt) = linear_ticks(lo, hi, 6);
            for t in xt.into_iter().filter(|t| *t >= lo - 1e-9 && *t <= hi + 1e-9) {
                let px = x0 + (t - lo) / (hi - lo) * (x1 - x0);
                text(&mut body, px, plot_bottom + 16.0, "t3", 11.0, "middle", &NumFmt::default().axis(t));
            }
        }
        body.push_str(&labels);
        want_table = needs_relief && !all_fit;
    } else {
        // line, area, scatter: a numeric or date x axis
        let n = spec.x.len();
        let tick_w = value_axis(spec, 0.0, 1.0).ticks.iter().map(|t| tw(&fmt.tick(*t), 11.0)).fold(0.0, f64::max);
        let last_labels: Vec<(usize, f64, String)> = if spec.kind == Kind::Scatter {
            vec![]
        } else {
            spec.series
                .iter()
                .enumerate()
                .filter_map(|(si, s)| s.values.iter().rposition(|v| v.is_some()).map(|i| (si, i, s.values[i].unwrap_or(0.0))))
                .map(|(si, _, v)| (si, v, if k == 1 { fmt.val(v) } else { format!("{}  {}", spec.series[si].name, fmt.val(v)) }))
                .collect()
        };
        let label_w = last_labels.iter().map(|l| tw(&l.2, 11.0)).fold(0.0, f64::max);
        let direct = !last_labels.is_empty() && label_w + 16.0 <= w * 0.3;
        let (x0, x1) = (PAD + tick_w + 10.0, w - PAD - if direct { label_w + 16.0 } else { 4.0 });
        let ax = value_axis(spec, plot_bottom, plot_top);
        for (t, label) in ax.ticks.iter().zip(ticks_text(&ax)) {
            let py = ax.at(*t);
            line(&mut body, "g", x0, py, x1, py);
            text(&mut body, x0 - 8.0, py + 4.0, "t3", 11.0, "end", &label);
        }
        line(&mut body, "ax", x0, plot_bottom, x1, plot_bottom);
        let xs: Vec<f64> = match &spec.x {
            XAxis::Categories(_) => (0..n).map(|i| i as f64).collect(),
            XAxis::Numbers(v) | XAxis::Dates(v) => v.clone(),
        };
        let (xlo, xhi) = if spec.kind == Kind::Scatter {
            let (lo, hi) = (xs.iter().cloned().fold(f64::INFINITY, f64::min), xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
            if matches!(spec.x, XAxis::Dates(_)) { (lo, hi) } else { let (a, b, _) = linear_ticks(lo, hi, 6); (a, b) }
        } else {
            (xs[0], xs[n - 1])
        };
        let xp = |v: f64| if xhi > xlo { x0 + (v - xlo) / (xhi - xlo) * (x1 - x0) } else { (x0 + x1) / 2.0 };
        // x ticks
        let xt: Vec<(f64, String)> = match &spec.x {
            XAxis::Dates(_) => date_ticks(xlo, xhi),
            XAxis::Numbers(_) => {
                let step = nice_step(xhi - xlo, 6);
                let first = (xlo / step).ceil() * step;
                (0..20).map(|i| first + i as f64 * step).take_while(|v| *v <= xhi + step * 1e-9).map(|v| (v, fmt.xnum((v * 1e6).round() / 1e6))).collect()
            }
            XAxis::Categories(c) => {
                let widest = c.iter().map(|l| tw(l, 11.0)).fold(0.0, f64::max) + 10.0;
                let every = ((widest / ((x1 - x0) / (n - 1).max(1) as f64)).ceil() as usize).max(1);
                (0..n).step_by(every).map(|i| (i as f64, c[i].clone())).collect()
            }
        };
        for (v, label) in &xt {
            let px = xp(*v);
            line(&mut body, "ax", px, plot_bottom, px, plot_bottom + 4.0);
            text(&mut body, px, plot_bottom + 18.0, "t3", 11.0, "middle", label);
        }
        // annotations: a hairline and a short label
        for (pos, label) in &spec.annotations {
            let px = xp(*pos);
            line(&mut body, "ref", px, plot_top, px, plot_bottom);
            let anchor_end = px > (x0 + x1) / 2.0;
            text(&mut body, if anchor_end { px - 5.0 } else { px + 5.0 }, plot_top + 12.0, "t2", 11.0, if anchor_end { "end" } else { "start" }, label);
        }
        // marks
        for (si, s) in spec.series.iter().enumerate() {
            let cls = si + 1;
            if spec.kind == Kind::Scatter {
                for (i, v) in s.values.iter().enumerate() {
                    if let Some(v) = v {
                        body.push_str(&format!("<circle class=\"d{}\" cx=\"{}\" cy=\"{}\" r=\"4\"/>\n", cls, f(xp(xs[i])), f(ax.at(*v))));
                    }
                }
                continue;
            }
            let mut d = String::new();
            let mut pen_up = true;
            let mut segs: Vec<Vec<(f64, f64)>> = vec![];
            for (i, v) in s.values.iter().enumerate() {
                match v {
                    Some(v) => {
                        let p = (xp(xs[i]), ax.at(*v));
                        if pen_up {
                            segs.push(vec![]);
                            pen_up = false;
                        }
                        if let Some(sg) = segs.last_mut() {
                            sg.push(p);
                        }
                    }
                    None => pen_up = true,
                }
            }
            for sg in &segs {
                for (pi, p) in sg.iter().enumerate() {
                    d.push_str(&format!("{}{},{} ", if pi == 0 { "M" } else { "L" }, f(p.0), f(p.1)));
                }
            }
            if spec.kind == Kind::Area {
                for sg in &segs {
                    if sg.len() > 1 {
                        let zero = ax.at(0f64.clamp(ax.lo, ax.hi));
                        let mut a = format!("M{},{} ", f(sg[0].0), f(zero));
                        for p in sg {
                            a.push_str(&format!("L{},{} ", f(p.0), f(p.1)));
                        }
                        a.push_str(&format!("L{},{} Z", f(sg[sg.len() - 1].0), f(zero)));
                        body.push_str(&format!("<path class=\"a{}\" d=\"{}\"/>\n", cls, a));
                    }
                }
            }
            body.push_str(&format!("<path class=\"k{}\" d=\"{}\"/>\n", cls, d.trim_end()));
            if let Some(p) = segs.iter().rev().find_map(|sg| sg.last()) {
                body.push_str(&format!("<circle class=\"d{}\" cx=\"{}\" cy=\"{}\" r=\"4\"/>\n", cls, f(p.0), f(p.1)));
            }
        }
        // direct labels at the line ends, only if none would touch another
        let mut placed: Vec<(f64, String)> = vec![];
        if direct {
            for (si, v, label) in &last_labels {
                let _ = si;
                placed.push((ax.at(*v), label.clone()));
            }
            placed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        }
        let collide = placed.windows(2).any(|w| w[1].0 - w[0].0 < 15.0);
        let labelled = direct && !collide;
        if labelled {
            for (py, label) in &placed {
                text(&mut body, x1 + 10.0, py + 4.0, "t2", 11.0, "start", label);
            }
        }
        want_table = needs_relief && spec.kind != Kind::Scatter && !labelled;
    }

    // --- the values table, appended below when the marks could not carry the values
    let mut tbl = String::new();
    if want_table {
        let rows = table_rows(spec, &fmt);
        let th = table(&mut tbl, &rows, 0.0, w, spec);
        extra_h = th + 14.0;
    }
    let h = base_h + extra_h;

    // --- assemble
    let mut svg = String::with_capacity(body.len() + 4096);
    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    if let Some(c) = comment {
        svg.push_str(&format!("<!-- {} -->\n", c));
    }
    let desc = describe(spec, &fmt);
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"viz-root\"{} viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\" aria-labelledby=\"chart-title chart-desc\">\n",
        match spec.theme {
            Theme::Auto => "",
            Theme::Light => " data-theme=\"light\"",
            Theme::Dark => " data-theme=\"dark\"",
        },
        w = f(w),
        h = f(h)
    ));
    svg.push_str(&format!("<title id=\"chart-title\">{}</title>\n<desc id=\"chart-desc\">{}</desc>\n<style>\n{}</style>\n", escape(&spec.title), escape(&desc), style()));
    svg.push_str(&format!("<rect class=\"surface\" width=\"{}\" height=\"{}\"/>\n", f(w), f(h)));
    svg.push_str(&head);
    svg.push_str(&body);
    if want_table {
        svg.push_str(&format!("<g transform=\"translate(0,{})\">\n{}</g>\n", f(plot_bottom + x_axis_h + 24.0), tbl));
    }
    let mut fy = h - PAD - foot.len().saturating_sub(1) as f64 * 15.0;
    for line_text in &foot {
        text(&mut svg, PAD, fy, "t3", 11.0, "start", &trunc(line_text, w - 2.0 * PAD, 11.0));
        fy += 15.0;
    }
    svg.push_str("</svg>\n");
    svg
}

/// Equal-width bins over the values: (counts, edges).
fn histogram(s: &Series, bins: Option<usize>) -> (Vec<f64>, Vec<f64>) {
    let v: Vec<f64> = s.values.iter().flatten().copied().collect();
    let n = v.len() as f64;
    let (lo, hi) = (v.iter().cloned().fold(f64::INFINITY, f64::min), v.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    let b = bins.unwrap_or_else(|| ((n.log2().ceil() + 1.0) as usize).clamp(5, 40));
    let (lo, hi, _) = if hi > lo { let (a, c, t) = linear_ticks(lo, hi, b); (a, c, t) } else { (lo - 0.5, hi + 0.5, vec![]) };
    let width = (hi - lo) / b as f64;
    let mut counts = vec![0.0; b];
    for x in v {
        let i = (((x - lo) / width) as usize).min(b - 1);
        counts[i] += 1.0;
    }
    (counts, (0..=b).map(|i| lo + i as f64 * width).collect())
}
