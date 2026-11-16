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
//!   cycle is refused); clip-path chains at most [`MAX_CLIP_CHAIN`] long and no marker inside a marker, checked before the renderer
//!   follows either; no `pattern`, `filter` or `mask` (each draws into a picture of its own whose size the file chooses); at most
//!   [`MAX_LAYERS`] groups that draw into a layer, none nested deeper than [`MAX_LAYER_DEPTH`] and none larger than [`MAX_LAYER_AREA`]
//!   canvases; a canvas of at most [`MAX_SIDE`] pixels a side; painting of at most [`MAX_PAINT_CANVASES`] canvases (gradients
//!   weighted) and [`MAX_DASHES`] dashes, counted on the renderer's tree before it draws; one drawing at a time in the whole process
//!   (a drawing past its [`TIME_LIMIT`] is refused but keeps the one slot until it ends, so drawings never pile up).
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

/// The painting one SVG may cost, in canvases: the area of every shape's fill and stroke (clipped to the canvas, a gradient counted
/// [`GRADIENT_WEIGHT`] times) and of every layer, summed.
pub const MAX_PAINT_CANVASES: f64 = 200.0;
/// A gradient's weight per pixel: this, plus one for every [`STOPS_PER_WEIGHT`] stops (the painter searches the stops per pixel).
pub const GRADIENT_WEIGHT: f64 = 3.0;
pub const STOPS_PER_WEIGHT: f64 = 8.0;
/// The most stops one gradient may have.
pub const MAX_STOPS: usize = 64;
/// Dash segments over the whole file (a stroke's length over its dash pattern's length).
pub const MAX_DASHES: f64 = 200_000.0;

/// The drawing thread's stack: address space, mostly never touched. usvg converts by recursion; the audits refuse every deep
/// recursion they know of, and this is the margin for one they do not.
const SVG_STACK: usize = 256 << 20;

/// The one slot a drawing runs in, for the whole process. The drawing's own thread holds it until the drawing ends, so a drawing
/// that passed its time limit still holds it and no second one starts beside it: CPU spent on drawings is at most one core.
/// It holds when the drawing that has it began.
static SLOT_BUSY: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
static SLOT_FREED: std::sync::Condvar = std::sync::Condvar::new();

struct Slot;

impl Slot {
    /// The slot, waiting at most `wait` for the drawing that holds it to end. A drawing already past [`TIME_LIMIT`] will not end
    /// soon, so a caller is refused at once rather than waiting for it.
    fn acquire(wait: Duration) -> Option<Slot> {
        Self::acquire_from(wait, std::time::Instant::now())
    }

    /// [`Slot::acquire`], the drawing that takes the slot counted as begun at `began` (a test sets an earlier time).
    fn acquire_from(wait: Duration, began: std::time::Instant) -> Option<Slot> {
        let deadline = std::time::Instant::now() + wait;
        let mut busy = SLOT_BUSY.lock().unwrap_or_else(|e| e.into_inner());
        while let Some(since) = *busy {
            let overdue = TIME_LIMIT.checked_sub(since.elapsed()).filter(|d| !d.is_zero())?;
            let left = deadline.checked_duration_since(std::time::Instant::now()).filter(|d| !d.is_zero())?;
            busy = SLOT_FREED.wait_timeout(busy, left.min(overdue)).unwrap_or_else(|e| e.into_inner()).0;
        }
        *busy = Some(began);
        Some(Slot)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        *SLOT_BUSY.lock().unwrap_or_else(|e| e.into_inner()) = None;
        SLOT_FREED.notify_all();
    }
}

/// Draw `svg` to a PNG under every bound of this module. `Err` is a fixed sentence saying why it was refused.
pub fn render(svg: &[u8]) -> Result<Drawn, &'static str> {
    render_with(svg, TIME_LIMIT, TIME_LIMIT)
}

/// [`render`], waiting at most `wait` for the slot and `limit` for the drawing.
fn render_with(svg: &[u8], wait: Duration, limit: Duration) -> Result<Drawn, &'static str> {
    if svg.len() > MAX_SVG_BYTES {
        return Err("an SVG larger than 2 MiB is not drawn");
    }
    let slot = Slot::acquire(wait).ok_or("another SVG is still being drawn")?;
    let bytes = svg.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("svg-render".into()).stack_size(SVG_STACK).spawn(move || {
        let _slot = slot; // released when this drawing ends, not when its caller stops waiting
        let _ = tx.send(render_now(&bytes));
    });
    if spawned.is_err() {
        return Err("the SVG could not be drawn");
    }
    match rx.recv_timeout(limit) {
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
    audit_tree(&tree, scale, (w, h))?;
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
    let mut markers = false;
    for n in doc.descendants().filter(|n| n.is_element()) {
        let name = n.tag_name().name();
        if REFUSED_ELEMENTS.contains(&name) {
            return Err("the SVG uses a pattern, a filter or a mask, which are not drawn here");
        }
        if matches!(name, "linearGradient" | "radialGradient") && n.children().filter(|c| c.is_element() && c.tag_name().name() == "stop").count() > MAX_STOPS {
            return Err("the SVG has a gradient with too many stops to draw");
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
    }
    audit_references(doc, &ids)?;
    // What one vertex of a shape costs when markers may be drawn on it: every marker of the file, as if each were drawn at every
    // vertex of every shape, whatever names them (an attribute, a style, inheritance). Markers inside markers are refused above.
    let marker_cost = if markers {
        let (mut memo, mut busy) = (HashMap::new(), Vec::new());
        let mut c = Cost::default();
        for n in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "marker") {
            c = c.plus(cost(n, &ids, Cost::default(), &mut memo, &mut busy)?);
        }
        c
    } else {
        Cost::default()
    };
    let (mut memo, mut busy) = (HashMap::new(), Vec::new());
    let total = cost(doc.root_element(), &ids, marker_cost, &mut memo, &mut busy)?;
    if total.over() {
        return Err("the SVG has too many parts to draw once its references are counted");
    }
    Ok(())
}

/// Path segments (and polyline points) the renderer may build once every `use` is expanded and every marker drawn, over the whole
/// file. usvg builds each `use` instance's path anew, at about 470 bytes a segment, so this bounds the tree's memory before it is
/// built: 1,000,000 segments is about 470 MB.
pub const MAX_SEGMENTS: u64 = 1_000_000;

/// Points (a segment is one to three) on the renderer's built tree, checked before it draws.
pub const MAX_TREE_POINTS: u64 = 3 * MAX_SEGMENTS;

/// What drawing an element costs, with every `use` expanded: elements, and path segments.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Cost {
    elements: u64,
    segments: u64,
}

impl Cost {
    fn plus(self, o: Cost) -> Cost {
        Cost { elements: self.elements.saturating_add(o.elements), segments: self.segments.saturating_add(o.segments) }
    }
    fn times(self, k: u64) -> Cost {
        Cost { elements: self.elements.saturating_mul(k), segments: self.segments.saturating_mul(k) }
    }
    fn over(self) -> bool {
        self.elements > MAX_EXPANDED || self.segments > MAX_SEGMENTS
    }
}

/// The vertices of a shape, as the renderer will find them: a path's segments and a polyline's points read by resvg's own parsers
/// (so `l.5.5.5.5` is two segments, not one number), two for a line, and an upper bound of ten for the closed shapes.
fn vertices(n: roxmltree::Node<'_, '_>) -> u64 {
    match n.tag_name().name() {
        "path" => svgtypes::PathParser::from(n.attribute("d").unwrap_or("")).take_while(|s| s.is_ok()).count() as u64,
        "polyline" | "polygon" => svgtypes::PointsParser::from(n.attribute("points").unwrap_or("")).count() as u64 + 1,
        "line" => 2,
        "rect" | "circle" | "ellipse" => 10,
        _ => 0,
    }
}

/// The `#id`s a property value names with `url(#id)` (quoted or not, with spaces or not).
fn url_ids(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some(p) = rest.find("url(") {
        let after = &rest[p + 4..];
        let Some(close) = after.find(')') else { break };
        let inner = after[..close].trim().trim_matches(|c| c == '"' || c == '\'').trim();
        if let Some(id) = inner.strip_prefix('#') {
            out.push(id);
        }
        rest = &after[close + 1..];
    }
    out
}

/// What element `n` names through property `prop`, set as an attribute or in its `style` attribute.
fn prop_ids<'a>(n: roxmltree::Node<'a, 'a>, prop: &str) -> Vec<&'a str> {
    let mut out: Vec<&'a str> = n.attributes().filter(|a| a.name() == prop).flat_map(|a| url_ids(a.value())).collect();
    if let Some(style) = n.attribute("style") {
        for decl in style.split(';') {
            if let Some((k, v)) = decl.split_once(':') {
                if k.trim() == prop {
                    out.extend(url_ids(v));
                }
            }
        }
    }
    out
}

/// How long a chain of clip paths may be: a clip path clipped by another is drawn into a picture the size of the canvas for each
/// link, held at once, and usvg follows the chain by recursion.
pub const MAX_CLIP_CHAIN: usize = 4;
const MARKER_PROPS: [&str; 4] = ["marker-start", "marker-mid", "marker-end", "marker"];

/// The references the renderer follows recursively and allocates for at each step, checked before it sees them:
///
/// - a clip path's chain (its own `clip-path`, or one on anything inside it) is at most [`MAX_CLIP_CHAIN`] long, with no loop;
/// - nothing inside a marker, and nothing a marker inherits, names a marker;
/// - a style sheet (`<style>`) that could set either is refused when the file has clip paths or markers, since its selectors are
///   not resolved here.
fn audit_references<'a>(doc: &'a roxmltree::Document<'a>, ids: &HashMap<&str, roxmltree::NodeId>) -> Result<(), &'static str> {
    let elem = |id: &str| ids.get(id).and_then(|n| doc.get_node(*n));
    let named = |n: roxmltree::Node<'a, 'a>, tag: &str| n.is_element() && n.tag_name().name() == tag;
    let has = |tag: &str| doc.descendants().any(|n| named(n, tag));
    let (clips, markers) = (has("clipPath"), has("marker"));
    let sheet: String = doc.descendants().filter(|n| named(*n, "style")).flat_map(|s| s.descendants().filter(|t| t.is_text()).map(|t| t.text().unwrap_or(""))).collect::<Vec<_>>().join(" ");
    if (clips && sheet.contains("clip-path")) || (markers && sheet.contains("marker")) {
        return Err("the SVG sets clip paths or markers from a style sheet, which is not read here");
    }
    let mut work = 0usize;
    // Clip chains: the depth of each clip path, by what it and everything drawn inside it (a `use` included) name.
    fn depth<'a>(c: roxmltree::Node<'a, 'a>, elem: &dyn Fn(&str) -> Option<roxmltree::Node<'a, 'a>>, memo: &mut HashMap<roxmltree::NodeId, usize>, busy: &mut Vec<roxmltree::NodeId>, work: &mut usize) -> Result<usize, &'static str> {
        if let Some(d) = memo.get(&c.id()) {
            return Ok(*d);
        }
        if busy.contains(&c.id()) {
            return Err("the SVG's clip paths refer to themselves in a loop");
        }
        if busy.len() >= MAX_CLIP_CHAIN {
            return Err("the SVG chains clip paths too deeply to draw");
        }
        busy.push(c.id());
        let mut d = 1;
        for n in drawn(c, elem, work)? {
            for id in prop_ids(n, "clip-path") {
                if let Some(t) = elem(id).filter(|t| t.is_element() && t.tag_name().name() == "clipPath") {
                    d = d.max(1 + depth(t, elem, memo, busy, work)?);
                }
            }
        }
        busy.pop();
        if d > MAX_CLIP_CHAIN {
            return Err("the SVG chains clip paths too deeply to draw");
        }
        memo.insert(c.id(), d);
        Ok(d)
    }
    let (mut memo, mut busy) = (HashMap::new(), Vec::new());
    for c in doc.descendants().filter(|n| named(*n, "clipPath")) {
        depth(c, &elem, &mut memo, &mut busy, &mut work)?;
    }
    // Markers inside markers: a marker, everything drawn inside it (a `use` included) and its ancestors (whose marker properties
    // its contents inherit) name none.
    for m in doc.descendants().filter(|n| named(*n, "marker")) {
        let inside = drawn(m, &elem, &mut work)?;
        let around = inside.into_iter().chain(m.ancestors().skip(1).filter(|n| n.is_element()));
        for n in around {
            if MARKER_PROPS.iter().any(|p| prop_ids(n, p).iter().any(|id| elem(id).is_some_and(|t| t.is_element() && t.tag_name().name() == "marker"))) {
                return Err("the SVG draws markers inside markers, which is not drawn here");
            }
        }
    }
    Ok(())
}

/// The work [`drawn`] may do over the whole file, in elements visited.
const MAX_REFERENCE_WORK: usize = 1_000_000;

/// Every element drawn when `start` is drawn: `start`, its descendants, and for each `use` among them what it names and that
/// element's descendants, followed transitively (each element once). The renderer follows a `use` wherever it is, so a property
/// the audit looks for may sit on a `use`'s target rather than inside `start`.
fn drawn<'a>(start: roxmltree::Node<'a, 'a>, elem: &dyn Fn(&str) -> Option<roxmltree::Node<'a, 'a>>, work: &mut usize) -> Result<Vec<roxmltree::Node<'a, 'a>>, &'static str> {
    let (mut out, mut seen, mut todo) = (Vec::new(), std::collections::HashSet::new(), vec![start]);
    while let Some(root) = todo.pop() {
        for n in root.descendants().filter(|n| n.is_element()) {
            if !seen.insert(n.id()) {
                continue;
            }
            *work += 1;
            if *work > MAX_REFERENCE_WORK {
                return Err("the SVG has too many parts to draw once its references are counted");
            }
            if n.tag_name().name() == "use" {
                if let Some(t) = n.attributes().find(|a| a.name() == "href").and_then(|a| a.value().strip_prefix('#')).and_then(elem) {
                    todo.push(t);
                }
            }
            out.push(n);
        }
    }
    Ok(out)
}

/// The elements `n` stands for when drawn: itself, its children, for a `use` what it names (a cycle is refused), and for a shape
/// `marker` elements at each of its vertices.
fn cost<'a>(n: roxmltree::Node<'a, 'a>, ids: &HashMap<&str, roxmltree::NodeId>, marker: Cost, memo: &mut HashMap<roxmltree::NodeId, Cost>, busy: &mut Vec<roxmltree::NodeId>) -> Result<Cost, &'static str> {
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
    // Every shape costs its segments (each `use` of it builds them again), and when the file has markers, every marker at each.
    let v = vertices(n);
    let mut total = Cost { elements: 1, segments: v }.plus(marker.times(v));
    for c in n.children().filter(|c| c.is_element()) {
        total = total.plus(cost(c, ids, marker, memo, busy)?);
        if total.over() {
            break;
        }
    }
    if n.tag_name().name() == "use" {
        let href = n.attributes().find(|a| a.name() == "href").map(|a| a.value()).unwrap_or("");
        if let Some(target) = href.strip_prefix('#').and_then(|id| ids.get(id)).and_then(|id| n.document().get_node(*id)) {
            total = total.plus(cost(target, ids, marker, memo, busy)?);
        }
    }
    busy.pop();
    memo.insert(n.id(), total);
    Ok(total)
}

/// The area of `r` (in the tree's units) that lies on the canvas `size`, in pixels at `scale`.
fn on_canvas(r: usvg::Rect, size: (f64, f64), scale: f64) -> f64 {
    let w = (r.right() as f64).min(size.0) - (r.left() as f64).max(0.0);
    let h = (r.bottom() as f64).min(size.1) - (r.top() as f64).max(0.0);
    if w.is_finite() && h.is_finite() { w.max(0.0) * h.max(0.0) * scale * scale } else { 0.0 }
}

/// What painting with `paint` costs, per pixel, against a flat colour.
fn paint_weight(paint: &usvg::Paint) -> f64 {
    let stops = |n: usize| GRADIENT_WEIGHT + n as f64 / STOPS_PER_WEIGHT;
    match paint {
        usvg::Paint::Color(_) => 1.0,
        usvg::Paint::LinearGradient(g) => stops(g.stops().len()),
        usvg::Paint::RadialGradient(g) => stops(g.stops().len()),
        usvg::Paint::Pattern(_) => f64::INFINITY,
    }
}

/// The length of a path's outline through its points (an upper bound on the length of its curves), in its own units.
fn outline_length(p: &usvg::tiny_skia_path::Path) -> f64 {
    p.points().windows(2).map(|w| (((w[1].x - w[0].x) as f64).powi(2) + ((w[1].y - w[0].y) as f64).powi(2)).sqrt()).filter(|d| d.is_finite()).sum()
}

/// The checks made on the renderer's own tree before it draws: the element count, the layers isolated groups will allocate, and
/// what painting it all will cost (the area painted, and the dashes stroked), so that drawing is bounded in time as well as memory.
fn audit_tree(tree: &usvg::Tree, scale: f64, size: (f64, f64)) -> Result<(), &'static str> {
    struct Walk {
        nodes: u64,
        layers: usize,
        painted: f64,
        dashes: f64,
        points: u64,
    }
    let canvas = (size.0 * scale).max(1.0) * (size.1 * scale).max(1.0);
    fn walk(g: &usvg::Group, depth: usize, scale: f64, size: (f64, f64), canvas: f64, w: &mut Walk) -> Result<(), &'static str> {
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
                w.painted += area; // the layer is composited onto what is under it
            }
            // Each clip path of a chain is drawn into a canvas-sized picture of its own, all held at once: each is a layer.
            let (mut clip, mut clip_depth) = (g.clip_path(), depth);
            while let Some(c) = clip {
                w.layers += 1;
                clip_depth += 1;
                if w.layers > MAX_LAYERS || clip_depth > MAX_LAYER_DEPTH {
                    return Err("the SVG has too many layers to draw");
                }
                w.painted += canvas; // a clip path is drawn over the whole canvas
                walk(c.root(), clip_depth, scale, size, canvas, w)?;
                clip = c.clip_path();
            }
        }
        for c in g.children() {
            w.nodes += 1;
            if w.nodes > MAX_EXPANDED {
                return Err("the SVG has too many parts to draw once its references are counted");
            }
            if let usvg::Node::Path(p) = c {
                // The built tree's own size, checked again before drawing: every point of every path, invisible ones included.
                w.points += p.data().points().len() as u64;
                if w.points > MAX_TREE_POINTS {
                    return Err("the SVG has too many parts to draw once its references are counted");
                }
            }
            match c {
                usvg::Node::Group(cg) => walk(cg, depth, scale, size, canvas, w)?,
                usvg::Node::Path(p) if p.is_visible() => {
                    if let (Some(f), Some(b)) = (p.fill(), p.abs_bounding_box()) {
                        w.painted += on_canvas(b, size, scale) * paint_weight(f.paint());
                    }
                    if let Some(s) = p.stroke() {
                        let (sx, sy) = p.abs_transform().get_scale();
                        let k = (sx.abs().max(sy.abs()) as f64) * scale;
                        let length = outline_length(p.data());
                        // A stroke paints about its length times its width, and never more than its bounds on the canvas.
                        let along = length * k * (s.width().get() as f64 * k).max(1.0);
                        let bounds = p.abs_stroke_bounding_box().map_or(0.0, |b| on_canvas(b, size, scale));
                        w.painted += along.min(bounds).max(0.0) * paint_weight(s.paint());
                        if let Some(d) = s.dasharray() {
                            let period: f64 = d.iter().map(|x| *x as f64).sum();
                            w.dashes += if period > 0.0 { length / period } else { f64::INFINITY };
                        }
                    }
                }
                _ => {}
            }
            if !(w.painted <= MAX_PAINT_CANVASES * canvas && w.dashes <= MAX_DASHES) {
                return Err("the SVG would take too long to draw");
            }
        }
        Ok(())
    }
    walk(tree.root(), 0, scale, size, canvas, &mut Walk { nodes: 0, layers: 0, painted: 0.0, dashes: 0.0, points: 0 })
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
        assert_eq!(render_now(&svg(&body)), Err("the SVG has too many parts to draw once its references are counted"));
        assert!(started.elapsed() < Duration::from_secs(2), "refused by counting, not by drawing: {:?}", started.elapsed());
        assert_eq!(render(&svg("<g id=\"a\"><use href=\"#b\"/></g><g id=\"b\"><use xlink:href=\"#a\"/></g>")), Err("the SVG refers to itself in a loop"));
    }

    /// A chain of clip paths, each clipped by the next: `n` links, the first used by a rectangle on a canvas `side` pixels square.
    fn clip_chain(n: usize, side: u32, how: &str) -> Vec<u8> {
        let mut s = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{side}\" height=\"{side}\"><defs>");
        for i in 0..n {
            let link = if i + 1 < n {
                match how {
                    "style" => format!(" style=\"fill:red; clip-path: url( '#c{}' )\"", i + 1),
                    _ => format!(" clip-path=\"url(#c{})\"", i + 1),
                }
            } else {
                String::new()
            };
            s.push_str(&format!("<clipPath id=\"c{i}\"{link}><rect width=\"{side}\" height=\"{side}\"/></clipPath>"));
        }
        s.push_str(&format!("</defs><rect width=\"{side}\" height=\"{side}\" clip-path=\"url(#c0)\"/></svg>"));
        s.into_bytes()
    }

    #[test]
    fn a_chain_of_clip_paths_is_refused_before_the_renderer_follows_it() {
        // 10,000 links (about 850 KB): usvg follows a clip path's own clip path by recursion, which overflowed the stack and aborted
        // the process. 200 links at 1,600 pixels: resvg holds a canvas-sized picture per link, 2 GB at once. Both are refused by the
        // count of the chain, before the renderer sees the file, and so allocate nothing for it.
        for (n, side) in [(10_000usize, 100u32), (200, 1_600)] {
            let file = clip_chain(n, side, "attribute");
            assert!(file.len() <= MAX_SVG_BYTES);
            let started = std::time::Instant::now();
            assert_eq!(render_now(&file), Err("the SVG chains clip paths too deeply to draw"), "{n} links");
            assert!(started.elapsed() < Duration::from_secs(1), "{n} links: {:?}", started.elapsed());
        }
        assert_eq!(render(&clip_chain(6, 100, "style")), Err("the SVG chains clip paths too deeply to draw"), "named in a style attribute");
        let sheet = "<style><![CDATA[ #c0 { fill: red } ]]><!-- split --> #c0 { clip-path: url(#c1) }</style><defs><clipPath id=\"c0\"><rect width=\"9\" height=\"9\"/></clipPath><clipPath id=\"c1\"><rect width=\"9\" height=\"9\"/></clipPath></defs><rect width=\"9\" height=\"9\" clip-path=\"url(#c0)\"/>";
        assert_eq!(render(&svg(sheet)), Err("the SVG sets clip paths or markers from a style sheet, which is not read here"));
        let looped = "<defs><clipPath id=\"a\" clip-path=\"url(#b)\"><rect width=\"9\" height=\"9\"/></clipPath><clipPath id=\"b\"><rect width=\"9\" height=\"9\" clip-path=\"url(#a)\"/></clipPath></defs><rect width=\"9\" height=\"9\" clip-path=\"url(#a)\"/>";
        assert_eq!(render(&svg(looped)), Err("the SVG's clip paths refer to themselves in a loop"));
        let one = "<defs><clipPath id=\"c\"><circle cx=\"100\" cy=\"60\" r=\"40\"/></clipPath></defs><rect width=\"200\" height=\"120\" fill=\"#00ff00\" clip-path=\"url(#c)\"/>";
        let d = render(&svg(one)).expect("a single clip path is drawn");
        assert_eq!((pixel(&d, 100, 60), pixel(&d, 5, 5)[3]), ([0, 255, 0, 255], 0));
        // Each link of a chain counts as a layer on the renderer's own tree too.
        assert_eq!(render(&clip_chain(4, 100, "attribute")), Err("the SVG has too many layers to draw"));
        assert!(render(&clip_chain(2, 100, "attribute")).is_ok());
    }

    /// A canvas `side` pixels square with `n` rectangles over all of it, each filled with a radial gradient at half opacity: the
    /// red-team's `cpu_grad.svg` (28,000 of them took about 170 s of CPU each time it was drawn).
    fn gradients(n: usize, side: u32) -> Vec<u8> {
        let rects: String = (0..n).map(|_| format!("<rect width=\"{side}\" height=\"{side}\" fill=\"url(#g)\" fill-opacity=\"0.5\"/>")).collect();
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{side}\" height=\"{side}\"><defs><radialGradient id=\"g\"><stop offset=\"0\" stop-color=\"red\"/><stop offset=\"1\" stop-color=\"blue\"/></radialGradient></defs>{rects}</svg>").into_bytes()
    }

    #[test]
    fn painting_that_would_take_too_long_is_refused_before_it_starts() {
        let many = gradients(20_000, 1_600);
        assert!(many.len() <= MAX_SVG_BYTES);
        assert_eq!(render(&many), Err("the SVG would take too long to draw"));
        let started = std::time::Instant::now();
        assert_eq!(render_now(&gradients(2_000, 1_600)), Err("the SVG would take too long to draw"));
        assert!(started.elapsed() < Duration::from_secs(1), "refused by the budget, not by the clock: {:?}", started.elapsed());
        let dashed = "<path d=\"M0 0 L200 120 L0 120 L200 0 L0 0\" stroke=\"black\" stroke-dasharray=\"0.0005\"/>";
        assert_eq!(render(&svg(dashed)), Err("the SVG would take too long to draw"), "a dash of 0.0005 over 700 units is over a million dashes");
        assert!(render(&gradients(20, 400)).is_ok(), "twenty gradients over the canvas are drawn");
        assert!(render(&svg("<path d=\"M0 0 L200 120\" stroke=\"black\" stroke-dasharray=\"4 2\"/>")).is_ok(), "an ordinary dashed line is drawn");
    }

    #[test]
    fn a_timed_out_drawing_keeps_the_one_slot_so_two_never_run_at_once() {
        let small = svg("<rect width=\"10\" height=\"10\"/>");
        // While a drawing holds the slot, another waits for it and, past its wait, is refused without starting.
        let held = Slot::acquire(Duration::from_secs(60)).expect("the slot comes free");
        assert_eq!(render_with(&small, Duration::from_millis(50), TIME_LIMIT), Err("another SVG is still being drawn"));
        drop(held);
        assert!(render_with(&small, Duration::from_secs(60), TIME_LIMIT).is_ok());
        // A drawing whose caller stopped waiting still holds the slot until it ends: no second drawing runs beside it.
        let slow = gradients(8, 1_600);
        assert_eq!(render_with(&slow, Duration::from_secs(60), Duration::ZERO), Err("the SVG took too long to draw"));
        assert!(Slot::acquire(Duration::ZERO).is_none(), "the timed-out drawing still holds the slot");
        drop(Slot::acquire(Duration::from_secs(120)).expect("and gives it back when it ends"));
    }

    /// The red-team's second pass: a chain through `use`. Each clip path holds only a `use` of a rectangle that is itself clipped by
    /// the next clip path, so no clip path names another directly; usvg follows it all the same, and 10,000 links (1.2 MB) aborted
    /// the process. The audit follows a `use` to what it draws, so the chain is counted and refused.
    #[test]
    fn a_chain_of_clip_paths_through_use_is_refused_too() {
        let n = 10_000;
        let mut s = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"100\" height=\"100\"><defs>");
        for i in 0..n {
            s.push_str(&format!("<clipPath id=\"c{i}\"><use xlink:href=\"#r{}\"/></clipPath>", i + 1));
            let clip = if i + 1 < n { format!(" clip-path=\"url(#c{})\"", i + 1) } else { String::new() };
            s.push_str(&format!("<rect id=\"r{}\" width=\"100\" height=\"100\"{clip}/>", i + 1));
        }
        s.push_str("</defs><rect width=\"100\" height=\"100\" clip-path=\"url(#c0)\"/></svg>");
        assert!(s.len() <= MAX_SVG_BYTES);
        let started = std::time::Instant::now();
        assert_eq!(render_now(s.as_bytes()), Err("the SVG chains clip paths too deeply to draw"));
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        assert_eq!(render(s.as_bytes()), Err("the SVG chains clip paths too deeply to draw"), "and through the drawing thread, no abort");
        // A marker that draws, through a `use`, a shape with a marker.
        let via_use = "<defs><path id=\"p\" d=\"M0 0 L4 4\" marker-end=\"url(#b)\"/><marker id=\"a\"><use href=\"#p\"/></marker><marker id=\"b\"><circle r=\"1\"/></marker></defs><path d=\"M0 0 L9 9\" marker-end=\"url(#a)\" stroke=\"black\"/>";
        assert_eq!(render(&svg(via_use)), Err("the SVG draws markers inside markers, which is not drawn here"));
        // A clip path that uses an unclipped shape is drawn.
        let ok = "<defs><circle id=\"o\" cx=\"100\" cy=\"60\" r=\"40\"/><clipPath id=\"c\"><use href=\"#o\"/></clipPath></defs><rect width=\"200\" height=\"120\" fill=\"#00ff00\" clip-path=\"url(#c)\"/>";
        assert_eq!(pixel(&render(&svg(ok)).unwrap(), 100, 60), [0, 255, 0, 255]);
    }

    /// A full-canvas rectangle filled with a gradient of `stops` stops, drawn `uses` more times through `use`.
    fn stops(stops: usize, uses: usize) -> Vec<u8> {
        let s: String = (0..stops).map(|i| format!("<stop offset=\"0.{:02}\" stop-color=\"#{:03x}\"/>", i * 100 / stops.max(1), i * 977 % 0xfff)).collect();
        let u: String = (0..uses).map(|_| "<use href=\"#r\"/>".to_string()).collect();
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1600\" height=\"1600\"><defs><linearGradient id=\"g\">{s}</linearGradient></defs><rect id=\"r\" width=\"1600\" height=\"1600\" fill=\"url(#g)\"/>{u}</svg>").into_bytes()
    }

    /// The red-team's second pass: the painter searches a gradient's stops for every pixel, so one full-canvas rectangle with 45,000
    /// stops took 4.8 s, and 60 more of it (under the paint budget, which counted every gradient alike) held the one slot for minutes.
    #[test]
    fn a_gradient_with_many_stops_is_refused_and_stops_weigh_on_the_budget() {
        for file in [stops(44_000, 60), stops(45_000, 0), stops(MAX_STOPS + 1, 0)] {
            assert!(file.len() <= MAX_SVG_BYTES);
            let started = std::time::Instant::now();
            assert_eq!(render_now(&file), Err("the SVG has a gradient with too many stops to draw"));
            assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        }
        // Within the cap, stops still weigh: 64 stops weigh 11 a pixel, so 60 canvases of them are over the budget.
        let started = std::time::Instant::now();
        assert_eq!(render_now(&stops(MAX_STOPS, 60)), Err("the SVG would take too long to draw"));
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        assert!(render_now(&stops(8, 2)).is_ok(), "an ordinary gradient is drawn");
    }

    #[test]
    fn a_caller_is_refused_at_once_when_the_drawing_in_the_slot_is_past_its_time_limit() {
        let small = svg("<rect width=\"10\" height=\"10\"/>");
        let long_ago = std::time::Instant::now().checked_sub(TIME_LIMIT + Duration::from_secs(1)).expect("the clock goes back that far");
        let held = Slot::acquire_from(Duration::from_secs(60), long_ago).expect("the slot comes free");
        let started = std::time::Instant::now();
        assert_eq!(render_with(&small, TIME_LIMIT, TIME_LIMIT), Err("another SVG is still being drawn"));
        assert!(started.elapsed() < Duration::from_secs(1), "no wait for an overdue drawing: {:?}", started.elapsed());
        drop(held);
        assert!(render_with(&small, Duration::from_secs(60), TIME_LIMIT).is_ok());
    }

    /// One long path drawn many times through levels of `use`: `levels` levels, each of `fan` uses of the level below.
    fn long_path_uses(segments: usize, levels: usize, fan: usize) -> Vec<u8> {
        let mut s = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"200\"><defs><path id=\"l0\" stroke=\"black\" d=\"M0 0{}\"/>", "l.1.1".repeat(segments));
        for l in 1..=levels {
            s.push_str(&format!("<g id=\"l{l}\">"));
            for _ in 0..fan {
                s.push_str(&format!("<use href=\"#l{}\"/>", l - 1));
            }
            s.push_str("</g>");
        }
        s.push_str(&format!("</defs><use href=\"#l{levels}\"/></svg>"));
        s.into_bytes()
    }

    /// The red-team's third pass: usvg builds every `use` instance's path anew, so one 50,000-segment path drawn 10,000 times
    /// through four levels of ten uses (11,110 elements, 251 KB) took 4.7 GB. Each shape's segments now count wherever it is drawn.
    #[test]
    fn a_long_path_drawn_many_times_is_counted_by_its_segments() {
        for (levels, fan) in [(4usize, 10usize), (2, 200)] {
            let file = long_path_uses(50_000, levels, fan);
            assert!(file.len() <= MAX_SVG_BYTES);
            let started = std::time::Instant::now();
            assert_eq!(render_now(&file), Err("the SVG has too many parts to draw once its references are counted"), "{levels} levels of {fan}");
            assert!(started.elapsed() < Duration::from_secs(1), "{levels} levels of {fan}: {:?}", started.elapsed());
        }
        assert!(render_now(&long_path_uses(1_000, 2, 10)).is_ok(), "a 1,000-segment path drawn 100 times is drawn");
        let one = roxmltree::Document::parse("<g><path d=\"M0 0l1 1 2 2\"/><polyline points=\"0,0 1,1\"/><use href=\"#x\"/></g>").unwrap();
        let ids = HashMap::new();
        assert_eq!(cost(one.root_element(), &ids, Cost::default(), &mut HashMap::new(), &mut Vec::new()).unwrap(), Cost { elements: 4, segments: 6 });
    }

    #[test]
    fn markers_inside_markers_are_refused() {
        let nested = "<defs><marker id=\"a\"><path d=\"M0 0 L4 4\" marker-end=\"url(#b)\"/></marker><marker id=\"b\"><circle r=\"1\"/></marker></defs><path d=\"M0 0 L9 9\" marker-end=\"url(#a)\" stroke=\"black\"/>";
        assert_eq!(render(&svg(nested)), Err("the SVG draws markers inside markers, which is not drawn here"));
        let inherited = "<g marker-end=\"url(#a)\"><defs><marker id=\"a\"><path d=\"M0 0 L4 4\"/></marker></defs><path d=\"M0 0 L9 9\" stroke=\"black\"/></g>";
        assert_eq!(render(&svg(inherited)), Err("the SVG draws markers inside markers, which is not drawn here"));
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

    /// The red-team's bypasses of the first count, each of which reached 1.6 to 1.7 GB before it was refused: markers on `<line>`
    /// elements (no `d` or `points` to count), a compact path whose numbers run together (`l.5.5.5.5` read as one number), and markers
    /// set by a style sheet split by a comment. Each is now refused by the count before the renderer expands anything.
    #[test]
    fn markers_on_lines_compact_paths_and_through_style_sheets_are_counted() {
        let arrow = "<defs><marker id=\"m\" markerWidth=\"4\" markerHeight=\"4\"><path d=\"M0 0 L4 2 L0 4z\"/></marker></defs>";
        let lines: String = (0..20_000).map(|i| format!("<line x1=\"{}\" y1=\"0\" x2=\"0\" y2=\"{}\" marker-end=\"url(#m)\"/>", i % 200, i % 120)).collect();
        let compact = format!("<path stroke=\"black\" marker-mid=\"url(#m)\" d=\"M0 0{}\"/>", "l.5.5".repeat(32_000));
        let inherited = format!("<g marker-mid=\"url(#m)\"><path stroke=\"black\" d=\"M0 0{}\"/></g>", "l.5.5".repeat(32_000));
        for (what, body) in [("lines", format!("{arrow}{lines}")), ("compact path", format!("{arrow}{compact}")), ("inherited", format!("{arrow}{inherited}"))] {
            let file = svg(&body);
            assert!(file.len() <= MAX_SVG_BYTES, "{what}");
            let started = std::time::Instant::now();
            assert_eq!(render_now(&file), Err("the SVG has too many parts to draw once its references are counted"), "{what}");
            assert!(started.elapsed() < Duration::from_secs(2), "{what}: refused by counting, not by drawing: {:?}", started.elapsed());
        }
        let sheet = format!("<style>path {{ stroke: black }}<!-- a comment splits the text --> path {{ marker-mid: url(#m) }}</style>{arrow}<path d=\"M0 0 L9 9\"/>");
        assert_eq!(render(&svg(&sheet)), Err("the SVG sets clip paths or markers from a style sheet, which is not read here"));
        assert_eq!(vertices(roxmltree::Document::parse("<path d=\"M0 0l.5.5.5.5.5.5\"/>").unwrap().root_element()), 4);
        assert_eq!(vertices(roxmltree::Document::parse("<polyline points=\"0,0 1,1 2,2\"/>").unwrap().root_element()), 4);
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
