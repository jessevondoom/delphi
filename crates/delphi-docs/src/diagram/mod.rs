//! Diagrams from a description (`specs/014` §8.5): flows, trees, sequences, timelines and matrices. The agent says
//! what the things are and how they connect; the layout is computed, never drawn by hand, and it is deterministic
//! (layered ordering with fixed tie-breaks, no simulation), so the same description always gives the same picture.
//!
//! A diagram is an SVG in the same vocabulary as a chart (same palette, light and dark, text in ink tokens, accessible
//! title and description), so it embeds in pages and decks, and becomes a PNG through the same rasterizer.

mod flow;
mod sequence;
#[cfg(test)]
mod tests;
mod tree;

use crate::chart::palette::{mix, SERIES_DARK, SERIES_LIGHT, SURFACE_DARK, SURFACE_LIGHT};
use crate::chart::{css_custom, text, trunc, tw, Theme, PAD};
use crate::markdown::escape;
use crate::DocError;
use serde_json::Value;

const MAX_PNG_W: f64 = 1800.0;
const MAX_PNG_H: f64 = 2400.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Flow,
    Tree,
    Sequence,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Flow => "flow",
            Kind::Tree => "tree",
            Kind::Sequence => "sequence",
        }
    }
}

#[derive(Debug, Clone)]
enum Body {
    Flow(flow::Flow),
    Tree(tree::Tree),
    Sequence(sequence::Sequence),
}

#[derive(Debug, Clone)]
pub struct DiagramSpec {
    pub kind: Kind,
    pub title: String,
    pub subtitle: Option<String>,
    pub source: Option<String>,
    pub theme: Theme,
    body: Body,
}

/// A laid-out diagram: its size, its marks and the words that describe it for anyone who cannot see them.
pub(crate) struct Scene {
    pub w: f64,
    pub h: f64,
    pub body: String,
    pub desc: String,
}

pub(crate) fn bad(msg: impl Into<String>) -> DocError {
    DocError(msg.into())
}

fn str_field<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

/// A required or optional short text field, trimmed, with a character limit.
pub(crate) fn short(v: &Value, k: &str, max: usize, what: &str) -> Result<Option<String>, DocError> {
    match str_field(v, k).map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) if s.chars().count() > max => Err(bad(format!("{} is over {} characters", what, max))),
        Some(s) => Ok(Some(s.to_string())),
        None => Ok(None),
    }
}

/// Read and check a diagram. The error says what is wrong and what would work.
pub fn from_json(v: &Value) -> Result<DiagramSpec, DocError> {
    let kind = str_field(v, "type").ok_or_else(|| bad("a diagram needs a `type`: flow, tree or sequence"))?;
    let title = short(v, "title", 140, "the title")?.ok_or_else(|| bad("a diagram needs a `title` that says what it shows"))?;
    let subtitle = short(v, "subtitle", 200, "the subtitle")?;
    let source = short(v, "source", 300, "`source`")?;
    let theme = match str_field(v, "theme").unwrap_or("auto") {
        "auto" => Theme::Auto,
        "light" => Theme::Light,
        "dark" => Theme::Dark,
        _ => return Err(bad("`theme` is auto, light or dark")),
    };
    let (kind, body) = match kind {
        "flow" | "flowchart" => (Kind::Flow, Body::Flow(flow::parse(v)?)),
        "tree" | "hierarchy" => (Kind::Tree, Body::Tree(tree::parse(v)?)),
        "sequence" => (Kind::Sequence, Body::Sequence(sequence::parse(v)?)),
        "timeline" | "matrix" => return Err(bad(format!("`{}` diagrams are not available yet; use flow, tree or sequence (or a `table` block for a matrix)", kind))),
        other => return Err(bad(format!("unknown diagram type `{}`; use flow, tree or sequence", other))),
    };
    Ok(DiagramSpec { kind, title, subtitle, source, theme, body })
}

/// Group names for the legend (flows colour nodes by group).
fn groups(spec: &DiagramSpec) -> &[String] {
    match &spec.body {
        Body::Flow(f) => &f.groups,
        _ => &[],
    }
}

fn top_offset(spec: &DiagramSpec) -> f64 {
    PAD + 28.0 + if spec.subtitle.is_some() { 20.0 } else { 0.0 } + if groups(spec).is_empty() { 0.0 } else { 26.0 } + 10.0
}

fn layout(spec: &DiagramSpec) -> Scene {
    let top = top_offset(spec);
    match &spec.body {
        Body::Flow(f) => flow::layout(f, top),
        Body::Tree(t) => tree::layout(t, top),
        Body::Sequence(s) => sequence::layout(s, top),
    }
}

/// Whether the diagram is small enough to be a readable PNG (the bitmap font cannot be shrunk to fit).
pub fn png_fit(spec: &DiagramSpec) -> Result<(), DocError> {
    let s = layout(spec);
    if s.w > MAX_PNG_W || s.h > MAX_PNG_H {
        return Err(bad(format!("this diagram is {:.0} x {:.0} and a PNG carries at most {:.0} x {:.0} legibly. Write it as .svg (the full-quality form), or split it into smaller diagrams", s.w, s.h, MAX_PNG_W, MAX_PNG_H)));
    }
    Ok(())
}

// ---- drawing

/// Wrap a label into at most `max_lines` lines of about `max_chars` characters, breaking at spaces.
pub(crate) fn wrap(label: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    let mut cur = String::new();
    for word in label.split_whitespace() {
        let word: String = if word.chars().count() > max_chars + 6 { format!("{}…", word.chars().take(max_chars + 5).collect::<String>()) } else { word.to_string() };
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > max_chars {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&word);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            *last = format!("{}…", last.trim_end_matches('…'));
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// An arrowhead with its tip at `(x, y)` pointing along the unit vector `(dx, dy)`.
pub(crate) fn arrow(out: &mut String, class: &str, x: f64, y: f64, dx: f64, dy: f64) {
    use crate::chart::scale::f;
    let (bx, by) = (x - dx * 9.0, y - dy * 9.0);
    let (px, py) = (-dy * 4.5, dx * 4.5);
    out.push_str(&format!("<path class=\"{}\" d=\"M{},{} L{},{} L{},{} Z\"/>\n", class, f(x), f(y), f(bx + px), f(by + py), f(bx - px), f(by - py)));
}

/// Stylesheet variables per mode: the tint a grouped node is filled with (its series colour at 14% on the surface).
fn mode_vars(light: bool) -> String {
    let (series, surface) = if light { (SERIES_LIGHT, SURFACE_LIGHT) } else { (SERIES_DARK, SURFACE_DARK) };
    (0..4).map(|i| format!("--n{}:{};", i + 1, mix(series[i], surface, 0.14))).collect()
}

const RULES: &str = ".nd{fill:var(--surface);stroke:var(--muted);stroke-width:1.5}\n.nd1{fill:var(--n1);stroke:var(--s1);stroke-width:2}.nd2{fill:var(--n2);stroke:var(--s2);stroke-width:2}.nd3{fill:var(--n3);stroke:var(--s3);stroke-width:2}.nd4{fill:var(--n4);stroke:var(--s4);stroke-width:2}\n.ed{stroke:var(--ink2);stroke-width:1.5;fill:none;stroke-linecap:round;stroke-linejoin:round}.ed2{stroke:var(--muted);stroke-width:1.5;fill:none;stroke-linecap:round;stroke-linejoin:round}.ar{fill:var(--ink2)}.ar2{fill:var(--muted)}.ll{stroke:var(--muted);stroke-width:1;fill:none}.bk{fill:var(--surface)}\n@media (forced-colors: active){.nd,.nd1,.nd2,.nd3,.nd4{fill:Canvas;stroke:CanvasText}.ed,.ed2,.ll{stroke:CanvasText}.ar,.ar2{fill:CanvasText}.bk{fill:Canvas}}\n";

/// The stylesheet for diagrams: the chart palette and ink classes, plus the node, edge and lifeline marks.
pub fn css() -> String {
    css_custom(&mode_vars, RULES)
}

fn draw(spec: &DiagramSpec, comment: Option<&str>, inline: Option<&str>) -> String {
    let sc = layout(spec);
    let foot: Vec<String> = spec.source.iter().map(|s| format!("Source: {}", s)).collect();
    let w = sc.w.max(tw(&spec.title, 18.0) + 2.0 * PAD).max(320.0).ceil();
    let foot_h = if foot.is_empty() { 0.0 } else { foot.len() as f64 * 15.0 + 6.0 };
    let h = (sc.h + foot_h).ceil();
    let mut head = String::new();
    text(&mut head, PAD, PAD + 16.0, "t1 b", 18.0, "start", &trunc(&spec.title, w - 2.0 * PAD, 18.0));
    let mut y = PAD + 24.0;
    if let Some(s) = &spec.subtitle {
        text(&mut head, PAD, y + 12.0, "t2", 13.0, "start", &trunc(s, w - 2.0 * PAD, 13.0));
        y += 20.0;
    }
    let gs = groups(spec);
    if !gs.is_empty() {
        let mut x = PAD;
        for (i, g) in gs.iter().enumerate() {
            let label = trunc(g, 160.0, 12.0);
            head.push_str(&format!("<rect class=\"nd{}\" x=\"{}\" y=\"{}\" width=\"12\" height=\"12\" rx=\"3\"/>\n", i + 1, crate::chart::scale::f(x), crate::chart::scale::f(y + 6.0)));
            text(&mut head, x + 18.0, y + 16.0, "t2", 12.0, "start", &label);
            x += tw(&label, 12.0) + 40.0;
        }
    }
    let idp = inline.map(|i| format!("{}-", i)).unwrap_or_default();
    let mut svg = String::with_capacity(sc.body.len() + 4096);
    if inline.is_none() {
        svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        if let Some(c) = comment {
            svg.push_str(&format!("<!-- {} -->\n", c));
        }
    }
    let theme = match spec.theme {
        Theme::Auto => "",
        Theme::Light => " data-theme=\"light\"",
        Theme::Dark => " data-theme=\"dark\"",
    };
    svg.push_str(&format!(
        "<svg{} class=\"viz-root\"{} viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\" aria-labelledby=\"{p}chart-title {p}chart-desc\">\n",
        if inline.is_none() { " xmlns=\"http://www.w3.org/2000/svg\"" } else { "" },
        theme,
        w = crate::chart::scale::f(w),
        h = crate::chart::scale::f(h),
        p = idp
    ));
    svg.push_str(&format!("<title id=\"{p}chart-title\">{}</title>\n<desc id=\"{p}chart-desc\">{}</desc>\n", escape(&spec.title), escape(&sc.desc), p = idp));
    if inline.is_none() {
        svg.push_str(&format!("<style>\n{}</style>\n", css()));
    }
    svg.push_str(&format!("<rect class=\"surface\" width=\"{}\" height=\"{}\"/>\n", crate::chart::scale::f(w), crate::chart::scale::f(h)));
    svg.push_str(&head);
    svg.push_str(&sc.body);
    let mut fy = h - PAD - foot.len().saturating_sub(1) as f64 * 15.0;
    for l in &foot {
        text(&mut svg, PAD, fy, "t3", 11.0, "start", &trunc(l, w - 2.0 * PAD, 11.0));
        fy += 15.0;
    }
    svg.push_str("</svg>\n");
    svg
}

/// The diagram as an SVG document. `comment` is the in-file provenance note (sanitised by the caller).
pub fn render_svg(spec: &DiagramSpec, comment: Option<&str>) -> String {
    draw(spec, comment, None)
}

/// The diagram as an `<svg>` element to place inside a page, with ids prefixed by `id` so several can share a page.
pub fn inline_svg(spec: &DiagramSpec, id: &str) -> String {
    draw(spec, None, Some(id))
}
