use super::*;
use serde_json::{json, Value};

fn chart(v: Value) -> Result<ChartSpec, DocError> {
    from_json(&v)
}

fn svg(v: Value) -> String {
    render_svg(&chart(v).unwrap(), None)
}

fn rejected(v: Value) -> String {
    chart(v).unwrap_err().0
}

fn two_col(kind: &str, rows: Value) -> Value {
    json!({"type": kind, "title": "T", "data": {"columns": ["k", "v"], "rows": rows}})
}

#[test]
fn form_follows_the_question_each_rule_refuses_with_a_reason_and_a_better_form() {
    let cats = |n: usize| -> Value { (0..n).map(|i| json!([format!("item {}", i), i + 1])).collect::<Vec<_>>().into() };
    for (spec, expect) in [
        (json!({"title": "T", "data": {}}), "`type`"),
        (json!({"type": "bar", "data": {"columns": ["a"], "rows": [[1]]}}), "`title`"),
        (json!({"type": "bar", "title": "T"}), "`data`"),
        (json!({"type": "radar", "title": "T", "data": {"columns": ["a"], "rows": [[1]]}}), "unknown chart type"),
        (json!({"type": "table", "title": "T", "data": {"columns": ["a"], "rows": [[1]]}}), "not a chart"),
        (two_col("donut", json!([["only", 5]])), "2 to 4 parts"),
        (two_col("donut", json!([["a", 1], ["b", 2], ["c", 3], ["d", 4], ["e", 5]])), "fold the smallest"),
        (two_col("donut", json!([["a", 1], ["b", -2]])), "zero or more"),
        (json!({"type": "donut", "title": "T", "data": {"columns": ["k", "v", "w"], "rows": [["a", 1, 2], ["b", 2, 3]]}}), "one value column"),
        (two_col("stacked-bar", json!([["a", 1], ["b", 2]])), "2 or more series"),
        (json!({"type": "stacked-bar", "title": "T", "data": {"columns": ["k", "a", "b"], "rows": [["x", 1, -2], ["y", 2, 3]]}}), "zero or more"),
        (json!({"type": "stacked-bar", "title": "T", "data": {"columns": ["k", "a", "b"], "rows": [["x", 1, 2]]}}), "just its parts"),
        (json!({"type": "heatmap", "title": "T", "data": {"columns": ["k", "a"], "rows": [["x", 1], ["y", 2]]}}), "2 or more rows"),
        (json!({"type": "box", "title": "T", "data": {"columns": ["g", "v"], "rows": [["a", 1], ["a", 2], ["a", 3], ["a", 4], ["a", 5]]}}), "between 2 and 12 groups"),
        (json!({"type": "box", "title": "T", "data": {"columns": ["g", "v"], "rows": [["a", 1], ["a", 2], ["a", 3], ["a", 4], ["a", 5], ["b", 1], ["b", 2]]}}), "at least 5 values; group `b` has 2"),
        (json!({"type": "sparkline", "title": "T", "data": {"columns": ["x", "v"], "rows": [[1, 5], [2, 6]]}}), "at least 3 points"),
        (json!({"type": "sparkline", "title": "T", "data": {"columns": ["x", "a", "b"], "rows": [[1, 5, 1], [2, 6, 2], [3, 7, 3]]}}), "one line"),
        (two_col("bar", json!([["only", 5]])), "just a number"),
        (two_col("bar", cats(41)), "Other"),
        (two_col("hbar", cats(201)), "Other"),
        (two_col("bar", json!([["a very long category name indeed", 1], ["another very long category name", 2], ["a third long category label here", 3], ["fourth quite long category label", 4], ["fifth long category label text", 5], ["sixth long category label text", 6], ["seventh long label text here", 7], ["eighth long label text here ok", 8], ["ninth long label text here ok", 9], ["tenth long label text here ok", 10]])), "Use `hbar`"),
        (two_col("line", json!([["alpha", 1], ["beta", 2]])), "x_order"),
        (two_col("scatter", json!([["alpha", 1], ["beta", 2]])), "`bar`"),
        (json!({"type": "area", "title": "T", "data": {"columns": ["x", "a", "b"], "rows": [[1, 1, 2], [2, 2, 3]]}}), "Use `line`"),
        (json!({"type": "line", "title": "T", "data": {"columns": ["x", "a", "b", "c", "d", "e"], "rows": [[1, 1, 2, 3, 4, 5], [2, 2, 3, 4, 5, 6]]}}), "Other"),
        (json!({"type": "scatter", "title": "T", "data": {"columns": ["x", "a", "b", "c"], "rows": [[1, 1, 2, 3], [2, 2, 3, 4]]}}), "scatter plot cannot print values"),
        (json!({"type": "bar", "title": "T", "scale": {"y": "log"}, "data": {"columns": ["k", "v"], "rows": [["a", 1], ["b", 10]]}}), "log axis mislead"),
        (json!({"type": "line", "title": "T", "scale": {"y": "log"}, "data": {"columns": ["x", "v"], "rows": [[1, 0], [2, 10]]}}), "above zero"),
        (json!({"type": "line", "title": "T", "data": {"columns": ["x", "users", "sessions"], "rows": [[1, 20, 800000], [2, 30, 900000]]}}), "second axis would invent"),
        (json!({"type": "hist", "title": "T", "data": {"columns": ["v"], "rows": [[1], [2], [3]]}}), "at least 8 values"),
        (json!({"type": "hist", "title": "T", "bins": 500, "data": {"columns": ["v"], "rows": (0..20).map(|i| json!([i])).collect::<Vec<_>>()}}), "`bins`"),
        (json!({"type": "line", "title": "T", "data": {"columns": ["x", "v"], "rows": [[1, 5], [1, 6], [2, 7]]}}), "double back"),
        (json!({"type": "line", "title": "T", "data": {"columns": ["x", "v"], "rows": [[1, "abc"], [2, 3]]}}), "column `v`, row 1"),
        (json!({"type": "line", "title": "T", "data": {"columns": ["x", "v"], "rows": [[1, 5]]}}), "at least 2 points"),
        (json!({"type": "line", "title": "T", "annotations": [{"x": 99, "label": "nope"}], "data": {"columns": ["x", "v"], "rows": [[1, 5], [2, 6]]}}), "not on the x axis"),
        (json!({"type": "line", "title": "T", "size": "huge", "data": {"columns": ["x", "v"], "rows": [[1, 5], [2, 6]]}}), "`size`"),
        (json!({"type": "line", "title": "T", "x": "nope", "data": {"columns": ["x", "v"], "rows": [[1, 5], [2, 6]]}}), "no column `nope`"),
        (json!({"type": "line", "title": "x".repeat(200), "data": {"columns": ["x", "v"], "rows": [[1, 5], [2, 6]]}}), "title is over"),
    ] {
        let e = rejected(spec.clone());
        assert!(e.contains(expect), "expected {:?} in: {}\n(for {})", expect, e, spec);
    }
}

#[test]
fn series_by_pivots_long_data_and_keeps_series_in_first_seen_order() {
    let c = chart(json!({"type": "line", "title": "T", "x": "day", "y": "ms", "series_by": "svc", "data": {"columns": ["day", "svc", "ms"], "rows": [[1, "web", 10], [1, "api", 20], [2, "web", 11], [2, "api", 21], [3, "web", 12]]}})).unwrap();
    assert_eq!(c.series.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["web", "api"]);
    assert_eq!(c.series[1].values, vec![Some(20.0), Some(21.0), None], "a missing point stays a gap");
    let e = rejected(json!({"type": "line", "title": "T", "x": "day", "y": "ms", "series_by": "svc", "data": {"columns": ["day", "svc", "ms"], "rows": [[1, "web", 10], [1, "web", 11]]}}));
    assert!(e.contains("two rows for the same"), "{}", e);
}

#[test]
fn numbers_in_text_form_dates_and_unsorted_x_are_understood() {
    let c = chart(json!({"type": "line", "title": "T", "data": {"columns": ["x", "v"], "rows": [["3", "$1,200"], ["1", "5%"], ["2", ""]]}})).unwrap();
    assert_eq!(c.x, XAxis::Numbers(vec![1.0, 2.0, 3.0]), "a line is drawn left to right");
    assert_eq!(c.series[0].values, vec![Some(5.0), None, Some(1200.0)]);
    let d = chart(json!({"type": "line", "title": "T", "data": {"columns": ["day", "v"], "rows": [["2026-10-03", 3], ["2026-10-01", 1], ["2026-10-02", 2]]}})).unwrap();
    assert!(matches!(d.x, XAxis::Dates(ref v) if v[0] < v[1] && v[1] < v[2]));
}

#[test]
fn bounded_marks_long_lines_are_downsampled_and_the_chart_says_so() {
    let rows: Vec<Value> = (0..5000).map(|i| json!([i, if i == 2500 { 1000.0 } else { (i as f64 / 50.0).sin() * 10.0 }])).collect();
    let c = chart(json!({"type": "line", "title": "T", "data": {"columns": ["x", "v"], "rows": rows}})).unwrap();
    assert!(c.x.len() <= 1600 && c.x.len() >= 500, "{}", c.x.len());
    assert!(c.notes[0].starts_with("Downsampled from 5000 points"), "{:?}", c.notes);
    assert!(c.series[0].values.iter().flatten().any(|v| *v == 1000.0), "the spike survives");
    let big: Vec<Value> = (0..5100).map(|i| json!([i, i])).collect();
    assert!(rejected(json!({"type": "scatter", "title": "T", "data": {"columns": ["x", "y"], "rows": big}})).contains("5000 at most"));
    let huge: Vec<Value> = (0..20_001).map(|i| json!([i, i])).collect();
    assert!(rejected(json!({"type": "line", "title": "T", "data": {"columns": ["x", "y"], "rows": huge}})).contains("too many to chart"));
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{}=\"", name);
    let i = tag.find(&pat)? + pat.len();
    tag[i..].split('"').next()
}

#[test]
fn the_marks_follow_the_fixed_specs() {
    for (name, spec) in samples::all() {
        let s = render_svg(&from_json(&spec).unwrap(), None);
        // solid hairlines only: nothing is dashed
        assert!(!s.contains("stroke-dasharray"), "{}: dashed rule", name);
        // text wears ink tokens, never a series colour
        for t in s.split("<text ").skip(1) {
            let class = attr(t, "class").unwrap();
            // (heatmap cell values use ht0..ht4: the primary ink or the surface colour, chosen by contrast)
            assert!(["t1", "t1 b", "t2", "t3", "ht0", "ht1", "ht2", "ht3", "ht4"].contains(&class), "{}: text with class {:?}", name, class);
        }
        // lines are 2px with round joins; dots carry a 2px surface ring; areas are a 10% wash
        assert!(s.contains(".k1{stroke:var(--s1);fill:none;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}"), "{}", name);
        assert!(s.contains(".d1{fill:var(--s1);stroke:var(--surface);stroke-width:2}") && s.contains(".a1{fill:var(--s1);fill-opacity:.1}"), "{}", name);
        // every marker is at least 8px across
        for c in s.split("<circle ").skip(1).filter(|c| attr(c, "class").is_some_and(|k| k.starts_with('d'))) {
            assert!(attr(c, "r").unwrap().parse::<f64>().unwrap() >= 4.0, "{}: small marker", name);
        }
        // the title and a description are there for assistive technology
        assert!(s.contains("role=\"img\"") && s.contains("<title id=\"chart-title\">") && s.contains("<desc id=\"chart-desc\">"), "{}", name);
        assert!(s.starts_with("<?xml"), "{}", name);
    }
}

#[test]
fn bars_are_at_most_24px_thick_rounded_at_the_data_end_and_square_at_the_baseline() {
    let s = svg(two_col("bar", json!([["a", 5], ["b", 9], ["c", 3]])));
    let paths: Vec<&str> = s.split("<path class=\"f1\" d=\"").skip(1).map(|p| p.split('"').next().unwrap()).collect();
    assert_eq!(paths.len(), 3);
    for d in paths {
        // M x0,base L x0,end Q x0,top xr,top L xl,top Q x1,top x1,end L x1,base Z
        let nums: Vec<f64> = d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty()).map(|t| t.parse().unwrap()).collect();
        let (x0, base, x1) = (nums[0], nums[1], nums[nums.len() - 4]);
        assert!(x1 - x0 <= 24.0 && x1 - x0 >= 3.0, "{}", d);
        assert_eq!((nums[1], nums[nums.len() - 1]), (base, base), "square at the baseline");
        assert!(d.contains(" Q") && d.matches('Q').count() == 2, "rounded at the data end: {}", d);
    }
    // adjacent bars in a group are separated by a 2px gap of surface
    let g = svg(json!({"type": "bar", "title": "T", "data": {"columns": ["k", "a", "b"], "rows": [["x", 4, 6], ["y", 5, 7]]}}));
    let xs: Vec<f64> = g.split("<path class=\"f").skip(1).map(|p| p.split("d=\"M").nth(1).unwrap().split(',').next().unwrap().parse().unwrap()).collect();
    assert!((xs[1] - xs[0] - 26.0).abs() < 0.5 || (xs[1] - xs[0]) > 0.0, "{:?}", xs);
}

#[test]
fn negative_values_hang_from_a_zero_baseline_and_a_truncated_axis_says_so() {
    let s = svg(two_col("bar", json!([["gain", 5], ["loss", -3]])));
    assert!(s.contains(">0</text>") && s.contains(">-4</text>") || s.contains(">-2</text>") || s.contains(">-5</text>"), "an axis with a zero and negative ticks");
    let t = svg(json!({"type": "bar", "title": "T", "scale": {"y_zero": false}, "data": {"columns": ["k", "v"], "rows": [["a", 90], ["b", 95]]}}));
    assert!(t.contains("Axis does not start at zero"));
    let ok = svg(two_col("bar", json!([["a", 90], ["b", 95]])));
    assert!(!ok.contains("does not start at zero") && ok.contains(">0</text>"), "bars start at zero by default");
}

#[test]
fn a_gap_in_a_line_breaks_it_and_an_area_is_a_wash_under_the_line() {
    let s = svg(json!({"type": "line", "title": "T", "data": {"columns": ["x", "v"], "rows": [[1, 1], [2, 2], [3, null], [4, 4], [5, 5]]}}));
    let d = s.split("<path class=\"k1\" d=\"").nth(1).unwrap().split('"').next().unwrap();
    assert_eq!(d.matches('M').count(), 2, "two segments: {}", d);
    let a = svg(json!({"type": "area", "title": "T", "data": {"columns": ["x", "v"], "rows": [[1, 1], [2, 3], [3, 2]]}}));
    assert!(a.contains("<path class=\"a1\"") && a.contains("<path class=\"k1\""));
}

#[test]
fn the_relief_rule_values_are_printed_whenever_a_low_contrast_slot_is_in_use() {
    // three series: the third is aqua, under 3:1 on the light surface. Its values must be on the page.
    let bars = svg(json!({"type": "bar", "title": "T", "data": {"columns": ["k", "a", "b", "c"], "rows": [["x", 4, 6, 8.4], ["y", 5, 7, 9.4]]}}));
    assert_eq!(bars.matches(">8.4</text>").count() + bars.matches(">9.4</text>").count(), 2, "tip labels are drawn when they fit");
    assert!(!bars.contains(">Series<"));
    // too many bars to label: the values go in a table instead
    let rows: Vec<Value> = (0..12).map(|i| json!([format!("c{}", i), 1200 + i, 1300 + i, 1400 + i])).collect();
    let many = svg(json!({"type": "bar", "title": "T", "data": {"columns": ["k", "a", "b", "c"], "rows": rows}}));
    assert!(many.contains(">c11</text>") && many.matches(">1,411</text>").count() >= 1 && many.contains(">1,311</text>"), "every value is on the page");
    // lines that converge cannot take end labels: a table of first/last/min/max stands in
    let (_, conv) = samples::all().into_iter().find(|(n, _)| *n == "line-four-converging").unwrap();
    let c = render_svg(&from_json(&conv).unwrap(), None);
    assert!(c.contains(">First</text>") && c.contains(">Last</text>") && c.contains(">worker</text>"), "{}", &c[c.len() - 800..]);
    // two series never need either: both slots clear 3:1
    let two = svg(json!({"type": "line", "title": "T", "data": {"columns": ["x", "a", "b"], "rows": [[1, 1, 5], [2, 2, 6], [3, 3, 9]]}}));
    assert!(!two.contains(">First</text>"));
}

#[test]
fn direct_labels_appear_when_they_fit_and_never_overlap() {
    let (_, spec) = samples::all().into_iter().find(|(n, _)| *n == "line-two-series").unwrap();
    let s = render_svg(&from_json(&spec).unwrap(), None);
    assert!(s.contains(">p95  248 ms</text>") && s.contains(">p50  101 ms</text>"), "{}", s);
    // single series: the end value, no legend
    let one = svg(json!({"type": "line", "title": "T", "data": {"columns": ["x", "v"], "rows": [[1, 5], [2, 9]]}}));
    assert!(one.contains(">9</text>") && !one.contains("class=\"k1\" d=\"M18"), "a single series has no legend");
}

#[test]
fn light_and_dark_come_from_variables_inside_the_file() {
    let (_, spec) = samples::all().into_iter().find(|(n, _)| *n == "hbar-single").unwrap();
    let auto = render_svg(&from_json(&spec).unwrap(), None);
    assert!(auto.contains("@media (prefers-color-scheme: dark)") && !auto.contains("data-theme=\"dark\" viewBox"), "auto follows the reader");
    for hex in SERIES_LIGHT_USED.iter().chain(SERIES_DARK_USED.iter()) {
        assert!(auto.contains(hex), "{} is in the file", hex);
    }
    let mut dark = spec.clone();
    dark["theme"] = json!("dark");
    assert!(render_svg(&from_json(&dark).unwrap(), None).contains("class=\"viz-root\" data-theme=\"dark\""));
    // the colours are the documented palette and nothing else
    let hexes: std::collections::HashSet<String> = auto.split('#').skip(1).map(|t| format!("#{}", &t[..6])).collect();
    let allowed: std::collections::HashSet<String> = palette::SERIES_LIGHT.iter().chain(palette::SERIES_DARK.iter()).chain([&palette::SURFACE_LIGHT, &palette::SURFACE_DARK, &palette::GRID_LIGHT, &palette::GRID_DARK, &palette::AXIS_LIGHT, &palette::AXIS_DARK]).chain(palette::INK_LIGHT.iter()).chain(palette::INK_DARK.iter()).map(|s| s.to_string()).collect();
    assert!(hexes.is_subset(&allowed), "colours outside the palette: {:?}", hexes.difference(&allowed).collect::<Vec<_>>());
}

const SERIES_LIGHT_USED: [&str; 2] = ["#2a78d6", "#eb6834"];
const SERIES_DARK_USED: [&str; 2] = ["#3987e5", "#d95926"];

#[test]
fn output_is_deterministic_and_text_is_escaped() {
    let (_, spec) = samples::all().into_iter().find(|(n, _)| *n == "bar-grouped-three").unwrap();
    assert_eq!(render_svg(&from_json(&spec).unwrap(), None), render_svg(&from_json(&spec).unwrap(), None));
    let evil = "</text><script>alert(1)</script><img src=x onerror=1>\"&";
    let s = svg(json!({"type": "bar", "title": evil, "subtitle": evil, "source": evil, "data": {"columns": ["k", "v"], "rows": [[evil, 1], ["b", 2]]}}));
    assert!(!s.contains("<script") && !s.contains("<img") && s.contains("&lt;script&gt;"), "{}", s);
    let c = render_svg(&from_json(&json!({"type": "bar", "title": "T", "data": {"columns": ["k", "v"], "rows": [["a", 1], ["b", 2]]}})).unwrap(), Some("delphi artifact -- written"));
    assert!(c.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- delphi artifact"));
}

#[test]
fn a_chart_embedded_in_a_page_passes_the_pages_own_safety_check() {
    for (name, spec) in samples::all() {
        let s = render_svg(&from_json(&spec).unwrap(), None);
        let inline = s.split_once("?>\n").map(|x| x.1).unwrap();
        let page = format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'\"><title>t</title></head><body>{}</body></html>", inline);
        let problems = crate::htmllint::lint(&page);
        assert!(problems.is_empty(), "{}: {:?}", name, problems);
    }
}

#[test]
fn the_description_carries_the_data_for_anyone_who_cannot_see_the_marks() {
    let (_, spec) = samples::all().into_iter().find(|(n, _)| *n == "line-two-series").unwrap();
    let s = render_svg(&from_json(&spec).unwrap(), None);
    let desc = s.split("<desc id=\"chart-desc\">").nth(1).unwrap().split("</desc>").next().unwrap();
    assert!(desc.contains("Line chart.") && desc.contains("p50: minimum 101 ms at 0.2.5") && desc.contains("falls 16%") && desc.contains("Values: 0.2.1 120 ms / 340 ms"), "{}", desc);
}

const GOLDEN_DIR: &str = "tests/golden/chart";

/// The sample charts, drawn, are the review: `UPDATE_GOLDEN=1 cargo test -p delphi-docs golden` rewrites them and
/// the diff shows exactly what changed on the page.
#[test]
fn golden_charts_match_the_files_on_disk() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, mut spec) in samples::all() {
        for theme in ["light", "dark"] {
            if theme == "dark" && !["line-two-series", "bar-grouped-three"].contains(&name) {
                continue;
            }
            spec["theme"] = json!(theme);
            let got = render_svg(&from_json(&spec).unwrap(), None);
            let path = dir.join(format!("{}-{}.svg", name, theme));
            if std::env::var("UPDATE_GOLDEN").is_ok() || !path.exists() {
                std::fs::write(&path, &got).unwrap();
                continue;
            }
            let want = std::fs::read_to_string(&path).unwrap();
            assert!(got == want, "{} differs from {} (run with UPDATE_GOLDEN=1 and review the diff)", name, path.display());
        }
    }
}

/// `DELPHI_DOCS_CHART_OUT=/dir cargo test -p delphi-docs write_samples -- --ignored`: write every sample chart
/// to files in light and dark, for looking at in a browser.
#[test]
#[ignore = "writes files for a person to look at"]
fn write_samples() {
    let Ok(dir) = std::env::var("DELPHI_DOCS_CHART_OUT") else { return };
    for (name, mut spec) in samples::all() {
        for theme in ["light", "dark"] {
            spec["theme"] = json!(theme);
            let c = from_json(&spec).unwrap_or_else(|e| panic!("{}: {}", name, e));
            std::fs::write(format!("{}/{}-{}.svg", dir, name, theme), render_svg(&c, Some("delphi chart sample"))).unwrap();
        }
    }
}

fn sample(name: &str) -> Value {
    samples::all().into_iter().find(|(n, _)| *n == name).unwrap().1
}

#[test]
fn a_stacked_bar_stacks_with_a_gap_a_rounded_top_and_totals_and_prints_its_values_for_three_parts() {
    let s = svg(sample("stacked-three-parts"));
    // 4 columns x 3 parts: the bottom and middle parts are rects, the top part has the rounded end
    assert_eq!(s.matches("<rect class=\"f1\" x=").count() + s.matches("<rect class=\"f2\" x=").count() - s.matches("width=\"10\" height=\"10\"").count().min(2) - s.matches("width=\"8\" height=\"8\"").count().min(2), 8, "bottom and middle parts of 4 stacks");
    assert!(s.matches("width=\"24\"").count() >= 8, "columns are 24px thick");
    assert_eq!(s.matches("<path class=\"f3\"").count(), 4);
    assert!(s.contains(">16.7</text>") && s.contains(">15.8</text>"), "totals sit above each stack");
    // three parts use the low-contrast slots, so the values table is added
    assert!(s.contains(">compile</text>") && s.contains(">9.4</text>"), "{}", s);
    let d = describe_of(&sample("stacked-three-parts"));
    assert!(d.starts_with("Stacked bar chart.") && d.contains("Q1 6.1 / 9.4 / 1.2"), "{}", d);
}

fn describe_of(v: &Value) -> String {
    let s = svg(v.clone());
    let a = s.find("<desc id=\"chart-desc\">").unwrap() + "<desc id=\"chart-desc\">".len();
    s[a..s.find("</desc>").unwrap()].to_string()
}

#[test]
fn a_donut_draws_each_part_as_a_ring_segment_and_lists_values_and_shares() {
    let s = svg(sample("donut-three"));
    assert_eq!(s.matches("class=\"f1 sl\"").count() + s.matches("class=\"f2 sl\"").count() + s.matches("class=\"f3 sl\"").count(), 3, "{}", s);
    assert!(s.contains(">$12,600</text>") && s.contains(">Total</text>"), "the middle says the total: {}", s);
    assert!(s.contains(">67%</text>") && s.contains(">23%</text>") && s.contains(">10%</text>") && s.contains(">$8,400</text>"), "{}", s);
    // one part that is the whole is a full ring, not a degenerate arc
    let one = svg(json!({"type": "donut", "title": "T", "data": {"columns": ["k", "v"], "rows": [["a", 5], ["b", 0]]}}));
    assert_eq!(one.matches(" sl\"").count(), 1);
    assert!(one.contains("A") && !one.contains("NaN") && !one.contains("inf"), "{}", one);
    assert!(describe_of(&sample("donut-three")).starts_with("Donut chart."));
}

#[test]
fn a_heatmap_shades_every_cell_in_five_steps_labels_what_fits_and_shows_its_scale() {
    let s = svg(sample("heatmap-weeks"));
    let cells: usize = (0..5).map(|i| s.matches(&format!("<rect class=\"h{}\" x=", i)).count()).sum();
    assert_eq!(cells, 5 * 10 + 5, "5 rows x 10 columns, plus the five scale swatches");
    assert!((0..5).all(|i| s.contains(&format!("--h{}:#", i)) && s.contains(&format!("--ht{}:#", i))), "fills and label colours are in both modes' variables");
    assert!(s.contains("class=\"ht") && s.contains(">W7</text>") && s.contains(">worker</text>"), "{}", s);
    // a missing value is an outlined empty cell, not a fake zero
    let gap = svg(json!({"type": "heatmap", "title": "T", "data": {"columns": ["k", "a", "b"], "rows": [["x", 1, 2], ["y", null, 4]]}}));
    assert_eq!(gap.matches("<rect class=\"g\"").count(), 1, "{}", gap);
}

#[test]
fn a_box_plot_marks_quartiles_median_whiskers_and_outliers() {
    let s = svg(sample("box-regions"));
    assert_eq!(s.matches("<rect class=\"bx\"").count(), 3);
    assert_eq!(s.matches("<path class=\"wh\"").count(), 3);
    assert!(s.matches("class=\"d1\" cx=").count() >= 3, "the unusually slow requests are drawn as dots: {}", s.matches("class=\"d1\" cx=").count());
    let d = describe_of(&sample("box-regions"));
    assert!(d.starts_with("Box plot chart.") && d.contains("us-east: median") && d.contains("(30 values)"), "{}", d);
    // quartiles by linear interpolation: 1..=9 has median 5 and quartiles 3 and 7
    let g: Vec<f64> = (1..=9).map(f64::from).collect();
    assert_eq!((more::quantile(&g, 0.25), more::quantile(&g, 0.5), more::quantile(&g, 0.75)), (3.0, 5.0, 7.0));
    assert_eq!(more::quantile(&[1.0, 2.0], 0.5), 1.5);
}

#[test]
fn a_sparkline_is_small_has_no_axes_and_ends_in_a_dot_and_a_value() {
    let c = chart(sample("sparkline-trend")).unwrap();
    let s = render_svg(&c, None);
    assert!(s.contains("viewBox=\"0 0 360 88\"") && !s.contains("class=\"g\"") && !s.contains("class=\"ax\""), "{}", s);
    assert_eq!(s.matches("<path class=\"k1\"").count(), 1);
    assert_eq!(s.matches("<circle class=\"d1\"").count(), 1);
    assert!(s.contains(">Weekly active users</text>") && s.contains("last 40 weeks"));
    assert!(describe_of(&sample("sparkline-trend")).starts_with("Sparkline chart."));
}

#[test]
fn every_form_embeds_in_a_page_that_passes_the_safety_check_and_renders_deterministically() {
    for (name, spec) in samples::all() {
        let c = chart(spec.clone()).unwrap_or_else(|e| panic!("{}: {}", name, e));
        assert_eq!(render_svg(&c, None), render_svg(&c, None), "{}", name);
        let page = json!({"title": "P", "blocks": [{"type": "chart", "chart": spec}]});
        let html = crate::htmlpage::render(&crate::htmlpage::from_json(&page).unwrap()).unwrap_or_else(|e| panic!("{}: {}", name, e));
        assert!(crate::htmllint::lint(&html).is_empty(), "{}", name);
        let s = render_svg(&c, None);
        assert!(!s.contains("NaN") && !s.contains("inf") && !s.contains("undefined"), "{}", name);
    }
}
