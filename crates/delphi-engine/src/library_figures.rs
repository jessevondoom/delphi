//! Agent-made figures in generated pages (`specs/118` §5; slice P5).
//!
//! The writer may ask for a chart or diagram in a draft using a fenced `figure` block naming the kind, its data rows or
//! nodes/edges (the 095 tools' JSON format), and the claim or source markers the data came from (`from`). The daemon:
//! 1. checks every marker against the library/records at the page's scope (never private on an external page);
//! 2. for a chart, verifies every number in its data appears in one of the cited records named in `from`;
//! 3. for a diagram, verifies every node label appears in the page text outside figure blocks;
//! 4. renders the SVG via `delphi_docs::{chart, diagram}` and rasterizes it to PNG via `delphi_docs::raster`;
//! 5. stores the PNG under `library/figures/` with an asset record (`licence: "generated"`, `made_by: <role>`, `markers`)
//!    and puts `![alt](lib:_figures/<hash>.png)` in the page (followed by `Figure made by <role> from [markers]. Not from a source.`
//!    when `with_credits` writes the page's daemon lines).
//!
//! Any figure that fails validation, number/label verification or rendering is dropped from the page and the failure is
//! recorded. A figure is never presented as coming from a source. A `lib:_figures/` reference the writer typed itself
//! (not asked for with a figure block) is dropped too: a figure is checked against the page it is made for, never reused.

use super::assets::{one_line, FIGURES_BUCKET, GENERATED_LICENCE};
use super::page_check::{parse_marker_list, strip_markers, Cite, Rec, Scope};
use super::Library;
use delphi_docs::{chart, diagram, pixels, raster};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Most figures rendered on one generated page.
pub const MAX_FIGURES_PER_PAGE: usize = 6;
/// Maximum PNG size for a rendered figure (2 MiB, matching `artifacts::MAX_PNG_BYTES`).
const MAX_FIGURE_BYTES: usize = 2 * 1024 * 1024;
const MAX_MARKERS_PER_FIGURE: usize = 20;
/// The address prefix of an agent-made figure; only the daemon writes it into a page.
const TYPED_FIGURE_REF: &str = "lib:_figures/";

const CHART_TYPES: &[&str] = &[
    "bar", "column", "hbar", "line", "area", "scatter", "hist", "histogram", "stacked-bar", "stacked", "donut", "pie", "heatmap", "box", "boxplot", "sparkline",
];
const DIAGRAM_TYPES: &[&str] = &["flow", "flowchart", "tree", "hierarchy", "sequence", "timeline", "matrix"];

/// A figure that failed validation, number/label checks or rendering and was dropped from the page (`specs/118` §5 item 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FigureFailure {
    pub kind: String,
    pub title: String,
    pub reason: String,
}

impl std::fmt::Display for FigureFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let who = if self.title.is_empty() { self.kind.as_str() } else { self.title.as_str() };
        write!(f, "{} figure `{}` dropped: {}", self.kind, who, self.reason)
    }
}

/// Outcome of resolving all ` ```figure ` blocks in a page draft.
#[derive(Debug, Default)]
pub struct ResolvedFigures {
    pub body: String,
    pub failures: Vec<FigureFailure>,
    pub assets: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Chart,
    Diagram,
}

enum Segment<'a> {
    Text(&'a str),
    Figure(String),
    Unclosed,
}

/// Split `draft` into plain markdown lines and ` ```figure ... ``` ` blocks, ignoring ` ```figure ` inside another code fence.
fn split_blocks(draft: &str) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let lines: Vec<&str> = draft.split_inclusive('\n').collect();
    let mut i = 0;
    let mut other_fence: Option<(char, usize)> = None;
    let mut text_start = 0usize;
    let mut byte_pos = 0usize;

    fn fence_of(line: &str) -> Option<(char, usize, &str)> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let lead = trimmed.len() - trimmed.trim_start_matches(' ').len();
        if lead > 3 {
            return None;
        }
        let rest = &trimmed[lead..];
        let ch = rest.bytes().next().filter(|&b| b == b'`' || b == b'~')? as char;
        let len = rest.bytes().take_while(|&b| b as char == ch).count();
        (len >= 3).then_some((ch, len, rest[len..].trim()))
    }

    while i < lines.len() {
        let line = lines[i];
        let line_end = byte_pos + line.len();
        if let Some((fch, flen)) = other_fence {
            if let Some((c, l, info)) = fence_of(line) {
                if c == fch && l >= flen && info.is_empty() {
                    other_fence = None;
                }
            }
            byte_pos = line_end;
            i += 1;
            continue;
        }
        if let Some((ch, len, info)) = fence_of(line) {
            if ch == '`' && info.eq_ignore_ascii_case("figure") {
                if text_start < byte_pos {
                    out.push(Segment::Text(&draft[text_start..byte_pos]));
                }
                let mut body = String::new();
                let mut cur_end = line_end;
                let mut closed = false;
                i += 1;
                while i < lines.len() {
                    let fline = lines[i];
                    cur_end += fline.len();
                    if let Some((c, l, finfo)) = fence_of(fline) {
                        if c == '`' && l >= len && finfo.is_empty() {
                            closed = true;
                            i += 1;
                            break;
                        }
                    }
                    body.push_str(fline);
                    i += 1;
                }
                if closed {
                    out.push(Segment::Figure(body));
                } else {
                    out.push(Segment::Unclosed);
                }
                text_start = cur_end;
                byte_pos = cur_end;
                continue;
            }
            other_fence = Some((ch, len));
        }
        byte_pos = line_end;
        i += 1;
    }
    if text_start < draft.len() {
        out.push(Segment::Text(&draft[text_start..]));
    }
    out
}

/// Extract numeric tokens from prose/record text with boundary rules: handles leading `-` when not preceded by alphanumeric
/// or `.` (so `2024-2025` yields `2024` and `2025`), thousands commas `,ddd` (so `1,200` yields `1200` while `1,2,3` yields
/// `1`, `2`, `3`), and decimals `.ddd` (trailing periods are ignored).
pub(crate) fn extract_numbers(text: &str) -> Vec<f64> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let neg = bytes[i] == b'-' && (i == 0 || (!bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'.')) && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit();
        if bytes[i].is_ascii_digit() || neg {
            let mut buf = String::new();
            if neg {
                buf.push('-');
                i += 1;
            }
            while i < bytes.len() {
                if bytes[i].is_ascii_digit() {
                    buf.push(bytes[i] as char);
                    i += 1;
                } else if bytes[i] == b',' && i + 3 < bytes.len() && bytes[i + 1].is_ascii_digit() && bytes[i + 2].is_ascii_digit() && bytes[i + 3].is_ascii_digit() && !bytes.get(i + 4).is_some_and(|b| b.is_ascii_digit()) {
                    i += 1;
                } else {
                    break;
                }
            }
            if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
                buf.push('.');
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    buf.push(bytes[i] as char);
                    i += 1;
                }
            }
            if let Some(v) = buf.parse::<f64>().ok().filter(|x| x.is_finite()) {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

fn numbers_match(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn format_num(v: f64) -> String {
    if v.fract().abs() < 1e-12 && v.abs() < 1e15 {
        format!("{:.0}", v)
    } else {
        v.to_string()
    }
}

fn norm_words(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `phrase` appears as a contiguous word sequence in `text` (case-insensitive, punctuation-insensitive).
pub(crate) fn phrase_in_text(phrase: &str, text: &str) -> bool {
    let pw = norm_words(phrase);
    if pw.is_empty() {
        let p = phrase.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        let t = text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        return !p.is_empty() && t.contains(&p);
    }
    let tw = norm_words(text);
    format!(" {} ", tw).contains(&format!(" {} ", pw))
}

fn parse_num_cell(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite()),
        Value::String(s) => {
            let t: String = s.trim().chars().filter(|c| !matches!(c, ',' | '$' | '%' | ' ')).collect();
            if t.is_empty() { None } else { t.parse::<f64>().ok().filter(|x| x.is_finite()) }
        }
        _ => None,
    }
}

/// Every number in the chart's data rows, series, box groups, numeric x-axis and numeric annotations.
fn chart_numbers(spec_json: &Value, spec: &chart::ChartSpec) -> Vec<f64> {
    let mut nums = Vec::new();
    if let Some(rows) = spec_json.pointer("/data/rows").and_then(Value::as_array) {
        for r in rows {
            if let Some(cells) = r.as_array() {
                for cell in cells {
                    if let Some(n) = parse_num_cell(cell) {
                        nums.push(n);
                    }
                }
            }
        }
    }
    for s in &spec.series {
        nums.extend(s.values.iter().flatten().copied());
    }
    for g in &spec.groups {
        nums.extend(g.iter().copied());
    }
    if let chart::XAxis::Numbers(xs) = &spec.x {
        nums.extend(xs.iter().copied());
    }
    if let Some(ann) = spec_json.get("annotations").and_then(Value::as_array) {
        for a in ann {
            if let Some(x) = a.get("x").and_then(parse_num_cell) {
                nums.push(x);
            }
        }
    }
    nums
}

/// The free text a chart draws (title, subtitle, column names, annotation labels): a number written there is a claim
/// drawn in the picture, so it is held to the same rule as the data.
fn chart_text(spec_json: &Value, spec: &chart::ChartSpec) -> Vec<(&'static str, String)> {
    let mut out = vec![("title", spec.title.clone())];
    if let Some(s) = &spec.subtitle {
        out.push(("subtitle", s.clone()));
    }
    if let Some(cols) = spec_json.pointer("/data/columns").and_then(Value::as_array) {
        out.extend(cols.iter().filter_map(Value::as_str).map(|c| ("column name", c.to_string())));
    }
    out.extend(spec.annotations.iter().map(|(_, l)| ("annotation", l.clone())));
    out
}

/// Parse the `from` (or `markers` / `cites` / `sources`) field of a figure spec into deduplicated `Cite`s.
fn parse_from_markers(v: &Value) -> Result<Vec<Cite>, String> {
    let raw = v.get("from").or_else(|| v.get("markers")).or_else(|| v.get("cites")).or_else(|| v.get("sources")).ok_or_else(|| "figure spec must name cited markers in `from`".to_string())?;
    let items: Vec<&str> = match raw {
        Value::Array(a) => a.iter().map(|x| x.as_str().ok_or_else(|| "each `from` entry must be a marker string".to_string())).collect::<Result<_, _>>()?,
        Value::String(s) => vec![s.as_str()],
        _ => return Err("`from` must be an array of marker strings".into()),
    };
    let mut out = Vec::new();
    for item in items {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let wrapped = if trimmed.starts_with('[') && trimmed.ends_with(']') { trimmed.to_string() } else { format!("[{}]", trimmed) };
        let parsed = parse_marker_list(&wrapped).ok_or_else(|| format!("invalid citation marker `{}`", trimmed))?;
        for c in parsed {
            if !out.iter().any(|x: &Cite| x.label() == c.label()) {
                out.push(c);
            }
        }
    }
    if out.is_empty() {
        return Err("figure spec must name at least one citation marker in `from`".into());
    }
    if out.len() > MAX_MARKERS_PER_FIGURE {
        return Err(format!("figure names {} markers; at most {} are allowed", out.len(), MAX_MARKERS_PER_FIGURE));
    }
    Ok(out)
}

struct NormalizedSpec {
    family: Family,
    spec: Value,
    title: String,
    alt: String,
}

/// Normalize the writer's JSON before passing it to `chart` or `diagram`.
fn normalize_spec(v: &Value) -> Result<NormalizedSpec, FigureFailure> {
    let fail = |kind: &str, title: String, reason: String| FigureFailure { kind: kind.to_string(), title, reason };
    let obj = v.as_object().ok_or_else(|| fail("unknown", String::new(), "figure spec must be a JSON object".into()))?;
    let outer_kind = obj.get("kind").and_then(Value::as_str).map(|s| s.trim().to_ascii_lowercase());
    let mut spec = match outer_kind.as_deref() {
        Some("chart") if obj.get("type").is_none() && obj.get("chart").is_some_and(Value::is_object) => obj["chart"].clone(),
        Some("diagram") if obj.get("type").is_none() && obj.get("diagram").is_some_and(Value::is_object) => obj["diagram"].clone(),
        _ => v.clone(),
    };
    let title_hint = spec["title"].as_str().or_else(|| obj.get("title").and_then(Value::as_str)).unwrap_or("").trim().to_string();
    let ty = match spec.get("type").and_then(Value::as_str) {
        Some(t) => t.trim().to_ascii_lowercase(),
        None => match outer_kind.as_deref().filter(|k| *k != "chart" && *k != "diagram") {
            Some(k) => {
                spec["type"] = json!(k);
                k.to_string()
            }
            None => return Err(fail("unknown", title_hint, "figure spec needs a chart or diagram `type`".into())),
        },
    };
    let family = if CHART_TYPES.contains(&ty.as_str()) {
        Family::Chart
    } else if DIAGRAM_TYPES.contains(&ty.as_str()) {
        Family::Diagram
    } else {
        return Err(fail("unknown", title_hint, format!("unknown figure type `{}`", ty)));
    };
    let kind_name = match family {
        Family::Chart => "chart",
        Family::Diagram => "diagram",
    };
    if let Some(k) = outer_kind.as_deref().filter(|k| *k == "chart" || *k == "diagram") {
        if k != kind_name {
            return Err(fail(kind_name, title_hint, format!("`kind` is `{}` but `type` `{}` is a {}", k, ty, kind_name)));
        }
    }
    if let Some(m) = spec.as_object_mut() {
        // A generated figure never presents itself as coming from a source (`specs/118` §9).
        m.remove("source");
    }
    if family == Family::Chart && spec.pointer("/data/from").is_some() {
        return Err(fail("chart", title_hint, "page figures must provide inline `data` rows, not `data.from` files".into()));
    }
    let alt = obj.get("alt").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).unwrap_or(&title_hint).to_string();
    Ok(NormalizedSpec { family, spec, title: title_hint, alt })
}

/// Resolve markers against the library/records at `scope` and collect their record texts (`Rec::Claim`, `Rec::Source`, `Rec::Record`).
fn resolve_record_texts(lib: &Library, cites: &[Cite], scope: Scope) -> Result<(Vec<String>, String), String> {
    let mut labels = Vec::with_capacity(cites.len());
    let mut texts = String::new();
    for c in cites {
        let rec = lib.resolve(c, scope).map_err(|f| f.to_string())?;
        labels.push(c.label());
        match rec {
            Rec::Claim(cl) => {
                texts.push_str(cl["text"].as_str().unwrap_or(""));
                texts.push('\n');
            }
            Rec::Source(id, _) => {
                let t = lib.stored_text(&id).map_err(|f| f.to_string())?;
                texts.push_str(&t);
                texts.push('\n');
            }
            Rec::Record(r) => {
                texts.push_str(&r.text);
                texts.push('\n');
            }
        }
    }
    Ok((labels, texts))
}

impl Library {
    pub fn figures_dir(&self) -> PathBuf {
        self.root.join("figures")
    }

    /// Look up the asset licence record of a generated figure `lib:_figures/<file>` (`specs/118` §5 item 2).
    pub(super) fn figure_asset_record(&self, file: &str) -> Option<Value> {
        let plain = !file.is_empty() && !file.contains(['/', '\\']) && file != "." && file != "..";
        let stem = file.strip_suffix(".png")?;
        if !plain || stem.is_empty() || !self.figures_dir().join(file).is_file() {
            return None;
        }
        let rec: Value = serde_json::from_slice(&std::fs::read(self.figures_dir().join(format!("{}.json", stem))).ok()?).ok()?;
        let valid = rec["file"].as_str() == Some(file)
            && rec["licence"].as_str() == Some(GENERATED_LICENCE)
            && rec["made_by"].as_str().is_some_and(|s| !s.trim().is_empty())
            && rec["markers"].as_array().is_some_and(|a| !a.is_empty() && a.iter().all(|x| x.as_str().is_some_and(|s| !s.trim().is_empty())));
        valid.then_some(rec)
    }

    /// The daemon's label line directly under a generated figure `lib:_figures/<file>` (`specs/118` §5 item 2).
    pub fn figure_label(&self, file: &str) -> Option<String> {
        let rec = self.figure_asset_record(file)?;
        let made_by = rec["made_by"].as_str()?;
        let markers: Vec<&str> = rec["markers"].as_array()?.iter().filter_map(Value::as_str).collect();
        (!markers.is_empty()).then(|| format!("Figure made by {} from [{}]. Not from a source.", made_by, markers.join(", ")))
    }

    /// Store PNG bytes and asset record under `library/figures/` and return `(uri, alt)`.
    fn store_figure_asset(&self, png: &[u8], kind: &str, title: &str, alt: &str, made_by: &str, markers: &[String]) -> Result<(String, String), String> {
        let png_hash = blake3::hash(png).to_hex().to_string();
        let pic = pixels::decode(png).map_err(|e| e.0)?;
        let made_by = one_line(made_by, 60);
        if made_by.is_empty() {
            return Err("figure requires a non-empty role id".into());
        }
        let mut h = blake3::Hasher::new();
        h.update(png);
        h.update(b"\0");
        h.update(made_by.as_bytes());
        h.update(b"\0");
        h.update(markers.join(",").as_bytes());
        let id_hex = h.finalize().to_hex().to_string();
        let file = format!("{}.png", &id_hex[..32]);
        let dir = self.figures_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("could not create figures directory: {}", e))?;
        let img_path = dir.join(&file);
        if !img_path.exists() {
            std::fs::write(&img_path, png).map_err(|e| format!("could not write figure PNG: {}", e))?;
        }
        let rec = json!({
            "file": file,
            "licence": GENERATED_LICENCE,
            "made_by": made_by,
            "markers": markers,
            "kind": kind,
            "title": one_line(title, 200),
            "retrieved": delphi_core::time::now_rfc3339(),
            "blake3": png_hash,
            "width": pic.width,
            "height": pic.height,
        });
        let rec_path = dir.join(format!("{}.json", &id_hex[..32]));
        self.write_json(&rec_path, &rec)?;
        let alt = one_line(alt, 200);
        let alt = if alt.is_empty() { "Figure".to_string() } else { alt };
        Ok((format!("lib:{}/{}", FIGURES_BUCKET, file), alt))
    }

    fn render_one_figure(&self, raw_json: &str, scope: Scope, made_by: &str, page_text: &str) -> Result<(String, String), FigureFailure> {
        let fail = |kind: &str, title: &str, reason: String| FigureFailure { kind: kind.to_string(), title: title.to_string(), reason };
        let v: Value = serde_json::from_str(raw_json).map_err(|e| fail("unknown", "", format!("invalid figure JSON: {}", e)))?;
        let NormalizedSpec { family, spec: spec_json, title: title_hint, alt } = normalize_spec(&v)?;
        let kind_name = if family == Family::Chart { "chart" } else { "diagram" };
        let cites = parse_from_markers(&v).map_err(|r| fail(kind_name, &title_hint, r))?;
        let (marker_labels, record_texts) = resolve_record_texts(self, &cites, scope).map_err(|r| fail(kind_name, &title_hint, r))?;

        let (title, png) = match family {
            Family::Chart => {
                let spec = chart::from_json(&spec_json).map_err(|e| fail(kind_name, &title_hint, e.0))?;
                chart::png_fit(&spec).map_err(|e| fail(kind_name, &spec.title, e.0))?;
                let rec_nums = extract_numbers(&record_texts);
                for n in chart_numbers(&spec_json, &spec) {
                    if !rec_nums.iter().any(|&r| numbers_match(r, n)) {
                        return Err(fail(kind_name, &spec.title, format!("chart number {} does not appear in any cited record ({})", format_num(n), marker_labels.join(", "))));
                    }
                }
                for (place, text) in chart_text(&spec_json, &spec) {
                    for n in extract_numbers(&text) {
                        if !rec_nums.iter().any(|&r| numbers_match(r, n)) {
                            return Err(fail(kind_name, &spec.title, format!("chart number {} (in its {}) does not appear in any cited record ({})", format_num(n), place, marker_labels.join(", "))));
                        }
                    }
                }
                let svg = chart::render_svg(&spec, None);
                let dark = spec.theme == chart::Theme::Dark;
                let png = raster::svg_to_png(&svg, dark, 2, &[("Title", &spec.title), ("Software", "delphi figure")]).map_err(|e| fail(kind_name, &spec.title, e.0))?;
                (spec.title, png)
            }
            Family::Diagram => {
                let spec = diagram::from_json(&spec_json).map_err(|e| fail(kind_name, &title_hint, e.0))?;
                diagram::png_fit(&spec).map_err(|e| fail(kind_name, &spec.title, e.0))?;
                for label in spec.node_labels() {
                    if !phrase_in_text(&label, page_text) {
                        return Err(fail(kind_name, &spec.title, format!("diagram node label `{}` does not appear in the page text", label)));
                    }
                }
                let svg = diagram::render_svg(&spec, None);
                let dark = spec.theme == chart::Theme::Dark;
                let png = raster::svg_to_png(&svg, dark, 2, &[("Title", &spec.title), ("Software", "delphi figure")]).map_err(|e| fail(kind_name, &spec.title, e.0))?;
                (spec.title, png)
            }
        };
        if png.len() > MAX_FIGURE_BYTES {
            return Err(fail(kind_name, &title, format!("rendered PNG is {} bytes, over the {}-byte cap", png.len(), MAX_FIGURE_BYTES)));
        }
        self.store_figure_asset(&png, kind_name, &title, &alt, made_by, &marker_labels).map_err(|r| fail(kind_name, &title, r))
    }

    /// Parse, verify and render every ` ```figure ` block in `draft`. Passing figures are stored under `library/figures/` and
    /// replaced with `![alt](lib:_figures/<hash>.png)`; failing figures are dropped and recorded (`specs/118` §5 item 3).
    pub fn resolve_figures(&self, draft: &str, scope: Scope, made_by: &str) -> ResolvedFigures {
        let segments = split_blocks(draft);
        let typed_ref = |t: &str| t.contains(TYPED_FIGURE_REF);
        if !segments.iter().any(|s| matches!(s, Segment::Figure(_) | Segment::Unclosed) || matches!(s, Segment::Text(t) if typed_ref(t))) {
            return ResolvedFigures { body: draft.to_string(), failures: Vec::new(), assets: Vec::new() };
        }
        let page_text: String = segments
            .iter()
            .filter_map(|s| if let Segment::Text(t) = s { Some(*t) } else { None })
            .collect::<Vec<_>>()
            .join("\n");
        let page_text_stripped = strip_markers(&page_text);

        let mut out = String::with_capacity(draft.len());
        let mut failures = Vec::new();
        let mut assets = Vec::new();
        let mut count = 0usize;

        for seg in segments {
            match seg {
                Segment::Text(t) if typed_ref(t) => {
                    for line in t.split_inclusive('\n') {
                        if typed_ref(line) {
                            failures.push(FigureFailure { kind: "unknown".into(), title: String::new(), reason: "a figure is asked for with a figure block; a `lib:_figures/` reference written in the text is dropped".into() });
                        } else {
                            out.push_str(line);
                        }
                    }
                }
                Segment::Text(t) => out.push_str(t),
                Segment::Unclosed => {
                    failures.push(FigureFailure { kind: "unknown".into(), title: String::new(), reason: "unclosed figure block".into() });
                }
                Segment::Figure(raw) => {
                    count += 1;
                    if count > MAX_FIGURES_PER_PAGE {
                        failures.push(FigureFailure { kind: "unknown".into(), title: String::new(), reason: format!("more than {} figures on one page", MAX_FIGURES_PER_PAGE) });
                        continue;
                    }
                    match self.render_one_figure(&raw, scope, made_by, &page_text_stripped) {
                        Ok((uri, alt)) => {
                            if !out.is_empty() && !out.ends_with('\n') {
                                out.push('\n');
                            }
                            out.push_str(&format!("![{}]({})\n", alt, uri));
                            assets.push(uri);
                        }
                        Err(f) => failures.push(f),
                    }
                }
            }
        }
        // The title and reason carry the writer's words: one plain line each before a log, a sidecar or a terminal.
        let failures: Vec<FigureFailure> = failures.into_iter().map(|f| FigureFailure { kind: f.kind, title: one_line(&f.title, 120), reason: one_line(&f.reason, 300) }).collect();
        if !failures.is_empty() {
            self.log("figure_dropped", json!({"failures": failures}));
        }
        ResolvedFigures { body: collapse_blank_runs(&out), failures, assets }
    }
}

pub(super) fn has_figure_blocks(body: &str) -> bool {
    split_blocks(body).iter().any(|s| matches!(s, Segment::Figure(_) | Segment::Unclosed))
}

fn collapse_blank_runs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blanks = 0usize;
    for line in s.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks <= 1 && !out.is_empty() {
                out.push('\n');
            }
        } else {
            blanks = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(text: &str, n: f64) -> bool {
        extract_numbers(text).iter().any(|&r| numbers_match(r, n))
    }

    #[test]
    fn a_number_matches_only_a_whole_number_in_the_text_never_a_part_of_one() {
        // `1` is inside `2019` and `120`, `20` inside `2019` and `120`, `0.5` inside `10.55`: none of them is found.
        let t = "In 2019 the median was 120 ms and the p99 was 10.55 s.";
        for absent in [1.0, 2.0, 9.0, 19.0, 20.0, 201.0, 0.5, 10.5, 55.0] {
            assert!(!has(t, absent), "{absent} must not be found in {t:?}: {:?}", extract_numbers(t));
        }
        for present in [2019.0, 120.0, 99.0, 10.55] {
            assert!(has(t, present), "{present} must be found in {t:?}");
        }
    }

    #[test]
    fn thousands_ranges_signs_and_trailing_periods_read_as_people_write_them() {
        assert!(has("about 1,200 sites", 1200.0) && !has("about 1,200 sites", 200.0));
        assert!(has("values 1,2,3", 2.0) && !has("values 1,2,3", 123.0));
        assert!(has("from 2024-2025", 2024.0) && has("from 2024-2025", 2025.0) && !has("from 2024-2025", -2025.0));
        assert!(has("it fell to -4.5 degrees.", -4.5) && !has("it fell to -4.5 degrees.", 4.5));
        assert!(has("ended at 42.", 42.0));
        assert!(has("3.0 percent", 3.0));
    }

    #[test]
    fn a_node_label_must_appear_as_whole_words() {
        assert!(phrase_in_text("Fetch stage", "The fetch-stage runs first."));
        assert!(!phrase_in_text("Fetch", "Prefetching runs first."));
        assert!(!phrase_in_text("Ghost Node", "A ghost appears; a node too."));
    }

    #[test]
    fn a_figure_block_inside_another_fence_is_text_and_an_unclosed_one_is_flagged() {
        let quoted = "# T\n\n````markdown\n```figure\n{}\n```\n````\n";
        assert!(!has_figure_blocks(quoted));
        assert!(has_figure_blocks("# T\n\n```figure\n{\"type\": \"bar\"}\n"));
        assert!(has_figure_blocks("# T\n\n```figure\n{}\n```\n"));
    }
}
