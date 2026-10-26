//! Capability-based route resolution (`specs/002` §4 and §4.1, D-26): which `(endpoint, model)` should
//! serve a request, given what it needs and which pairs are usable now.
//!
//! A pure function. It takes the config, a snapshot of the pool state and `now`, touches nothing, and
//! returns a [`Route`]. Nothing calls it yet: the engine still picks endpoints the old way, and M4
//! switches `model_turn` over.
//!
//! **Hard requirements** (an ineligible pair is never chosen, whatever else is true): the endpoint is
//! allowed (the pool only holds allowed ones), it can run the role if the role mutates, the model fits the
//! context the request needs (an unknown context passes), and a call can be paid for (a price in the
//! catalog, or a subscription that is metered in tokens).
//!
//! **Preferences** (they order the candidates and degrade, never block): the role's target model, its
//! tier, the family it is usually served by, and not sharing a family with the author (`[H-007]`,
//! `[H-008]`).
//!
//! The outcome follows the ratified rule for targets: use the target if it is usable; otherwise a
//! *similar* model (at least the target's tier, context not shorter) without asking; otherwise a *weaker*
//! one, which the caller must put to the operator; otherwise nothing is usable.

use crate::pool::{Entry, State};
use delphi_adapter::config::{Config, PoolKey};

/// What a dispatch asks for.
#[derive(Debug, Clone, Default)]
pub struct RouteRequest {
    pub role: String,
    /// `fast`, `workhorse` or `frontier`.
    pub tier: String,
    /// The family this role is usually served by (`role_family`): tried first, never required.
    pub prefer_family: Option<String>,
    /// Families to avoid if there is a choice (the author's, for a critic). Using one anyway is reported as `homogeneous`.
    pub avoid_families: Vec<String>,
    /// Writes files or runs commands, so it needs an endpoint with an approval bridge.
    pub mutating: bool,
    /// Harness endpoints that can serve a `mutating` request although `Endpoint::can_mutate` says no: an `agy` seat whose
    /// gate has been seen to hold, for the installed version (`specs/020` §8.7). Decided by the engine, from its record.
    pub mutating_harnesses: Vec<String>,
    /// Context the request needs, in tokens; 0 when it does not matter.
    pub min_context_tokens: u64,
    /// The role's target model (`[routing.targets]`).
    pub target: Option<String>,
    /// The caller can only drive one model turn at a time, so a harness (which runs a whole span) is not a candidate.
    pub turn_by_turn: bool,
}

/// An eligible pair that cannot be used right now, and when it may come back.
#[derive(Debug, Clone, PartialEq)]
pub struct Blocked {
    pub key: PoolKey,
    pub why: String,
    pub back_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    /// The target (or, with none set, the preferred pair) is usable.
    Target { key: PoolKey, homogeneous: bool },
    /// The target is not usable; this one is at least as capable. Use it and say so.
    Similar { key: PoolKey, target: PoolKey, why: String, homogeneous: bool },
    /// Only less capable pairs are usable. The caller asks the operator before using it.
    Weaker { key: PoolKey, target: PoolKey, why: String, back_at: Option<i64>, lost: Vec<String>, homogeneous: bool },
    /// Nothing eligible is usable now. `back_at` is the soonest any blocked pair returns, if known.
    NoModel { target: Option<PoolKey>, blocked: Vec<Blocked>, ineligible: Vec<(PoolKey, String)>, back_at: Option<i64> },
}

impl Route {
    /// The pair to call, if there is one.
    pub fn key(&self) -> Option<&PoolKey> {
        match self {
            Route::Target { key, .. } | Route::Similar { key, .. } | Route::Weaker { key, .. } => Some(key),
            Route::NoModel { .. } => None,
        }
    }

    /// True when the operator must confirm before the pair is used.
    pub fn needs_confirmation(&self) -> bool {
        matches!(self, Route::Weaker { .. })
    }

    /// For an escalation message: who is blocked and when, and what could never serve this request.
    pub fn explain(&self, now: i64) -> String {
        let Route::NoModel { blocked, ineligible, back_at, .. } = self else { return String::new() };
        let mut parts: Vec<String> = blocked.iter().map(|b| format!("{}/{} {}{}", b.key.endpoint_id, b.key.model_id, b.why, b.back_at.map(|t| format!(", back in {}", crate::pool::fmt_secs(t - now))).unwrap_or_default())).collect();
        parts.extend(ineligible.iter().map(|(k, why)| format!("{}/{} cannot serve this: {}", k.endpoint_id, k.model_id, why)));
        let soonest = back_at.map(|t| format!("; the soonest returns in {}", crate::pool::fmt_secs(t - now))).unwrap_or_default();
        if parts.is_empty() {
            return "no configured model can serve this request (check [[endpoints]], [access] and the role's family with `delphi models`)".to_string();
        }
        format!("{}{}", parts.join("; "), soonest)
    }

    pub fn homogeneous(&self) -> bool {
        matches!(self, Route::Target { homogeneous: true, .. } | Route::Similar { homogeneous: true, .. } | Route::Weaker { homogeneous: true, .. })
    }
}

fn rank(tier: &str) -> i32 {
    match tier {
        "fast" => 0,
        "workhorse" => 1,
        "frontier" => 2,
        _ => 1,
    }
}

fn tier_name(rank: i32) -> &'static str {
    match rank {
        0 => "fast",
        2 => "frontier",
        _ => "workhorse",
    }
}

/// The tier a pair counts as when judged against `want`. An endpoint can map one model to several tiers
/// (a subscription with a single model does), and the pool lists that pair once under its highest tier;
/// it still counts as `want` when it serves it.
fn rank_at(cfg: &Config, key: &PoolKey, want: i32) -> i32 {
    let serves = cfg.endpoint(&key.endpoint_id).and_then(|ep| cfg.resolve_model(ep, tier_name(want))).as_deref() == Some(key.model_id.as_str());
    if serves { want } else { rank(&key.tier) }
}

enum Avail {
    Yes,
    No { why: String, back_at: Option<i64> },
}

fn availability(entry: Option<&Entry>, now: i64) -> Avail {
    let Some(e) = entry else { return Avail::Yes };
    match (e.state, e.until) {
        (State::Available, _) => Avail::Yes,
        (State::CoolingDown, Some(t)) if t > now => Avail::No { why: format!("rate limited ({})", e.reason), back_at: Some(t) },
        (State::CoolingDown, _) => Avail::Yes,
        (State::Exhausted, Some(t)) if t > now => Avail::No { why: format!("exhausted: {}", e.reason), back_at: Some(t) },
        (State::Exhausted, Some(_)) => Avail::Yes, // the reset time has passed
        (State::Exhausted, None) => Avail::No { why: format!("exhausted: {}", e.reason), back_at: e.next_probe_at },
    }
}

fn context_of(cfg: &Config, key: &PoolKey) -> u64 {
    cfg.catalog_entry(&key.model_id).map(|c| c.context_tokens).unwrap_or(0)
}

/// Why a pair can never serve this request, or `None` if it is eligible.
fn ineligible(cfg: &Config, key: &PoolKey, req: &RouteRequest) -> Option<String> {
    let ep = cfg.endpoint(&key.endpoint_id)?;
    if !cfg.is_allowed(&ep.id) {
        return Some("the endpoint is not in allowed_endpoints".into());
    }
    if req.turn_by_turn && ep.path == delphi_adapter::config::AccessPath::Harness {
        return Some("a harness runs whole spans, not single turns".into());
    }
    if req.mutating && !ep.can_mutate() && !req.mutating_harnesses.contains(&ep.id) {
        return Some("this harness has no approval bridge, so it can only serve read-only roles".into());
    }
    let ctx = context_of(cfg, key);
    if req.min_context_tokens > 0 && ctx > 0 && ctx < req.min_context_tokens {
        return Some(format!("its context window ({} tokens) is smaller than the {} needed", ctx, req.min_context_tokens));
    }
    if !ep.is_metered_by_subscription() && !ep.is_local() && !cfg.catalog_entry(&key.model_id).is_some_and(|c| c.has_prices()) {
        return Some("it has no prices in the catalog, so a call cannot be reserved against the budget".into());
    }
    None
}

/// Resolve a request. `state` is the pool snapshot ([`crate::pool::Pool::snapshot`]).
pub fn resolve_route(cfg: &Config, state: &[Entry], now: i64, req: &RouteRequest) -> Route {
    let find = |k: &PoolKey| state.iter().find(|e| e.endpoint_id == k.endpoint_id && e.model_id == k.model_id);
    let avoided = |k: &PoolKey| req.avoid_families.iter().any(|f| f == &k.family);

    // Every pool pair, split into eligible and not.
    let mut eligible: Vec<(usize, PoolKey)> = Vec::new();
    let mut rejected: Vec<(PoolKey, String)> = Vec::new();
    for (i, k) in cfg.pool_entries().into_iter().enumerate() {
        match ineligible(cfg, &k, req) {
            Some(why) => rejected.push((k, why)),
            None => eligible.push((i, k)),
        }
    }

    // Candidate order: pairs outside the avoided families first, then the preferred family, then the
    // operator's `[routing.pool]` order if there is one. Without it, the default pool is tier-major, which
    // says nothing about which endpoint to try first for one tier, so the ratified endpoint order within
    // the family decides (the order the engine walks today), then pool order.
    let explicit = cfg.routing.pool.is_some();
    let policy_index = |k: &PoolKey| if explicit { 0 } else { cfg.endpoints_for(&k.family).iter().position(|e| e.id == k.endpoint_id).unwrap_or(usize::MAX) };
    let order = |k: &PoolKey, i: usize| (avoided(k) as u8, (req.prefer_family.as_deref() != Some(k.family.as_str())) as u8, policy_index(k), i);
    let mut by_preference = eligible.clone();
    by_preference.sort_by_key(|(i, k)| order(k, *i));

    // The ideal pair: the target model if one is set, otherwise the preferred pair at the wanted tier,
    // or the nearest tier when that tier is not configured.
    // A target is a preference. If nothing serves it, or every pair that does is ineligible for this request (a
    // read-only harness model as a Builder's target, an unpriced model), the role falls back to the ordinary choice
    // instead of being left with no model while other usable ones exist.
    let target = req.target.as_deref().filter(|t| !t.is_empty());
    let from_target = target.and_then(|t| by_preference.iter().map(|(_, k)| k).find(|k| k.model_id == t).cloned());
    let ideal: Option<PoolKey> = from_target.clone().or_else(|| {
        // The first pair, in preference order, at the nearest tier (`min_by_key` keeps the first minimum).
        let want = rank(&req.tier);
        by_preference.iter().map(|(_, k)| k).min_by_key(|k| ((rank_at(cfg, k, want) - want).abs(), (rank_at(cfg, k, want) < want) as i32)).cloned()
    });
    let Some(ideal) = ideal else {
        return Route::NoModel { target: None, blocked: vec![], ineligible: rejected, back_at: None };
    };
    let homogeneous = |k: &PoolKey| !req.avoid_families.is_empty() && avoided(k);

    if matches!(availability(find(&ideal), now), Avail::Yes) {
        return Route::Target { homogeneous: homogeneous(&ideal), key: ideal };
    }

    // The ideal is not usable. Classify the usable pairs against it.
    let ideal_rank = if from_target.is_some() { rank(&ideal.tier) } else { rank_at(cfg, &ideal, rank(&req.tier)) };
    let ideal_ctx = context_of(cfg, &ideal);
    let mut blocked: Vec<Blocked> = Vec::new();
    let mut usable: Vec<PoolKey> = Vec::new();
    for (_, k) in &by_preference {
        if k == &ideal {
            continue;
        }
        match availability(find(k), now) {
            Avail::Yes => usable.push(k.clone()),
            Avail::No { why, back_at } => blocked.push(Blocked { key: k.clone(), why, back_at }),
        }
    }
    let mut ideal_why = String::new();
    let mut ideal_back_at = None;
    if let Avail::No { why, back_at } = availability(find(&ideal), now) {
        ideal_why = why.clone();
        ideal_back_at = back_at;
        blocked.insert(0, Blocked { key: ideal.clone(), why, back_at });
    }
    let similar = |k: &PoolKey| rank_at(cfg, k, ideal_rank) >= ideal_rank && !(ideal_ctx > 0 && context_of(cfg, k) > 0 && context_of(cfg, k) < ideal_ctx);
    // Among similar pairs prefer the closest tier (equal before higher), then the usual order.
    let pick_similar = usable.iter().enumerate().filter(|(_, k)| similar(k)).min_by_key(|(i, k)| (rank_at(cfg, k, ideal_rank) - ideal_rank, *i)).map(|(_, k)| k.clone());
    if let Some(key) = pick_similar {
        return Route::Similar { homogeneous: homogeneous(&key), key, why: ideal_why, target: ideal };
    }
    // Weaker: the most capable of what is left, then the usual order.
    let pick_weaker = usable.iter().enumerate().min_by_key(|(i, k)| (-rank_at(cfg, k, ideal_rank), *i)).map(|(_, k)| k.clone());
    if let Some(key) = pick_weaker {
        let mut lost = Vec::new();
        if rank_at(cfg, &key, ideal_rank) < ideal_rank {
            lost.push(format!("tier {} instead of {}", tier_name(rank_at(cfg, &key, ideal_rank)), tier_name(ideal_rank)));
        }
        let ctx = context_of(cfg, &key);
        if ideal_ctx > 0 && ctx > 0 && ctx < ideal_ctx {
            lost.push(format!("a {}-token context instead of {}", ctx, ideal_ctx));
        }
        return Route::Weaker { homogeneous: homogeneous(&key), key, why: ideal_why, back_at: ideal_back_at, target: ideal, lost };
    }
    let back_at = blocked.iter().filter_map(|b| b.back_at).min();
    Route::NoModel { target: Some(ideal), blocked, ineligible: rejected, back_at }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::{Entry, State};

    const NOW: i64 = 1_790_000_000;

    /// Claude on a direct API and on Bedrock (a different workhorse model), Gemini on the API, and a
    /// read-only `agy` subscription. Every API model is priced; the harness is metered by subscription.
    /// An endpoint serves its family's models unless its own `models` overrides a tier, so Bedrock also
    /// serves `opus-x` and `haiku-x`.
    const CONFIG: &str = r#"
[models.claude]
fast = "haiku-x"
workhorse = "sonnet-x"
frontier = "opus-x"
[models.gemini]
workhorse = "gem-flash"
frontier = "gem-pro"

[models.catalog."haiku-x"]
max_output_tokens = 8000
context_tokens = 200000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 5.0
[models.catalog."sonnet-x"]
max_output_tokens = 64000
context_tokens = 200000
input_usd_per_mtok = 3.0
output_usd_per_mtok = 15.0
[models.catalog."bedrock-sonnet-x"]
max_output_tokens = 64000
context_tokens = 200000
input_usd_per_mtok = 3.0
output_usd_per_mtok = 15.0
[models.catalog."opus-x"]
max_output_tokens = 64000
context_tokens = 200000
input_usd_per_mtok = 15.0
output_usd_per_mtok = 75.0
[models.catalog."gem-flash"]
max_output_tokens = 64000
input_usd_per_mtok = 0.3
output_usd_per_mtok = 2.0
[models.catalog."gem-pro"]
max_output_tokens = 64000
context_tokens = 1000000
input_usd_per_mtok = 2.0
output_usd_per_mtok = 12.0

[[endpoints]]
id = "direct"
family = "claude"
path = "api"
credential = "env:K"

[[endpoints]]
id = "bedrock"
family = "claude"
path = "cloud"
platform = "bedrock"
region = "us-east-1"
credential = "aws:sso"
models = { workhorse = "bedrock-sonnet-x" }

[[endpoints]]
id = "gem"
family = "gemini"
path = "api"
credential = "env:G"

[[endpoints]]
id = "agy-sub"
family = "gemini"
path = "harness"
harness = "agy"
credential = "harness-login"
cost_model = "subscription"
models = { fast = "gem-agy", workhorse = "gem-agy", frontier = "gem-agy" }

[access]
order = { claude = ["direct", "bedrock"] }
"#;

    fn cfg() -> Config {
        Config::parse(CONFIG).unwrap()
    }

    fn req(role: &str, tier: &str) -> RouteRequest {
        RouteRequest { role: role.into(), tier: tier.into(), prefer_family: Some("claude".into()), ..Default::default() }
    }

    fn entry(endpoint_id: &str, model_id: &str, state: State, until: Option<i64>) -> Entry {
        Entry { endpoint_id: endpoint_id.into(), model_id: model_id.into(), state, since: NOW - 10, until, reason: "quota reached".into(), throttles: vec![], probe_interval_secs: 900, next_probe_at: if state == State::Exhausted && until.is_none() { Some(NOW + 600) } else { None } }
    }

    /// Every pool pair serving one of `models` is out of quota for another hour.
    fn down(c: &Config, models: &[&str]) -> Vec<Entry> {
        c.pool_entries().iter().filter(|k| models.contains(&k.model_id.as_str())).map(|k| entry(&k.endpoint_id, &k.model_id, State::Exhausted, Some(NOW + 3600))).collect()
    }

    /// Every pool pair except those serving `model` is out of quota for another hour.
    fn only(c: &Config, model: &str) -> Vec<Entry> {
        c.pool_entries().iter().filter(|k| k.model_id != model).map(|k| entry(&k.endpoint_id, &k.model_id, State::Exhausted, Some(NOW + 3600))).collect()
    }

    fn picked(r: &Route) -> String {
        r.key().map(|k| format!("{}/{}", k.endpoint_id, k.model_id)).unwrap_or_else(|| "none".into())
    }

    #[test]
    fn with_nothing_wrong_the_preferred_family_serves_the_wanted_tier_in_policy_order() {
        let c = cfg();
        for (tier, want) in [("workhorse", "direct/sonnet-x"), ("frontier", "direct/opus-x"), ("fast", "direct/haiku-x")] {
            let r = resolve_route(&c, &[], NOW, &req("builder", tier));
            assert!(matches!(r, Route::Target { homogeneous: false, .. }), "{:?}", r);
            assert_eq!(picked(&r), want, "tier {}", tier);
        }
        // A preference for Gemini moves it first without changing the tier.
        let r = resolve_route(&c, &[], NOW, &RouteRequest { prefer_family: Some("gemini".into()), ..req("architect", "frontier") });
        assert_eq!(picked(&r), "gem/gem-pro");
    }

    /// An unchanged config must route as it does today: the first allowed endpoint of the role's family, in
    /// policy order, that has a model for the tier.
    #[test]
    fn it_agrees_with_the_current_endpoint_walk_for_every_family_and_tier() {
        let c = cfg();
        for family in ["claude", "gemini"] {
            for tier in ["fast", "workhorse", "frontier"] {
                let old = c.endpoints_for(family).into_iter().find_map(|ep| c.resolve_model(ep, tier).map(|m| format!("{}/{}", ep.id, m)));
                let Some(old) = old else { continue };
                let r = resolve_route(&c, &[], NOW, &RouteRequest { prefer_family: Some(family.into()), ..req("r", tier) });
                assert_eq!(picked(&r), old, "{} {}", family, tier);
            }
        }
    }

    #[test]
    fn an_exhausted_target_is_replaced_by_the_same_tier_on_another_endpoint_without_asking() {
        let c = cfg();
        let r = resolve_route(&c, &[entry("direct", "sonnet-x", State::Exhausted, Some(NOW + 3600))], NOW, &req("builder", "workhorse"));
        match &r {
            Route::Similar { key, target, .. } => assert_eq!((key.endpoint_id.as_str(), key.model_id.as_str(), target.endpoint_id.as_str()), ("bedrock", "bedrock-sonnet-x", "direct")),
            other => panic!("{:?}", other),
        }
        assert!(!r.needs_confirmation());
    }

    #[test]
    fn the_same_tier_on_another_family_beats_a_higher_tier_on_the_preferred_one() {
        let c = cfg();
        let state = down(&c, &["sonnet-x", "bedrock-sonnet-x"]);
        let r = resolve_route(&c, &state, NOW, &req("builder", "workhorse"));
        assert!(matches!(r, Route::Similar { .. }), "{:?}", r);
        assert_eq!(picked(&r), "gem/gem-flash", "equal tier first: the pricier frontier model is not used when an equal one is up");
    }

    #[test]
    fn with_no_same_tier_left_a_higher_tier_is_similar_and_the_preferred_family_comes_first() {
        let c = cfg();
        let state = down(&c, &["sonnet-x", "bedrock-sonnet-x", "gem-flash", "gem-agy"]);
        let r = resolve_route(&c, &state, NOW, &req("builder", "workhorse"));
        assert!(matches!(r, Route::Similar { .. }), "{:?}", r);
        assert_eq!(picked(&r), "direct/opus-x");
    }

    #[test]
    fn only_less_capable_pairs_left_is_weaker_and_needs_the_operators_confirmation() {
        let c = cfg();
        let state = down(&c, &["opus-x", "gem-pro", "gem-agy"]);
        let r = resolve_route(&c, &state, NOW, &req("architect", "frontier"));
        match &r {
            Route::Weaker { key, target, lost, .. } => {
                assert_eq!((key.model_id.as_str(), target.model_id.as_str()), ("sonnet-x", "opus-x"), "the most capable of what is left");
                assert_eq!(lost, &vec!["tier workhorse instead of frontier".to_string()]);
            }
            other => panic!("{:?}", other),
        }
        assert!(r.needs_confirmation());
    }

    #[test]
    fn a_shorter_context_alone_makes_a_substitute_weaker() {
        let text = CONFIG.replace("[models.catalog.\"bedrock-sonnet-x\"]\nmax_output_tokens = 64000\ncontext_tokens = 200000", "[models.catalog.\"bedrock-sonnet-x\"]\nmax_output_tokens = 64000\ncontext_tokens = 128000");
        let c = Config::parse(&text).unwrap();
        let r = resolve_route(&c, &only(&c, "bedrock-sonnet-x"), NOW, &req("builder", "workhorse"));
        match &r {
            Route::Weaker { key, lost, .. } => {
                assert_eq!(key.model_id, "bedrock-sonnet-x");
                assert_eq!(lost, &vec!["a 128000-token context instead of 200000".to_string()]);
            }
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn nothing_usable_reports_who_is_blocked_and_when_the_soonest_returns() {
        let c = cfg();
        let mut state: Vec<Entry> = c.pool_entries().iter().filter(|k| k.endpoint_id != "agy-sub").map(|k| entry(&k.endpoint_id, &k.model_id, State::Exhausted, Some(NOW + 7200))).collect();
        state[2].until = Some(NOW + 600);
        let r = resolve_route(&c, &state, NOW, &RouteRequest { mutating: true, ..req("builder", "workhorse") });
        match r {
            Route::NoModel { target, blocked, ineligible, back_at } => {
                assert!(target.is_some() && blocked.len() >= 4 && back_at == Some(NOW + 600), "{:?} {:?}", blocked.len(), back_at);
                assert!(ineligible.iter().any(|(k, why)| k.endpoint_id == "agy-sub" && why.contains("read-only")), "{:?}", ineligible);
            }
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn a_read_only_harness_never_serves_a_role_that_writes_but_can_serve_one_that_reads() {
        let c = cfg();
        let state = only(&c, "gem-agy");
        let writer = resolve_route(&c, &state, NOW, &RouteRequest { mutating: true, ..req("builder", "frontier") });
        assert_eq!(picked(&writer), "none", "{:?}", writer);
        let reader = resolve_route(&c, &state, NOW, &req("red-team", "frontier"));
        assert_eq!(picked(&reader), "agy-sub/gem-agy");
    }

    #[test]
    fn a_context_requirement_excludes_small_windows_but_never_unknown_ones() {
        let c = cfg();
        let gemini = |min| RouteRequest { min_context_tokens: min, prefer_family: Some("gemini".into()), ..req("scout", "workhorse") };
        assert_eq!(picked(&resolve_route(&c, &[], NOW, &gemini(150_000))), "gem/gem-flash", "gem-flash has no stated context, so it is not ruled out");
        // Above 500K only gem-pro states a window that fits, but the unknown ones (gem-flash, gem-agy) are still allowed.
        assert_eq!(picked(&resolve_route(&c, &down(&c, &["gem-flash"]), NOW, &RouteRequest { min_context_tokens: 500_000, ..req("scout", "frontier") })), "gem/gem-pro");
        // Once the harness states a small window too, the 200K Claude models and it are all ineligible, not merely weaker.
        let text = format!("{}\n[models.catalog.\"gem-agy\"]\nmax_output_tokens = 64000\ncontext_tokens = 100000\n", CONFIG);
        let small = Config::parse(&text).unwrap();
        match resolve_route(&small, &down(&small, &["gem-pro", "gem-flash"]), NOW, &RouteRequest { min_context_tokens: 500_000, ..req("scout", "workhorse") }) {
            Route::NoModel { ineligible, .. } => {
                assert!(ineligible.iter().any(|(k, why)| k.model_id == "sonnet-x" && why.contains("smaller than")), "{:?}", ineligible);
                assert!(ineligible.iter().any(|(k, why)| k.model_id == "gem-agy" && why.contains("100000")), "{:?}", ineligible);
            }
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn a_priced_catalog_entry_is_required_except_for_a_subscription() {
        let text = CONFIG.replace("input_usd_per_mtok = 2.0\noutput_usd_per_mtok = 12.0", "");
        let c = Config::parse(&text).unwrap();
        let r = resolve_route(&c, &[], NOW, &RouteRequest { prefer_family: Some("gemini".into()), ..req("red-team", "frontier") });
        assert_eq!(picked(&r), "agy-sub/gem-agy", "gem-pro has no price, so it is never chosen; the subscription needs none");
        match resolve_route(&c, &down(&c, &["gem-agy", "opus-x"]), NOW, &RouteRequest { prefer_family: Some("gemini".into()), ..req("red-team", "frontier") }) {
            Route::NoModel { ineligible, .. } => assert!(ineligible.iter().any(|(k, why)| k.model_id == "gem-pro" && why.contains("no prices")), "{:?}", ineligible),
            Route::Weaker { key, .. } | Route::Similar { key, .. } | Route::Target { key, .. } => assert_ne!(key.model_id, "gem-pro"),
        }
    }

    // A live run caught this: `delphi setup` writes a `cost_model = "local"` endpoint with no prices, the config check accepts it
    // ("a free local model needs no prices"), and the router refused it, so the Ollama fallback could never serve a call.
    #[test]
    fn a_free_local_model_is_chosen_without_prices_but_a_priceless_remote_one_is_not() {
        let local = "[[endpoints]]\nid=\"ollama\"\nfamily=\"local\"\npath=\"gateway-compat\"\nbase_url=\"http://localhost:11434\"\ncredential=\"none\"\ncost_model=\"local\"\n[endpoints.models]\nworkhorse=\"qwen3:8b\"\n[models.catalog.\"qwen3:8b\"]\nmax_output_tokens = 8192\n[access]\nrole_family = { scribe = \"local\" }\nallowed_endpoints = [\"ollama\"]\n";
        let c = Config::parse(local).unwrap();
        assert_eq!(picked(&resolve_route(&c, &[], NOW, &req("scribe", "workhorse"))), "ollama/qwen3:8b");
        let remote = Config::parse(&local.replace("cost_model=\"local\"\n", "").replace("credential=\"none\"", "credential=\"env:K\"").replace("http://localhost:11434", "https://gw.example.com")).unwrap();
        match resolve_route(&remote, &[], NOW, &req("scribe", "workhorse")) {
            Route::NoModel { ineligible, .. } => assert!(ineligible.iter().any(|(_, why)| why.contains("no prices")), "{:?}", ineligible),
            other => panic!("a remote model with no prices must not be chosen: {:?}", other),
        }
    }

    #[test]
    fn a_critic_avoids_the_authors_family_when_it_can_and_says_so_when_it_cannot() {
        let c = cfg();
        let r = resolve_route(&c, &[], NOW, &RouteRequest { avoid_families: vec!["claude".into()], ..req("red-team", "frontier") });
        assert_eq!((picked(&r), r.homogeneous()), ("gem/gem-pro".to_string(), false), "Gemini is chosen even though Claude is the usual family");
        let state: Vec<Entry> = c.pool_entries().iter().filter(|k| k.family == "gemini").map(|k| entry(&k.endpoint_id, &k.model_id, State::Exhausted, Some(NOW + 3600))).collect();
        let r = resolve_route(&c, &state, NOW, &RouteRequest { avoid_families: vec!["claude".into()], ..req("red-team", "frontier") });
        assert_eq!(picked(&r), "direct/opus-x");
        assert!(r.homogeneous(), "no other family is usable, so the pass is homogeneous rather than blocked");
    }

    #[test]
    fn an_explicit_target_wins_and_a_missing_target_falls_back_in_the_same_way() {
        let c = cfg();
        let r = resolve_route(&c, &[], NOW, &RouteRequest { target: Some("gem-pro".into()), ..req("red-team", "workhorse") });
        assert_eq!(picked(&r), "gem/gem-pro", "the target overrides the role's tier and family");
        // gem-pro has a 1M window; of the replacements only the harness (window unknown) is not known to be shorter.
        let r = resolve_route(&c, &down(&c, &["gem-pro"]), NOW, &RouteRequest { target: Some("gem-pro".into()), ..req("red-team", "workhorse") });
        match &r {
            Route::Similar { key, target, .. } => assert_eq!((target.model_id.as_str(), key.model_id.as_str()), ("gem-pro", "gem-agy")),
            other => panic!("{:?}", other),
        }
        // With the harness down too, the 200K Claude models are the same tier but a shorter window: weaker.
        let r = resolve_route(&c, &down(&c, &["gem-pro", "gem-agy"]), NOW, &RouteRequest { target: Some("gem-pro".into()), ..req("red-team", "workhorse") });
        match &r {
            Route::Weaker { key, lost, .. } => assert!(key.model_id == "opus-x" && lost[0].contains("200000-token context instead of 1000000"), "{:?}", r),
            other => panic!("{:?}", other),
        }
        // A target nobody serves is a preference nothing can meet: the role gets its ordinary choice (config validation rejects it earlier).
        let r = resolve_route(&c, &[], NOW, &RouteRequest { target: Some("nope".into()), ..req("red-team", "workhorse") });
        assert_eq!(picked(&r), "direct/sonnet-x", "{:?}", r);
    }

    #[test]
    fn a_route_with_nothing_to_try_is_never_explained_with_an_empty_string() {
        let none = Route::NoModel { target: None, blocked: vec![], ineligible: vec![], back_at: None };
        assert!(none.explain(NOW).contains("no configured model can serve this request"), "{}", none.explain(NOW));
    }

    #[test]
    fn a_target_that_cannot_serve_this_request_falls_back_instead_of_leaving_the_role_with_no_model() {
        let c = cfg();
        // `gem-agy` is the read-only subscription's model: it can never run a Builder.
        let r = resolve_route(&c, &[], NOW, &RouteRequest { target: Some("gem-agy".into()), mutating: true, ..req("builder", "workhorse") });
        assert_eq!(picked(&r), "direct/sonnet-x", "the Builder keeps working on an eligible model: {:?}", r);
        // The same target is used as asked by a role that can use it.
        let r = resolve_route(&c, &[], NOW, &RouteRequest { target: Some("gem-agy".into()), ..req("red-team", "workhorse") });
        assert_eq!(picked(&r), "agy-sub/gem-agy");
        // And an unpriced API model as the target is ignored the same way.
        let unpriced = Config::parse(&CONFIG.replace("input_usd_per_mtok = 2.0\noutput_usd_per_mtok = 12.0", "")).unwrap();
        let r = resolve_route(&unpriced, &[], NOW, &RouteRequest { target: Some("gem-pro".into()), ..req("architect", "frontier") });
        assert!(r.key().is_some_and(|k| k.model_id != "gem-pro"), "{:?}", r);
    }

    #[test]
    fn cooldowns_and_resets_expire_as_the_clock_moves() {
        let c = cfg();
        let state = [entry("direct", "sonnet-x", State::CoolingDown, Some(NOW + 60)), entry("bedrock", "bedrock-sonnet-x", State::Exhausted, Some(NOW + 60))];
        assert_eq!(picked(&resolve_route(&c, &state, NOW, &req("builder", "workhorse"))), "gem/gem-flash", "both Claude workhorse pairs are down: an equal tier elsewhere");
        assert_eq!(picked(&resolve_route(&c, &state, NOW + 61, &req("builder", "workhorse"))), "direct/sonnet-x", "after the wait the target is back");
        let probing = [entry("direct", "sonnet-x", State::Exhausted, None)];
        assert!(matches!(resolve_route(&c, &probing, NOW + 100_000, &req("builder", "workhorse")), Route::Similar { .. }), "no known reset: it stays down until a probe clears it");
    }

    #[test]
    fn an_explicit_pool_order_overrides_the_endpoint_policy_order_within_a_tier() {
        // By policy `direct` comes before `bedrock`; the operator's pool order puts Bedrock's model first.
        let text = format!("{}\n[routing.pool]\norder = [\"bedrock-sonnet-x\", \"sonnet-x\"]\n", CONFIG);
        let c = Config::parse(&text).unwrap();
        assert_eq!(picked(&resolve_route(&c, &[], NOW, &req("builder", "workhorse"))), "bedrock/bedrock-sonnet-x");
        assert_eq!(picked(&resolve_route(&cfg(), &[], NOW, &req("builder", "workhorse"))), "direct/sonnet-x", "without a pool order, policy order decides");
    }

    #[test]
    fn endpoints_outside_the_pin_never_appear() {
        let text = CONFIG.replace("[access]\norder = { claude = [\"direct\", \"bedrock\"] }", "[access]\norder = { claude = [\"bedrock\"] }\nallowed_endpoints = [\"bedrock\", \"gem\"]");
        let c = Config::parse(&text).unwrap();
        for tier in ["fast", "workhorse", "frontier"] {
            for state in [vec![], down(&c, &["bedrock-sonnet-x", "opus-x", "haiku-x"])] {
                let r = resolve_route(&c, &state, NOW, &req("builder", tier));
                assert!(r.key().is_none_or(|k| k.endpoint_id != "direct" && k.endpoint_id != "agy-sub"), "{} {:?}", tier, r);
            }
        }
    }
}
