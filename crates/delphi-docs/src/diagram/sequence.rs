//! Sequence diagrams: participants across the top with a lifeline each, and messages as arrows between them in
//! the order given, one row apiece. Lanes are as wide as the longest message label between neighbours needs, so no
//! label is cut or crosses another.

use super::{arrow, bad, short, Scene};
use crate::chart::scale::f;
use crate::chart::{text, tw, PAD};
use crate::DocError;
use serde_json::Value;

const MAX_PARTICIPANTS: usize = 8;
const MAX_MESSAGES: usize = 60;
const BOX_H: f64 = 34.0;
const ROW: f64 = 46.0;

#[derive(Debug, Clone)]
pub(super) struct Participant {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub(super) struct Message {
    pub from: usize,
    pub to: usize,
    pub label: String,
    pub reply: bool,
}

#[derive(Debug, Clone)]
pub(super) struct Sequence {
    pub participants: Vec<Participant>,
    pub messages: Vec<Message>,
}

pub(super) fn parse(v: &Value) -> Result<Sequence, DocError> {
    let list = v.get("participants").and_then(|p| p.as_array()).ok_or_else(|| bad("a sequence needs `participants`: [{id, label?}] in the order they appear left to right"))?;
    if list.len() < 2 || list.len() > MAX_PARTICIPANTS {
        return Err(bad(format!("a sequence has between 2 and {} participants; with more, split it into several diagrams", MAX_PARTICIPANTS)));
    }
    let mut participants: Vec<Participant> = vec![];
    for (i, p) in list.iter().enumerate() {
        let id = short(p, "id", 40, "a participant id")?.ok_or_else(|| bad(format!("participant {} needs an `id`", i + 1)))?;
        if participants.iter().any(|x| x.id == id) {
            return Err(bad(format!("two participants have the id `{}`", id)));
        }
        let label = short(p, "label", 40, "a participant label")?.unwrap_or_else(|| id.clone());
        participants.push(Participant { id, label });
    }
    let msgs = v.get("messages").and_then(|m| m.as_array()).ok_or_else(|| bad("a sequence needs `messages`: [{from, to, label, kind?}] in time order"))?;
    if msgs.is_empty() || msgs.len() > MAX_MESSAGES {
        return Err(bad(format!("a sequence has between 1 and {} messages", MAX_MESSAGES)));
    }
    let known = || participants.iter().map(|p| format!("`{}`", p.id)).collect::<Vec<_>>().join(", ");
    let mut messages = vec![];
    for (i, m) in msgs.iter().enumerate() {
        let find = |k: &str| -> Result<usize, DocError> {
            let id = short(m, k, 40, "a message endpoint")?.ok_or_else(|| bad(format!("message {} needs `{}`", i + 1, k)))?;
            participants.iter().position(|p| p.id == id).ok_or_else(|| bad(format!("message {} {} `{}`, who is not a participant; the participants are {}", i + 1, if k == "from" { "comes from" } else { "goes to" }, id, known())))
        };
        let (from, to) = (find("from")?, find("to")?);
        let label = short(m, "label", 70, "a message label")?.ok_or_else(|| bad(format!("message {} needs a `label` that says what is sent", i + 1)))?;
        let reply = match m.get("kind").and_then(|k| k.as_str()).unwrap_or("call") {
            "call" | "message" => false,
            "reply" | "return" => true,
            other => return Err(bad(format!("message {} has kind `{}`; use call or reply", i + 1, other))),
        };
        messages.push(Message { from, to, label, reply });
    }
    Ok(Sequence { participants, messages })
}

/// A surface-coloured patch behind a label, so a lifeline passing through it does not strike the words out.
fn back(out: &mut String, x: f64, baseline: f64, anchor: &str, label: &str) {
    let w = tw(label, 12.0) + 8.0;
    let left = match anchor {
        "middle" => x - w / 2.0,
        _ => x - 4.0,
    };
    out.push_str(&format!("<rect class=\"bk\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"16\"/>\n", f(left), f(baseline - 12.0), f(w)));
}

pub(super) fn layout(s: &Sequence, top: f64) -> Scene {
    let k = s.participants.len();
    // a lane has to hold its participant's box and the labels of the messages that cross it
    let mut lane = s.participants.iter().map(|p| tw(&p.label, 12.0) + 40.0).fold(120.0f64, f64::max);
    for m in &s.messages {
        let span = (m.from as f64 - m.to as f64).abs();
        let need = tw(&m.label, 12.0) + 36.0;
        lane = lane.max(if span == 0.0 { 0.0 } else { need / span });
    }
    let lane = lane.min(300.0);
    let x = |i: usize| PAD + lane / 2.0 + i as f64 * lane;
    let mut body = String::new();
    let first_y = top + BOX_H + 34.0;
    let last_y = first_y + (s.messages.len() as f64 - 1.0) * ROW;
    let life_end = last_y + 30.0;
    for i in 0..k {
        body.push_str(&format!("<line class=\"ll\" x1=\"{x}\" y1=\"{}\" x2=\"{x}\" y2=\"{}\"/>\n", f(top + BOX_H), f(life_end), x = f(x(i))));
    }
    for (i, p) in s.participants.iter().enumerate() {
        let w = (tw(&p.label, 12.0) + 28.0).max(80.0).min(lane - 12.0);
        body.push_str(&format!("<rect class=\"nd1\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\"/>\n", f(x(i) - w / 2.0), f(top), f(w), f(BOX_H)));
        text(&mut body, x(i), top + BOX_H / 2.0 + 4.0, "t1 b", 12.0, "middle", &crate::chart::trunc(&p.label, w - 12.0, 12.0));
    }
    for (n, m) in s.messages.iter().enumerate() {
        let y = first_y + n as f64 * ROW;
        let (ed, ar) = if m.reply { ("ed2", "ar2") } else { ("ed", "ar") };
        if m.from == m.to {
            let x0 = x(m.from);
            body.push_str(&format!("<path class=\"{}\" d=\"M{},{} L{},{} L{},{} L{},{}\"/>\n", ed, f(x0), f(y - 8.0), f(x0 + 38.0), f(y - 8.0), f(x0 + 38.0), f(y + 12.0), f(x0 + 9.0), f(y + 12.0)));
            arrow(&mut body, ar, x0, y + 12.0, -1.0, 0.0);
            back(&mut body, x0 + 46.0, y + 8.0, "start", &m.label);
            text(&mut body, x0 + 46.0, y + 8.0, "t2", 12.0, "start", &m.label);
        } else {
            let (xa, xb) = (x(m.from), x(m.to));
            let dir = if xb > xa { 1.0 } else { -1.0 };
            body.push_str(&format!("<path class=\"{}\" d=\"M{},{} L{},{}\"/>\n", ed, f(xa), f(y), f(xb - dir * 9.0), f(y)));
            arrow(&mut body, ar, xb, y, dir, 0.0);
            back(&mut body, (xa + xb) / 2.0, y - 7.0, "middle", &m.label);
            text(&mut body, (xa + xb) / 2.0, y - 7.0, "t2", 12.0, "middle", &m.label);
        }
    }
    let width = PAD * 2.0 + lane * k as f64;
    let names: Vec<String> = s.participants.iter().map(|p| p.label.clone()).collect();
    let mut desc = format!("Sequence diagram between {}. ", names.join(", "));
    let steps: Vec<String> = s.messages.iter().enumerate().take(60).map(|(i, m)| format!("{}. {} to {}: {}", i + 1, s.participants[m.from].label, s.participants[m.to].label, m.label)).collect();
    desc.push_str(&steps.join("; "));
    desc.push('.');
    Scene { w: width, h: life_end + PAD, body, desc }
}
