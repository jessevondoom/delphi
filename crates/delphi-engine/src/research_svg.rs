//! SVG pictures drawn to PNG at capture (`specs/118` §4 item 2, §9): the one parser of untrusted input that spec 118 adds.
//!
//! An SVG a trusted host serves is never kept to be shown as SVG. The daemon parses it here, checks it against fixed bounds, draws
//! it with `resvg` into a PNG of at most [`MAX_SIDE`] pixels a side, and keeps the SVG only beside that PNG, for the record; nothing
//! serves the SVG. What makes this safe to run on a page's file:
//!
//! - **No script.** `resvg` has no script engine: `<script>`, event attributes and `<foreignObject>` are not run, only skipped.
//! - **No request, no file.** Every `<image>` reference, a `data:` one included, is refused by the resolver set here, so the renderer
//!   reads no file and asks no network for one (it has no network code of its own). No resources folder is set. Text is not drawn,
//!   so no font is looked up or read, and an SVG that has text is refused rather than kept without its words.
//! - **Bounded work.** At most [`MAX_SVG_BYTES`] of input; no DTD (so no entity can expand); at most [`MAX_XML_NODES`] XML nodes and
//!   [`MAX_DEPTH`] levels; at most [`MAX_EXPANDED`] elements once every `use` and marker is counted where it is drawn (a reference
//!   cycle is refused); no `pattern`, `filter` or `mask` (each draws into a picture of its own whose size the file chooses); at most
//!   [`MAX_LAYERS`] groups that draw into a layer, none nested deeper than [`MAX_LAYER_DEPTH`] and none larger than [`MAX_LAYER_AREA`]
//!   canvases; a canvas of at most [`MAX_SIDE`] pixels a side; and [`TIME_LIMIT`] for all of it, after which the picture is refused.
//!
//! Every refusal is a fixed sentence chosen here, never words of the file.

use resvg::tiny_skia;
use resvg::usvg;
use resvg::usvg::roxmltree;
use std::collections::HashMap;
use std::time::Duration;

/// An SVG larger than this is refused before it is parsed (§4 item 2).
pub const MAX_SVG_BYTES: usize = 2 * 1024 * 1024;
/// The drawn PNG is at most this many pixels a side: the image method's own ceiling (`kbserve::IMAGE_SIDE_MAX`).
pub const MAX_SIDE: u32 = 1_600;
/// XML nodes the parser may build (elements, text and attributes' nodes as `roxmltree` counts them).
pub const MAX_XML_NODES: u32 = 200_000;
/// How deep elements may nest.
pub const MAX_DEPTH: u32 = 64;
/// Elements once each `use` is replaced by what it names and each marker counted at every vertex it may be drawn on.
pub const MAX_EXPANDED: u64 = 50_000;
/// Groups that draw into a layer of their own (opacity, a clip, a blend), and how deep such groups may nest.
pub const MAX_LAYERS: usize = 256;
pub const MAX_LAYER_DEPTH: usize = 4;
/// The largest layer, in canvases: a layer is a picture as large as the group's bounds.
pub const MAX_LAYER_AREA: f64 = 2.0;
/// Parsing and drawing together; past this the picture is refused (the work, already bounded above, finishes on its own thread).
pub const TIME_LIMIT: Duration = Duration::from_secs(10);

/// Elements whose drawing makes a picture of its own whose size the file decides, or that run something; refused outright.
const REFUSED_ELEMENTS: [&str; 3] = ["pattern", "filter", "mask"];

/// A drawn SVG: the PNG's bytes and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawn {
    pub png: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

/// Draw `svg` to a PNG under every bound of this module. `Err` is a fixed sentence saying why it was refused.
pub fn render(svg: &[u8]) -> Result<Drawn, &'static str> {
    if svg.len() > MAX_SVG_BYTES {
        return Err("an SVG larger than 2 MiB is not drawn");
    }
    let bytes = svg.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("svg-render".into()).stack_size(8 << 20).spawn(move || {
        let _ = tx.send(render_now(&bytes));
    });
    if spawned.is_err() {
        return Err("the SVG could not be drawn");
    }
    match rx.recv_timeout(TIME_LIMIT) {
        Ok(r) => r,
        Err(_) => Err("the SVG took too long to draw"),
    }
}

/// [`render`] on the calling thread, with no time limit of its own.
fn render_now(svg: &[u8]) -> Result<Drawn, &'static str> {
    let text = std::str::from_utf8(svg).map_err(|_| "the SVG is not UTF-8 text (a compressed SVG is not drawn)")?;
    let xml_opt = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: MAX_XML_NODES, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(text, xml_opt).map_err(|e| match e {
        roxmltree::Error::DtdDetected => "the SVG declares a DTD, which is not read",
        roxmltree::Error::NodesLimitReached => "the SVG has too many parts to draw",
        _ => "the SVG could not be drawn: it is not well-formed XML",
    })?;
    audit_xml(&doc)?;
    let opt = options();
    let tree = usvg::Tree::from_xmltree(&doc, &opt).map_err(|e| match e {
        usvg::Error::ElementsLimitReached => "the SVG has too many parts to draw",
        usvg::Error::InvalidSize => "the SVG has no size to draw at",
        _ => "the SVG could not be drawn",
    })?;
    drop(doc);
    let size = tree.size();
    let (w, h) = (size.width() as f64, size.height() as f64);
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return Err("the SVG has no size to draw at");
    }
    let scale = (MAX_SIDE as f64 / w).min(MAX_SIDE as f64 / h).min(1.0);
    let (pw, ph) = (((w * scale).round() as u32).clamp(1, MAX_SIDE), ((h * scale).round() as u32).clamp(1, MAX_SIDE));
    audit_tree(&tree, scale, pw as f64 * ph as f64)?;
    let mut pixmap = tiny_skia::Pixmap::new(pw, ph).ok_or("the SVG could not be drawn")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale as f32, scale as f32), &mut pixmap.as_mut());
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    let (width, height) = (pw as usize, ph as usize);
    let png = delphi_docs::png::encode(&delphi_docs::png::Image { width, height, rgba }, &[]);
    Ok(Drawn { png, width, height })
}

/// The renderer's options: no resources folder, every `<image>` reference refused (a `data:` one too), no style sheet of ours, and
/// a nesting limit.
fn options() -> usvg::Options<'static> {
    usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver { resolve_data: Box::new(|_, _, _| None), resolve_string: Box::new(|_, _| None) },
        style_sheet: None,
        max_depth: MAX_DEPTH,
        ..usvg::Options::default()
    }
}

/// The checks made on the XML before the renderer sees it: no refused element, no text, a bounded depth, and a bounded count of
/// elements once `use` and markers are expanded.
fn audit_xml(doc: &roxmltree::Document<'_>) -> Result<(), &'static str> {
    let mut ids: HashMap<&str, roxmltree::NodeId> = HashMap::new();
    let (mut markers, mut marker_cost, mut vertices, mut marker_style) = (false, 0u64, 0u64, false);
    for n in doc.descendants().filter(|n| n.is_element()) {
        let name = n.tag_name().name();
        if REFUSED_ELEMENTS.contains(&name) {
            return Err("the SVG uses a pattern, a filter or a mask, which are not drawn here");
        }
        if matches!(name, "text" | "textPath" | "tspan") && n.descendants().any(|t| t.is_text() && !t.text().unwrap_or("").trim().is_empty()) {
            return Err("the SVG has text, and text is not drawn here");
        }
        if n.ancestors().filter(|a| a.is_element()).count() > MAX_DEPTH as usize {
            return Err("the SVG nests too deeply to draw");
        }
        if let Some(id) = n.attribute("id") {
            ids.entry(id).or_insert(n.id());
        }
        if name == "marker" {
            markers = true;
        }
        if name == "style" && n.text().unwrap_or("").contains("marker") {
            marker_style = true;
        }
        for a in n.attributes() {
            match a.name() {
                "d" | "points" => vertices += a.value().split(|c: char| !(c.is_ascii_digit() || c == '.')).filter(|s| !s.is_empty()).count() as u64,
                "style" if a.value().contains("marker") => marker_style = true,
                n if n.starts_with("marker") => marker_style = true,
                _ => {}
            }
        }
    }
    let mut memo: HashMap<roxmltree::NodeId, u64> = HashMap::new();
    let mut busy: Vec<roxmltree::NodeId> = Vec::new();
    let total = cost(doc.root_element(), &ids, &mut memo, &mut busy)?;
    if markers {
        for n in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "marker") {
            marker_cost = marker_cost.saturating_add(cost(n, &ids, &mut memo, &mut busy)?);
        }
    }
    // A marker is drawn at every vertex of a path that names it; counting every number of every path is an upper bound on that.
    let drawn_markers = if markers && marker_style { marker_cost.saturating_mul(vertices) } else { 0 };
    if total.saturating_add(drawn_markers) > MAX_EXPANDED {
        return Err("the SVG has too many parts to draw once its references are counted");
    }
    Ok(())
}

/// The elements `n` stands for when drawn: itself, its children, and for a `use`, what it names (a cycle is refused).
fn cost<'a>(n: roxmltree::Node<'a, 'a>, ids: &HashMap<&str, roxmltree::NodeId>, memo: &mut HashMap<roxmltree::NodeId, u64>, busy: &mut Vec<roxmltree::NodeId>) -> Result<u64, &'static str> {
    if let Some(c) = memo.get(&n.id()) {
        return Ok(*c);
    }
    if busy.contains(&n.id()) {
        return Err("the SVG refers to itself in a loop");
    }
    if busy.len() > MAX_DEPTH as usize * 4 {
        return Err("the SVG nests too deeply to draw");
    }
    busy.push(n.id());
    let mut total: u64 = 1;
    for c in n.children().filter(|c| c.is_element()) {
        total = total.saturating_add(cost(c, ids, memo, busy)?);
        if total > MAX_EXPANDED {
            break;
        }
    }
    if n.tag_name().name() == "use" {
        let href = n.attributes().find(|a| a.name() == "href").map(|a| a.value()).unwrap_or("");
        if let Some(target) = href.strip_prefix('#').and_then(|id| ids.get(id)).and_then(|id| n.document().get_node(*id)) {
            total = total.saturating_add(cost(target, ids, memo, busy)?);
        }
    }
    busy.pop();
    memo.insert(n.id(), total);
    Ok(total)
}

/// The checks made on the renderer's own tree before it draws: the element count, and the layers isolated groups will allocate.
fn audit_tree(tree: &usvg::Tree, scale: f64, canvas: f64) -> Result<(), &'static str> {
    struct Walk {
        nodes: u64,
        layers: usize,
    }
    fn walk(g: &usvg::Group, depth: usize, scale: f64, canvas: f64, w: &mut Walk) -> Result<(), &'static str> {
        let mut depth = depth;
        if g.should_isolate() {
            w.layers += 1;
            depth += 1;
            if w.layers > MAX_LAYERS || depth > MAX_LAYER_DEPTH {
                return Err("the SVG has too many layers to draw");
            }
            if let Some(b) = g.abs_layer_bounding_box() {
                let area = (b.width() as f64 * scale) * (b.height() as f64 * scale);
                if !area.is_finite() || area > canvas * MAX_LAYER_AREA {
                    return Err("the SVG has a layer too large to draw");
                }
            }
            if let Some(clip) = g.clip_path() {
                walk(clip.root(), depth, scale, canvas, w)?;
            }
        }
        for c in g.children() {
            w.nodes += 1;
            if w.nodes > MAX_EXPANDED {
                return Err("the SVG has too many parts to draw once its references are counted");
            }
            if let usvg::Node::Group(cg) = c {
                walk(cg, depth, scale, canvas, w)?;
            }
        }
        Ok(())
    }
    walk(tree.root(), 0, scale, canvas, &mut Walk { nodes: 0, layers: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svg(body: &str) -> Vec<u8> {
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"200\" height=\"120\" viewBox=\"0 0 200 120\">{}</svg>", body).into_bytes()
    }

    fn pixel(d: &Drawn, x: usize, y: usize) -> [u8; 4] {
        let img = delphi_docs::pixels::decode(&d.png).unwrap();
        img.pixel(x, y)
    }

    #[test]
    fn a_plain_svg_is_drawn_to_a_png_of_its_own_size() {
        let d = render(&svg("<rect x=\"0\" y=\"0\" width=\"200\" height=\"120\" fill=\"#ff0000\"/><circle cx=\"100\" cy=\"60\" r=\"30\" fill=\"blue\"/>")).unwrap();
        assert_eq!((d.width, d.height), (200, 120));
        assert!(d.png.starts_with(b"\x89PNG"));
        assert_eq!(pixel(&d, 5, 5), [255, 0, 0, 255]);
        assert_eq!(pixel(&d, 100, 60), [0, 0, 255, 255]);
    }

    #[test]
    fn a_script_and_every_image_reference_are_neither_run_nor_read() {
        // A file the renderer would read if it resolved references: it must stay unread, and nothing of it may be drawn.
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("secret.png");
        let green = delphi_docs::png::encode(&delphi_docs::png::Image { width: 4, height: 4, rgba: [0u8, 255, 0, 255].repeat(16) }, &[]);
        std::fs::write(&local, &green).unwrap();
        use base64::Engine as _;
        let data = base64::engine::general_purpose::STANDARD.encode(&green);
        let body = format!(
            "<script>throw new Error('ran')</script><script xlink:href=\"https://evil.example/x.js\"/>\
             <rect width=\"200\" height=\"120\" fill=\"white\" onload=\"alert(1)\"/>\
             <image x=\"0\" y=\"0\" width=\"50\" height=\"50\" xlink:href=\"https://evil.example/track.png\"/>\
             <image x=\"50\" y=\"0\" width=\"50\" height=\"50\" href=\"{}\"/>\
             <image x=\"100\" y=\"0\" width=\"50\" height=\"50\" href=\"file://{}\"/>\
             <image x=\"150\" y=\"0\" width=\"50\" height=\"50\" href=\"data:image/png;base64,{}\"/>\
             <foreignObject width=\"200\" height=\"120\"><iframe xmlns=\"http://www.w3.org/1999/xhtml\" src=\"https://evil.example/\"/></foreignObject>\
             <a href=\"https://evil.example/\"><rect x=\"0\" y=\"60\" width=\"200\" height=\"60\" fill=\"black\"/></a>",
            local.display(),
            local.display(),
            data
        );
        let d = render(&svg(&body)).unwrap();
        for x in [25, 75, 125, 175] {
            assert_eq!(pixel(&d, x, 25), [255, 255, 255, 255], "no referenced picture is drawn at x={}", x);
        }
        assert_eq!(pixel(&d, 10, 100), [0, 0, 0, 255], "a link's content is drawn as plain shapes");
    }

    #[test]
    fn a_dtd_and_so_any_entity_expansion_is_refused() {
        let laughs = "<?xml version=\"1.0\"?><!DOCTYPE svg [<!ENTITY a \"lol\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;\"><!ENTITY c \"&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;\"><!ENTITY d \"&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;\">]><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><desc>&d;&d;&d;&d;</desc></svg>";
        assert_eq!(render(laughs.as_bytes()), Err("the SVG declares a DTD, which is not read"));
        let external = "<?xml version=\"1.0\"?><!DOCTYPE svg SYSTEM \"https://evil.example/svg.dtd\"><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>";
        assert_eq!(render(external.as_bytes()), Err("the SVG declares a DTD, which is not read"));
    }

    #[test]
    fn use_chains_that_multiply_are_refused_before_drawing_and_a_loop_is_refused() {
        // Ten levels, each using the one before ten times: 10^10 shapes if drawn.
        let mut body = String::from("<defs><rect id=\"l0\" width=\"1\" height=\"1\"/>");
        for i in 1..=10 {
            body.push_str(&format!("<g id=\"l{}\">", i));
            for _ in 0..10 {
                body.push_str(&format!("<use href=\"#l{}\"/>", i - 1));
            }
            body.push_str("</g>");
        }
        body.push_str("</defs><use href=\"#l10\"/>");
        let started = std::time::Instant::now();
        assert_eq!(render(&svg(&body)), Err("the SVG has too many parts to draw once its references are counted"));
        assert!(started.elapsed() < Duration::from_secs(2), "refused by counting, not by drawing: {:?}", started.elapsed());
        assert_eq!(render(&svg("<g id=\"a\"><use href=\"#b\"/></g><g id=\"b\"><use xlink:href=\"#a\"/></g>")), Err("the SVG refers to itself in a loop"));
    }

    #[test]
    fn markers_on_many_vertices_count_where_they_are_drawn() {
        let mut marker = String::from("<marker id=\"m\" markerWidth=\"4\" markerHeight=\"4\">");
        for _ in 0..50 {
            marker.push_str("<circle r=\"1\"/>");
        }
        marker.push_str("</marker>");
        let d: String = (0..5_000).map(|i| format!("L{} {} ", i % 200, i % 120)).collect();
        let body = format!("<defs>{}</defs><path d=\"M0 0 {}\" marker-mid=\"url(#m)\" stroke=\"black\"/>", marker, d);
        assert_eq!(render(&svg(&body)), Err("the SVG has too many parts to draw once its references are counted"));
        let small = "<defs><marker id=\"m\"><path d=\"M0 0 L4 2 L0 4z\"/></marker></defs><path d=\"M10 10 L190 110\" marker-end=\"url(#m)\" stroke=\"black\"/>";
        assert!(render(&svg(small)).is_ok(), "an arrow head on a line is drawn");
    }

    #[test]
    fn huge_and_deep_files_are_bounded() {
        let huge = render(b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10000000\" height=\"5000000\"><rect width=\"10000000\" height=\"5000000\" fill=\"red\"/></svg>").unwrap();
        assert_eq!((huge.width, huge.height), (1_600, 800), "a huge canvas is drawn shrunk to the ceiling, never allocated at its own size");
        let tall = render(b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 100000\" width=\"1\" height=\"100000\"/>").unwrap();
        assert_eq!((tall.width, tall.height), (1, 1_600));
        let deep = format!("{}{}", "<g>".repeat(200), "</g>".repeat(200));
        assert_eq!(render(&svg(&deep)), Err("the SVG nests too deeply to draw"));
        let big = vec![b' '; MAX_SVG_BYTES + 1];
        assert_eq!(render(&big), Err("an SVG larger than 2 MiB is not drawn"));
        let many = svg(&"<rect width=\"1\" height=\"1\"/>".repeat(60_000));
        assert!(many.len() <= MAX_SVG_BYTES);
        assert_eq!(render(&many), Err("the SVG has too many parts to draw once its references are counted"));
        let nodes = svg(&"<g/>".repeat(300_000));
        assert!(nodes.len() <= MAX_SVG_BYTES);
        assert_eq!(render(&nodes), Err("the SVG has too many parts to draw"));
        assert_eq!(render(b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"0\" height=\"10\"/>"), Err("the SVG has no size to draw at"));
        assert!(render(b"\x1f\x8b\x08\0compressed").is_err());
        assert!(render(b"<svg").is_err());
    }

    #[test]
    fn patterns_filters_masks_text_and_large_layers_are_refused() {
        for body in [
            "<defs><pattern id=\"p\" width=\"0.0001\" height=\"0.0001\" patternTransform=\"scale(100000000)\"><rect width=\"1\" height=\"1\"/></pattern></defs><rect width=\"200\" height=\"120\" fill=\"url(#p)\"/>",
            "<defs><filter id=\"f\"><feGaussianBlur stdDeviation=\"1000\"/></filter></defs><rect width=\"200\" height=\"120\" filter=\"url(#f)\"/>",
            "<defs><mask id=\"k\"><rect width=\"1\" height=\"1\" fill=\"white\"/></mask></defs><rect width=\"200\" height=\"120\" mask=\"url(#k)\"/>",
        ] {
            assert_eq!(render(&svg(body)), Err("the SVG uses a pattern, a filter or a mask, which are not drawn here"), "{}", body);
        }
        assert_eq!(render(&svg("<text x=\"10\" y=\"20\">Label</text>")), Err("the SVG has text, and text is not drawn here"));
        assert!(render(&svg("<text x=\"10\" y=\"20\">  </text><rect width=\"9\" height=\"9\"/>")).is_ok(), "an empty text element hides nothing");
        let layers = format!("{}<rect width=\"9\" height=\"9\"/>{}", "<g opacity=\"0.9\">".repeat(6), "</g>".repeat(6));
        assert_eq!(render(&svg(&layers)), Err("the SVG has too many layers to draw"));
        let wide = "<g opacity=\"0.5\"><rect x=\"-100000\" y=\"-100000\" width=\"200000\" height=\"200000\"/></g>";
        assert_eq!(render(&svg(wide)), Err("the SVG has a layer too large to draw"));
    }
}
