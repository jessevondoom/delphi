//! Reports and slide decks (`specs/014` §7.2, §7.3, §8.6): two arrangements of the same blocks as a page, with
//! the same escaping, the same safety check on the finished file and the same audited runtime. Neither takes a
//! script; a person who needs one writes a page with `write_html`, where the operator is asked.
//!
//! - A **report** is a titled document of sections with an optional summary and contents list, laid out to read
//!   on a screen and to print ("Save as PDF").
//! - A **deck** is a title slide and then one slide per entry, each with a title, content, and speaker notes.
//!   Without scripting every slide is shown in turn; with the runtime it becomes one slide at a time with arrow,
//!   page and space keys, swipe, buttons, an index and a notes toggle. Printing gives one slide per page, with
//!   the notes beneath it as a handout.

use crate::htmlpage::{assemble, parse_block, render_block, Block, Budget, Cx, Shell, Theme, MAX_BLOCKS};
use crate::markdown::{escape, slug, to_html_shift};
use crate::DocError;
use serde_json::{json, Value};

const MAX_SECTIONS: usize = 40;
const MAX_SLIDES: usize = 60;

#[derive(Debug, Clone)]
pub struct Section {
    pub heading: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
pub struct ReportSpec {
    pub title: String,
    pub subtitle: Option<String>,
    pub summary: Option<String>,
    pub sections: Vec<Section>,
    pub toc: bool,
    pub theme: Theme,
    pub comment: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Slide {
    pub title: String,
    pub blocks: Vec<Block>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeckSpec {
    pub title: String,
    pub subtitle: Option<String>,
    pub author: Option<String>,
    pub slides: Vec<Slide>,
    pub theme: Theme,
    pub comment: Option<String>,
}

fn bad(msg: impl Into<String>) -> DocError {
    DocError(msg.into())
}

fn text_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

fn short(v: &Value, k: &str, max: usize, what: &str) -> Result<Option<String>, DocError> {
    match text_of(v, k).map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) if s.chars().count() > max => Err(bad(format!("the {} is over {} characters", what, max))),
        Some(s) => Ok(Some(s.to_string())),
        None => Ok(None),
    }
}

fn theme_of(v: &Value) -> Result<Theme, DocError> {
    Ok(match text_of(v, "theme").unwrap_or("auto") {
        "auto" => Theme::Auto,
        "light" => Theme::Light,
        "dark" => Theme::Dark,
        _ => return Err(bad("`theme` is auto, light or dark")),
    })
}

/// A section's or slide's content: `body_md`, then `blocks`, then a `chart` shorthand.
fn content(v: &Value, b: &mut Budget) -> Result<Vec<Block>, DocError> {
    let mut out = vec![];
    if let Some(md) = text_of(v, "body_md") {
        b.text(md)?;
        b.blocks += 1;
        out.push(Block::Markdown(md.to_string()));
    }
    if let Some(list) = v.get("blocks") {
        for x in list.as_array().ok_or_else(|| bad("`blocks` is an array"))? {
            out.push(parse_block(x, 0, b)?);
        }
    }
    if let Some(c) = v.get("chart") {
        out.push(parse_block(&json!({"type": "chart", "chart": c, "caption": v.get("chart_caption")}), 0, b)?);
    }
    if b.blocks > MAX_BLOCKS {
        return Err(bad(format!("a document has at most {} blocks", MAX_BLOCKS)));
    }
    Ok(out)
}

pub fn report_from_json(v: &Value) -> Result<ReportSpec, DocError> {
    let title = short(v, "title", 200, "title")?.ok_or_else(|| bad("a report needs a `title`"))?;
    let mut b = Budget::new();
    b.text = title.len();
    let summary = short(v, "summary", 3000, "summary")?;
    if let Some(s) = &summary {
        b.text(s)?;
    }
    let list = v.get("sections").and_then(|s| s.as_array()).ok_or_else(|| bad("a report needs a `sections` array: [{heading, body_md?, blocks?, chart?}]"))?;
    if list.is_empty() || list.len() > MAX_SECTIONS {
        return Err(bad(format!("a report has between 1 and {} sections", MAX_SECTIONS)));
    }
    let mut sections = vec![];
    for (i, s) in list.iter().enumerate() {
        let heading = short(s, "heading", 120, "section heading")?.ok_or_else(|| bad(format!("section {} needs a `heading`", i + 1)))?;
        b.text(&heading)?;
        let blocks = content(s, &mut b)?;
        if blocks.is_empty() {
            return Err(bad(format!("section `{}` has no content: give it `body_md`, `blocks` or a `chart`", heading)));
        }
        sections.push(Section { heading, blocks });
    }
    let toc = v.get("toc").and_then(|t| t.as_bool()).unwrap_or(sections.len() >= 3);
    Ok(ReportSpec { title, subtitle: short(v, "subtitle", 200, "subtitle")?, summary, sections, toc, theme: theme_of(v)?, comment: None })
}

pub fn deck_from_json(v: &Value) -> Result<DeckSpec, DocError> {
    let title = short(v, "title", 200, "title")?.ok_or_else(|| bad("a deck needs a `title`"))?;
    let mut b = Budget::new();
    b.text = title.len();
    let list = v.get("slides").and_then(|s| s.as_array()).ok_or_else(|| bad("a deck needs a `slides` array: [{title, body_md?, blocks?, chart?, notes?}]"))?;
    if list.is_empty() || list.len() > MAX_SLIDES {
        return Err(bad(format!("a deck has between 1 and {} slides (the title slide is added for you)", MAX_SLIDES)));
    }
    let mut slides = vec![];
    for (i, s) in list.iter().enumerate() {
        let st = short(s, "title", 120, "slide title")?.ok_or_else(|| bad(format!("slide {} needs a `title`", i + 1)))?;
        b.text(&st)?;
        let notes = short(s, "notes", 4000, "speaker notes")?;
        if let Some(n) = &notes {
            b.text(n)?;
        }
        slides.push(Slide { title: st, blocks: content(s, &mut b)?, notes });
    }
    Ok(DeckSpec { title, subtitle: short(v, "subtitle", 200, "subtitle")?, author: short(v, "author", 120, "author")?, slides, theme: theme_of(v)?, comment: None })
}

fn section_id(i: usize, heading: &str) -> String {
    let s = slug(heading);
    format!("sec{}{}", i + 1, if s.is_empty() { String::new() } else { format!("-{}", s.chars().take(40).collect::<String>()) })
}

pub fn render_report(spec: &ReportSpec) -> Result<String, DocError> {
    let mut cx = Cx::new(2);
    let mut body = String::new();
    if let Some(s) = &spec.summary {
        body.push_str(&format!("<section class=\"md lede\">\n{}</section>\n", to_html_shift(s, 2)));
    }
    if spec.toc {
        body.push_str("<nav class=\"toc\" aria-label=\"Contents\"><h2>Contents</h2><ol>");
        for (i, s) in spec.sections.iter().enumerate() {
            body.push_str(&format!("<li><a href=\"#{}\">{}</a></li>", section_id(i, &s.heading), escape(&s.heading)));
        }
        body.push_str("</ol></nav>\n");
    }
    for (i, s) in spec.sections.iter().enumerate() {
        body.push_str(&format!("<section class=\"section\" id=\"{}\">\n<h2>{}</h2>\n", section_id(i, &s.heading), escape(&s.heading)));
        for b in &s.blocks {
            render_block(b, &mut cx, &mut body);
        }
        body.push_str("</section>\n");
    }
    let open = format!("<main class=\"report\">\n<header class=\"report-head\"><h1>{}</h1>{}</header>\n", escape(&spec.title), spec.subtitle.as_ref().map(|s| format!("<p class=\"sub\">{}</p>", escape(s))).unwrap_or_default());
    assemble(&Shell { title: &spec.title, theme: spec.theme, comment: spec.comment.as_deref(), charts: cx.charts, runtime: cx.runtime, script: None, open: &open, body: &body, close: "</main>\n" })
}

pub fn render_deck(spec: &DeckSpec) -> Result<String, DocError> {
    let mut cx = Cx::new(2);
    cx.runtime = true;
    let mut body = String::new();
    let total = spec.slides.len() + 1;
    body.push_str(&format!(
        "<section class=\"slide title-slide\" id=\"s1\" aria-label=\"Slide 1 of {}: title\">\n<h1>{}</h1>{}{}\n</section>\n",
        total,
        escape(&spec.title),
        spec.subtitle.as_ref().map(|s| format!("<p class=\"sub\">{}</p>", escape(s))).unwrap_or_default(),
        spec.author.as_ref().map(|s| format!("<p class=\"by\">{}</p>", escape(s))).unwrap_or_default()
    ));
    for (i, s) in spec.slides.iter().enumerate() {
        let n = i + 2;
        body.push_str(&format!("<section class=\"slide\" id=\"s{n}\" aria-label=\"Slide {n} of {total}: {}\">\n<h2>{}</h2>\n<div class=\"slide-body\">\n", escape(&s.title), escape(&s.title), n = n, total = total));
        for b in &s.blocks {
            render_block(b, &mut cx, &mut body);
        }
        body.push_str("</div>\n");
        if let Some(notes) = &s.notes {
            body.push_str(&format!("<aside class=\"notes\" aria-label=\"Speaker notes\"><h3>Speaker notes</h3>\n{}</aside>\n", to_html_shift(notes, 3)));
        }
        body.push_str("</section>\n");
    }
    body.push_str("<nav class=\"deck-ui\" aria-label=\"Slide controls\"><button type=\"button\" data-deck-prev aria-label=\"Previous slide\">&lsaquo;</button><span data-deck-count aria-live=\"polite\"></span><button type=\"button\" data-deck-next aria-label=\"Next slide\">&rsaquo;</button><button type=\"button\" data-deck-index aria-label=\"Slide index (i)\">Index</button><button type=\"button\" data-deck-notes aria-pressed=\"false\" aria-label=\"Speaker notes (n)\">Notes</button></nav>\n");
    body.push_str("<div class=\"deck-index\" data-deck-index-panel hidden><h2>Slides</h2><ol>");
    body.push_str(&format!("<li><a href=\"#s1\">{}</a></li>", escape(&spec.title)));
    for (i, s) in spec.slides.iter().enumerate() {
        body.push_str(&format!("<li><a href=\"#s{}\">{}</a></li>", i + 2, escape(&s.title)));
    }
    body.push_str("</ol></div>\n");
    assemble(&Shell { title: &spec.title, theme: spec.theme, comment: spec.comment.as_deref(), charts: cx.charts, runtime: true, script: None, open: "<div class=\"deck\" data-deck>\n", body: &body, close: "</div>\n" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_chart() -> Value {
        json!({"type": "line", "title": "Latency fell", "x": "release", "y": ["p50", "p95"], "x_order": "ordered", "data": {"columns": ["release", "p50", "p95"], "rows": [["0.2.1", 120, 340], ["0.2.2", 104, 255], ["0.2.3", 101, 248]]}})
    }

    fn report() -> Value {
        json!({"title": "Q3 review", "subtitle": "Engineering", "summary": "Latency **fell**; adoption grew.", "sections": [
            {"heading": "Latency", "body_md": "# Findings\n\nP50 down 16%.", "chart": line_chart()},
            {"heading": "Adoption", "blocks": [
                {"type": "stats", "items": [{"label": "Users", "value": "12.4K", "delta": "+9%"}]},
                {"type": "table", "columns": ["Region", "Users"], "rows": [["North", 6100], ["South", 6300]], "sortable": true}]},
            {"heading": "Next", "body_md": "- ship\n- measure"}
        ]})
    }

    fn deck() -> Value {
        json!({"title": "Roadmap", "subtitle": "Q4 plan", "author": "The team", "slides": [
            {"title": "Where we are", "body_md": "- fast\n- stable", "notes": "Say **thanks** here."},
            {"title": "Latency", "chart": line_chart(), "notes": "Point at the drop."},
            {"title": "Questions"}
        ]})
    }

    #[test]
    fn a_report_has_a_contents_list_sections_and_embedded_charts_and_passes_the_safety_check() {
        let h = render_report(&report_from_json(&report()).unwrap()).unwrap();
        assert!(h.contains("<main class=\"report\">") && h.contains("<h1>Q3 review</h1>") && h.contains("<p class=\"sub\">Engineering</p>"));
        assert!(h.contains("<nav class=\"toc\"") && h.contains("<a href=\"#sec1-latency\">Latency</a>") && h.contains("<section class=\"section\" id=\"sec2-adoption\">"), "{}", h);
        assert!(h.contains("<h3 id=\"findings\">Findings</h3>"), "a markdown heading sits under the section title");
        assert!(h.contains("<figure class=\"chart\"><svg class=\"viz-root\"") && h.contains("aria-labelledby=\"c") && h.matches(".viz-root{color-scheme:light").count() == 1, "the chart is inline and its stylesheet is in the page once");
        assert!(h.contains("Latency <strong>fell</strong>") && h.contains("<dd>12.4K<small>+9%</small></dd>") && h.contains("data-sortable"));
        assert_eq!(h.matches("<script>").count(), 1, "only the runtime, because a table is sortable");
        assert!(crate::htmllint::lint(&h).is_empty());
    }

    #[test]
    fn several_charts_share_one_stylesheet_and_have_distinct_ids() {
        let two = json!({"title": "T", "sections": [{"heading": "A", "chart": line_chart()}, {"heading": "B", "blocks": [{"type": "grid", "columns": 2, "blocks": [{"type": "chart", "chart": line_chart(), "caption": "Again"}, {"type": "stats", "items": [{"label": "n", "value": 1}]}]}]}]});
        let h = render_report(&report_from_json(&two).unwrap()).unwrap();
        assert_eq!(h.matches(".viz-root{color-scheme:light").count(), 1);
        let ids: Vec<&str> = h.split(" id=\"").skip(1).map(|t| t.split('"').next().unwrap()).collect();
        let unique: std::collections::HashSet<&&str> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "every id in the page is unique: {:?}", ids);
        assert!(h.contains("<div class=\"grid cols2\">") && h.contains("<figcaption>Again</figcaption>"));
        assert!(crate::htmllint::lint(&h).is_empty());
    }

    #[test]
    fn a_small_report_has_no_contents_list_unless_asked_and_no_script_without_interactive_blocks() {
        let h = render_report(&report_from_json(&json!({"title": "T", "sections": [{"heading": "One", "body_md": "x"}, {"heading": "Two", "body_md": "y"}]})).unwrap()).unwrap();
        assert!(!h.contains("class=\"toc\"") && !h.contains("<script"));
        let h = render_report(&report_from_json(&json!({"title": "T", "toc": true, "sections": [{"heading": "One", "body_md": "x"}]})).unwrap()).unwrap();
        assert!(h.contains("class=\"toc\""));
    }

    #[test]
    fn a_deck_is_a_title_slide_then_each_slide_with_notes_controls_and_an_index() {
        let h = render_deck(&deck_from_json(&deck()).unwrap()).unwrap();
        assert!(h.contains("<div class=\"deck\" data-deck>") && h.contains("<section class=\"slide title-slide\" id=\"s1\" aria-label=\"Slide 1 of 4: title\">") && h.contains("<h1>Roadmap</h1><p class=\"sub\">Q4 plan</p><p class=\"by\">The team</p>"), "{}", h);
        assert!(h.contains("<section class=\"slide\" id=\"s2\" aria-label=\"Slide 2 of 4: Where we are\">") && h.contains("<h2>Where we are</h2>") && h.contains("<li>fast</li>"), "{}", h);
        assert!(h.contains("<aside class=\"notes\"") && h.contains("Say <strong>thanks</strong> here.") && h.contains("<h3>Speaker notes</h3>"));
        assert!(h.contains("<figure class=\"chart\">") && h.contains("<section class=\"slide\" id=\"s4\"") && h.contains("<h2>Questions</h2>"));
        for hook in ["data-deck-prev", "data-deck-next", "data-deck-index", "data-deck-notes", "data-deck-count", "data-deck-index-panel"] {
            assert!(h.contains(hook), "{}", hook);
        }
        assert!(h.contains("<li><a href=\"#s1\">Roadmap</a></li>") && h.contains("<li><a href=\"#s3\">Latency</a></li>"));
        assert_eq!(h.matches("<script>").count(), 1, "the audited runtime and nothing else");
        assert!(crate::htmllint::lint(&h).is_empty());
    }

    #[test]
    fn text_from_the_agent_is_text_in_both() {
        let evil = "</title><script>alert(1)</script><img src=x onerror=1>\"&";
        let d = render_deck(&deck_from_json(&json!({"title": evil, "subtitle": evil, "author": evil, "slides": [{"title": evil, "body_md": evil, "notes": evil}]})).unwrap()).unwrap();
        let r = render_report(&report_from_json(&json!({"title": evil, "subtitle": evil, "summary": evil, "sections": [{"heading": evil, "body_md": evil}], "toc": true})).unwrap()).unwrap();
        for h in [d, r] {
            assert!(!h.contains("<script>alert") && !h.contains("<img") && !h.contains("</title><script"), "{}", h);
            assert!(h.contains("&lt;/title&gt;&lt;script&gt;") && crate::htmllint::lint(&h).is_empty());
        }
    }

    #[test]
    fn bad_input_is_refused_with_a_reason() {
        for (v, expect, deck) in [
            (json!({"slides": [{"title": "a"}]}), "`title`", true),
            (json!({"title": "T"}), "`slides`", true),
            (json!({"title": "T", "slides": []}), "between 1 and 60", true),
            (json!({"title": "T", "slides": [{"body_md": "x"}]}), "needs a `title`", true),
            (json!({"title": "T", "slides": [{"title": "a", "chart": {"type": "pie"}}]}), "chart block", true),
            (json!({"title": "T", "slides": [{"title": "a", "notes": "x".repeat(5000)}]}), "speaker notes", true),
            (json!({"title": "T", "theme": "neon", "slides": [{"title": "a"}]}), "theme", true),
            (json!({"title": "T", "sections": []}), "between 1 and 40", false),
            (json!({"title": "T", "sections": [{"body_md": "x"}]}), "`heading`", false),
            (json!({"title": "T", "sections": [{"heading": "H"}]}), "no content", false),
            (json!({"sections": [{"heading": "H", "body_md": "x"}]}), "`title`", false),
        ] {
            let e = if deck { deck_from_json(&v).unwrap_err() } else { report_from_json(&v).unwrap_err() };
            assert!(e.0.contains(expect), "{} -> {}", v, e);
        }
        let many: Vec<Value> = (0..61).map(|i| json!({"title": format!("s{}", i)})).collect();
        assert!(deck_from_json(&json!({"title": "T", "slides": many})).is_err());
        let charts: Vec<Value> = (0..13).map(|_| json!({"type": "chart", "chart": line_chart()})).collect();
        assert!(report_from_json(&json!({"title": "T", "sections": [{"heading": "H", "blocks": charts}]})).unwrap_err().0.contains("at most 12 charts"));
    }

    /// `DELPHI_DOCS_HTML_OUT=/dir cargo test -p delphi-docs write_kind_samples -- --ignored`, then open the files in a
    /// real browser. `deck-driven.html` is the deck with a test driver appended (not something the generator can
    /// produce) that presses keys, clicks and swipes, and reports what happened in `<title>`.
    #[test]
    #[ignore = "writes files for a browser to check"]
    fn write_kind_samples() {
        let Ok(dir) = std::env::var("DELPHI_DOCS_HTML_OUT") else { return };
        let d = render_deck(&deck_from_json(&deck()).unwrap()).unwrap();
        let driver = "<script>\n(function () {\n var out = [];\n var deck = document.querySelector('[data-deck]');\n function vis() { var v = []; document.querySelectorAll('.slide').forEach(function (s, i) { if (!s.hidden) { v.push(i + 1); } }); return v.join(','); }\n function key(k) { document.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true })); }\n var panel = document.querySelector('[data-deck-index-panel]');\n out.push('enhanced:' + deck.classList.contains('enhanced') + ' start:' + vis() + ' count:' + document.querySelector('[data-deck-count]').textContent);\n key('ArrowRight'); out.push('right:' + vis());\n key(' '); out.push('space:' + vis());\n key('ArrowLeft'); out.push('left:' + vis());\n key('End'); out.push('end:' + vis());\n key('ArrowRight'); out.push('pastEnd:' + vis());\n key('Home'); out.push('home:' + vis());\n key('n'); out.push('notes:' + deck.classList.contains('notes-on'));\n key('n'); out.push('notesOff:' + deck.classList.contains('notes-on'));\n key('i'); out.push('index:' + !panel.hidden);\n document.querySelectorAll('[data-deck-index-panel] a')[2].click(); out.push('jump:' + vis() + ' indexClosed:' + panel.hidden);\n document.querySelector('[data-deck-next]').click(); out.push('btnNext:' + vis());\n function swipe(a, b) { var s = new Event('touchstart', { bubbles: true }); Object.defineProperty(s, 'changedTouches', { value: [{ clientX: a }] }); deck.dispatchEvent(s); var e = new Event('touchend', { bubbles: true }); Object.defineProperty(e, 'changedTouches', { value: [{ clientX: b }] }); deck.dispatchEvent(e); }\n swipe(100, 300); out.push('swipeRight:' + vis());\n swipe(300, 100); out.push('swipeLeft:' + vis());\n swipe(100, 130); out.push('smallSwipe:' + vis());\n document.title = out.join(' | ');\n})();\n</script>\n</body>";
        std::fs::write(format!("{}/deck-driven.html", dir), d.replace("</body>", driver)).unwrap();
        std::fs::write(format!("{}/deck.html", dir), &d).unwrap();
        std::fs::write(format!("{}/report.html", dir), render_report(&report_from_json(&report()).unwrap()).unwrap()).unwrap();
        let mut dark = report();
        dark["theme"] = json!("dark");
        std::fs::write(format!("{}/report-dark.html", dir), render_report(&report_from_json(&dark).unwrap()).unwrap()).unwrap();
    }

    /// The same page of every chart form, plain and with a test driver appended (not something the generator can
    /// produce) that points at marks, clicks legend items and drags out a zoom, reporting what happened in `<title>`.
    #[test]
    #[ignore = "writes files for a browser to check"]
    fn write_chart_page_samples() {
        let Ok(dir) = std::env::var("DELPHI_DOCS_HTML_OUT") else { return };
        let blocks: Vec<Value> = crate::chart::samples::all().into_iter().map(|(_, c)| json!({"type": "chart", "chart": c})).collect();
        let page = crate::htmlpage::from_json(&json!({"title": "Every chart", "blocks": blocks})).unwrap();
        let html = crate::htmlpage::render(&page).unwrap();
        let driver = r#"<script>
(function () {
  var out = [];
  function fire(el, type, x, y) { el.dispatchEvent(new PointerEvent(type, { bubbles: true, clientX: x, clientY: y, button: 0, pointerId: 1 })); }
  function click(el) { el.dispatchEvent(new MouseEvent('click', { bubbles: true })); }
  var figs = document.querySelectorAll('figure.chart');
  out.push('charts:' + figs.length + ' tips:' + document.querySelectorAll('.tip').length);
  var line = figs[5];
  var svg = line.querySelector('svg');
  var hit = svg.querySelector('.hit');
  var r = hit.getBoundingClientRect();
  fire(hit, 'pointermove', r.left + r.width / 2, r.top + 30);
  var tip = line.querySelector('.tip');
  out.push('line tip:' + (!tip.hidden) + ' "' + tip.textContent + '"');
  fire(svg, 'pointerleave', 0, 0);
  out.push('left:' + tip.hidden);
  var item = svg.querySelector('[data-t="1"]');
  click(item);
  out.push('hid:' + svg.querySelectorAll('.hid').length + ' pressed:' + item.getAttribute('aria-pressed'));
  click(item);
  out.push('shown:' + svg.querySelectorAll('.hid').length + ' pressed:' + item.getAttribute('aria-pressed'));
  var b = svg.getBoundingClientRect();
  var vb0 = svg.getAttribute('viewBox');
  fire(svg, 'pointerdown', b.left + 100, b.top + 120);
  fire(svg, 'pointermove', b.left + 320, b.top + 260);
  out.push('brush:' + !svg.querySelector('.brush').hasAttribute('hidden'));
  fire(svg, 'pointerup', b.left + 320, b.top + 260);
  out.push('zoomed:' + line.classList.contains('zoomed') + ' vb:' + vb0 + ' -> ' + svg.getAttribute('viewBox'));
  svg.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  out.push('reset:' + (svg.getAttribute('viewBox') === vb0) + ' zoomed:' + line.classList.contains('zoomed'));
  var say = [];
  figs.forEach(function (f) {
    var m = f.querySelector('[data-v]');
    if (!m) { say.push('none'); return; }
    var q = m.getBoundingClientRect();
    fire(m, 'pointermove', q.left + q.width / 2, q.top + q.height / 2);
    var t = f.querySelector('.tip');
    say.push(t.hidden ? 'HIDDEN' : t.textContent);
  });
  out.push('tips: ' + say.join(' // '));
  document.title = out.join(' | ');
})();
</script>
</body>"#;
        std::fs::write(format!("{}/charts.html", dir), &html).unwrap();
        std::fs::write(format!("{}/charts-driven.html", dir), html.replace("</body>", driver)).unwrap();
    }

    #[test]
    fn output_is_deterministic() {
        let d = deck_from_json(&deck()).unwrap();
        assert_eq!(render_deck(&d).unwrap(), render_deck(&d).unwrap());
        let r = report_from_json(&report()).unwrap();
        assert_eq!(render_report(&r).unwrap(), render_report(&r).unwrap());
    }
}
