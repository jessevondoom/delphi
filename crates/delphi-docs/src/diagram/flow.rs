//! Flow diagrams: nodes joined by arrows, laid out in layers. The layout is the usual layered one, kept small and
//! deterministic: edges that close a cycle are laid out reversed and drawn pointing back; each node goes in the layer
//! after its last predecessor; an edge that spans layers passes through invisible stand-ins so it can be ordered
//! with the rest; layers are ordered by the average position of their neighbours over a few sweeps; and positions
//! across a layer follow the neighbours while keeping the order and a gap.

use super::{arrow, bad, short, wrap, Scene};
use crate::chart::scale::f;
use crate::chart::{text, tw, PAD};
use crate::DocError;
use serde_json::Value;

const MAX_NODES: usize = 60;
const MAX_EDGES: usize = 120;
const MAX_GROUPS: usize = 4;
const CROSS_GAP: f64 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodeKind {
    Step,
    Start,
    End,
    Decision,
}

#[derive(Debug, Clone)]
pub(super) struct Node {
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
    pub group: Option<usize>,
}

#[derive(Debug, Clone)]
pub(super) struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct Flow {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub right: bool,
    pub groups: Vec<String>,
}

pub(super) fn parse(v: &Value) -> Result<Flow, DocError> {
    let list = v.get("nodes").and_then(|n| n.as_array()).ok_or_else(|| bad("a flow needs `nodes`: [{id, label?, kind?, group?}]"))?;
    if list.is_empty() || list.len() > MAX_NODES {
        return Err(bad(format!("a flow has between 1 and {} nodes; with more, split it into several diagrams or show the main path", MAX_NODES)));
    }
    let (mut nodes, mut groups): (Vec<Node>, Vec<String>) = (vec![], vec![]);
    for (i, n) in list.iter().enumerate() {
        let id = short(n, "id", 40, "a node id")?.ok_or_else(|| bad(format!("node {} needs an `id`", i + 1)))?;
        if nodes.iter().any(|x| x.id == id) {
            return Err(bad(format!("two nodes have the id `{}`", id)));
        }
        let label = short(n, "label", 80, "a node label")?.unwrap_or_else(|| id.clone());
        let kind = match n.get("kind").and_then(|k| k.as_str()).unwrap_or("step") {
            "step" | "process" => NodeKind::Step,
            "start" => NodeKind::Start,
            "end" => NodeKind::End,
            "decision" => NodeKind::Decision,
            other => return Err(bad(format!("node `{}` has kind `{}`; use step, start, end or decision", id, other))),
        };
        let group = match short(n, "group", 40, "a group name")? {
            Some(g) => Some(match groups.iter().position(|x| *x == g) {
                Some(p) => p,
                None => {
                    if groups.len() == MAX_GROUPS {
                        return Err(bad(format!("a flow colours at most {} groups; `{}` would be a fifth. Merge groups, or drop `group` and let the labels say it", MAX_GROUPS, g)));
                    }
                    groups.push(g);
                    groups.len() - 1
                }
            }),
            None => None,
        };
        nodes.push(Node { id, label, kind, group });
    }
    let index = |id: &str| nodes.iter().position(|n| n.id == id);
    let known = || nodes.iter().map(|n| format!("`{}`", n.id)).take(12).collect::<Vec<_>>().join(", ");
    let mut edges: Vec<Edge> = vec![];
    if let Some(list) = v.get("edges") {
        let list = list.as_array().ok_or_else(|| bad("`edges` is an array of {from, to, label?}"))?;
        if list.len() > MAX_EDGES {
            return Err(bad(format!("a flow has at most {} edges", MAX_EDGES)));
        }
        for (i, e) in list.iter().enumerate() {
            let (a, b) = (short(e, "from", 40, "an edge's `from`")?, short(e, "to", 40, "an edge's `to`")?);
            let (a, b) = (a.ok_or_else(|| bad(format!("edge {} needs `from`", i + 1)))?, b.ok_or_else(|| bad(format!("edge {} needs `to`", i + 1)))?);
            let from = index(&a).ok_or_else(|| bad(format!("edge {} starts at `{}`, which is not a node; the nodes are {}", i + 1, a, known())))?;
            let to = index(&b).ok_or_else(|| bad(format!("edge {} ends at `{}`, which is not a node; the nodes are {}", i + 1, b, known())))?;
            if from == to {
                return Err(bad(format!("edge {} joins `{}` to itself; a loop on one step is better said in its label", i + 1, a)));
            }
            if edges.iter().any(|x| x.from == from && x.to == to) {
                return Err(bad(format!("`{}` to `{}` is listed twice", a, b)));
            }
            edges.push(Edge { from, to, label: short(e, "label", 40, "an edge label")? });
        }
    }
    let right = match v.get("direction").and_then(|d| d.as_str()).unwrap_or("down") {
        "down" | "top-down" => false,
        "right" | "left-right" => true,
        other => return Err(bad(format!("`direction` is down or right, not `{}`", other))),
    };
    Ok(Flow { nodes, edges, right, groups })
}

/// One thing placed in a layer: a node, or the invisible stand-in for an edge passing through.
struct Item {
    layer: usize,
    cross: f64,
    along: f64,
    pos: f64,
}

struct Shape {
    lines: Vec<String>,
    w: f64,
    h: f64,
}

fn shape(n: &Node) -> Shape {
    let lines = wrap(&n.label, 22, 3);
    let widest = lines.iter().map(|l| tw(l, 12.0)).fold(0.0, f64::max);
    let h = (24.0 + lines.len() as f64 * 15.0).max(38.0);
    let (w, h) = match n.kind {
        NodeKind::Decision => (widest * 1.45 + 44.0, (h * 1.5).max(56.0)),
        _ => ((widest + 28.0).max(72.0), h),
    };
    Shape { lines, w, h }
}

pub(super) fn layout(fl: &Flow, top: f64) -> Scene {
    let n = fl.nodes.len();
    let shapes: Vec<Shape> = fl.nodes.iter().map(shape).collect();

    // --- cycles: find the edges that close one, by depth-first search from the sources
    let mut out: Vec<Vec<usize>> = vec![vec![]; n];
    let mut indeg = vec![0usize; n];
    for (i, e) in fl.edges.iter().enumerate() {
        out[e.from].push(i);
        indeg[e.to] += 1;
    }
    let mut state = vec![0u8; n];
    let mut back = vec![false; fl.edges.len()];
    let roots: Vec<usize> = (0..n).filter(|v| indeg[*v] == 0).chain((0..n).filter(|v| indeg[*v] > 0)).collect();
    for r in roots {
        if state[r] != 0 {
            continue;
        }
        let mut stack = vec![(r, 0usize)];
        state[r] = 1;
        while let Some(&(v, i)) = stack.last() {
            if i < out[v].len() {
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
                let ei = out[v][i];
                let w = fl.edges[ei].to;
                match state[w] {
                    0 => {
                        state[w] = 1;
                        stack.push((w, 0));
                    }
                    1 => back[ei] = true,
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    // effective (acyclic) edges: a back edge is laid out reversed
    let eff: Vec<(usize, usize)> = fl.edges.iter().enumerate().map(|(i, e)| if back[i] { (e.to, e.from) } else { (e.from, e.to) }).collect();

    // --- layers: each node one past its deepest predecessor (Kahn's order, ties by index)
    let mut layer = vec![0usize; n];
    let mut deg = vec![0usize; n];
    let mut succ: Vec<Vec<usize>> = vec![vec![]; n];
    for (a, b) in &eff {
        deg[*b] += 1;
        succ[*a].push(*b);
    }
    let mut ready: Vec<usize> = (0..n).filter(|v| deg[*v] == 0).collect();
    while !ready.is_empty() {
        let v = ready.remove(0);
        for &w in &succ[v] {
            layer[w] = layer[w].max(layer[v] + 1);
            deg[w] -= 1;
            if deg[w] == 0 {
                let at = ready.iter().position(|x| *x > w).unwrap_or(ready.len());
                ready.insert(at, w);
            }
        }
    }
    let layers_n = layer.iter().copied().max().unwrap_or(0) + 1;

    // --- items: nodes, plus stand-ins on layers an edge passes through
    let dir_down = !fl.right;
    let mut items: Vec<Item> = (0..n)
        .map(|i| {
            let s = &shapes[i];
            let (cross, along) = if dir_down { (s.w, s.h) } else { (s.h, s.w) };
            Item { layer: layer[i], cross, along, pos: 0.0 }
        })
        .collect();
    let mut chains: Vec<Vec<usize>> = vec![];
    let mut links: Vec<(usize, usize)> = vec![];
    for (a, b) in &eff {
        let mut chain = vec![*a];
        for l in layer[*a] + 1..layer[*b] {
            items.push(Item { layer: l, cross: 14.0, along: 0.0, pos: 0.0 });
            chain.push(items.len() - 1);
        }
        chain.push(*b);
        for w in chain.windows(2) {
            links.push((w[0], w[1]));
        }
        chains.push(chain);
    }
    let mut order: Vec<Vec<usize>> = vec![vec![]; layers_n];
    for (i, it) in items.iter().enumerate() {
        order[it.layer].push(i);
    }
    let set_pos = |order: &Vec<Vec<usize>>, items: &mut Vec<Item>| {
        for l in order {
            for (p, it) in l.iter().enumerate() {
                items[*it].pos = p as f64;
            }
        }
    };
    set_pos(&order, &mut items);

    // --- order within layers: average position of the neighbours, a few sweeps down and up
    let neighbours = |it: usize, up: bool| -> Vec<usize> { links.iter().filter(|(a, b)| if up { *b == it } else { *a == it }).map(|(a, b)| if up { *a } else { *b }).collect() };
    for sweep in 0..6 {
        let up = sweep % 2 == 0;
        let range: Vec<usize> = if up { (1..layers_n).collect() } else { (0..layers_n.saturating_sub(1)).rev().collect() };
        for l in range {
            let mut keyed: Vec<(f64, usize, usize)> = order[l]
                .iter()
                .enumerate()
                .map(|(cur, it)| {
                    let ns = neighbours(*it, up);
                    let key = if ns.is_empty() { cur as f64 } else { ns.iter().map(|x| items[*x].pos).sum::<f64>() / ns.len() as f64 };
                    (key, cur, *it)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
            order[l] = keyed.into_iter().map(|k| k.2).collect();
            set_pos(&order, &mut items);
        }
    }

    // --- across the layer: packed, then pulled towards the neighbours without breaking order or gaps
    let mut centre = vec![0.0f64; items.len()];
    let mut widest = 0.0f64;
    let mut totals = vec![0.0f64; layers_n];
    for (l, row) in order.iter().enumerate() {
        let mut x = 0.0;
        for it in row {
            centre[*it] = x + items[*it].cross / 2.0;
            x += items[*it].cross + CROSS_GAP;
        }
        totals[l] = (x - CROSS_GAP).max(0.0);
        widest = widest.max(totals[l]);
    }
    for (l, row) in order.iter().enumerate() {
        let shift = (widest - totals[l]) / 2.0;
        for it in row {
            centre[*it] += shift;
        }
    }
    for pass in 0..6 {
        let up = pass % 2 == 0;
        let range: Vec<usize> = if up { (1..layers_n).collect() } else { (0..layers_n.saturating_sub(1)).rev().collect() };
        for l in range {
            let mut limit = f64::NEG_INFINITY;
            for it in order[l].clone() {
                let ns = neighbours(it, up);
                let want = if ns.is_empty() { centre[it] } else { ns.iter().map(|x| centre[*x]).sum::<f64>() / ns.len() as f64 };
                let least = limit + CROSS_GAP + items[it].cross / 2.0;
                centre[it] = if limit.is_finite() { want.max(least) } else { want };
                limit = centre[it] + items[it].cross / 2.0;
            }
        }
    }
    let left = items.iter().enumerate().map(|(i, it)| centre[i] - it.cross / 2.0).fold(f64::INFINITY, f64::min);
    let right_edge = items.iter().enumerate().map(|(i, it)| centre[i] + it.cross / 2.0).fold(f64::NEG_INFINITY, f64::max);
    let span = if left.is_finite() { right_edge - left } else { 0.0 };

    // --- along the flow: layer bands, with room between them for arrows and labels
    let gap = if dir_down { 54.0 } else { 84.0 };
    let band: Vec<f64> = (0..layers_n).map(|l| order[l].iter().map(|it| items[*it].along).fold(0.0, f64::max)).collect();
    let mut start = vec![0.0f64; layers_n];
    for l in 1..layers_n {
        start[l] = start[l - 1] + band[l - 1] + gap;
    }
    let along_total = start[layers_n - 1] + band[layers_n - 1];

    let (ox, oy) = (PAD, top);
    let point = |it: usize| -> (f64, f64) {
        let c = centre[it] - left;
        let a = start[items[it].layer] + band[items[it].layer] / 2.0;
        if dir_down {
            (ox + c, oy + a)
        } else {
            (ox + a, oy + c)
        }
    };
    let flow_dir = if dir_down { (0.0, 1.0) } else { (1.0, 0.0) };

    // --- where edges meet a node: spread along its side, in the order of where they come from or go to
    let mut out_ports: Vec<Vec<(f64, usize)>> = vec![vec![]; n];
    let mut in_ports: Vec<Vec<(f64, usize)>> = vec![vec![]; n];
    for (ei, ch) in chains.iter().enumerate() {
        out_ports[ch[0]].push((centre[ch[1]], ei));
        in_ports[ch[ch.len() - 1]].push((centre[ch[ch.len() - 2]], ei));
    }
    let mut off_out = vec![0.0f64; chains.len()];
    let mut off_in = vec![0.0f64; chains.len()];
    for i in 0..n {
        for (ports, offs) in [(&mut out_ports[i], &mut off_out), (&mut in_ports[i], &mut off_in)] {
            ports.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
            let count = ports.len() as f64;
            let step = ((items[i].cross - 24.0) / count.max(1.0)).clamp(6.0, 16.0);
            for (k, (_, ei)) in ports.iter().enumerate() {
                offs[*ei] = (k as f64 - (count - 1.0) / 2.0) * step;
            }
        }
    }
    // how far a node's border is from its centre along the flow, at an offset across it (a diamond narrows to its points)
    let reach = |it: usize, off: f64| -> f64 {
        let s = items[it].along / 2.0;
        if it < n && fl.nodes[it].kind == NodeKind::Decision {
            s * (1.0 - (off.abs() / (items[it].cross / 2.0)).min(1.0))
        } else {
            s
        }
    };
    let across = |p: (f64, f64), off: f64| if dir_down { (p.0 + off, p.1) } else { (p.0, p.1 + off) };

    // --- draw: edges first, so the nodes sit over them
    let mut body = String::new();
    let mut labels = String::new();
    for (ei, chain) in chains.iter().enumerate() {
        let is_back = back[ei];
        let mut pts: Vec<(f64, f64)> = chain.iter().map(|it| point(*it)).collect();
        // leave and enter at the node borders facing the flow
        let first = chain[0];
        let last = chain[chain.len() - 1];
        let (o_out, o_in) = (off_out[ei], off_in[ei]);
        let s0 = across(pts[0], o_out);
        pts[0] = (s0.0 + flow_dir.0 * reach(first, o_out), s0.1 + flow_dir.1 * reach(first, o_out));
        let end = pts.len() - 1;
        let e0 = across(pts[end], o_in);
        pts[end] = (e0.0 - flow_dir.0 * reach(last, o_in), e0.1 - flow_dir.1 * reach(last, o_in));
        let (tip, dir) = if is_back { (pts[0], (-flow_dir.0, -flow_dir.1)) } else { (pts[end], flow_dir) };
        if is_back {
            pts[0] = (pts[0].0 + flow_dir.0 * 9.0, pts[0].1 + flow_dir.1 * 9.0);
        } else {
            pts[end] = (pts[end].0 - flow_dir.0 * 9.0, pts[end].1 - flow_dir.1 * 9.0);
        }
        let mut d = format!("M{},{}", f(pts[0].0), f(pts[0].1));
        for w in pts.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            let (c1, c2) = if dir_down { ((x0, y0 + (y1 - y0) / 2.0), (x1, y1 - (y1 - y0) / 2.0)) } else { ((x0 + (x1 - x0) / 2.0, y0), (x1 - (x1 - x0) / 2.0, y1)) };
            d.push_str(&format!(" C{},{} {},{} {},{}", f(c1.0), f(c1.1), f(c2.0), f(c2.1), f(x1), f(y1)));
        }
        body.push_str(&format!("<path class=\"ed\" d=\"{}\"/>\n", d));
        arrow(&mut body, "ar", tip.0, tip.1, dir.0, dir.1);
        if let Some(label) = &fl.edges[ei].label {
            let mid = (pts.len() - 1) / 2;
            let (a, b) = (pts[mid], pts[mid + 1]);
            let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let lw = tw(label, 11.0) + 12.0;
            labels.push_str(&format!("<rect class=\"bk\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"18\" rx=\"4\"/>\n", f(mx - lw / 2.0), f(my - 9.0), f(lw)));
            text(&mut labels, mx, my + 4.0, "t2", 11.0, "middle", label);
        }
    }
    body.push_str(&labels);

    // --- nodes
    for (i, node) in fl.nodes.iter().enumerate() {
        let s = &shapes[i];
        let (cx, cy) = point(i);
        let class = match node.group {
            Some(g) => format!("nd{}", g + 1),
            None => "nd".to_string(),
        };
        let (x0, y0) = (cx - s.w / 2.0, cy - s.h / 2.0);
        match node.kind {
            NodeKind::Decision => body.push_str(&format!("<path class=\"{}\" d=\"M{},{} L{},{} L{},{} L{},{} Z\"/>\n", class, f(cx), f(y0), f(cx + s.w / 2.0), f(cy), f(cx), f(cy + s.h / 2.0), f(cx - s.w / 2.0), f(cy))),
            NodeKind::Start | NodeKind::End => body.push_str(&format!("<rect class=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"/>\n", class, f(x0), f(y0), f(s.w), f(s.h), f(s.h / 2.0))),
            NodeKind::Step => body.push_str(&format!("<rect class=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\"/>\n", class, f(x0), f(y0), f(s.w), f(s.h))),
        }
        let first = cy - (s.lines.len() as f64 - 1.0) * 7.5 + 4.0;
        for (li, line) in s.lines.iter().enumerate() {
            text(&mut body, cx, first + li as f64 * 15.0, if matches!(node.kind, NodeKind::Start | NodeKind::End) { "t1 b" } else { "t1" }, 12.0, "middle", line);
        }
    }

    // --- size and description
    let (cw, ch) = if dir_down { (span, along_total) } else { (along_total, span) };
    let steps = fl.nodes.iter().filter(|n| n.kind != NodeKind::Decision).count();
    let mut desc = format!("Flow diagram with {} step{}{} and {} connection{}, running {}. ", steps, if steps == 1 { "" } else { "s" }, if steps == n { String::new() } else { format!(" and {} decision{}", n - steps, if n - steps == 1 { "" } else { "s" }) }, fl.edges.len(), if fl.edges.len() == 1 { "" } else { "s" }, if dir_down { "top to bottom" } else { "left to right" });
    let names = |i: usize| escape_text(&fl.nodes[i].label);
    let parts: Vec<String> = fl.edges.iter().take(60).map(|e| format!("{} to {}{}", names(e.from), names(e.to), e.label.as_ref().map(|l| format!(" ({})", escape_text(l))).unwrap_or_default())).collect();
    desc.push_str(&parts.join("; "));
    desc.push('.');
    Scene { w: cw + 2.0 * PAD, h: top + ch + PAD, body, desc }
}

/// Text for a description: on one line (the writer escapes it when it becomes an element).
fn escape_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
