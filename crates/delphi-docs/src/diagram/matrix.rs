//! Matrices: rows against columns, each cell a mark or a few words. `yes`/`true`, `partial` and `no`/`false` are drawn
//! as a filled dot, a half dot and an empty ring (three shapes, so colour never carries the meaning alone), with a key
//! under the grid; any other text is printed in the cell.

use super::{bad, short, wrap, Scene};
use crate::chart::scale::f;
use crate::chart::{text, tw, PAD};
use crate::DocError;
use serde_json::Value;

const MAX_ROWS: usize = 30;
const MAX_COLS: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Cell {
    Empty,
    Yes,
    Partial,
    No,
    Text(String),
}

#[derive(Debug, Clone)]
pub(super) struct Matrix {
    pub rows: Vec<String>,
    pub cols: Vec<String>,
    pub cells: Vec<Vec<Cell>>,
    pub corner: Option<String>,
}

fn cell_of(v: &Value, what: &str) -> Result<Cell, DocError> {
    Ok(match v {
        Value::Null => Cell::Empty,
        Value::Bool(true) => Cell::Yes,
        Value::Bool(false) => Cell::No,
        Value::Number(n) => Cell::Text(n.to_string()),
        Value::String(s) => match s.trim().to_lowercase().as_str() {
            "" => Cell::Empty,
            "yes" | "y" | "true" => Cell::Yes,
            "partial" | "some" | "partly" => Cell::Partial,
            "no" | "n" | "false" => Cell::No,
            _ if s.chars().count() > 40 => return Err(bad(format!("{} is over 40 characters; a cell holds a few words", what))),
            _ => Cell::Text(s.trim().to_string()),
        },
        _ => return Err(bad(format!("{} is a list or object; a cell is yes, partial, no, text, a number or empty", what))),
    })
}

pub(super) fn parse(v: &Value) -> Result<Matrix, DocError> {
    let names = |k: &str, max: usize| -> Result<Vec<String>, DocError> {
        let list = v.get(k).and_then(|x| x.as_array()).ok_or_else(|| bad(format!("a matrix needs `{}`: an array of labels", k)))?;
        if list.len() < 2 || list.len() > max {
            return Err(bad(format!("a matrix has between 2 and {} {}; for one, use a list", max, k)));
        }
        list.iter().map(|x| x.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s.chars().count() <= 40).ok_or_else(|| bad(format!("every entry of `{}` is a label of up to 40 characters", k)))).collect()
    };
    let (rows, cols) = (names("rows", MAX_ROWS)?, names("columns", MAX_COLS)?);
    let grid = v.get("cells").and_then(|c| c.as_array()).ok_or_else(|| bad("a matrix needs `cells`: one array per row, one value per column"))?;
    if grid.len() != rows.len() {
        return Err(bad(format!("`cells` has {} rows but there are {} `rows`", grid.len(), rows.len())));
    }
    let mut cells = vec![];
    for (i, r) in grid.iter().enumerate() {
        let r = r.as_array().ok_or_else(|| bad(format!("row {} of `cells` is not an array", i + 1)))?;
        if r.len() != cols.len() {
            return Err(bad(format!("row {} (`{}`) of `cells` has {} values but there are {} `columns`", i + 1, rows[i], r.len(), cols.len())));
        }
        cells.push(r.iter().enumerate().map(|(j, c)| cell_of(c, &format!("`{}` / `{}`", rows[i], cols[j]))).collect::<Result<Vec<_>, _>>()?);
    }
    Ok(Matrix { rows, cols, cells, corner: short(v, "corner", 40, "`corner`")? })
}

pub(super) fn layout(m: &Matrix, top: f64) -> Scene {
    let row_label_w = m.rows.iter().map(|r| tw(r, 12.0)).fold(tw(m.corner.as_deref().unwrap_or(""), 11.0), f64::max).min(240.0) + 24.0;
    let col_lines: Vec<Vec<String>> = m.cols.iter().map(|c| wrap(c, 14, 2)).collect();
    let cell_w = m
        .cells
        .iter()
        .flatten()
        .map(|c| if let Cell::Text(t) = c { tw(t, 12.0) + 20.0 } else { 0.0 })
        .chain(col_lines.iter().map(|l| l.iter().map(|x| tw(x, 12.0)).fold(0.0, f64::max) + 20.0))
        .fold(84.0, f64::max)
        .min(150.0);
    let head_h = 12.0 + col_lines.iter().map(|l| l.len()).max().unwrap_or(1) as f64 * 15.0;
    let row_h = 34.0;
    let (x0, y0) = (PAD, top);
    let grid_w = row_label_w + cell_w * m.cols.len() as f64;
    let mut body = String::new();
    if let Some(c) = &m.corner {
        text(&mut body, x0, y0 + head_h - 6.0, "t3", 11.0, "start", c);
    }
    for (j, lines) in col_lines.iter().enumerate() {
        let cx = x0 + row_label_w + cell_w * (j as f64 + 0.5);
        for (li, l) in lines.iter().enumerate() {
            text(&mut body, cx, y0 + 16.0 + li as f64 * 15.0, "t1 b", 12.0, "middle", l);
        }
    }
    let line_y = y0 + head_h;
    body.push_str(&format!("<line class=\"ax\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n", f(x0), f(line_y), f(x0 + grid_w), f(line_y)));
    let mut used = [false; 3];
    for (i, r) in m.rows.iter().enumerate() {
        let ry = line_y + i as f64 * row_h;
        text(&mut body, x0, ry + row_h / 2.0 + 4.0, "t1", 12.0, "start", &crate::chart::trunc(r, row_label_w - 12.0, 12.0));
        if i + 1 < m.rows.len() {
            body.push_str(&format!("<line class=\"g\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n", f(x0), f(ry + row_h), f(x0 + grid_w), f(ry + row_h)));
        }
        for (j, c) in m.cells[i].iter().enumerate() {
            let (cx, cy) = (x0 + row_label_w + cell_w * (j as f64 + 0.5), ry + row_h / 2.0);
            match c {
                Cell::Empty => {}
                Cell::Yes => {
                    used[0] = true;
                    dot(&mut body, 0, cx, cy);
                }
                Cell::Partial => {
                    used[1] = true;
                    dot(&mut body, 1, cx, cy);
                }
                Cell::No => {
                    used[2] = true;
                    dot(&mut body, 2, cx, cy);
                }
                Cell::Text(t) => text(&mut body, cx, cy + 4.0, "t2", 12.0, "middle", &crate::chart::trunc(t, cell_w - 12.0, 12.0)),
            }
        }
    }
    let grid_bottom = line_y + m.rows.len() as f64 * row_h;
    body.push_str(&format!("<line class=\"ax\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n", f(x0), f(grid_bottom), f(x0 + grid_w), f(grid_bottom)));
    let mut key_y = grid_bottom;
    if used.iter().any(|u| *u) {
        key_y += 24.0;
        let mut x = x0;
        for (k, name) in ["yes", "partial", "no"].iter().enumerate() {
            if used[k] {
                dot(&mut body, k, x + 7.0, key_y - 4.0);
                text(&mut body, x + 20.0, key_y, "t2", 12.0, "start", name);
                x += 40.0 + tw(name, 12.0);
            }
        }
        key_y += 8.0;
    }
    let word = |c: &Cell| match c {
        Cell::Yes => "yes".to_string(),
        Cell::Partial => "partial".to_string(),
        Cell::No => "no".to_string(),
        Cell::Text(t) => t.clone(),
        Cell::Empty => "blank".to_string(),
    };
    let mut desc = format!("Matrix of {} rows by {} columns: {}. ", m.rows.len(), m.cols.len(), m.cols.join(", "));
    desc.push_str(&m.rows.iter().zip(&m.cells).take(40).map(|(r, cs)| format!("{}: {}", r, m.cols.iter().zip(cs).map(|(c, v)| format!("{} {}", c, word(v))).collect::<Vec<_>>().join(", "))).collect::<Vec<_>>().join("; "));
    desc.push('.');
    Scene { w: grid_w + 2.0 * PAD, h: key_y + PAD, body, desc }
}

/// 0 filled dot, 1 half dot, 2 empty ring.
fn dot(out: &mut String, which: usize, cx: f64, cy: f64) {
    match which {
        0 => out.push_str(&format!("<circle class=\"f1\" cx=\"{}\" cy=\"{}\" r=\"7\"/>\n", f(cx), f(cy))),
        1 => {
            out.push_str(&format!("<path class=\"f1\" d=\"M{},{} A7,7 0 0 0 {},{} Z\"/>\n", f(cx), f(cy - 7.0), f(cx), f(cy + 7.0)));
            out.push_str(&format!("<circle class=\"k1\" cx=\"{}\" cy=\"{}\" r=\"7\"/>\n", f(cx), f(cy)));
        }
        _ => out.push_str(&format!("<circle class=\"ed2\" cx=\"{}\" cy=\"{}\" r=\"6\"/>\n", f(cx), f(cy))),
    }
}
