use super::*;
use serde_json::json;

pub(crate) fn samples() -> Vec<(&'static str, Value)> {
    vec![
        ("flow-deploy", json!({
            "type": "flow", "title": "A failed smoke test rolls the release back", "subtitle": "release pipeline",
            "nodes": [
                {"id": "merge", "label": "Merge to main", "kind": "start", "group": "CI"},
                {"id": "build", "label": "Build and unit tests", "group": "CI"},
                {"id": "gate", "label": "All tests pass?", "kind": "decision", "group": "CI"},
                {"id": "stage", "label": "Deploy to staging", "group": "CD"},
                {"id": "smoke", "label": "Smoke tests", "group": "CD"},
                {"id": "ok", "label": "Healthy?", "kind": "decision", "group": "CD"},
                {"id": "prod", "label": "Promote to production", "group": "CD"},
                {"id": "back", "label": "Roll back and page the owner", "group": "CD"},
                {"id": "done", "label": "Done", "kind": "end"}
            ],
            "edges": [
                {"from": "merge", "to": "build"}, {"from": "build", "to": "gate"},
                {"from": "gate", "to": "stage", "label": "yes"}, {"from": "gate", "to": "back", "label": "no"},
                {"from": "stage", "to": "smoke"}, {"from": "smoke", "to": "ok"},
                {"from": "ok", "to": "prod", "label": "yes"}, {"from": "ok", "to": "back", "label": "no"},
                {"from": "prod", "to": "done"}, {"from": "back", "to": "merge", "label": "fix and retry"}
            ]})),
        ("flow-right", json!({
            "type": "flow", "title": "Documents become pages in four steps", "direction": "right",
            "nodes": [
                {"id": "read", "label": "read_doc", "group": "Read"}, {"id": "plan", "label": "Plan the sections", "group": "Write"},
                {"id": "chart", "label": "write_chart", "group": "Write"}, {"id": "report", "label": "write_report", "group": "Write"},
                {"id": "open", "label": "Operator opens it", "kind": "end"}
            ],
            "edges": [{"from": "read", "to": "plan"}, {"from": "plan", "to": "chart"}, {"from": "plan", "to": "report"}, {"from": "chart", "to": "report", "label": "embed"}, {"from": "report", "to": "open"}]})),
        ("tree-crates", json!({
            "type": "tree", "title": "Delphi is a handful of crates", "subtitle": "what depends on what is the layout of the workspace",
            "root": {"label": "delphi", "children": [
                {"label": "delphi-core", "children": [{"label": "pathguard"}, {"label": "provenance"}, {"label": "sandbox policy"}]},
                {"label": "delphi-engine", "children": [{"label": "tools"}, {"label": "artifacts"}, {"label": "approvals"}]},
                {"label": "delphi-docs", "children": [{"label": "readers"}, {"label": "chart engine"}, {"label": "diagrams"}]},
                {"label": "delphi-adapter"}]}})),
        ("sequence-approval", json!({
            "type": "sequence", "title": "A scripted page always asks the operator",
            "participants": [{"id": "agent", "label": "Agent"}, {"id": "engine", "label": "Engine"}, {"id": "op", "label": "Operator"}, {"id": "fs", "label": "Mission folder"}],
            "messages": [
                {"from": "agent", "to": "engine", "label": "write_html(page, script)"},
                {"from": "engine", "to": "engine", "label": "check the page and its script"},
                {"from": "engine", "to": "op", "label": "approval card with the full script"},
                {"from": "op", "to": "engine", "label": "approve once", "kind": "reply"},
                {"from": "engine", "to": "fs", "label": "write page and provenance record"},
                {"from": "engine", "to": "agent", "label": "created report.html", "kind": "reply"}]})),
    ]
}

fn diagram(v: Value) -> Result<DiagramSpec, DocError> {
    from_json(&v)
}

fn sample(name: &str) -> Value {
    samples().into_iter().find(|(n, _)| *n == name).unwrap().1
}

fn rejected(v: Value) -> String {
    diagram(v).unwrap_err().0
}

fn flow(nodes: Value, edges: Value) -> Value {
    json!({"type": "flow", "title": "T", "nodes": nodes, "edges": edges})
}

#[test]
fn each_rule_refuses_with_a_reason_that_says_what_would_work() {
    let two = json!([{"id": "a"}, {"id": "b"}]);
    for (spec, expect) in [
        (json!({"title": "T"}), "`type`"),
        (json!({"type": "flow"}), "`title`"),
        (json!({"type": "flow", "title": "T"}), "needs `nodes`"),
        (json!({"type": "radar", "title": "T"}), "unknown diagram type"),
        (json!({"type": "timeline", "title": "T"}), "not available yet"),
        (json!({"type": "flow", "title": "T", "theme": "neon", "nodes": two.clone()}), "`theme`"),
        (flow(json!([]), json!([])), "between 1 and 60"),
        (flow(json!([{"id": "a"}, {"id": "a"}]), json!([])), "two nodes have the id `a`"),
        (flow(json!([{"label": "x"}]), json!([])), "needs an `id`"),
        (flow(json!([{"id": "a", "kind": "oval"}]), json!([])), "use step, start, end or decision"),
        (flow(two.clone(), json!([{"from": "a", "to": "zz"}])), "`zz`, which is not a node; the nodes are `a`, `b`"),
        (flow(two.clone(), json!([{"from": "a", "to": "a"}])), "to itself"),
        (flow(two.clone(), json!([{"from": "a", "to": "b"}, {"from": "a", "to": "b"}])), "listed twice"),
        (flow(json!([{"id": "a", "group": "1"}, {"id": "b", "group": "2"}, {"id": "c", "group": "3"}, {"id": "d", "group": "4"}, {"id": "e", "group": "5"}]), json!([])), "at most 4 groups"),
        (json!({"type": "flow", "title": "T", "direction": "up", "nodes": two.clone()}), "down or right"),
        (json!({"type": "tree", "title": "T"}), "needs a `root`"),
        (json!({"type": "tree", "title": "T", "root": {"label": "only"}}), "just a label"),
        (json!({"type": "tree", "title": "T", "root": {"children": []}}), "needs a `label`"),
        (json!({"type": "sequence", "title": "T", "participants": [{"id": "a"}], "messages": []}), "between 2 and 8"),
        (json!({"type": "sequence", "title": "T", "participants": [{"id": "a"}, {"id": "b"}], "messages": [{"from": "a", "to": "c", "label": "x"}]}), "who is not a participant; the participants are `a`, `b`"),
        (json!({"type": "sequence", "title": "T", "participants": [{"id": "a"}, {"id": "b"}], "messages": [{"from": "a", "to": "b"}]}), "needs a `label`"),
        (json!({"type": "sequence", "title": "T", "participants": [{"id": "a"}, {"id": "b"}], "messages": [{"from": "a", "to": "b", "label": "x", "kind": "shout"}]}), "call or reply"),
    ] {
        let e = rejected(spec.clone());
        assert!(e.contains(expect), "expected {:?} in: {}\n(for {})", expect, e, spec);
    }
    let many: Vec<Value> = (0..61).map(|i| json!({"id": format!("n{}", i)})).collect();
    assert!(rejected(flow(json!(many), json!([]))).contains("between 1 and 60"));
    // a tree deeper than 8, and wider than 80 nodes
    let mut deep = json!({"label": "leaf"});
    for i in 0..9 {
        deep = json!({"label": format!("l{}", i), "children": [deep]});
    }
    assert!(rejected(json!({"type": "tree", "title": "T", "root": deep})).contains("levels deep"));
    let wide = json!({"type": "tree", "title": "T", "root": {"label": "r", "children": (0..80).map(|i| json!({"label": format!("c{}", i)})).collect::<Vec<_>>()}});
    assert!(rejected(wide).contains("at most 80 nodes"));
}

/// The boxes of the nodes, in node order, read back from the drawing: (x0, y0, x1, y1).
fn node_boxes(svg: &str) -> Vec<(f64, f64, f64, f64)> {
    let num = |tag: &str, k: &str| -> f64 { tag.split(&format!(" {}=\"", k)).nth(1).unwrap().split('"').next().unwrap().parse().unwrap() };
    let mut out = vec![];
    for tag in svg.split('<').skip(1) {
        let class = tag.split("class=\"").nth(1).and_then(|c| c.split('"').next()).unwrap_or("");
        if !(class == "nd" || (class.starts_with("nd") && class.len() == 3)) {
            continue;
        }
        if tag.starts_with("rect") {
            let (x, y, w, h) = (num(tag, "x"), num(tag, "y"), num(tag, "width"), num(tag, "height"));
            if w > 14.0 || h > 14.0 {
                out.push((x, y, x + w, y + h));
            }
        } else if tag.starts_with("path") {
            let d = tag.split(" d=\"").nth(1).unwrap().split('"').next().unwrap();
            let n: Vec<f64> = d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty()).map(|t| t.parse().unwrap()).collect();
            let xs: Vec<f64> = n.iter().step_by(2).copied().collect();
            let ys: Vec<f64> = n.iter().skip(1).step_by(2).copied().collect();
            out.push((xs.iter().cloned().fold(f64::INFINITY, f64::min), ys.iter().cloned().fold(f64::INFINITY, f64::min), xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max), ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)));
        }
    }
    out
}

#[test]
fn flow_nodes_never_overlap_and_edges_run_forward_except_the_ones_that_close_a_cycle() {
    let spec = diagram(sample("flow-deploy")).unwrap();
    let svg = render_svg(&spec, None);
    let boxes = node_boxes(&svg);
    assert_eq!(boxes.len(), 9, "{}", svg);
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            let apart = a.2 + 4.0 <= b.0 || b.2 + 4.0 <= a.0 || a.3 + 4.0 <= b.1 || b.3 + 4.0 <= a.1;
            assert!(apart, "{:?} overlaps {:?}", a, b);
        }
    }
    let y = |i: usize| (boxes[i].1 + boxes[i].3) / 2.0;
    // merge(0) build(1) gate(2) stage(3) smoke(4) ok(5) prod(6) back(7) done(8): forward edges go down the page
    for (a, b) in [(0, 1), (1, 2), (2, 3), (2, 7), (3, 4), (4, 5), (5, 6), (5, 7), (6, 8)] {
        assert!(y(b) > y(a), "{} -> {} does not go down", a, b);
    }
    // back -> merge closes a cycle: it is drawn pointing up, with its arrowhead at the top
    assert!(y(0) < y(7));
    assert!(svg.contains(">fix and retry</text>") && svg.contains(">yes</text>") && svg.contains(">no</text>"));
    assert_eq!(svg.matches("<path class=\"ar\"").count(), 10, "one arrowhead per edge");
    assert_eq!(svg.matches("<path class=\"ed\"").count(), 10);
}

#[test]
fn a_right_flow_runs_left_to_right() {
    let svg = render_svg(&diagram(sample("flow-right")).unwrap(), None);
    let b = node_boxes(&svg);
    assert_eq!(b.len(), 5);
    let cx = |i: usize| (b[i].0 + b[i].2) / 2.0;
    assert!(cx(0) < cx(1) && cx(1) < cx(2) && cx(2) < cx(3) && cx(3) < cx(4), "{:?}", b);
}

#[test]
fn a_tree_centres_each_parent_over_its_children_without_overlaps() {
    let svg = render_svg(&diagram(sample("tree-crates")).unwrap(), None);
    let b = node_boxes(&svg);
    assert_eq!(b.len(), 14);
    for (i, a) in b.iter().enumerate() {
        for c in &b[i + 1..] {
            assert!(a.2 + 4.0 <= c.0 || c.2 + 4.0 <= a.0 || a.3 + 4.0 <= c.1 || c.3 + 4.0 <= a.1, "{:?} overlaps {:?}", a, c);
        }
    }
    let cx = |i: usize| (b[i].0 + b[i].2) / 2.0;
    // node order is depth-first: 0 root, 1 core (2,3,4), 5 engine (6,7,8), 9 docs (10,11,12), 13 adapter
    assert!((cx(1) - cx(3)).abs() < 1.0 && (cx(5) - cx(7)).abs() < 1.0 && (cx(9) - cx(11)).abs() < 1.0, "parents sit over the middle child");
    assert!(b[0].1 < b[1].1 && b[1].1 < b[2].1, "levels run down the page");
}

#[test]
fn a_sequence_keeps_message_order_and_gives_every_label_room() {
    let svg = render_svg(&diagram(sample("sequence-approval")).unwrap(), None);
    assert_eq!(svg.matches("<line class=\"ll\"").count(), 4, "one lifeline per participant");
    for label in ["write_html(page, script)", "check the page and its script", "approval card with the full script", "approve once", "write page and provenance record", "created report.html"] {
        assert!(svg.contains(&format!(">{}</text>", label)), "{} is whole and present", label);
    }
    // replies are the muted style, calls the strong one; a self message is a loop with its own arrowhead
    assert_eq!(svg.matches("class=\"ed2\"").count(), 2);
    assert_eq!(svg.matches("<path class=\"ar\"").count(), 4);
    let d = &describe(&svg);
    assert!(d.starts_with("Sequence diagram between Agent, Engine, Operator, Mission folder.") && d.contains("2. Engine to Engine: check the page and its script"), "{}", d);
}

fn describe(svg: &str) -> String {
    let a = svg.find("<desc id=\"chart-desc\">").unwrap() + "<desc id=\"chart-desc\">".len();
    svg[a..svg.find("</desc>").unwrap()].to_string()
}

#[test]
fn every_diagram_is_deterministic_accessible_and_embeds_in_a_page() {
    for (name, spec) in samples() {
        let d = diagram(spec.clone()).unwrap_or_else(|e| panic!("{}: {}", name, e));
        let (a, b) = (render_svg(&d, None), render_svg(&d, None));
        assert_eq!(a, b, "{}", name);
        assert!(a.contains("role=\"img\"") && a.contains("<title id=\"chart-title\">") && describe(&a).len() > 40, "{}", name);
        assert!(!a.contains("NaN") && !a.contains("inf\"") && !a.contains("<script"), "{}", name);
        let inline = inline_svg(&d, "d1");
        assert!(!inline.contains("<style") && !inline.contains("<?xml") && inline.contains("aria-labelledby=\"d1-chart-title d1-chart-desc\""), "{}", name);
        for dark in [false, true] {
            let mut s = spec.clone();
            s["theme"] = json!(if dark { "dark" } else { "light" });
            let svg = render_svg(&diagram(s).unwrap(), None);
            let png = crate::raster::svg_to_png(&svg, dark, 2, &[("Title", name)]).unwrap_or_else(|e| panic!("{}: {}", name, e));
            let img = crate::png::decode(&png).unwrap();
            let surface = img.pixel(1, 1);
            let drawn = img.rgba.chunks(4).filter(|p| p[..3] != surface[..3]).count();
            assert!(drawn * 100 > img.width * img.height, "{}: almost nothing was drawn", name);
        }
    }
}

#[test]
fn text_from_the_description_cannot_break_the_picture() {
    let evil = "\"><script>alert(1)</script>&";
    let d = diagram(json!({"type": "flow", "title": evil, "subtitle": evil, "source": evil, "nodes": [{"id": "a", "label": evil, "group": evil}, {"id": "b", "label": "b"}], "edges": [{"from": "a", "to": "b", "label": evil}]})).unwrap();
    let svg = render_svg(&d, None);
    assert!(!svg.contains("<script") && svg.contains("&lt;script&gt;"), "{}", svg);
}

#[test]
fn a_huge_diagram_is_refused_as_a_png_with_a_pointer_to_svg() {
    let nodes: Vec<Value> = (0..40).map(|i| json!({"id": format!("n{}", i), "label": format!("Step number {} of the process", i)})).collect();
    let edges: Vec<Value> = (1..40).map(|i| json!({"from": format!("n{}", (i - 1) / 2), "to": format!("n{}", i)})).collect();
    let big = diagram(flow(json!(nodes), json!(edges))).unwrap();
    assert!(png_fit(&big).unwrap_err().0.contains(".svg"));
    assert!(png_fit(&diagram(sample("flow-deploy")).unwrap()).is_ok());
}

/// `DELPHI_DOCS_CHART_OUT=/dir cargo test -p delphi-docs write_diagram_samples -- --ignored`: every sample as an SVG
/// and a PNG, light and dark.
#[test]
#[ignore = "writes files for a person to look at"]
fn write_diagram_samples() {
    let Ok(dir) = std::env::var("DELPHI_DOCS_CHART_OUT") else { return };
    for (name, mut spec) in samples() {
        for dark in [false, true] {
            spec["theme"] = json!(if dark { "dark" } else { "light" });
            let svg = render_svg(&diagram(spec.clone()).unwrap(), Some("delphi diagram sample"));
            let m = if dark { "dark" } else { "light" };
            std::fs::write(format!("{}/{}-{}.svg", dir, name, m), &svg).unwrap();
            std::fs::write(format!("{}/{}-{}.png", dir, name, m), crate::raster::svg_to_png(&svg, dark, 2, &[("Title", name)]).unwrap()).unwrap();
        }
    }
}
