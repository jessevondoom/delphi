//! Discovered model assignment (`specs/056` §3): choose the pool member for a dispatch by capability class,
//! measured results and quota, not by a fixed tier.
//!
//! A pure function, like [`crate::route::resolve_route`], which calls it only under
//! `[routing] assignment = "discovered"`. The role's *floor* is never crossed: when nothing at or above it
//! is usable the task waits (or escalates when no pool member meets it at all). Every choice is among
//! pool members the operator ratified; nothing here reaches outside that set.

use crate::models::{measured_ineligible, Candidate, Class, ClassRules, Measured, Presence};
use crate::pool::Entry;
use crate::route::{availability, ineligible, Avail, Blocked, Route, RouteRequest};
use delphi_adapter::config::{Config, PoolKey};

/// Measured results of one `(endpoint, model)` for the dispatch's role and kind.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Evidence {
    pub endpoint_id: String,
    pub model_id: String,
    pub runs: u32,
    pub accepted: u32,
    pub cost_micro_usd: u64,
}

impl Evidence {
    fn score(&self) -> f64 {
        self.accepted as f64 / (self.cost_micro_usd.max(1)) as f64
    }
}

/// What discovered assignment needs to know about one dispatch.
#[derive(Debug, Clone, PartialEq)]
pub struct Assign {
    pub floor: Class,
    pub preferred: Class,
    pub plan_making: bool,
    pub plan_following: bool,
    /// Class of every ready pool member; a member absent here (or marked gone) is not selectable.
    pub classes: Vec<(String, String, Class)>,
    /// The measured attributes of every ready pool member (`specs/056` §10.2); a member absent here has none.
    pub measured: Vec<(String, String, Measured)>,
    pub gone: Vec<(String, String)>,
    pub evidence: Vec<Evidence>,
    /// Results after which a model is scored by evidence alone.
    pub enough: u32,
    /// Seats the operator or the meter says are close to their cap.
    pub near_cap: Vec<(String, String)>,
    /// Classes above (+) or below (-) the preferred one: a grade, a ratchet, a raised tier.
    pub step: i32,
}

/// Rank of a class for comparison: quick and local sit together at the bottom.
pub fn level(c: Class) -> Option<i32> {
    match c {
        Class::Quick | Class::Local => Some(0),
        Class::Steady => Some(1),
        Class::Deep => Some(2),
        // Below every floor: it has no rank to compare, so no floor is ever met by it.
        Class::Emergency | Class::Unclassified => None,
    }
}

impl Assign {
    /// The role's policy and the pool's classes, from the rules and the discovered candidates.
    pub fn build(rules: &ClassRules, role: &str, candidates: &[Candidate], enough: u32) -> Assign {
        let policy = rules.roles.get(role).or_else(|| rules.roles.get("default"));
        let parse = |v: Option<&str>| v.and_then(Class::parse).unwrap_or(Class::Steady);
        Assign {
            floor: parse(policy.map(|p| p.floor.as_str())),
            preferred: parse(policy.map(|p| p.preferred.as_str())),
            plan_making: policy.is_some_and(|p| p.plan_making),
            plan_following: policy.is_some_and(|p| p.plan_following),
            classes: candidates.iter().filter(|c| c.presence == Presence::Ready && c.class != Class::Emergency).map(|c| (c.endpoint_id.clone(), c.model_id.clone(), c.class)).collect(),
            measured: candidates.iter().filter(|c| c.presence == Presence::Ready).map(|c| (c.endpoint_id.clone(), c.model_id.clone(), c.measured.clone())).collect(),
            gone: candidates.iter().filter(|c| c.presence == Presence::Gone).map(|c| (c.endpoint_id.clone(), c.model_id.clone())).collect(),
            evidence: vec![],
            enough,
            near_cap: vec![],
            step: 0,
        }
    }

    fn class_of(&self, k: &PoolKey) -> Option<Class> {
        if self.gone.iter().any(|(e, m)| e == &k.endpoint_id && m == &k.model_id) {
            return None;
        }
        self.classes.iter().find(|(e, m, _)| e == &k.endpoint_id && m == &k.model_id).map(|(_, _, c)| *c)
    }

    fn measured_of(&self, k: &PoolKey) -> Measured {
        self.measured.iter().find(|(e, m, _)| e == &k.endpoint_id && m == &k.model_id).map(|(_, _, m)| m.clone()).unwrap_or_default()
    }

    fn evidence_of(&self, k: &PoolKey) -> Option<&Evidence> {
        self.evidence.iter().find(|e| e.endpoint_id == k.endpoint_id && e.model_id == k.model_id)
    }

    /// For the `model.assigned` event: the class and what evidence stood behind the choice.
    pub fn describe(&self, k: &PoolKey) -> (String, String) {
        let class = self.class_of(k).unwrap_or_default().as_str().to_string();
        let evidence = match self.evidence_of(k) {
            Some(e) if e.runs >= self.enough => format!("measured: {}/{} accepted", e.accepted, e.runs),
            Some(e) => format!("thin: {}/{} accepted, class decides", e.accepted, e.runs),
            None => "none: class decides".to_string(),
        };
        (class, evidence)
    }

    fn near_cap(&self, k: &PoolKey, state: &[Entry]) -> bool {
        self.near_cap.iter().any(|(e, m)| e == &k.endpoint_id && m == &k.model_id) || state.iter().any(|e| e.endpoint_id == k.endpoint_id && e.model_id == k.model_id && !e.throttles.is_empty())
    }
}

/// Measured results for `role` from `<ws>/local/model-evidence.json` (written from retrospectives, `specs/056` M4); empty when absent.
pub fn read_evidence(ws_dir: &std::path::Path, role: &str) -> Vec<Evidence> {
    let Ok(text) = std::fs::read_to_string(ws_dir.join("local/model-evidence.json")) else { return vec![] };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    let mut out: Vec<Evidence> = Vec::new();
    for e in v["entries"].as_array().into_iter().flatten().filter(|e| e["role"].as_str() == Some(role)) {
        let (Some(ep), Some(m)) = (e["endpoint_id"].as_str(), e["model_id"].as_str()) else { continue };
        let n = |k: &str| e[k].as_u64().unwrap_or(0);
        match out.iter_mut().find(|x| x.endpoint_id == ep && x.model_id == m) {
            Some(x) => {
                x.runs += n("runs") as u32;
                x.accepted += n("accepted") as u32;
                x.cost_micro_usd += n("cost_micro_usd");
            }
            None => out.push(Evidence { endpoint_id: ep.into(), model_id: m.into(), runs: n("runs") as u32, accepted: n("accepted") as u32, cost_micro_usd: n("cost_micro_usd") }),
        }
    }
    out
}

/// Seats the operator marked close to their cap in `<ws>/local/seat-headroom.json` (`{"near_cap": [{"endpoint_id", "model_id"}]}`); empty when absent.
/// A live meter feed is not read here: usage readings exist only at doctor time today.
pub fn read_near_cap(ws_dir: &std::path::Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(ws_dir.join("local/seat-headroom.json")) else { return vec![] };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    v["near_cap"].as_array().into_iter().flatten().filter_map(|e| Some((e["endpoint_id"].as_str()?.to_string(), e["model_id"].as_str()?.to_string()))).collect()
}

/// Pick a member for `req`. Never returns a model below the role's floor, and never a `Weaker` route.
pub fn pick(cfg: &Config, state: &[Entry], now: i64, req: &RouteRequest, a: &Assign) -> Route {
    let find = |k: &PoolKey| state.iter().find(|e| e.endpoint_id == k.endpoint_id && e.model_id == k.model_id);
    let floor = level(a.floor).unwrap_or(1);
    let top = a.classes.iter().filter_map(|(_, _, c)| level(*c)).max().unwrap_or(floor);
    let desired = (level(a.preferred).unwrap_or(floor) + a.step).clamp(floor, top.max(floor));

    let mut members: Vec<(usize, PoolKey, i32)> = Vec::new();
    let mut rejected: Vec<(PoolKey, String)> = Vec::new();
    for (i, k) in cfg.pool_entries().into_iter().enumerate() {
        if let Some(why) = ineligible(cfg, &k, req) {
            rejected.push((k, why));
            continue;
        }
        if let Some(why) = measured_ineligible(&a.measured_of(&k), req.min_context_tokens, req.needs_json) {
            rejected.push((k, why));
            continue;
        }
        match a.class_of(&k) {
            Some(c) => match level(c) {
                Some(l) if l >= floor => members.push((i, k, l)),
                Some(_) => rejected.push((k, "below the role's floor".into())),
                // An unclassified model may be tried only where a deterministic gate checks the work (056 OD-4).
                None if a.plan_following => members.push((i, k, floor)),
                None => rejected.push((k, "not classified, and this role makes or judges plans".into())),
            },
            None => rejected.push((k, "not a ready pool member (gone or unseen)".into())),
        }
    }
    if members.is_empty() {
        return Route::NoModel { target: None, blocked: vec![], ineligible: rejected, back_at: None };
    }

    // Lower is better.
    let key_of = |(i, k, l): &(usize, PoolKey, i32)| {
        let measured = a.evidence_of(k).filter(|e| e.runs >= a.enough).map(|e| e.score());
        let diversity = req.avoid_families.iter().any(|f| f == &k.family);
        (
            measured.map(|s| -s).map(ordered).unwrap_or(i64::MAX),
            a.near_cap(k, state),
            diversity,
            (l - desired).abs() * 2 + (*l < desired) as i32,
            *i,
        )
    };
    members.sort_by_key(key_of);

    let mut usable: Vec<&(usize, PoolKey, i32)> = Vec::new();
    let mut blocked: Vec<Blocked> = Vec::new();
    for m in &members {
        match availability(find(&m.1), now) {
            Avail::Yes => usable.push(m),
            Avail::No { why, back_at } => blocked.push(Blocked { key: m.1.clone(), why, back_at }),
        }
    }
    let homogeneous = |k: &PoolKey| !req.avoid_families.is_empty() && req.avoid_families.iter().any(|f| f == &k.family);
    let first = &members[0].1;
    match usable.first() {
        Some(m) if &m.1 == first => Route::Target { homogeneous: homogeneous(&m.1), key: m.1.clone() },
        Some(m) => {
            let why = blocked.iter().find(|b| &b.key == first).map(|b| b.why.clone()).unwrap_or_default();
            Route::Similar { homogeneous: homogeneous(&m.1), key: m.1.clone(), target: first.clone(), why }
        }
        None => {
            let back_at = blocked.iter().filter_map(|b| b.back_at).min();
            Route::NoModel { target: Some(first.clone()), blocked, ineligible: rejected, back_at }
        }
    }
}

/// A float as a sortable integer (micro-units are plenty for a ratio of results to cost).
fn ordered(x: f64) -> i64 {
    (x * 1e12) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ClassRules;
    use crate::pool::State;

    const NOW: i64 = 1_790_000_000;

    fn cfg() -> Config {
        Config::parse(
            r#"
[models.fam-a]
fast = "m-small-a"
workhorse = "m-mid-a"
frontier = "m-big-a"
[models.fam-b]
fast = "m-small-b"
workhorse = "m-mid-b"
frontier = "m-big-b"
[models.catalog."m-small-a"]
max_output_tokens = 8000
context_tokens = 100000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 2.0
[models.catalog."m-mid-a"]
max_output_tokens = 8000
context_tokens = 100000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 2.0
[models.catalog."m-big-a"]
max_output_tokens = 8000
context_tokens = 100000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 2.0
[models.catalog."m-small-b"]
max_output_tokens = 8000
context_tokens = 100000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 2.0
[models.catalog."m-mid-b"]
max_output_tokens = 8000
context_tokens = 100000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 2.0
[models.catalog."m-big-b"]
max_output_tokens = 8000
context_tokens = 100000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 2.0

[[endpoints]]
id = "ep-a"
family = "fam-a"
path = "api"
credential = "env:A"
[[endpoints]]
id = "ep-b"
family = "fam-b"
path = "api"
credential = "env:B"

[routing]
assignment = "discovered"
"#,
        )
        .unwrap()
    }

    fn assign_for(c: &Config, role: &str) -> Assign {
        let rules = ClassRules::shipped();
        let mut list = crate::models::discover(c, None, NOW);
        crate::models::classify(&rules, &mut list);
        Assign::build(&rules, role, &list, 8)
    }

    fn req(role: &str, a: Assign) -> RouteRequest {
        RouteRequest { role: role.into(), tier: "workhorse".into(), assign: Some(a), ..Default::default() }
    }

    fn down(c: &Config, model: &str, state: State) -> Entry {
        let k = c.pool_entries().into_iter().find(|k| k.model_id == model).unwrap();
        Entry { endpoint_id: k.endpoint_id, model_id: k.model_id, state, since: NOW, until: Some(NOW + 3600), reason: "quota".into(), throttles: vec![], probe_interval_secs: 0, next_probe_at: None }
    }

    #[test]
    fn invented_ids_are_assigned_by_class_not_name() {
        let c = cfg();
        let a = assign_for(&c, "architect");
        let r = resolve(&c, &[], &req("architect", a));
        let k = r.key().unwrap();
        assert!(k.model_id.starts_with("m-big"), "a plan-making role starts at the deep class: {k:?}");
    }

    #[test]
    fn better_evidence_per_cost_wins_whatever_the_provider() {
        let c = cfg();
        let mut a = assign_for(&c, "builder");
        for (ep, m, accepted, cost) in [("ep-a", "m-mid-a", 8, 800_000u64), ("ep-b", "m-mid-b", 8, 200_000)] {
            a.evidence.push(Evidence { endpoint_id: ep.into(), model_id: m.into(), runs: 8, accepted, cost_micro_usd: cost });
        }
        let r = resolve(&c, &[], &req("builder", a));
        assert_eq!(r.key().unwrap().model_id, "m-mid-b");
    }

    #[test]
    fn a_seat_near_its_cap_ranks_lower_and_failover_at_the_cap_records_why() {
        let c = cfg();
        let mut a = assign_for(&c, "builder");
        a.near_cap.push(("ep-a".into(), "m-mid-a".into()));
        let r = resolve(&c, &[], &req("builder", a.clone()));
        assert_eq!(r.key().unwrap().model_id, "m-mid-b", "near cap goes last");
        a.near_cap.clear();
        let st = [down(&c, "m-mid-a", State::Exhausted)];
        match resolve(&c, &st, &req("builder", a)) {
            Route::Similar { key, target, why, .. } => {
                assert_eq!(key.model_id, "m-mid-b");
                assert_eq!(target.model_id, "m-mid-a");
                assert!(why.contains("exhausted"), "{why}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn only_a_lower_class_left_means_waiting_never_below_the_floor() {
        let c = cfg();
        let a = assign_for(&c, "validator");
        let st = [down(&c, "m-mid-a", State::Exhausted), down(&c, "m-mid-b", State::Exhausted), down(&c, "m-big-a", State::Exhausted), down(&c, "m-big-b", State::Exhausted)];
        let r = resolve(&c, &st, &req("validator", a));
        assert!(matches!(r, Route::NoModel { .. }), "{r:?}");
        assert!(r.explain(NOW).contains("exhausted"));
    }

    #[test]
    fn a_gone_model_is_never_selected_and_nothing_eligible_escalates() {
        let c = cfg();
        let mut a = assign_for(&c, "validator");
        a.classes.clear();
        a.gone = c.pool_entries().into_iter().map(|k| (k.endpoint_id, k.model_id)).collect();
        let r = resolve(&c, &[], &req("validator", a));
        match r {
            Route::NoModel { blocked, ineligible, .. } => {
                assert!(blocked.is_empty());
                assert!(!ineligible.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn one_eligible_model_gives_the_same_choice_as_static() {
        let mut c = cfg();
        c.routing.pool = Some(vec!["m-mid-a".into()]);
        let a = assign_for(&c, "builder");
        let discovered = resolve(&c, &[], &req("builder", a.clone()));
        c.routing.assignment = delphi_adapter::config::Assignment::Static;
        let fixed = resolve(&c, &[], &req("builder", a));
        assert_eq!(discovered.key(), fixed.key());
    }

    #[test]
    fn an_avoided_family_goes_last_but_still_runs_when_alone() {
        let c = cfg();
        let a = assign_for(&c, "red-team");
        let mut r = req("red-team", a.clone());
        r.avoid_families = vec!["fam-a".into()];
        assert_eq!(resolve(&c, &[], &r).key().unwrap().family, "fam-b");
        let mut one = c.clone();
        one.routing.pool = Some(vec!["m-big-a".into()]);
        let route = resolve(&one, &[], &r);
        assert!(route.key().is_some() && route.homogeneous(), "{route:?}");
    }

    #[test]
    fn a_grade_moves_the_wanted_class_but_never_past_the_floor_or_the_pools_top() {
        let c = cfg();
        let class_of = |role: &str, step: i32| {
            let mut a = assign_for(&c, role);
            a.step = step;
            let r = resolve(&c, &[], &req(role, a));
            r.key().unwrap().model_id.clone()
        };
        assert!(class_of("builder", 1).starts_with("m-big"), "promoted from steady to deep");
        assert!(class_of("builder", -1).starts_with("m-small"), "nerfed to quick, which is its floor");
        assert!(class_of("builder", 2).starts_with("m-big"), "the pool has nothing above deep");
        assert!(class_of("validator", -2).starts_with("m-mid"), "a floor of steady holds");
    }

    /// Give one pool member a measured record.
    fn with_measured(mut a: Assign, model: &str, m: Measured) -> Assign {
        let k = a.classes.iter().find(|(_, id, _)| id == model).map(|(e, id, _)| (e.clone(), id.clone())).unwrap();
        a.measured.retain(|(e, id, _)| !(e == &k.0 && id == &k.1));
        a.measured.push((k.0, k.1, m));
        a
    }

    /// `specs/056` §10.2 use 1: under `discovered` a prompt over the measured budget, or a schema answer without a valid JSON-in-text
    /// record, rules the pair out for that dispatch and the next one serves.
    #[test]
    fn a_measured_budget_or_json_record_rules_a_pair_out_under_discovered() {
        let c = cfg();
        let small = Measured { prompt_budget_tokens: 50_000, json_in_text: "bare".into(), ..Default::default() };
        let a = with_measured(assign_for(&c, "builder"), "m-mid-a", small);
        let r = RouteRequest { min_context_tokens: 80_000, ..req("builder", a.clone()) };
        assert_eq!(resolve(&c, &[], &r).key().unwrap().model_id, "m-mid-b", "the prompt does not fit m-mid-a's measured budget");
        assert_eq!(resolve(&c, &[], &req("builder", a.clone())).key().unwrap().model_id, "m-mid-a", "no prompt size given: nothing is ruled out");
        let nested = || Measured { json_in_text: "nested-string".into(), ..Default::default() };
        let mut every = with_measured(a, "m-mid-b", nested());
        for (e, m, _) in every.classes.clone().into_iter().filter(|(_, m, _)| m != "m-mid-b" && m != "m-mid-a") {
            every.measured.retain(|(x, y, _)| !(x == &e && y == &m));
            every.measured.push((e, m, Measured { json_in_text: "bare".into(), ..Default::default() }));
        }
        let pick = resolve(&c, &[], &RouteRequest { needs_json: true, ..req("builder", every) });
        assert_ne!(pick.key().unwrap().model_id, "m-mid-b", "{pick:?}");
        let only_nested = with_measured(assign_for(&c, "builder"), "m-mid-b", nested());
        match resolve(&c, &[], &RouteRequest { needs_json: true, ..req("builder", only_nested) }) {
            Route::NoModel { ineligible, .. } => assert!(ineligible.iter().any(|(k, why)| k.model_id == "m-mid-a" && why.contains("no record")) && ineligible.iter().any(|(_, why)| why.contains("nested-string")), "{ineligible:?}"),
            other => panic!("an empty record is never a pass: {other:?}"),
        }
    }

    /// M1a exit: `static` routes are byte-identical whatever the measured attributes say, over every role and tier.
    #[test]
    fn static_routes_are_unchanged_by_the_measured_attributes() {
        let mut c = cfg();
        c.routing.assignment = delphi_adapter::config::Assignment::Static;
        let mut golden = Vec::new();
        for role in ["architect", "builder", "validator", "scout"] {
            for tier in ["fast", "workhorse", "frontier"] {
                let mut a = assign_for(&c, role);
                for (e, m, _) in a.classes.clone() {
                    a.measured.push((e, m, Measured { prompt_budget_tokens: 1, json_in_text: "untested".into(), served_match: Some(false), ..Default::default() }));
                }
                let plain = RouteRequest { role: role.into(), tier: tier.into(), ..Default::default() };
                let with = RouteRequest { assign: Some(a), min_context_tokens: 0, needs_json: true, ..plain.clone() };
                let (x, y) = (resolve(&c, &[], &plain), resolve(&c, &[], &with));
                assert_eq!(format!("{x:?}"), format!("{y:?}"), "{role} {tier}");
                golden.push(format!("{role}/{tier}={}", x.key().unwrap().model_id));
            }
        }
        assert_eq!(golden.iter().filter(|g| g.ends_with("=m-small-a")).count(), 4);
        assert_eq!(golden.iter().filter(|g| g.ends_with("=m-mid-a")).count(), 4);
        assert_eq!(golden.iter().filter(|g| g.ends_with("=m-big-a")).count(), 4);
    }

    fn resolve(c: &Config, st: &[Entry], r: &RouteRequest) -> Route {
        crate::route::resolve_route(c, st, NOW, r)
    }
}
