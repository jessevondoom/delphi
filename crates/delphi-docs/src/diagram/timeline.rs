//! Timelines: events along an axis. When every event has a date the axis is proportional (a month between two
//! events looks like a month); otherwise events are spaced evenly in the order given. Event cards sit on the nearest
//! free tier above or below the axis, so neighbours never overlap, and each hangs from its dot on a stem.

use super::{bad, short, wrap, Scene};
use crate::chart::scale::{date_ticks, f, parse_date};
use crate::chart::{text, tw, PAD};
use crate::DocError;
use serde_json::Value;

const MAX_EVENTS: usize = 24;
const MAX_GROUPS: usize = 4;
const TIERS: usize = 6;

#[derive(Debug, Clone)]
pub(super) struct Event {
    pub when: String,
    pub key: f64,
    pub label: String,
    pub detail: Option<String>,
    pub group: Option<usize>,
}

#[derive(Debug, Clone)]
pub(super) struct Timeline {
    pub events: Vec<Event>,
    pub groups: Vec<String>,
    pub dated: bool,
}

pub(super) fn parse(v: &Value) -> Result<Timeline, DocError> {
    let list = v.get("events").and_then(|e| e.as_array()).ok_or_else(|| bad("a timeline needs `events`: [{when, label, detail?, group?}]"))?;
    if list.len() < 2 || list.len() > MAX_EVENTS {
        return Err(bad(format!("a timeline has between 2 and {} events; keep the ones that matter and say the rest in `detail`", MAX_EVENTS)));
    }
    let (mut events, mut groups) = (vec![], Vec::<String>::new());
    for (i, e) in list.iter().enumerate() {
        let when = short(e, "when", 30, "`when`")?.ok_or_else(|| bad(format!("event {} needs `when`: a date like 2026-10-25, or a short name for the moment", i + 1)))?;
        let label = short(e, "label", 60, "an event label")?.ok_or_else(|| bad(format!("event {} needs a `label`", i + 1)))?;
        let group = match short(e, "group", 40, "a group name")? {
            Some(g) => Some(match groups.iter().position(|x| *x == g) {
                Some(p) => p,
                None => {
                    if groups.len() == MAX_GROUPS {
                        return Err(bad(format!("a timeline colours at most {} groups; `{}` would be a fifth", MAX_GROUPS, g)));
                    }
                    groups.push(g);
                    groups.len() - 1
                }
            }),
            None => None,
        };
        events.push(Event { key: parse_date(&when).unwrap_or(f64::NAN), when, label, detail: short(e, "detail", 120, "an event detail")?, group });
    }
    let dated = events.iter().all(|e| e.key.is_finite());
    if dated {
        events.sort_by(|a, b| a.key.partial_cmp(&b.key).unwrap_or(std::cmp::Ordering::Equal));
    }
    Ok(Timeline { events, groups, dated })
}

struct Card {
    cx: f64,
    left: f64,
    w: f64,
    h: f64,
    lines: Vec<(String, &'static str, f64)>,
    tier: usize,
}

pub(super) fn layout(t: &Timeline, top: f64) -> Scene {
    let n = t.events.len();
    let span = (n as f64 * 110.0).clamp(640.0, 1500.0);
    let (x0, x1) = (PAD + 80.0, PAD + 80.0 + span);
    let (lo, hi) = (t.events.first().map(|e| e.key).unwrap_or(0.0), t.events.last().map(|e| e.key).unwrap_or(1.0));
    let xs: Vec<f64> = t.events.iter().enumerate().map(|(i, e)| if t.dated && hi > lo { x0 + (e.key - lo) / (hi - lo) * span } else { x0 + (i as f64 + 0.5) / n as f64 * span }).collect();
    // cards
    let mut cards: Vec<Card> = vec![];
    for (i, e) in t.events.iter().enumerate() {
        let mut lines: Vec<(String, &'static str, f64)> = vec![(e.when.clone(), "t3", 11.0)];
        lines.extend(wrap(&e.label, 20, 3).into_iter().map(|l| (l, "t1 b", 12.0)));
        if let Some(d) = &e.detail {
            lines.extend(wrap(d, 26, 3).into_iter().map(|l| (l, "t2", 11.0)));
        }
        let w = lines.iter().map(|(l, _, s)| tw(l, *s)).fold(0.0, f64::max) + 30.0;
        let h = 12.0 + lines.iter().map(|(_, c, _)| if *c == "t1 b" { 15.0 } else { 14.0 }).sum::<f64>();
        let left = (xs[i] - w / 2.0).max(PAD);
        cards.push(Card { cx: xs[i], left, w, h, lines, tier: 0 });
    }
    // each card takes the first tier whose last card ends before this one starts (tiers alternate above and below)
    let mut right_edge = [f64::NEG_INFINITY; TIERS];
    for c in cards.iter_mut() {
        let free = (0..TIERS).find(|t| right_edge[*t] + 12.0 <= c.left);
        c.tier = free.unwrap_or_else(|| (0..TIERS).min_by(|a, b| right_edge[*a].partial_cmp(&right_edge[*b]).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(0));
        right_edge[c.tier] = c.left + c.w;
    }
    // band heights: level 1 is nearest the axis; even tiers are above, odd below
    let level = |tier: usize| tier / 2;
    let mut band_above = [0.0f64; TIERS / 2];
    let mut band_below = [0.0f64; TIERS / 2];
    for c in &cards {
        let b = if c.tier % 2 == 0 { &mut band_above } else { &mut band_below };
        b[level(c.tier)] = b[level(c.tier)].max(c.h);
    }
    let above_total: f64 = band_above.iter().filter(|h| **h > 0.0).map(|h| h + 22.0).sum();
    let axis_y = top + above_total.max(30.0) + 10.0;
    let mut body = String::new();
    body.push_str(&format!("<path class=\"ed2\" d=\"M{},{} L{},{}\"/>\n", f(x0 - 20.0), f(axis_y), f(x1 + 20.0), f(axis_y)));
    if t.dated && hi > lo {
        // a date label is left out where a stem hangs through the space it would take
        let below_stems: Vec<f64> = cards.iter().filter(|c| c.tier % 2 == 1).map(|c| c.cx).collect();
        for (v, label) in date_ticks(lo, hi) {
            let x = x0 + (v - lo) / (hi - lo) * span;
            let clear = !below_stems.iter().any(|s| (s - x).abs() < tw(&label, 11.0) / 2.0 + 6.0);
            body.push_str(&format!("<line class=\"ll\" x1=\"{x}\" y1=\"{}\" x2=\"{x}\" y2=\"{}\"/>\n", f(axis_y - 4.0), f(axis_y + 4.0), x = f(x)));
            if clear {
                text(&mut body, x, axis_y + 18.0, "t3", 11.0, "middle", &label);
            }
        }
    }
    let below_start = axis_y + 34.0;
    let mut stems = String::new();
    let mut marks = String::new();
    for (i, c) in cards.iter().enumerate() {
        let above = c.tier % 2 == 0;
        let lv = level(c.tier);
        let offset: f64 = (0..lv).map(|k| (if above { band_above[k] } else { band_below[k] }) + 22.0).sum();
        // a card rests against the near edge of its band: above the axis its bottom, below the axis its top
        let y_top = if above { axis_y - 18.0 - offset - c.h } else { below_start + offset };
        let stem_to = if above { y_top + c.h } else { y_top };
        stems.push_str(&format!("<line class=\"ll\" x1=\"{x}\" y1=\"{}\" x2=\"{x}\" y2=\"{}\"/>\n", f(axis_y), f(stem_to), x = f(c.cx)));
        let e = &t.events[i];
        let class = match e.group {
            Some(g) => format!("nd{}", g + 1),
            None => "nd".to_string(),
        };
        let left = c.left.min(x1 + 80.0 - c.w);
        marks.push_str(&format!("<rect class=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\"/>\n", class, f(left), f(y_top), f(c.w), f(c.h)));
        let mut y = y_top + 6.0;
        for (l, cls, size) in &c.lines {
            let step = if *cls == "t1 b" { 15.0 } else { 14.0 };
            text(&mut marks, left + 10.0, y + step - 4.0, cls, *size, "start", l);
            y += step;
        }
        marks.push_str(&format!("<circle class=\"{}\" cx=\"{}\" cy=\"{}\" r=\"5\"/>\n", match e.group { Some(g) => format!("d{}", g + 1), None => "nd".into() }, f(c.cx), f(axis_y)));
    }
    body.push_str(&stems);
    body.push_str(&marks);
    let below_total: f64 = band_below.iter().filter(|h| **h > 0.0).map(|h| h + 22.0).sum();
    let h = axis_y + 34.0 + below_total + PAD;
    let width = x1 + 80.0 + PAD;
    let first = t.events.first().map(|e| e.when.clone()).unwrap_or_default();
    let last = t.events.last().map(|e| e.when.clone()).unwrap_or_default();
    let mut desc = format!("Timeline of {} events from {} to {}. ", n, first, last);
    desc.push_str(&t.events.iter().map(|e| format!("{}: {}{}", e.when, e.label.split_whitespace().collect::<Vec<_>>().join(" "), e.detail.as_ref().map(|d| format!(" ({})", d)).unwrap_or_default())).collect::<Vec<_>>().join("; "));
    desc.push('.');
    Scene { w: width, h, body, desc }
}
