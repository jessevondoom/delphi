//! Pre-dispatch budget reservation and the halt-on-ceiling rule (`specs/003` §3.2).
//!
//! Money is counted in **micro-USD** (one USD per million tokens is one micro-USD per token, so a
//! price in USD per million tokens multiplies straight into micro-USD). Subscription endpoints use a
//! token ceiling instead (`specs/009` §7.10). The budget is pure state rebuilt from events
//! (`budget.reserve`, `budget.release`, `agent.usage`), so recovery needs nothing else.
//!
//! Before each model call the daemon reserves the call's worst case. If it doesn't fit, the call is
//! refused and the node escalates; parallel calls can't jointly overshoot because every reservation
//! counts against the same remaining headroom until it is settled.

use crate::event::EventEnvelope;
use crate::usage::UsagePayload;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    MicroUsd,
    Tokens,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BudgetError {
    /// The reservation doesn't fit in the remaining headroom.
    Exceeded { requested: u64, available: u64 },
    DuplicateReservation(String),
    UnknownReservation(String),
}

impl std::fmt::Display for BudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BudgetError::Exceeded { requested, available } => write!(f, "budget exceeded: reservation of {} exceeds the {} remaining", requested, available),
            BudgetError::DuplicateReservation(id) => write!(f, "reservation `{}` already exists", id),
            BudgetError::UnknownReservation(id) => write!(f, "no open reservation `{}`", id),
        }
    }
}

impl std::error::Error for BudgetError {}

/// Per-token-class prices in USD per million tokens, for one resolved `(endpoint_id, model_id)`
/// (`specs/002` §4 catalog, with optional per-endpoint overrides).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

fn ceil_micro(tokens: u64, usd_per_mtok: f64) -> u64 {
    (tokens as f64 * usd_per_mtok).ceil() as u64
}

impl Price {
    /// Worst-case cost of one call in micro-USD (`specs/003` §3.2): estimated input
    /// `ceil(bytes / 4) × 1.5` at the input rate (the part covered by a written cache breakpoint at the
    /// cache-write rate), plus `max_tokens` at the output rate. Thinking is billed at the output rate
    /// and is already inside `max_tokens`.
    pub fn worst_case_micro(&self, input_bytes: u64, cache_write_bytes: u64, max_tokens: u64) -> u64 {
        let est = |bytes: u64| ((bytes.div_ceil(4)) as f64 * 1.5).ceil() as u64;
        let cached = est(cache_write_bytes.min(input_bytes));
        let plain = est(input_bytes).saturating_sub(cached);
        ceil_micro(plain, self.input) + ceil_micro(cached, self.cache_write) + ceil_micro(max_tokens, self.output)
    }

    /// Actual cost of a completed call in micro-USD, priced per token class.
    pub fn actual_micro(&self, u: &UsagePayload) -> u64 {
        ceil_micro(u.input_tokens, self.input)
            + ceil_micro(u.cache_read_tokens, self.cache_read)
            + ceil_micro(u.cache_write_tokens, self.cache_write)
            + ceil_micro(u.output_tokens + u.thinking_tokens, self.output)
    }
}

/// Worst-case token count for a subscription reservation: `ceil(bytes / 4) × 1.5` input plus `max_tokens`.
pub fn worst_case_tokens(input_bytes: u64, max_tokens: u64) -> u64 {
    ((input_bytes.div_ceil(4)) as f64 * 1.5).ceil() as u64 + max_tokens
}

pub fn usd_to_micro(usd: f64) -> u64 {
    (usd * 1_000_000.0).round() as u64
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    pub unit: Unit,
    pub cap: u64,
    spent: u64,
    open: BTreeMap<String, u64>,
}

impl Budget {
    pub fn usd(cap_usd: f64) -> Budget {
        Budget { unit: Unit::MicroUsd, cap: usd_to_micro(cap_usd), spent: 0, open: BTreeMap::new() }
    }

    pub fn tokens(cap: u64) -> Budget {
        Budget { unit: Unit::Tokens, cap, spent: 0, open: BTreeMap::new() }
    }

    pub fn cap_micro(&self) -> u64 {
        self.cap
    }

    /// Change the ceiling (`/budget`). Existing spend and reservations are unaffected.
    pub fn set_cap(&mut self, cap: u64) {
        self.cap = cap;
    }

    pub fn spent(&self) -> u64 {
        self.spent
    }

    pub fn reserved(&self) -> u64 {
        self.open.values().sum()
    }

    /// Headroom a new reservation can use.
    pub fn available(&self) -> u64 {
        self.cap.saturating_sub(self.spent + self.reserved())
    }

    /// Open reservations (calls in flight, or interrupted by a crash).
    pub fn open_reservations(&self) -> Vec<(String, u64)> {
        self.open.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }

    pub fn reserve(&mut self, id: &str, amount: u64) -> Result<(), BudgetError> {
        if self.open.contains_key(id) {
            return Err(BudgetError::DuplicateReservation(id.into()));
        }
        let available = self.available();
        if amount > available {
            return Err(BudgetError::Exceeded { requested: amount, available });
        }
        self.open.insert(id.into(), amount);
        Ok(())
    }

    /// Reconcile a reservation to the actual cost when the stream ends.
    pub fn settle(&mut self, id: &str, actual: u64) -> Result<(), BudgetError> {
        self.open.remove(id).ok_or_else(|| BudgetError::UnknownReservation(id.into()))?;
        self.spent += actual;
        Ok(())
    }

    /// Release a reservation for a call that never started.
    pub fn release(&mut self, id: &str) -> Result<(), BudgetError> {
        self.open.remove(id).map(|_| ()).ok_or_else(|| BudgetError::UnknownReservation(id.into()))
    }

    /// Recovery (`specs/005` §2.5): every reservation still open belongs to an interrupted call and is
    /// charged in full. Returns what was charged so the daemon can write `agent.usage` events
    /// (`outcome: cancelled`, `usage_source: estimated`) for them. Reservations for calls that never
    /// started must be released first.
    pub fn charge_interrupted(&mut self) -> Vec<(String, u64)> {
        let charged = self.open_reservations();
        for (_, amount) in &charged {
            self.spent += amount;
        }
        self.open.clear();
        charged
    }

    /// Rebuild from a mission's events. The log is the truth, so ceilings are not re-checked.
    pub fn apply_event(&mut self, e: &EventEnvelope, price_for: &dyn Fn(&EventEnvelope) -> Option<Price>) {
        let p = &e.payload;
        match e.kind.as_str() {
            "budget.reserve" => {
                if let (Some(id), Some(amount)) = (p["reservation_id"].as_str(), p["amount"].as_u64()) {
                    self.open.insert(id.into(), amount);
                }
            }
            "budget.cap" => {
                if let Some(c) = p["cap"].as_u64() {
                    self.cap = c;
                }
            }
            "budget.release" => {
                if let Some(id) = p["reservation_id"].as_str() {
                    self.open.remove(id);
                }
            }
            "agent.usage" => {
                if let Ok(u) = UsagePayload::from_value(p) {
                    let actual = match self.unit {
                        Unit::Tokens => u.total_tokens(),
                        Unit::MicroUsd => match price_for(e) {
                            Some(price) if u.cost_model == "metered" && u.cost_usd == 0.0 => price.actual_micro(&u),
                            _ => usd_to_micro(u.cost_usd),
                        },
                    };
                    match u.reservation_id.as_deref() {
                        Some(id) if self.open.remove(id).is_some() => self.spent += actual,
                        _ => self.spent += actual,
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PRICE: Price = Price { input: 3.0, output: 15.0, cache_read: 0.3, cache_write: 3.75 };

    #[test]
    fn worst_case_follows_the_spec_formula() {
        // 4,000 bytes → 1,000 tokens × 1.5 = 1,500 input; 2,000 max tokens.
        let micro = PRICE.worst_case_micro(4_000, 0, 2_000);
        assert_eq!(micro, 1_500 * 3 + 2_000 * 15);
        // The first 1,000 bytes are written to the cache and cost the cache-write rate instead.
        let with_cache = PRICE.worst_case_micro(4_000, 1_000, 2_000);
        let cached = (250.0_f64 * 1.5).ceil() as u64;
        assert_eq!(with_cache, (1_500 - cached) * 3 + (cached as f64 * 3.75).ceil() as u64 + 2_000 * 15);
        assert!(with_cache > micro);
        assert_eq!(worst_case_tokens(4_000, 2_000), 3_500);
    }

    #[test]
    fn reservations_cannot_jointly_overshoot() {
        let mut b = Budget::usd(1.0); // 1,000,000 micro-USD
        assert!(b.reserve("a", 600_000).is_ok());
        assert_eq!(b.reserve("b", 600_000), Err(BudgetError::Exceeded { requested: 600_000, available: 400_000 }));
        assert!(b.reserve("c", 400_000).is_ok());
        assert_eq!(b.available(), 0);
        // Settling below the reservation frees headroom for the next call.
        b.settle("a", 100_000).unwrap();
        assert_eq!(b.available(), 500_000);
        assert!(b.reserve("d", 500_000).is_ok());
        assert_eq!(b.reserve("d", 1), Err(BudgetError::DuplicateReservation("d".into())));
        assert_eq!(b.release("zzz"), Err(BudgetError::UnknownReservation("zzz".into())));
    }

    #[test]
    fn released_reservations_free_their_headroom() {
        let mut b = Budget::tokens(1_000);
        b.reserve("a", 800).unwrap();
        assert!(b.reserve("b", 300).is_err());
        b.release("a").unwrap();
        assert!(b.reserve("b", 300).is_ok());
    }

    // specs/003 §3.2 + specs/005 §2.5: an interrupted call is charged its full reservation.
    #[test]
    fn rebuild_from_events_and_charge_interrupted_calls() {
        let mk = |kind: &str, payload: serde_json::Value| EventEnvelope::new("m1", kind, payload);
        let usage = |id: &str, cost: f64| {
            let mut e = mk("agent.usage", json!({"provider": "anthropic", "input_tokens": 10, "output_tokens": 5, "cost_usd": cost, "reserved_usd": 0.5,
                "usage_source": "reported", "cost_model": "metered", "outcome": "ok", "reservation_id": id}));
            e.agent = Some("builder".into());
            e.span_id = Some("s".into());
            e.task_id = Some("t".into());
            e.endpoint_id = Some("direct".into());
            e.access_path = Some("api".into());
            e.model_tier = Some("fast".into());
            e.model_id = Some("m".into());
            e.attempt = Some(1);
            e
        };
        let events = vec![
            mk("budget.reserve", json!({"reservation_id": "r1", "amount": 500_000})),
            usage("r1", 0.2),
            mk("budget.reserve", json!({"reservation_id": "r2", "amount": 700_000})),
            mk("budget.reserve", json!({"reservation_id": "r3", "amount": 100_000})),
            mk("budget.release", json!({"reservation_id": "r3"})),
        ];
        let mut b = Budget::usd(2.0);
        for e in &events {
            b.apply_event(e, &|_| None);
        }
        assert_eq!(b.spent(), 200_000);
        assert_eq!(b.open_reservations(), vec![("r2".to_string(), 700_000)]);
        assert_eq!(b.available(), 2_000_000 - 200_000 - 700_000);
        // r2's call was interrupted by a crash: charged in full.
        assert_eq!(b.charge_interrupted(), vec![("r2".to_string(), 700_000)]);
        assert_eq!(b.spent(), 900_000);
        assert_eq!(b.reserved(), 0);
    }

    #[test]
    fn the_operator_can_change_the_cap_and_a_replay_restores_the_last_one() {
        let mut b = Budget::usd(1.0);
        b.reserve("a", 900_000).unwrap();
        let ev = |cap: u64| EventEnvelope::new("m1", "budget.cap", json!({"cap": cap}));
        let mut replayed = Budget::usd(1.0);
        replayed.apply_event(&EventEnvelope::new("m1", "budget.reserve", json!({"reservation_id": "a", "amount": 900_000})), &|_| None);
        for e in [ev(3_000_000), ev(2_000_000)] {
            replayed.apply_event(&e, &|_| None);
        }
        assert_eq!(replayed.cap_micro(), 2_000_000);
        assert_eq!(replayed.available(), 1_100_000, "reservations carry over a cap change");
        b.set_cap(500_000);
        assert_eq!(b.available(), 0, "lowering below what is committed leaves nothing to reserve, never a negative");
    }

    #[test]
    fn subscription_budget_counts_tokens() {
        let mut b = Budget::tokens(10_000);
        let mut e = EventEnvelope::new("m1", "agent.usage", json!({"provider": "anthropic", "input_tokens": 1000, "cache_read_tokens": 200, "output_tokens": 300, "thinking_tokens": 50,
            "cost_usd": 0.0, "reserved_usd": 0.0, "usage_source": "estimated", "cost_model": "subscription", "outcome": "ok"}));
        e.agent = Some("builder".into());
        b.apply_event(&e, &|_| None);
        assert_eq!(b.spent(), 1_550);
    }

    #[test]
    fn actual_cost_is_priced_per_token_class() {
        let u = UsagePayload { provider: "anthropic".into(), input_tokens: 1_000, cache_read_tokens: 10_000, cache_write_tokens: 2_000, output_tokens: 500, thinking_tokens: 500,
            cost_usd: 0.0, reserved_usd: 0.0, usage_source: "reported".into(), cost_model: "metered".into(), outcome: "ok".into(), reservation_id: None };
        assert_eq!(PRICE.actual_micro(&u), 1_000 * 3 + 3_000 + (2_000.0_f64 * 3.75) as u64 + 1_000 * 15);
    }
}
