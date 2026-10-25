//! Charts from a description (`specs/014` §8). The agent says what the data is and what question the chart
//! answers; this module checks that the chosen form fits, and draws it. The agent never writes SVG.
//!
//! "Form follows the question" is enforced here: a chart type the data does not suit is refused with the
//! reason and a better type, which is the main quality lever. The rules come from the dataviz method: one
//! y axis (never two scales), bars start at zero, a pie is for parts of a whole, a line needs an ordered x,
//! one number is a stat tile rather than a one-bar chart, and past a handful of series you facet or tabulate.

pub mod palette;
mod render;
#[cfg(test)]
pub(crate) mod samples;
pub mod scale;
#[cfg(test)]
mod tests;

use palette::{MAX_SERIES, MAX_SERIES_SCATTER};
use scale::{parse_date, NumFmt};
use serde_json::Value;

use crate::DocError;

pub use render::{css, inline_svg, render_svg};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bar,
    HBar,
    Line,
    Area,
    Scatter,
    Hist,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Bar => "bar",
            Kind::HBar => "hbar",
            Kind::Line => "line",
            Kind::Area => "area",
            Kind::Scatter => "scatter",
            Kind::Hist => "hist",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum XAxis {
    Categories(Vec<String>),
    Numbers(Vec<f64>),
    /// Days since 1970-01-01.
    Dates(Vec<f64>),
}

impl XAxis {
    pub fn len(&self) -> usize {
        match self {
            XAxis::Categories(c) => c.len(),
            XAxis::Numbers(n) | XAxis::Dates(n) => n.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    pub values: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Wide,
    Square,
    Tall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone)]
pub struct ChartSpec {
    pub kind: Kind,
    pub title: String,
    pub subtitle: Option<String>,
    pub x: XAxis,
    pub x_name: String,
    pub series: Vec<Series>,
    pub log_y: bool,
    /// The value axis was told not to start at zero (and the chart says so).
    pub truncated: bool,
    /// (position in the x domain, or the category index; label)
    pub annotations: Vec<(f64, String)>,
    pub y_fmt: NumFmt,
    pub size: Size,
    pub theme: Theme,
    pub source: Option<String>,
    /// Histogram values come from the single series; this is the bin count if given.
    pub bins: Option<usize>,
    /// Plain-language notes shown under the chart (for example "downsampled from 20,000 points").
    pub notes: Vec<String>,
}

const MAX_ROWS: usize = 20_000;
const MAX_SCATTER_POINTS: usize = 5_000;
const MAX_BARS: usize = 400;
const LINE_DOWNSAMPLE_OVER: usize = 2_000;

fn bad(msg: impl Into<String>) -> DocError {
    DocError(msg.into())
}

fn num_cell(v: &Value) -> Result<Option<f64>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| "a number out of range".to_string()),
        Value::String(s) => {
            let t: String = s.trim().chars().filter(|c| !matches!(c, ',' | '$' | '%' | ' ')).collect();
            if t.is_empty() {
                return Ok(None);
            }
            t.parse::<f64>().ok().filter(|x| x.is_finite()).map(Some).ok_or_else(|| format!("`{}` is not a number", s.chars().take(30).collect::<String>()))
        }
        _ => Err("a value that is not a number".to_string()),
    }
}

fn cell_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => scale::f(n.as_f64().unwrap_or(0.0)),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn str_field<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

/// Read and check a chart. The error says what is wrong with the request and, where the form is the problem,
/// which form to use instead.
pub fn from_json(v: &Value) -> Result<ChartSpec, DocError> {
    let kind = match str_field(v, "type").ok_or_else(|| bad("a chart needs a `type`: bar, hbar, line, area, scatter or hist"))? {
        "bar" | "column" => Kind::Bar,
        "hbar" => Kind::HBar,
        "line" => Kind::Line,
        "area" => Kind::Area,
        "scatter" => Kind::Scatter,
        "hist" | "histogram" => Kind::Hist,
        "pie" | "donut" => return Err(bad("pie and donut charts are not available yet. If the parts add up to a whole and there are at most 6 of them, use `hbar` with the shares; for close values a bar is easier to compare anyway")),
        "box" | "heatmap" | "stacked-bar" | "sparkline" | "table" => return Err(bad(format!("`{}` charts are not available yet; use bar, hbar, line, area, scatter or hist (or a table block in write_html)", str_field(v, "type").unwrap_or("")))),
        other => return Err(bad(format!("unknown chart type `{}`; use bar, hbar, line, area, scatter or hist", other))),
    };
    let title = str_field(v, "title").map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| bad("a chart needs a `title`; say what it shows, ideally the point it makes (\"Median latency fell 12% in 0.2.3\")"))?;
    if title.chars().count() > 140 {
        return Err(bad("the title is over 140 characters; a title states the point briefly"));
    }
    let subtitle = str_field(v, "subtitle").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if subtitle.as_ref().is_some_and(|s| s.chars().count() > 200) {
        return Err(bad("the subtitle is over 200 characters"));
    }

    // --- data
    let data = v.get("data").ok_or_else(|| bad("a chart needs `data`: {columns: [...], rows: [[...], ...]}"))?;
    let columns: Vec<String> = data.get("columns").and_then(|c| c.as_array()).ok_or_else(|| bad("`data` needs a `columns` array of names"))?.iter().map(cell_text).collect();
    if columns.is_empty() || columns.iter().any(|c| c.trim().is_empty()) || columns.iter().enumerate().any(|(i, c)| columns[..i].contains(c)) {
        return Err(bad("`columns` must be non-empty, distinct names"));
    }
    let rows_raw = data.get("rows").and_then(|r| r.as_array()).ok_or_else(|| bad("`data` needs a `rows` array of arrays"))?;
    if rows_raw.is_empty() {
        return Err(bad("the chart has no data rows"));
    }
    if rows_raw.len() > MAX_ROWS {
        return Err(bad(format!("{} rows is too many to chart; summarise it, or use `hist` for a distribution (limit {})", rows_raw.len(), MAX_ROWS)));
    }
    let mut rows: Vec<&Vec<Value>> = Vec::with_capacity(rows_raw.len());
    for (i, r) in rows_raw.iter().enumerate() {
        rows.push(r.as_array().ok_or_else(|| bad(format!("row {} is not an array", i + 1)))?);
    }
    let col = |name: &str| columns.iter().position(|c| c == name).ok_or_else(|| bad(format!("there is no column `{}`; the columns are {}", name, columns.iter().map(|c| format!("`{}`", c)).collect::<Vec<_>>().join(", "))));
    let column_numbers = |ci: usize| -> Result<Vec<Option<f64>>, DocError> {
        rows.iter().enumerate().map(|(i, r)| num_cell(r.get(ci).unwrap_or(&Value::Null)).map_err(|e| bad(format!("column `{}`, row {}: {}", columns[ci], i + 1, e)))).collect()
    };

    let y_names: Vec<String> = match v.get("y") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().map(cell_text).collect(),
        Some(_) => return Err(bad("`y` is a column name or an array of column names")),
        None => vec![],
    };
    let x_name = str_field(v, "x").map(String::from).unwrap_or_else(|| columns[0].clone());
    let series_by = str_field(v, "series_by").map(String::from);

    let scale = v.get("scale");
    let log_y = scale.and_then(|s| s.get("y")).and_then(|y| y.as_str()).map(|y| y == "log").unwrap_or(false);
    let y_zero_off = scale.and_then(|s| s.get("y_zero")).and_then(|z| z.as_bool()) == Some(false);

    let mut x: XAxis;
    let mut series: Vec<Series> = vec![];
    let mut notes: Vec<String> = vec![];

    if kind == Kind::Hist {
        let name = y_names.first().cloned().or_else(|| columns.iter().find(|c| *c != &x_name).cloned()).unwrap_or_else(|| columns[0].clone());
        if y_names.len() > 1 {
            return Err(bad("a histogram shows one column's distribution; name one column in `y`"));
        }
        let ci = col(&name)?;
        let vals = column_numbers(ci)?;
        let present = vals.iter().flatten().count();
        if present < 8 {
            return Err(bad(format!("a histogram needs at least 8 values; `{}` has {}. With so few, show them as bars or a table", name, present)));
        }
        x = XAxis::Numbers(vec![]);
        series.push(Series { name, values: vals });
    } else {
        // --- x
        let xi = col(&x_name)?;
        let xcells: Vec<&Value> = rows.iter().map(|r| r.get(xi).unwrap_or(&Value::Null)).collect();
        let nums: Option<Vec<f64>> = xcells.iter().map(|c| num_cell(c).ok().flatten()).collect();
        let dates: Option<Vec<f64>> = xcells.iter().map(|c| c.as_str().and_then(parse_date)).collect();
        let ordered_cats = str_field(v, "x_order") == Some("ordered");
        x = match kind {
            Kind::Bar | Kind::HBar => XAxis::Categories(xcells.iter().map(|c| cell_text(c)).collect()),
            Kind::Scatter => match (nums, dates) {
                (Some(n), _) if xcells.iter().all(|c| c.is_number() || c.as_str().is_some_and(|s| s.trim().parse::<f64>().is_ok())) => XAxis::Numbers(n),
                (_, Some(d)) => XAxis::Dates(d),
                _ => return Err(bad(format!("a scatter plot places points by two numbers, and `{}` is not numeric. To compare categories use `bar` or `hbar`", x_name))),
            },
            _ => match (nums, dates) {
                (Some(n), _) if xcells.iter().all(|c| c.is_number() || c.as_str().is_some_and(|s| s.trim().parse::<f64>().is_ok())) => XAxis::Numbers(n),
                (_, Some(d)) => XAxis::Dates(d),
                _ if ordered_cats => XAxis::Categories(xcells.iter().map(|c| cell_text(c)).collect()),
                _ => return Err(bad(format!("a {} chart needs an x axis with an order, such as numbers or dates, and `{}` holds names. If the names are already in order (versions, stages) add \"x_order\": \"ordered\"; if they are not, compare them with `bar` or `hbar`", kind.name(), x_name))),
            },
        };

        // --- series
        if let Some(sb) = &series_by {
            if y_names.len() != 1 {
                return Err(bad("with `series_by`, name exactly one value column in `y`"));
            }
            let (si, yi) = (col(sb)?, col(&y_names[0])?);
            let yv = column_numbers(yi)?;
            let keys: Vec<String> = (0..rows.len()).map(|i| cell_text(rows[i].get(si).unwrap_or(&Value::Null))).collect();
            let mut names: Vec<String> = vec![];
            for k in &keys {
                if !names.contains(k) {
                    names.push(k.clone());
                }
            }
            if names.len() > MAX_SERIES {
                return Err(too_many_series(names.len(), kind, Some(sb)));
            }
            // one distinct x position per x value, in first-seen order
            let labels: Vec<String> = xcells.iter().map(|c| cell_text(c)).collect();
            let mut positions: Vec<String> = vec![];
            for l in &labels {
                if !positions.contains(l) {
                    positions.push(l.clone());
                }
            }
            let mut grid: Vec<Vec<Option<f64>>> = vec![vec![None; positions.len()]; names.len()];
            for i in 0..rows.len() {
                let (s, p) = (names.iter().position(|n| *n == keys[i]).unwrap_or(0), positions.iter().position(|l| *l == labels[i]).unwrap_or(0));
                if grid[s][p].is_some() {
                    return Err(bad(format!("`{}` has two rows for the same `{}` and `{}`; aggregate them first", y_names[0], sb, x_name)));
                }
                grid[s][p] = yv[i];
            }
            x = rebuild_x(&x, &labels, &positions);
            series = names.into_iter().zip(grid).map(|(name, values)| Series { name, values }).collect();
        } else {
            let ys: Vec<String> = if y_names.is_empty() { columns.iter().filter(|c| **c != x_name).cloned().collect() } else { y_names.clone() };
            if ys.is_empty() {
                return Err(bad("there is no value column to plot; the data has only the x column"));
            }
            if ys.len() > MAX_SERIES {
                return Err(too_many_series(ys.len(), kind, None));
            }
            for name in &ys {
                series.push(Series { values: column_numbers(col(name)?)?, name: name.clone() });
            }
        }
        // sort by x for the forms that connect points, so the line goes left to right
        if matches!(kind, Kind::Line | Kind::Area) {
            if let XAxis::Numbers(xs) | XAxis::Dates(xs) = &x {
                let mut idx: Vec<usize> = (0..xs.len()).collect();
                idx.sort_by(|a, b| xs[*a].partial_cmp(&xs[*b]).unwrap_or(std::cmp::Ordering::Equal));
                if idx.windows(2).any(|w| xs[w[0]] == xs[w[1]]) && series.len() == 1 {
                    return Err(bad(format!("`{}` repeats the same x value, so the line would double back. Aggregate the repeats, or split them into series with `series_by`", x_name)));
                }
                let nx: Vec<f64> = idx.iter().map(|i| xs[*i]).collect();
                for s in &mut series {
                    s.values = idx.iter().map(|i| s.values[*i]).collect();
                }
                x = match x {
                    XAxis::Dates(_) => XAxis::Dates(nx),
                    _ => XAxis::Numbers(nx),
                };
            }
        }
    }

    // --- form checks
    let n = x.len();
    let all_values: Vec<f64> = series.iter().flat_map(|s| s.values.iter().flatten().copied()).collect();
    if all_values.is_empty() {
        return Err(bad("none of the values could be read as numbers"));
    }
    let max_series = if kind == Kind::Scatter { MAX_SERIES_SCATTER } else { MAX_SERIES };
    if series.len() > max_series {
        return Err(too_many_series(series.len(), kind, None));
    }
    match kind {
        Kind::Bar | Kind::HBar => {
            if n == 1 {
                return Err(bad("a bar chart with one bar is just a number: the number is the chart. Put it in a `stats` block in write_html (label, value, delta) instead"));
            }
            if n > 200 || (kind == Kind::Bar && n > 40) {
                return Err(bad(format!("{} categories is too many to read as {}s. Keep the top 10-15 and fold the rest into \"Other\", use `hist` if you are showing a distribution, or show a table", n, kind.name())));
            }
            if series.len() * n > MAX_BARS {
                return Err(bad(format!("{} bars is more than a chart can carry ({} at most); show fewer categories or series", series.len() * n, MAX_BARS)));
            }
            if log_y {
                return Err(bad("bars on a log axis mislead: bar length stops meaning amount. Use a `line` or `scatter` chart for log scales, or keep a linear axis"));
            }
            if kind == Kind::Bar {
                if let XAxis::Categories(c) = &x {
                    let longest = c.iter().map(|l| l.chars().count()).max().unwrap_or(0);
                    // a column needs its label to fit under it: about 6.4px per character at 11px type
                    let needed = n as f64 * (longest.min(24) as f64 * 6.4 + 10.0);
                    if needed > 640.0 * 0.9 {
                        return Err(bad(format!("{} categories with labels up to {} characters will not fit under columns without rotating the text. Use `hbar`, which gives each label its own row", n, longest)));
                    }
                }
            }
        }
        Kind::Line | Kind::Area | Kind::Scatter => {
            if n < 2 {
                return Err(bad(format!("a {} chart needs at least 2 points; with one value, show the number itself (a `stats` block in write_html)", kind.name())));
            }
        }
        Kind::Hist => {}
    }
    if kind == Kind::Area && series.len() > 1 {
        return Err(bad("an area chart shows one series (filled areas of several series hide each other). Use `line` for several series; for parts of a total over time, wait for `stacked-bar` or use `line`"));
    }
    if kind == Kind::Scatter && n * series.len() > MAX_SCATTER_POINTS {
        return Err(bad(format!("{} points is more than a scatter plot can show clearly ({} at most). Use `hist` for one variable, or aggregate", n * series.len(), MAX_SCATTER_POINTS)));
    }
    if log_y && all_values.iter().any(|v| *v <= 0.0) {
        return Err(bad("a log axis needs every value above zero; this data has zeros or negatives. Use a linear axis"));
    }
    if series.len() >= 2 {
        // two measures on very different scales would need two axes, which invent correlations
        let mags: Vec<f64> = series.iter().map(|s| s.values.iter().flatten().map(|v| v.abs()).fold(0.0, f64::max)).filter(|m| *m > 0.0).collect();
        if let (Some(hi), Some(lo)) = (mags.iter().cloned().reduce(f64::max), mags.iter().cloned().reduce(f64::min)) {
            if hi / lo > 50.0 && !log_y {
                return Err(bad(format!("the series differ in size by {:.0}x, so one y axis would flatten the smaller one, and a second axis would invent a relationship between them. Chart them separately, index both to 100 at the start, or use `\"scale\": {{\"y\": \"log\"}}` if they share a unit", hi / lo)));
            }
        }
    }

    // --- bounded marks
    if matches!(kind, Kind::Line | Kind::Area) && n > LINE_DOWNSAMPLE_OVER {
        let original = n;
        let (nx, ns) = downsample(&x, &series, 1000);
        x = nx;
        series = ns;
        notes.push(format!("Downsampled from {} points to {} (extremes kept)", original, x.len()));
    }

    // --- annotations
    let mut annotations = vec![];
    if let Some(list) = v.get("annotations").and_then(|a| a.as_array()) {
        if list.len() > 5 {
            return Err(bad("a chart carries at most 5 annotations"));
        }
        for a in list {
            let label = str_field(a, "label").filter(|l| !l.trim().is_empty() && l.chars().count() <= 60).ok_or_else(|| bad("each annotation needs a short `label` (60 characters)"))?;
            let at = a.get("x").ok_or_else(|| bad("each annotation needs an `x`"))?;
            // a marker outside the plotted range would be drawn off the chart, so it is refused instead
            let within = |p: f64, xs: &[f64]| xs.iter().cloned().fold(f64::INFINITY, f64::min) <= p && p <= xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let pos = match &x {
                XAxis::Categories(c) => c.iter().position(|l| *l == cell_text(at)).map(|i| i as f64),
                XAxis::Numbers(xs) => num_cell(at).ok().flatten().filter(|p| within(*p, xs)),
                XAxis::Dates(xs) => at.as_str().and_then(parse_date).filter(|p| within(*p, xs)),
            };
            annotations.push((pos.ok_or_else(|| bad(format!("annotation `{}` points at x = {}, which is not on the x axis", label, cell_text(at))))?, label.trim().to_string()));
        }
    }

    let y_fmt = match v.get("format") {
        Some(Value::Object(o)) => o.get("y").and_then(|y| y.as_str()).map(NumFmt::parse).unwrap_or_default(),
        _ => NumFmt::default(),
    };
    let size = match str_field(v, "size").unwrap_or("wide") {
        "wide" => Size::Wide,
        "square" => Size::Square,
        "tall" => Size::Tall,
        _ => return Err(bad("`size` is wide, square or tall")),
    };
    let theme = match str_field(v, "theme").unwrap_or("auto") {
        "auto" => Theme::Auto,
        "light" => Theme::Light,
        "dark" => Theme::Dark,
        _ => return Err(bad("`theme` is auto, light or dark")),
    };
    let source = str_field(v, "source").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if source.as_ref().is_some_and(|s| s.chars().count() > 300) {
        return Err(bad("`source` is over 300 characters"));
    }
    let bins = match v.get("bins") {
        Some(b) => Some(b.as_u64().filter(|b| (2..=60).contains(b)).ok_or_else(|| bad("`bins` is a whole number from 2 to 60"))? as usize),
        None => None,
    };
    Ok(ChartSpec { kind, title: title.to_string(), subtitle, x, x_name, series, log_y, truncated: y_zero_off && matches!(kind, Kind::Bar | Kind::HBar | Kind::Area | Kind::Hist), annotations, y_fmt, size, theme, source, bins, notes })
}

fn too_many_series(n: usize, kind: Kind, by: Option<&str>) -> DocError {
    let cap = if kind == Kind::Scatter { MAX_SERIES_SCATTER } else { MAX_SERIES };
    bad(format!(
        "{} series is more than {} can carry: past {} the colours stop being tell-apartable{}. Fold the smaller ones into \"Other\", show one chart per series (several charts in a report), or use a table{}",
        n,
        kind.name(),
        cap,
        if kind == Kind::Scatter { ", and a scatter plot cannot print values on its points" } else { "" },
        by.map(|b| format!(" (`{}` has {} distinct values)", b, n)).unwrap_or_default()
    ))
}

fn rebuild_x(x: &XAxis, labels: &[String], positions: &[String]) -> XAxis {
    let pick = |vals: &[f64]| -> Vec<f64> { positions.iter().map(|p| vals[labels.iter().position(|l| l == p).unwrap_or(0)]).collect() };
    match x {
        XAxis::Categories(_) => XAxis::Categories(positions.to_vec()),
        XAxis::Numbers(v) => XAxis::Numbers(pick(v)),
        XAxis::Dates(v) => XAxis::Dates(pick(v)),
    }
}

/// Reduce a long series to `target` buckets keeping each bucket's minimum and maximum, so spikes survive.
fn downsample(x: &XAxis, series: &[Series], target: usize) -> (XAxis, Vec<Series>) {
    let n = x.len();
    let bucket = n.div_ceil(target / 2).max(1);
    let mut keep: Vec<usize> = vec![];
    let mut i = 0;
    while i < n {
        let end = (i + bucket).min(n);
        keep.push(i);
        for s in series {
            let (mut lo, mut hi) = (None::<(usize, f64)>, None::<(usize, f64)>);
            for j in i..end {
                if let Some(v) = s.values[j] {
                    if lo.is_none_or(|(_, m)| v < m) {
                        lo = Some((j, v));
                    }
                    if hi.is_none_or(|(_, m)| v > m) {
                        hi = Some((j, v));
                    }
                }
            }
            keep.extend(lo.map(|l| l.0));
            keep.extend(hi.map(|h| h.0));
        }
        keep.push(end - 1);
        i = end;
    }
    keep.sort_unstable();
    keep.dedup();
    let nx = match x {
        XAxis::Categories(c) => XAxis::Categories(keep.iter().map(|i| c[*i].clone()).collect()),
        XAxis::Numbers(v) => XAxis::Numbers(keep.iter().map(|i| v[*i]).collect()),
        XAxis::Dates(v) => XAxis::Dates(keep.iter().map(|i| v[*i]).collect()),
    };
    (nx, series.iter().map(|s| Series { name: s.name.clone(), values: keep.iter().map(|i| s.values[*i]).collect() }).collect())
}
