//! Tree diagrams: a root and its children, nested, laid out top-down or left-to-right with elbow connectors. Leaves
//! take consecutive slots and a parent sits centred over its children, so the picture is as wide as the tree is wide
//! and no two nodes overlap.

use super::{bad, short, wrap, Scene};
use crate::chart::scale::f;
use crate::chart::{text, tw, PAD};
use crate::DocError;
use serde_json::Value;

const MAX_NODES: usize = 80;
const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone)]
pub(super) struct TNode {
    pub label: String,
    pub children: Vec<usize>,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub(super) struct Tree {
    pub nodes: Vec<TNode>,
    pub right: bool,
}

fn read(v: &Value, depth: usize, nodes: &mut Vec<TNode>) -> Result<usize, DocError> {
    if depth > MAX_DEPTH {
        return Err(bad(format!("a tree goes at most {} levels deep; fold the deeper parts into their parent's label or draw them as another diagram", MAX_DEPTH)));
    }
    if nodes.len() >= MAX_NODES {
        return Err(bad(format!("a tree has at most {} nodes; show the main branches and name the rest in a label", MAX_NODES)));
    }
    let label = short(v, "label", 60, "a node label")?.ok_or_else(|| bad("every node needs a `label`"))?;
    let at = nodes.len();
    nodes.push(TNode { label, children: vec![], depth });
    if let Some(kids) = v.get("children") {
        for k in kids.as_array().ok_or_else(|| bad("`children` is an array of nodes"))? {
            let c = read(k, depth + 1, nodes)?;
            nodes[at].children.push(c);
        }
    }
    Ok(at)
}

pub(super) fn parse(v: &Value) -> Result<Tree, DocError> {
    let root = v.get("root").ok_or_else(|| bad("a tree needs a `root`: {label, children: [{label, children: [...]}]}"))?;
    let mut nodes = vec![];
    read(root, 0, &mut nodes)?;
    if nodes.len() < 2 {
        return Err(bad("a tree with only a root is just a label; give the root `children`"));
    }
    let right = match v.get("direction").and_then(|d| d.as_str()).unwrap_or("down") {
        "down" | "top-down" => false,
        "right" | "left-right" => true,
        other => return Err(bad(format!("`direction` is down or right, not `{}`", other))),
    };
    Ok(Tree { nodes, right })
}

struct Boxed {
    lines: Vec<String>,
    w: f64,
    h: f64,
}

pub(super) fn layout(t: &Tree, top: f64) -> Scene {
    let n = t.nodes.len();
    let boxes: Vec<Boxed> = t
        .nodes
        .iter()
        .map(|nd| {
            let lines = wrap(&nd.label, if t.right { 28 } else { 18 }, 3);
            let widest = lines.iter().map(|l| tw(l, 12.0)).fold(0.0, f64::max);
            Boxed { w: (widest + 24.0).max(64.0), h: (20.0 + lines.len() as f64 * 15.0).max(34.0), lines }
        })
        .collect();
    let depth_max = t.nodes.iter().map(|x| x.depth).max().unwrap_or(0);
    // leaf slots in depth-first order; a parent's position is the middle of its first and last child
    let mut slot = vec![0.0f64; n];
    let mut next = 0.0;
    fn walk(t: &Tree, i: usize, slot: &mut [f64], next: &mut f64) {
        if t.nodes[i].children.is_empty() {
            slot[i] = *next;
            *next += 1.0;
        } else {
            for c in &t.nodes[i].children {
                walk(t, *c, slot, next);
            }
            let kids = &t.nodes[i].children;
            slot[i] = (slot[kids[0]] + slot[kids[kids.len() - 1]]) / 2.0;
        }
    }
    walk(t, 0, &mut slot, &mut next);
    let leaves = next;
    let (unit, rows): (f64, Vec<f64>) = if t.right {
        let unit = boxes.iter().map(|b| b.h).fold(0.0, f64::max) + 14.0;
        (unit, vec![])
    } else {
        (boxes.iter().map(|b| b.w).fold(0.0, f64::max) + 16.0, (0..=depth_max).map(|d| (0..n).filter(|i| t.nodes[*i].depth == d).map(|i| boxes[i].h).fold(0.0, f64::max)).collect())
    };
    // where each node's centre goes
    let mut centre = vec![(0.0f64, 0.0f64); n];
    if t.right {
        let col_w: Vec<f64> = (0..=depth_max).map(|d| (0..n).filter(|i| t.nodes[*i].depth == d).map(|i| boxes[i].w).fold(0.0, f64::max)).collect();
        let mut col_x = vec![0.0f64; depth_max + 1];
        for d in 1..=depth_max {
            col_x[d] = col_x[d - 1] + col_w[d - 1] + 54.0;
        }
        for i in 0..n {
            let d = t.nodes[i].depth;
            centre[i] = (PAD + col_x[d] + boxes[i].w / 2.0, top + slot[i] * unit + unit / 2.0);
        }
    } else {
        let mut row_y = vec![0.0f64; depth_max + 1];
        for d in 1..=depth_max {
            row_y[d] = row_y[d - 1] + rows[d - 1] + 42.0;
        }
        for i in 0..n {
            let d = t.nodes[i].depth;
            centre[i] = (PAD + slot[i] * unit + unit / 2.0, top + row_y[d] + rows[d] / 2.0);
        }
    }
    let mut body = String::new();
    for (i, nd) in t.nodes.iter().enumerate() {
        for &c in &nd.children {
            let (p, q) = (centre[i], centre[c]);
            if t.right {
                let (x0, x1, mid) = (p.0 + boxes[i].w / 2.0, q.0 - boxes[c].w / 2.0, (p.0 + boxes[i].w / 2.0 + q.0 - boxes[c].w / 2.0) / 2.0);
                body.push_str(&format!("<path class=\"ed\" d=\"M{},{} L{},{} L{},{} L{},{}\"/>\n", f(x0), f(p.1), f(mid), f(p.1), f(mid), f(q.1), f(x1), f(q.1)));
            } else {
                let (y0, y1) = (p.1 + boxes[i].h / 2.0, q.1 - boxes[c].h / 2.0);
                let mid = (y0 + y1) / 2.0;
                body.push_str(&format!("<path class=\"ed\" d=\"M{},{} L{},{} L{},{} L{},{}\"/>\n", f(p.0), f(y0), f(p.0), f(mid), f(q.0), f(mid), f(q.0), f(y1)));
            }
        }
    }
    for (i, nd) in t.nodes.iter().enumerate() {
        let b = &boxes[i];
        let (cx, cy) = centre[i];
        body.push_str(&format!("<rect class=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\"/>\n", if nd.depth == 0 { "nd1" } else { "nd" }, f(cx - b.w / 2.0), f(cy - b.h / 2.0), f(b.w), f(b.h)));
        let first = cy - (b.lines.len() as f64 - 1.0) * 7.5 + 4.0;
        for (li, line) in b.lines.iter().enumerate() {
            text(&mut body, cx, first + li as f64 * 15.0, if nd.depth == 0 { "t1 b" } else { "t1" }, 12.0, "middle", line);
        }
    }
    let (w, h) = if t.right {
        let col_right = centre.iter().zip(&boxes).map(|(c, b)| c.0 + b.w / 2.0).fold(0.0, f64::max);
        (col_right + PAD, top + leaves * unit + PAD)
    } else {
        (PAD * 2.0 + leaves * unit, top + rows.iter().sum::<f64>() + 42.0 * depth_max as f64 + PAD)
    };
    let names: Vec<String> = t.nodes[0].children.iter().map(|c| t.nodes[*c].label.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    let desc = format!("Tree diagram: {} has {} branch{}: {}. {} nodes in all, {} levels.", t.nodes[0].label.split_whitespace().collect::<Vec<_>>().join(" "), names.len(), if names.len() == 1 { "" } else { "es" }, names.join(", "), n, depth_max + 1);
    Scene { w, h, body, desc }
}
