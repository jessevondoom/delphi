//! Where a span's context goes, read from the mission log (`specs/045` T9, `specs/049` CX-7). Pure: events in, a report out.
//! It reads counts, sizes, event kinds and hashes only; never a prompt, a result or any text.

use delphi_core::event::EventEnvelope;
use serde::Serialize;
use std::collections::BTreeMap;

/// A turn where the cached prefix of the request changed, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PrefixChange {
    pub turn: u64,
    /// `results_cleared`, `args_trimmed`, both, or `compaction`.
    pub cause: String,
}

/// One span's account.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SpanContext {
    pub span_id: String,
    pub role: String,
    /// Model calls made.
    pub calls: u32,
    /// Input-side tokens of the first call, counted apart: a span's first call is cold by nature, so it does not drag the hit rate down.
    pub cold_tokens: u64,
    /// Tokens of the later calls: fresh input, cache reads and cache writes.
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub prefix_changes: Vec<PrefixChange>,
    /// Results kept in the payload store at the time they were produced, and their size.
    pub offloaded: u32,
    pub offloaded_bytes: u64,
    /// The latest totals of the request's copy of the history.
    pub trimmed_args: u32,
    pub trimmed_bytes: u64,
    pub cleared_results: u32,
    pub cleared_bytes: u64,
}

impl SpanContext {
    /// Cache reads as a share of the input side of the calls after the first, 0 to 1; `None` before there is one.
    pub fn hit_rate(&self) -> Option<f64> {
        let all = self.input + self.cache_read + self.cache_write;
        (all > 0).then(|| self.cache_read as f64 / all as f64)
    }

    /// Handles the span's history points at: stored results, and trimmed or cleared items it could read back.
    pub fn handles(&self) -> u32 {
        self.offloaded + self.trimmed_args + self.cleared_results
    }
}

/// Every span's account, in the order the spans first appear.
pub fn report(events: &[EventEnvelope]) -> Vec<SpanContext> {
    let mut order: Vec<String> = Vec::new();
    let mut spans: BTreeMap<String, SpanContext> = BTreeMap::new();
    for e in events {
        let Some(id) = e.span_id.clone() else { continue };
        if !e.kind.starts_with("loop.") && !(e.kind == "agent.usage" && e.payload["outcome"].as_str().is_none_or(|o| o == "ok")) {
            continue;
        }
        let s = spans.entry(id.clone()).or_insert_with(|| {
            order.push(id.clone());
            SpanContext { span_id: id.clone(), role: e.agent.clone().unwrap_or_default(), ..Default::default() }
        });
        let n = |k: &str| e.payload[k].as_u64().unwrap_or(0);
        match e.kind.as_str() {
            "agent.usage" => {
                let side = n("input_tokens") + n("cache_read_tokens") + n("cache_write_tokens");
                s.calls += 1;
                if s.calls == 1 {
                    s.cold_tokens = side;
                } else {
                    s.input += n("input_tokens");
                    s.cache_read += n("cache_read_tokens");
                    s.cache_write += n("cache_write_tokens");
                }
            }
            "loop.offload" => {
                s.offloaded += 1;
                s.offloaded_bytes += n("bytes");
            }
            "loop.prefix" => {
                s.trimmed_args = n("trimmed_args") as u32;
                s.trimmed_bytes = n("trimmed_bytes");
                s.cleared_results = n("cleared_results") as u32;
                s.cleared_bytes = n("cleared_bytes");
                s.prefix_changes.push(PrefixChange { turn: n("turn_index"), cause: e.payload["cause"].as_str().unwrap_or("").to_string() });
            }
            _ => {}
        }
    }
    order.into_iter().filter_map(|id| spans.remove(&id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn ev(seq: u64, span: &str, kind: &str, payload: Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m", kind, payload);
        e.seq = seq;
        e.span_id = Some(span.into());
        e.agent = Some("researcher".into());
        e
    }

    fn usage(seq: u64, span: &str, input: u64, read: u64, write: u64) -> EventEnvelope {
        ev(seq, span, "agent.usage", json!({"input_tokens": input, "cache_read_tokens": read, "cache_write_tokens": write, "outcome": "ok"}))
    }

    #[test]
    fn the_first_call_is_counted_apart_and_the_hit_rate_is_of_the_rest() {
        let r = report(&[usage(1, "s1", 1000, 0, 4000), usage(2, "s1", 100, 3000, 100), usage(3, "s1", 100, 3000, 100), usage(4, "s2", 50, 0, 0)]);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].calls, r[0].cold_tokens, r[0].cache_read, r[0].cache_write), (3, 5000, 6000, 200));
        assert!((r[0].hit_rate().unwrap() - 6000.0 / 6400.0).abs() < 1e-9);
        assert_eq!((r[1].calls, r[1].hit_rate()), (1, None), "a span's only call is cold, not a miss");
    }

    #[test]
    fn a_prefix_change_names_its_turn_and_cause_and_handles_are_counted() {
        let r = report(&[
            ev(1, "s1", "loop.offload", json!({"bytes": 40000, "blake3": "a"})),
            ev(2, "s1", "loop.offload", json!({"bytes": 20000, "blake3": "b"})),
            ev(3, "s1", "loop.prefix", json!({"turn_index": 9, "cause": "results_cleared", "trimmed_args": 0, "trimmed_bytes": 0, "cleared_results": 5, "cleared_bytes": 90000})),
            ev(4, "s1", "loop.prefix", json!({"turn_index": 15, "cause": "args_trimmed", "trimmed_args": 2, "trimmed_bytes": 8000, "cleared_results": 5, "cleared_bytes": 90000})),
        ]);
        assert_eq!(r[0].prefix_changes, vec![PrefixChange { turn: 9, cause: "results_cleared".into() }, PrefixChange { turn: 15, cause: "args_trimmed".into() }]);
        assert_eq!((r[0].offloaded, r[0].offloaded_bytes, r[0].cleared_results, r[0].trimmed_args, r[0].handles()), (2, 60000, 5, 2, 9));
    }

    #[test]
    fn a_failed_call_is_not_a_call_and_other_events_are_ignored() {
        let r = report(&[ev(1, "s1", "agent.usage", json!({"input_tokens": 5, "outcome": "cancelled"})), ev(2, "s1", "tool.result", json!({}))]);
        assert!(r.is_empty());
    }
}
