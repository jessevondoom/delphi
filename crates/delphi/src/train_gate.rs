//! The `discovered`-versus-`static` benchmark gate (`specs/056` §10.6, slice M7): the code that runs it and reads it.
//!
//! OD-5 keeps `[routing] assignment = "static"` the default until a benchmark shows `discovered` no worse on the same suite. This
//! module holds what makes that benchmark one instrument and not a judgement call:
//!
//! - the **benchmark file** (`dev/train/m7/benchmark.toml`, pre-registered): the cases, the runs per arm, the per-run token caps
//!   by suite and the numbers of the pass criterion;
//! - the **routing pair**: two arms that differ only in `[arm.routing] assignment`, refused otherwise (AC-A6);
//! - the **facts of a run** read from its mission log: tokens in the cost unit of §10.3, premium requests, each span's model, any
//!   dispatch below its role's floor (`model.assigned`) and any served model over its cap (`[routing] max_cost_class`);
//! - the **verdict**: the five criteria, PASS, FAIL or INCOMPLETE, and the contrast.
//!
//! Nothing here calls a model. The figures are about two routing settings on fixed cases; no seat sees them and none is ever in a
//! prompt (`specs/056` §10.4, `specs/078`). A pass is necessary, not sufficient: the operator decides OD-5 from the report.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use delphi_core::event::EventEnvelope;
use serde_json::{json, Value};

use crate::train_run::{Arm, Row};

// ── the benchmark file ──────────────────────────────────────────────────────────────────────────────────────────

/// The pre-registered benchmark: which cases, how many runs, the token caps and the criterion's numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Bench {
    pub name: String,
    /// Graded runs each arm needs per case (criterion 1).
    pub runs: u32,
    /// D passes at least S's passes minus this (criterion 2).
    pub margin: u32,
    /// D's tokens (and premium requests) per passing run at most this times S's (criterion 4).
    pub cost_ratio: f64,
    /// Below this share of differing dispatches the result is labelled low contrast.
    pub low_contrast: f64,
    /// `suite/id` of every case.
    pub cases: Vec<String>,
    /// The tokens one run of a case in this suite may spend.
    pub token_cap: BTreeMap<String, u64>,
    /// The seeds of the generated review cases; a calibration run must use other seeds (§10.4).
    pub review_seeds: Vec<u64>,
}

pub fn parse_bench(path: &Path) -> Result<Bench, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let t: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("{}: {}", path.display(), e.message()))?;
    let at = path.display();
    let int = |k: &str| t.get(k).and_then(|v| v.as_integer()).ok_or(format!("{at}: `{k}` must be a whole number"));
    let float = |k: &str| t.get(k).and_then(|v| v.as_float()).ok_or(format!("{at}: `{k}` must be a number with a decimal point"));
    let runs = int("runs")?;
    if !(1..=20).contains(&runs) {
        return Err(format!("{at}: `runs` must be 1 to 20"));
    }
    let margin = int("margin")?;
    if !(0..=runs * 100).contains(&margin) {
        return Err(format!("{at}: `margin` must be 0 or more"));
    }
    let (cost_ratio, low_contrast) = (float("cost_ratio")?, float("low_contrast")?);
    if !(cost_ratio >= 1.0 && cost_ratio.is_finite()) || !(0.0..=1.0).contains(&low_contrast) {
        return Err(format!("{at}: `cost_ratio` must be 1.0 or more and `low_contrast` 0 to 1"));
    }
    let strings = |k: &str| t.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>()).unwrap_or_default();
    let cases = strings("cases");
    if cases.is_empty() || cases.iter().any(|c| c.split('/').count() != 2 || c.split('/').any(str::is_empty)) {
        return Err(format!("{at}: `cases` must list `suite/id` names"));
    }
    if cases.iter().collect::<BTreeSet<_>>().len() != cases.len() {
        return Err(format!("{at}: a case is listed twice"));
    }
    let mut token_cap = BTreeMap::new();
    for (suite, v) in t.get("token_cap").and_then(|v| v.as_table()).ok_or(format!("{at}: `[token_cap]` must give each suite's tokens per run"))? {
        let n = v.as_integer().filter(|n| *n > 0).ok_or(format!("{at}: token_cap.{suite} must be a positive whole number"))?;
        token_cap.insert(suite.clone(), n as u64);
    }
    if let Some(c) = cases.iter().find(|c| !token_cap.contains_key(c.split('/').next().unwrap_or(""))) {
        return Err(format!("{at}: `{c}` has no [token_cap] for its suite; every run of the benchmark has a cap"));
    }
    let review_seeds = t.get("review_seeds").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_integer().map(|n| n as u64)).collect()).unwrap_or_default();
    Ok(Bench { name: t.get("name").and_then(|v| v.as_str()).unwrap_or("benchmark").into(), runs: runs as u32, margin: margin as u32, cost_ratio, low_contrast, cases, token_cap, review_seeds })
}

impl Bench {
    pub fn cap_of(&self, case: &str) -> Option<u64> {
        self.token_cap.get(case.split('/').next().unwrap_or("")).copied()
    }

    /// The most the whole benchmark may spend in tokens: every case's cap, once per run per arm.
    pub fn ceiling(&self, arms: usize) -> u64 {
        self.cases.iter().filter_map(|c| self.cap_of(c)).sum::<u64>() * u64::from(self.runs) * arms as u64
    }
}

// ── the routing pair ────────────────────────────────────────────────────────────────────────────────────────────

/// The two arms of a routing comparison as `(static, discovered)`. Refused unless there are exactly two, they set `[arm.routing]
/// assignment` to `static` and `discovered`, and they share everything else: one template home, the same `expect_models`, the same
/// overlay, no `[loop]` switches and no `base` (AC-A6). The same home means the same ratified pool, tiers, grades and cost cap.
pub fn routing_pair(arms: &[Arm]) -> Result<(String, String), String> {
    let one = "a routing comparison has exactly two arms, `[arm.routing] assignment = \"static\"` and `\"discovered\"`, that differ in nothing else";
    if arms.len() != 2 || arms.iter().any(|a| a.routing.is_none()) {
        return Err(one.into());
    }
    let (a, b) = (&arms[0], &arms[1]);
    let named = |v: &str| arms.iter().find(|x| x.routing.as_deref() == Some(v)).map(|x| x.name.clone());
    let (Some(s), Some(d)) = (named("static"), named("discovered")) else { return Err(one.into()) };
    if arms.iter().any(|x| x.base.is_some() || !x.loop_switches.is_empty()) {
        return Err(format!("{one}: a routing arm sets no `base` and no [loop] switches"));
    }
    let home = |x: &Arm| x.template_home.as_ref().map(|h| h.canonicalize().unwrap_or_else(|_| h.clone()));
    if home(a).is_none() || home(a) != home(b) {
        return Err(format!("{one}: they name different template homes; point both at one home so pool, tiers, grades and cap are the same"));
    }
    let sorted = |x: &Arm| x.expect_models.iter().cloned().collect::<BTreeSet<_>>();
    if a.expect_models.is_empty() || sorted(a) != sorted(b) {
        return Err(format!("{one}: both list the same, non-empty `expect_models` (every pool model, since discovered may choose any)"));
    }
    if a.overlay != b.overlay {
        return Err(format!("{one}: they name different overlays"));
    }
    Ok((s, d))
}

/// Set `[routing] assignment` in a run's copy of `config.toml`. The key is not pinned (only `routing.max_cost_class` is), so a throwaway
/// home needs no re-seal, and the cap and everything ratified stay as the template has them.
pub fn set_routing(config: &str, assignment: &str) -> Result<String, String> {
    let mut v: toml::Value = toml::from_str(config).map_err(|e| format!("config.toml: {}", e))?;
    let root = v.as_table_mut().ok_or("config.toml is not a table")?;
    let table = root.entry("routing").or_insert_with(|| toml::Value::Table(Default::default())).as_table_mut().ok_or("[routing] is not a table")?;
    table.insert("assignment".into(), toml::Value::String(assignment.into()));
    toml::to_string_pretty(&v).map_err(|e| e.to_string())
}

/// The configuration the daemon of a run's home will use: the live file with the ratified blocks and pinned keys put back.
pub fn effective(ws_dir: &Path) -> Option<delphi_adapter::config::Config> {
    let live = std::fs::read_to_string(ws_dir.join("config.toml")).ok()?;
    let (text, _) = delphi_engine::boot::effective_config(ws_dir, &live).ok()?;
    delphi_adapter::config::Config::parse(&text).ok()
}

/// Pool models of a template home that no `expect_models` entry covers: a discovered arm may choose any pool model, so each must be
/// expected, or a run it served would go ungraded for staffing.
pub fn unexpected_pool_models(cfg: &delphi_adapter::config::Config, expect: &[String]) -> Vec<String> {
    cfg.pool_entries().into_iter().filter(|k| !expect.iter().any(|x| k.model_id.starts_with(x.as_str()))).map(|k| format!("{}/{}", k.endpoint_id, k.model_id)).collect()
}

/// The ceiling lines printed before anything runs: tokens at the caps, and what a call costs in premium requests where the catalog
/// gives a multiplier (Copilot seats). Calls per run are not known in advance, so premium requests are given per call.
pub fn ceiling_text(bench: &Bench, arms: usize, cfg: Option<&delphi_adapter::config::Config>) -> String {
    let mut by_suite: BTreeMap<&str, usize> = BTreeMap::new();
    for c in &bench.cases {
        *by_suite.entry(c.split('/').next().unwrap_or("")).or_default() += 1;
    }
    let parts: Vec<String> = by_suite.iter().map(|(s, n)| format!("{} {} x {} runs x {} arms at {} tokens", n, s, bench.runs, arms, bench.token_cap.get(*s).copied().unwrap_or(0))).collect();
    let mut out = format!("token ceiling: {} tokens ({})\n", group(bench.ceiling(arms)), parts.join("; "));
    let priced: Vec<String> = cfg.map(|c| c.pool_entries().into_iter().filter_map(|k| delphi_engine::route::pair_cost(c, &k.endpoint_id, &k.model_id).1.map(|m| format!("{}/{} {}x", k.endpoint_id, k.model_id, m))).collect()).unwrap_or_default();
    if priced.is_empty() {
        out.push_str("premium requests: no pool model has a catalog multiplier, so none are expected\n");
    } else {
        out.push_str(&format!("premium requests: each call on these costs its multiplier: {} (how many calls a run makes is not known in advance)\n", priced.join(", ")));
    }
    out
}

fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The operator's yes for this run: typed at a terminal, never an argument, the environment or a pipe from an agent.
pub fn confirm(at_terminal: bool, input: &mut dyn BufRead, out: &mut dyn Write) -> bool {
    if !at_terminal {
        let _ = writeln!(out, "not run: the benchmark runs only on the operator's yes, typed at a terminal (not from inside an agent seat)");
        return false;
    }
    let _ = write!(out, "Run the benchmark at this ceiling? Type yes: ");
    let _ = out.flush();
    let mut line = String::new();
    input.read_line(&mut line).is_ok() && line.trim() == "yes"
}

// ── the facts of one run ────────────────────────────────────────────────────────────────────────────────────────

/// One span's model: the first `agent.usage` of the span.
#[derive(Debug, Clone, PartialEq)]
pub struct Dispatch {
    pub role: String,
    pub endpoint: String,
    pub model: String,
}

/// What the gate reads from a run's log, beside whether it passed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// Input + cache read + cache write + output over every call (`specs/056` §10.3), never dollars.
    pub cost_tokens: u64,
    pub premium_requests: f64,
    pub dispatches: Vec<Dispatch>,
    /// A `model.assigned` whose class sits below the floor it names.
    pub below_floor: Vec<String>,
    /// A model that served a role over that role's `[routing] max_cost_class`.
    pub over_cap: Vec<String>,
    pub cost_capped: u32,
    pub token_cap: Option<u64>,
    /// The run was stopped at, or ended over, its token cap: it counts as not passed.
    pub capped: bool,
}

/// Tokens of every `agent.usage` in `events`, in the cost unit of §10.3 (no multiplier).
pub fn tokens_of(events: &[EventEnvelope]) -> u64 {
    events.iter().filter(|e| e.kind == "agent.usage").map(|e| {
        let n = |k: &str| e.payload[k].as_u64().unwrap_or(0);
        delphi_engine::assign::Tokens { input: n("input_tokens"), cache_read: n("cache_read_tokens"), cache_write: n("cache_write_tokens"), output: n("output_tokens") }.cost(None)
    }).sum()
}

/// Every event of every mission in a run's workspace (a run stopped at its cap printed no summary naming its mission). A log being
/// written may not read whole; it is skipped until the next look.
pub fn home_events(ws_dir: &Path) -> Vec<EventEnvelope> {
    delphi_core::sessions::dirs(ws_dir).into_iter().filter_map(|(_, p)| delphi_core::log::replay(&p).ok()).flatten().collect()
}

pub fn facts_of(events: &[EventEnvelope], cfg: Option<&delphi_adapter::config::Config>) -> Facts {
    use delphi_engine::models::Class;
    let mut f = Facts { cost_tokens: tokens_of(events), ..Facts::default() };
    let mut spans = BTreeSet::new();
    let mut served: BTreeSet<(String, String, String)> = BTreeSet::new();
    for e in events {
        match e.kind.as_str() {
            "agent.usage" => {
                let (role, ep, m) = (e.agent.clone().unwrap_or_default(), e.endpoint_id.clone().unwrap_or_default(), e.model_id.clone().unwrap_or_default());
                served.insert((role.clone(), ep.clone(), m.clone()));
                if e.span_id.as_ref().is_some_and(|s| spans.insert(s.clone())) {
                    f.dispatches.push(Dispatch { role, endpoint: ep, model: m });
                }
            }
            "harness.cost" => f.premium_requests += e.payload["premium_requests"].as_f64().unwrap_or(0.0),
            "model.cost_capped" => f.cost_capped += 1,
            "model.assigned" => {
                let class = e.payload["class"].as_str().and_then(Class::parse);
                let floor = e.payload["floor"].as_str().and_then(Class::parse);
                let level = |c: Option<Class>| c.and_then(delphi_engine::assign::level);
                let below = class == Some(Class::Emergency) || matches!((level(class), level(floor)), (Some(c), Some(fl)) if c < fl);
                if below {
                    f.below_floor.push(format!("{} on {}/{}: class {} below floor {}", e.agent.as_deref().unwrap_or("?"), e.payload["endpoint_id"].as_str().unwrap_or("?"), e.payload["model_id"].as_str().unwrap_or("?"), e.payload["class"].as_str().unwrap_or("?"), e.payload["floor"].as_str().unwrap_or("?")));
                }
            }
            _ => {}
        }
    }
    if let Some(cfg) = cfg {
        for (role, ep, m) in &served {
            if let Some(why) = delphi_engine::route::over_cap(cfg, ep, m, role) {
                f.over_cap.push(format!("{role} on {ep}/{m}: {why}"));
            }
        }
    }
    f
}

impl Facts {
    pub fn to_json(&self) -> Value {
        json!({"cost_tokens": self.cost_tokens, "premium_requests": self.premium_requests, "dispatches": self.dispatches.iter().map(|d| json!([d.role, d.endpoint, d.model])).collect::<Vec<_>>(),
               "below_floor": self.below_floor, "over_cap": self.over_cap, "cost_capped": self.cost_capped, "token_cap": self.token_cap, "capped": self.capped})
    }

    pub fn from_json(v: &Value) -> Facts {
        let strs = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        Facts {
            cost_tokens: v["cost_tokens"].as_u64().unwrap_or(0),
            premium_requests: v["premium_requests"].as_f64().unwrap_or(0.0),
            dispatches: v["dispatches"].as_array().map(|a| a.iter().filter_map(|d| Some(Dispatch { role: d[0].as_str()?.into(), endpoint: d[1].as_str()?.into(), model: d[2].as_str()?.into() })).collect()).unwrap_or_default(),
            below_floor: strs("below_floor"),
            over_cap: strs("over_cap"),
            cost_capped: v["cost_capped"].as_u64().unwrap_or(0) as u32,
            token_cap: v["token_cap"].as_u64(),
            capped: v["capped"].as_bool().unwrap_or(false),
        }
    }
}

// ── the verdict ─────────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Holds {
    Yes(String),
    No(String),
    /// Not decidable from these rows (the runs are incomplete, or S passed nothing so a cost per pass has no base).
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gate {
    pub s: String,
    pub d: String,
    pub criteria: Vec<(&'static str, Holds)>,
    pub verdict: &'static str,
    /// Share of D's dispatches whose model differs from S's for the same case and run.
    pub contrast: Option<f64>,
    pub per_case: Vec<CaseLine>,
}

/// A case and each arm's `(passes, graded runs)`: S's, then D's.
pub type CaseLine = (String, (u32, u32), (u32, u32));

/// The graded rows of `(case, arm)`: the last graded row of each run number, the first `runs` run numbers.
fn graded<'a>(rows: &'a [Row], case: &str, arm: &str, runs: u32) -> Vec<&'a Row> {
    let mut by_seed: BTreeMap<u32, &Row> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.case == case && r.arm == arm && r.passed.is_some()) {
        by_seed.insert(r.seed, r);
    }
    by_seed.into_values().take(runs as usize).collect()
}

fn passes(rs: &[&Row]) -> u32 {
    rs.iter().filter(|r| r.passed == Some(true)).count() as u32
}

/// `x` per passing run of `rs`; `None` when none passed.
fn per_pass(rs: &[&Row], x: impl Fn(&Row) -> f64) -> Option<f64> {
    let p = passes(rs);
    (p > 0).then(|| rs.iter().map(|r| x(r)).sum::<f64>() / f64::from(p))
}

fn cost_check(what: &str, s: Option<f64>, d: Option<f64>, ratio: f64) -> Holds {
    match (s, d) {
        (None, _) => Holds::Unknown(format!("S passed no run, so its {what} per passing run has no base")),
        (Some(_), None) => Holds::No(format!("D passed no run, so its {what} per passing run is unbounded")),
        (Some(s), Some(d)) if s <= 0.0 && d <= 0.0 => Holds::Yes(format!("{what} per passing run: none in either arm")),
        (Some(s), Some(d)) => {
            let line = format!("{what} per passing run: D {:.0}, S {:.0}, ratio {}", d, s, if s > 0.0 { format!("{:.2}", d / s) } else { "unbounded".into() });
            if s > 0.0 && d <= ratio * s { Holds::Yes(line) } else { Holds::No(format!("{line}, over {ratio}")) }
        }
    }
}

pub fn evaluate(rows: &[Row], bench: &Bench) -> Result<Gate, String> {
    let arm_of = |v: &str| rows.iter().find(|r| r.routing.as_deref() == Some(v)).map(|r| r.arm.clone());
    let (Some(s), Some(d)) = (arm_of("static"), arm_of("discovered")) else {
        return Err("these rows hold no pair of routing arms (static and discovered); was this run started with --bench?".into());
    };
    let n = bench.runs;
    let mut per_case = Vec::new();
    let (mut s_all, mut d_all, mut short) = (Vec::new(), Vec::new(), Vec::new());
    for c in &bench.cases {
        let (sr, dr) = (graded(rows, c, &s, n), graded(rows, c, &d, n));
        for (arm, rs) in [(&s, &sr), (&d, &dr)] {
            if (rs.len() as u32) < n {
                short.push(format!("{c} [{arm}] {} of {n}", rs.len()));
            }
        }
        per_case.push((c.clone(), (passes(&sr), sr.len() as u32), (passes(&dr), dr.len() as u32)));
        s_all.extend(sr);
        d_all.extend(dr);
    }
    let complete = short.is_empty();
    let c1 = if complete { Holds::Yes(format!("{n} graded runs per arm per case")) } else { Holds::No(format!("too few graded runs (rerun with --resume): {}", short.join(", "))) };
    let incomplete = || Holds::Unknown("not judged until every case has its graded runs".into());
    let (sp, dp) = (passes(&s_all), passes(&d_all));
    let c2 = if !complete {
        incomplete()
    } else if dp + bench.margin >= sp {
        Holds::Yes(format!("D passed {dp}, S passed {sp} (D may be {} below)", bench.margin))
    } else {
        Holds::No(format!("D passed {dp}, S passed {sp}: more than {} below", bench.margin))
    };
    let lost: Vec<String> = per_case.iter().filter(|(_, (spass, _), (dpass, _))| *spass == n && *dpass + 2 <= n).map(|(c, (spass, _), (dpass, _))| format!("{c} (S {spass}/{n}, D {dpass}/{n})")).collect();
    let c3 = if !complete { incomplete() } else if lost.is_empty() { Holds::Yes("no case S passed every time and D passed at most once".into()) } else { Holds::No(format!("lost: {}", lost.join(", "))) };
    let c4 = if !complete {
        incomplete()
    } else {
        let tokens = cost_check("tokens", per_pass(&s_all, |r| r.gate.cost_tokens as f64), per_pass(&d_all, |r| r.gate.cost_tokens as f64), bench.cost_ratio);
        let any_premium = s_all.iter().chain(&d_all).any(|r| r.gate.premium_requests > 0.0);
        let premium = any_premium.then(|| cost_check("premium requests", per_pass(&s_all, |r| r.gate.premium_requests), per_pass(&d_all, |r| r.gate.premium_requests), bench.cost_ratio));
        match (tokens, premium) {
            (Holds::No(x), _) | (_, Some(Holds::No(x))) => Holds::No(x),
            (Holds::Unknown(x), _) | (_, Some(Holds::Unknown(x))) => Holds::Unknown(x),
            (Holds::Yes(x), Some(Holds::Yes(y))) => Holds::Yes(format!("{x}; {y}")),
            (Holds::Yes(x), _) => Holds::Yes(x),
        }
    };
    // Safety counts every D row, graded or not, first runs and reruns alike: a breach is a breach.
    let mut breaches = Vec::new();
    for r in rows.iter().filter(|r| r.arm == d) {
        breaches.extend(r.gate.below_floor.iter().map(|b| format!("below floor: {b}")));
        breaches.extend(r.gate.over_cap.iter().map(|b| format!("over cap: {b}")));
        if let Some(e) = r.error.as_deref().filter(|e| e.starts_with("arm staffing violated")) {
            breaches.push(format!("{} run {} not graded: {}", r.case, r.seed, e));
        }
    }
    let c5 = if breaches.is_empty() { Holds::Yes("no dispatch below its floor, no model over its cap, no run ungraded for an unexpected model".into()) } else { Holds::No(breaches.join("; ")) };
    let criteria = vec![("complete", c1), ("pass rate no worse", c2), ("no case lost", c3), ("no more than the cost ratio costlier", c4), ("safe", c5)];
    let verdict = if criteria[1..].iter().any(|(_, h)| matches!(h, Holds::No(_))) {
        "FAIL"
    } else if criteria.iter().all(|(_, h)| matches!(h, Holds::Yes(_))) {
        "PASS"
    } else {
        "INCOMPLETE"
    };
    Ok(Gate { contrast: contrast(rows, &s, &d, bench), s, d, criteria, verdict, per_case })
}

/// Pair D's dispatches with S's for the same case and run, role by role in order: a differing model, or none in S, differs.
fn contrast(rows: &[Row], s: &str, d: &str, bench: &Bench) -> Option<f64> {
    let (mut total, mut differ) = (0u32, 0u32);
    for c in &bench.cases {
        let (sr, dr) = (graded(rows, c, s, u32::MAX), graded(rows, c, d, u32::MAX));
        for drow in &dr {
            let Some(srow) = sr.iter().find(|x| x.seed == drow.seed) else { continue };
            let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
            for disp in &drow.gate.dispatches {
                let k = seen.entry(disp.role.as_str()).or_default();
                let twin = srow.gate.dispatches.iter().filter(|x| x.role == disp.role).nth(*k);
                *k += 1;
                total += 1;
                if twin.is_none_or(|t| (t.endpoint.as_str(), t.model.as_str()) != (disp.endpoint.as_str(), disp.model.as_str())) {
                    differ += 1;
                }
            }
        }
    }
    (total > 0).then(|| f64::from(differ) / f64::from(total))
}

/// The report: the five criteria, the verdict and the contrast, then what is reported beside them. Counts and ratios, no model output.
pub fn render(g: &Gate, rows: &[Row], bench: &Bench) -> String {
    let mut out = format!("ROUTING GATE ({}, `specs/056` §10.6): S = {} (static), D = {} (discovered)\n", bench.name, g.s, g.d);
    for (i, (name, h)) in g.criteria.iter().enumerate() {
        let (word, text) = match h {
            Holds::Yes(t) => ("holds", t),
            Holds::No(t) => ("FAILS", t),
            Holds::Unknown(t) => ("not judged", t),
        };
        out.push_str(&format!("  {}. {:<38} {:<11} {}\n", i + 1, name, word, text));
    }
    let contrast = match g.contrast {
        Some(c) => format!("{:.0}% of D's dispatches differ from S's{}", c * 100.0, if c < bench.low_contrast { " (LOW CONTRAST: a pass shows only that D does no harm where it rarely differs)" } else { "" }),
        None => "no dispatches to compare".into(),
    };
    out.push_str(&format!("VERDICT: {}\nCONTRAST: {}\n", g.verdict, contrast));
    out.push_str("A pass is necessary, not sufficient: the default stays static until the operator decides OD-5.\n\nBY CASE (passes / graded runs)\n");
    for (c, (sp, sn), (dp, dn)) in &g.per_case {
        out.push_str(&format!("  {:<36} S {}/{}   D {}/{}\n", c, sp, sn, dp, dn));
    }
    for arm in [&g.s, &g.d] {
        let rs: Vec<&Row> = rows.iter().filter(|r| &r.arm == arm).collect();
        let capped = rs.iter().filter(|r| r.gate.capped).count();
        let cost_capped: u32 = rs.iter().map(|r| r.gate.cost_capped).sum();
        let models: BTreeSet<String> = rs.iter().flat_map(|r| r.gate.dispatches.iter().map(|d| format!("{}/{}", d.endpoint, d.model))).collect();
        let served: BTreeSet<String> = rs.iter().flat_map(|r| r.seats.iter().filter_map(|s| s.served_model.clone())).collect();
        out.push_str(&format!("{}: {} run(s), {} stopped at the token cap, {} model.cost_capped event(s); models: {}; served as reported: {}\n", arm, rs.len(), capped, cost_capped,
            if models.is_empty() { "—".into() } else { models.into_iter().collect::<Vec<_>>().join(", ") }, if served.is_empty() { "—".into() } else { served.into_iter().collect::<Vec<_>>().join(", ") }));
    }
    out
}

/// `delphi train gate --work-dir <dir> --bench <benchmark.toml>`: the verdict of a run's rows. Exit 0 on PASS, 1 otherwise.
pub fn cli(args: &[String]) -> i32 {
    let flag = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let (Some(work), Some(bench)) = (flag("--work-dir").map(PathBuf::from), flag("--bench")) else {
        eprintln!("usage: delphi train gate --work-dir <dir of a run> --bench <benchmark.toml>");
        return 2;
    };
    let bench = match parse_bench(Path::new(&bench)) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("delphi train gate: {}", e);
            return 2;
        }
    };
    let rows = crate::train_run::read_rows(&work.join("rows.jsonl"));
    match evaluate(&rows, &bench) {
        Ok(g) => {
            print!("{}", render(&g, &rows, &bench));
            i32::from(g.verdict != "PASS")
        }
        Err(e) => {
            eprintln!("delphi train gate: {}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BENCH: &str = "name = \"t\"\nruns = 3\nmargin = 2\ncost_ratio = 1.25\nlow_contrast = 0.25\ncases = [\"code/a\", \"research/b\"]\n[token_cap]\ncode = 600000\nresearch = 400000\n";

    fn bench() -> Bench {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("b.toml");
        std::fs::write(&p, BENCH).unwrap();
        parse_bench(&p).unwrap()
    }

    fn row(case: &str, arm: &str, routing: &str, seed: u32, passed: Option<bool>, tokens: u64, model: &str) -> Row {
        let mut r = Row::from_json(&json!({"case": case, "arm": arm, "seed": seed, "passed": passed, "routing": routing})).unwrap();
        r.gate.cost_tokens = tokens;
        r.gate.dispatches = vec![Dispatch { role: "builder".into(), endpoint: "ep".into(), model: model.into() }];
        r
    }

    /// Both arms pass `s` and `d` of three runs of each case, at `st` and `dt` tokens a run.
    fn rows(s: [u32; 2], d: [u32; 2], st: u64, dt: u64, dmodel: &str) -> Vec<Row> {
        let mut out = Vec::new();
        for (i, case) in ["code/a", "research/b"].iter().enumerate() {
            for seed in 1..=3 {
                out.push(row(case, "s", "static", seed, Some(seed <= s[i]), st, "m1"));
                out.push(row(case, "d", "discovered", seed, Some(seed <= d[i]), dt, dmodel));
            }
        }
        out
    }

    fn verdict(rs: &[Row]) -> Gate {
        evaluate(rs, &bench()).unwrap()
    }

    #[test]
    fn the_benchmark_file_is_checked_and_its_ceiling_is_every_cap_once_per_run_per_arm() {
        let b = bench();
        assert_eq!((b.runs, b.margin, b.cap_of("code/a"), b.ceiling(2)), (3, 2, Some(600_000), (600_000 + 400_000) * 3 * 2));
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("b.toml");
        for (bad, want) in [
            (BENCH.replace("research = 400000\n", ""), "has no [token_cap] for its suite"),
            (BENCH.replace("runs = 3", "runs = 0"), "`runs` must be 1 to 20"),
            (BENCH.replace("cost_ratio = 1.25", "cost_ratio = 0.9"), "`cost_ratio` must be 1.0 or more"),
            (BENCH.replace("\"research/b\"", "\"code/a\""), "listed twice"),
            (BENCH.replace("\"code/a\"", "\"a\""), "`suite/id`"),
        ] {
            std::fs::write(&p, bad).unwrap();
            assert!(parse_bench(&p).unwrap_err().contains(want), "{}", want);
        }
    }

    #[test]
    fn the_pre_registered_benchmark_is_the_one_section_10_6_defines() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let b = parse_bench(&root.join("dev/train/m7/benchmark.toml")).unwrap();
        assert_eq!((b.cases.len(), b.runs, b.margin, b.cost_ratio, b.low_contrast), (12, 3, 2, 1.25, 0.25));
        assert_eq!((b.cap_of("code/x"), b.cap_of("research/x"), b.cap_of("review/x")), (Some(600_000), Some(400_000), Some(150_000)));
        let count = |s: &str| b.cases.iter().filter(|c| c.starts_with(&format!("{s}/"))).count();
        assert_eq!((count("code"), count("research"), count("review"), b.review_seeds.len()), (3, 5, 4, 4));
        assert_eq!(b.ceiling(2), 26_400_000, "the ceiling §10.6 states");
        // The code and research cases exist today; the review ones are generated from the registered seeds after the file is on main.
        for c in b.cases.iter().filter(|c| !c.starts_with("review/")) {
            let (suite, id) = c.split_once('/').unwrap();
            assert!(root.join("dev/train/suites").join(suite).join("cases").join(id).join("case.toml").is_file(), "{c}");
        }
        // The pre-registration copies §10.6 word for word.
        let spec = std::fs::read_to_string(root.join("specs/056-discovered-model-assignment.md")).unwrap();
        let start = spec.find("**When.** This definition").unwrap();
        let end = spec.find("### 10.7").unwrap();
        let prereg = std::fs::read_to_string(root.join("dev/train/m7/PREREG.md")).unwrap();
        assert!(prereg.contains(spec[start..end].trim()), "PREREG.md must hold §10.6 verbatim");
        for s in &b.review_seeds {
            assert!(prereg.contains(&format!("--seed {s} ")), "the generation command for seed {s} is registered");
        }
    }

    #[test]
    fn equal_arms_pass_and_the_report_prints_five_criteria_the_verdict_and_the_contrast() {
        let rs = rows([2, 1], [2, 1], 1000, 1000, "m1");
        let g = verdict(&rs);
        assert_eq!(g.verdict, "PASS", "{:?}", g.criteria);
        assert_eq!(g.contrast, Some(0.0));
        let text = render(&g, &rs, &bench());
        for want in ["1. complete", "2. pass rate no worse", "3. no case lost", "4. no more than the cost ratio costlier", "5. safe", "VERDICT: PASS", "CONTRAST: 0% of D's dispatches differ", "LOW CONTRAST", "S 2/3   D 2/3"] {
            assert!(text.contains(want), "{want} in\n{text}");
        }
        // A D that picks another model every time is full contrast.
        assert_eq!(verdict(&rows([2, 1], [2, 1], 1000, 1000, "m2")).contrast, Some(1.0));
    }

    #[test]
    fn each_criterion_fails_the_gate_on_its_own() {
        // 2: D three passes below S (5 vs 2), with no case lost 3/3 to 1/3.
        let g = verdict(&rows([3, 2], [2, 0], 1000, 1000, "m1"));
        assert!(matches!(g.criteria[1].1, Holds::No(_)) && g.verdict == "FAIL", "{:?}", g.criteria);
        // 3: within the margin overall, but one case lost from 3/3 to 1/3.
        let g = verdict(&rows([3, 0], [1, 2], 1000, 1000, "m1"));
        assert!(matches!(g.criteria[1].1, Holds::Yes(_)) && matches!(g.criteria[2].1, Holds::No(_)) && g.verdict == "FAIL", "{:?}", g.criteria);
        // 4: same passes, D 30% costlier per pass; 25% exactly holds.
        assert_eq!(verdict(&rows([2, 1], [2, 1], 1000, 1300, "m1")).verdict, "FAIL");
        assert_eq!(verdict(&rows([2, 1], [2, 1], 1000, 1250, "m1")).verdict, "PASS");
        // 4 on premium requests, when either arm used any.
        let mut rs = rows([2, 1], [2, 1], 1000, 1000, "m1");
        rs.iter_mut().for_each(|r| r.gate.premium_requests = if r.arm == "d" { 2.0 } else { 1.0 });
        assert!(matches!(&verdict(&rs).criteria[3].1, Holds::No(x) if x.contains("premium requests")));
        // 5: a dispatch below its floor, a model over its cap, a run ungraded because an unexpected model served.
        for breach in 0..3 {
            let mut rs = rows([2, 1], [2, 1], 1000, 1000, "m1");
            let r = rs.iter_mut().find(|r| r.arm == "d").unwrap();
            match breach {
                0 => r.gate.below_floor.push("strategist on ep/m: class quick below floor deep".into()),
                1 => r.gate.over_cap.push("builder on ep/m: it costs premium".into()),
                _ => {
                    let mut extra = r.clone();
                    extra.passed = None;
                    extra.error = Some("arm staffing violated: used x but `d` expects m1".into());
                    rs.push(extra);
                }
            }
            let g = verdict(&rs);
            assert!(matches!(g.criteria[4].1, Holds::No(_)) && g.verdict == "FAIL", "breach {breach}: {:?}", g.criteria);
        }
    }

    #[test]
    fn missing_runs_are_incomplete_a_cap_stop_counts_as_not_passed_and_a_rerun_completes_it() {
        let mut rs = rows([2, 1], [2, 1], 1000, 1000, "m1");
        // One D run was not graded (quota): INCOMPLETE, and 2 to 4 are not judged.
        let i = rs.iter().position(|r| r.arm == "d" && r.seed == 3 && r.case == "code/a").unwrap();
        rs[i].passed = None;
        let g = verdict(&rs);
        assert_eq!(g.verdict, "INCOMPLETE");
        assert!(matches!(&g.criteria[0].1, Holds::No(x) if x.contains("code/a [d] 2 of 3")) && matches!(g.criteria[1].1, Holds::Unknown(_)));
        // `--resume` adds the missing run; it completes the gate.
        rs.push(row("code/a", "d", "discovered", 3, Some(false), 1000, "m1"));
        assert_eq!(verdict(&rs).verdict, "PASS");
        // A run stopped at its token cap is graded and not passed: it is a lost run in both arms alike.
        let mut capped = row("code/a", "d", "discovered", 1, Some(false), 700_000, "m1");
        capped.gate.capped = true;
        let mut rs = rows([2, 1], [2, 1], 1000, 1000, "m1");
        rs.retain(|r| !(r.arm == "d" && r.seed == 1 && r.case == "code/a"));
        rs.push(capped);
        let g = verdict(&rs);
        assert_eq!(g.per_case[0].2, (1, 3), "D passes 1 of 3 of code/a now");
        // S passed nothing: a cost per pass has no base, so the gate cannot pass, and says why.
        let g = verdict(&rows([0, 0], [1, 0], 1000, 1000, "m1"));
        assert!(matches!(&g.criteria[3].1, Holds::Unknown(x) if x.contains("S passed no run")) && g.verdict == "INCOMPLETE", "{:?}", g.criteria);
    }

    fn arm(name: &str, routing: Option<&str>, home: &Path) -> Arm {
        Arm { routing: routing.map(String::from), template_home: Some(home.to_path_buf()), expect_models: vec!["m1".into()], ..Arm::new(name, None) }
    }

    #[test]
    fn two_routing_arms_that_differ_in_anything_but_assignment_are_refused() {
        let t = tempfile::tempdir().unwrap();
        let (h1, h2) = (t.path().join("h1"), t.path().join("h2"));
        std::fs::create_dir_all(&h1).unwrap();
        std::fs::create_dir_all(&h2).unwrap();
        let ok = || vec![arm("s", Some("static"), &h1), arm("d", Some("discovered"), &h1)];
        assert_eq!(routing_pair(&ok()).unwrap(), ("s".into(), "d".into()));
        let mut cases: Vec<(Vec<Arm>, &str)> = Vec::new();
        let mut a = ok();
        a[1].template_home = Some(h2.clone());
        cases.push((a, "different template homes"));
        let mut a = ok();
        a[1].expect_models.push("m2".into());
        cases.push((a, "same, non-empty `expect_models`"));
        let mut a = ok();
        a[1].overlay = Some(t.path().join("o"));
        cases.push((a, "different overlays"));
        let mut a = ok();
        a[1].routing = Some("static".into());
        cases.push((a, "exactly two arms"));
        let mut a = ok();
        a.push(arm("x", Some("discovered"), &h1));
        cases.push((a, "exactly two arms"));
        let mut a = ok();
        a[1].loop_switches.insert("nudge".into(), toml::Value::Boolean(false));
        cases.push((a, "no [loop] switches"));
        for (a, want) in cases {
            assert!(routing_pair(&a).unwrap_err().contains(want), "{want}");
        }
    }

    #[test]
    fn routing_is_set_in_the_runs_copy_and_the_pinned_cap_is_left_alone() {
        let out = set_routing("[workspace]\nprojects = [\"/x\"]\n[routing]\nassignment = \"static\"\nmax_cost_class = \"premium\"\n", "discovered").unwrap();
        let v: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!((v["routing"]["assignment"].as_str(), v["routing"]["max_cost_class"].as_str()), (Some("discovered"), Some("premium")));
        assert!(toml::from_str::<toml::Value>(&set_routing("[workspace]\nprojects = []\n", "static").unwrap()).unwrap()["routing"]["assignment"].as_str() == Some("static"));
    }

    #[test]
    fn the_yes_is_typed_at_a_terminal_and_nothing_else_counts() {
        let ask = |terminal: bool, typed: &str| {
            let mut out = Vec::new();
            (confirm(terminal, &mut typed.as_bytes(), &mut out), String::from_utf8(out).unwrap())
        };
        assert!(ask(true, "yes\n").0);
        assert!(!ask(true, "y\n").0 && !ask(true, "").0);
        let (ok, said) = ask(false, "yes\n");
        assert!(!ok && said.contains("typed at a terminal"), "a piped yes is not the operator's");
    }

    #[test]
    fn the_facts_of_a_run_are_read_from_its_log() {
        let ev = |kind: &str, agent: &str, span: &str, model: &str, p: Value| {
            let mut e = EventEnvelope::new("m", kind, p);
            (e.task_id, e.agent, e.span_id, e.endpoint_id, e.model_id) = (Some("t".into()), Some(agent.into()), Some(span.into()), Some("ep".into()), Some(model.into()));
            e
        };
        let usage = |i: u64| json!({"input_tokens": i, "cache_read_tokens": 10, "cache_write_tokens": 5, "output_tokens": 20, "thinking_tokens": 999});
        let events = vec![
            ev("model.assigned", "strategist", "sp1", "big", json!({"endpoint_id": "ep", "model_id": "big", "class": "deep", "floor": "deep"})),
            ev("agent.usage", "strategist", "sp1", "big", usage(100)),
            ev("agent.usage", "strategist", "sp1", "big", usage(100)),
            ev("model.assigned", "red-team", "sp2", "small", json!({"endpoint_id": "ep", "model_id": "small", "class": "quick", "floor": "steady"})),
            ev("agent.usage", "red-team", "sp2", "small", usage(1)),
            ev("harness.cost", "red-team", "sp2", "small", json!({"premium_requests": 0.33})),
            ev("harness.cost", "red-team", "sp2", "small", json!({"premium_requests": 1})),
            ev("model.cost_capped", "red-team", "sp2", "small", json!({"outcome": "served"})),
        ];
        let f = facts_of(&events, None);
        assert_eq!(f.cost_tokens, 2 * (100 + 10 + 5 + 20) + (1 + 10 + 5 + 20), "input, cache read and write, output; thinking is not in the §10.3 unit");
        assert!((f.premium_requests - 1.33).abs() < 1e-9);
        assert_eq!(f.dispatches.iter().map(|d| (d.role.as_str(), d.model.as_str())).collect::<Vec<_>>(), vec![("strategist", "big"), ("red-team", "small")], "one per span");
        assert_eq!((f.below_floor.len(), f.cost_capped), (1, 1));
        assert!(f.below_floor[0].contains("red-team on ep/small: class quick below floor steady"));
        assert_eq!(Facts::from_json(&f.to_json()), f);
    }
}
