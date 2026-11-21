//! The candidate inventory (`specs/056` §2.1, slice M0): every `(endpoint, model)` the access paths report, with the
//! attributes they report, and a class.
//!
//! A candidate is read from two places, and nothing here calls a model: the configured pool (an endpoint's tier mapping and
//! the catalog's context window) and the CLI model lists in the capabilities snapshot (`delphi capabilities`, specs 038,
//! 046, 047, 051). A model that was listed before and is not now is kept and marked `gone`; it is never selected.

use crate::hostmap::Snapshot;
use delphi_adapter::config::Config;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE: &str = "local/model-candidates.json";

/// A capability class (`context/model-selection-evidence.md` §2). `Unclassified` is what no rule has placed yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Deep,
    Steady,
    Quick,
    Local,
    /// Below every role floor (`specs/056` §3.5). Only configuration puts a model here (`[routing] emergency_*`); no rule can.
    /// Never eligible for dispatch; it saves state when every ratified seat is out of quota.
    Emergency,
    #[default]
    Unclassified,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Deep => "deep",
            Class::Steady => "steady",
            Class::Quick => "quick",
            Class::Local => "local",
            Class::Emergency => "emergency",
            Class::Unclassified => "unclassified",
        }
    }
    pub fn parse(s: &str) -> Option<Class> {
        Some(match s.trim() {
            "deep" => Class::Deep,
            "steady" => Class::Steady,
            "quick" => Class::Quick,
            "local" => Class::Local,
            "emergency" => Class::Emergency,
            "unclassified" => Class::Unclassified,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Presence {
    #[default]
    Ready,
    /// Listed before, not listed now. Kept for the record; never selected.
    Gone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub endpoint_id: String,
    pub model_id: String,
    /// Empty when only a CLI list reported the model.
    pub family: String,
    /// The tier the endpoint maps this model to (`fast`, `workhorse`, `frontier`), when it does.
    pub tier: String,
    /// Context window in tokens; 0 when not reported.
    pub context_tokens: u64,
    /// The catalog allows a thinking budget.
    pub can_think: bool,
    /// `metered`, `subscription` or `local`.
    pub cost_class: String,
    pub local: bool,
    /// `pool` (configured) or `cli:<name>` (a CLI's own list).
    pub source: String,
    /// Last time a source reported it.
    pub seen_unix: i64,
    #[serde(default)]
    pub presence: Presence,
    #[serde(default)]
    pub class: Class,
    /// Also far wider than its family's median prompt budget, with a retrieval record to show it (`specs/056` §10.2).
    #[serde(default)]
    pub wide: bool,
    /// What was measured of this model on its carrier (`specs/056` §10.2). Empty when nothing was.
    #[serde(default)]
    pub measured: Measured,
}

/// The measured attributes of a candidate (`specs/056` §10.2, slice M1a). Each is read from a record, never estimated by a model,
/// and an empty one is never read as a pass. None of them is a cost: the cost class stays a cost input only (§10.3).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Measured {
    /// The largest prompt, in tokens, the carrier accepts for this model after reserving output; 0 when there is no record.
    #[serde(default)]
    pub prompt_budget_tokens: u64,
    /// The JSON-in-text record: `bare` or `fenced` (every sample valid), `nested-string`, `untested`, or empty (no record).
    #[serde(default)]
    pub json_in_text: String,
    /// The fraction of the prompt budget at which every planted string was recovered; `None` when there is no record.
    #[serde(default)]
    pub recall_fraction: Option<f64>,
    /// `Some(false)`: the carrier reported serving another model than the one asked for.
    #[serde(default)]
    pub served_match: Option<bool>,
    /// Where the carrier puts the prompt's tokens: `input`, `cache_write` or `both`; empty when there is no record.
    #[serde(default)]
    pub token_accounting: String,
    /// When, on which carrier and version, and by what the record was taken.
    #[serde(default)]
    pub measured_on: String,
    #[serde(default)]
    pub carrier: String,
    #[serde(default)]
    pub carrier_version: String,
    #[serde(default)]
    pub command: String,
}

impl Measured {
    /// Every sample of the structured-answer check came back valid.
    pub fn json_valid(&self) -> bool {
        matches!(self.json_in_text.as_str(), "bare" | "fenced")
    }

    /// The seed table's row for `model` on `carrier` (`specs/132` C3), or nothing.
    pub fn from_seed(seed: &crate::model_catalog::Seed, carrier: &str, model: &str) -> Measured {
        let Some(m) = seed.model(carrier, model) else { return Measured::default() };
        let version = match carrier {
            "copilot" => seed.copilot_cli.clone(),
            "agy" => seed.agy_cli.clone(),
            _ => String::new(),
        };
        Measured {
            prompt_budget_tokens: m.prompt_budget_tokens,
            json_in_text: m.json_in_text.clone(),
            // Until the probe plants strings at several depths, recall is the tail delivered at the size tested (§10.2).
            recall_fraction: (m.tail_tested_tokens > 0 && m.prompt_budget_tokens > 0).then(|| m.tail_tested_tokens as f64 / m.prompt_budget_tokens as f64),
            served_match: None,
            token_accounting: m.token_accounting.clone(),
            measured_on: seed.measured.clone(),
            carrier: carrier.into(),
            carrier_version: version,
            command: "seed table (specs/132 C3): the carrier's own over-budget refusal and one tail-delivery call".into(),
        }
    }
}

impl Candidate {
    pub fn is_ready(&self) -> bool {
        self.presence == Presence::Ready
    }
}

/// The carrier whose records describe models on this endpoint: a harness endpoint's CLI, or a CLI list's own name.
fn carrier_of(cfg: &Config, endpoint_id: &str) -> Option<String> {
    if let Some(cli) = endpoint_id.strip_prefix("cli:") {
        return Some(cli.to_string());
    }
    cfg.endpoint(endpoint_id).filter(|e| e.path == delphi_adapter::config::AccessPath::Harness).and_then(|e| e.harness.clone())
}

/// Put the measured attributes on every candidate from the shipped seed table (`specs/056` §10.2). A model with no row keeps them
/// empty. A served-model match learned earlier is kept.
pub fn measure(cfg: &Config, list: &mut [Candidate]) {
    let seed = crate::model_catalog::Seed::shipped();
    for c in list.iter_mut() {
        let served = c.measured.served_match;
        c.measured = carrier_of(cfg, &c.endpoint_id).map(|carrier| Measured::from_seed(&seed, &carrier, &c.model_id)).unwrap_or_default();
        c.measured.served_match = served;
    }
}

/// Everything the sources report right now.
pub fn discover(cfg: &Config, snap: Option<&Snapshot>, now: i64) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for k in cfg.pool_entries() {
        let Some(ep) = cfg.endpoint(&k.endpoint_id) else { continue };
        let cat = cfg.catalog_entry(&k.model_id);
        out.push(Candidate {
            cost_class: if ep.is_local() { "local" } else if ep.is_metered_by_subscription() { "subscription" } else { "metered" }.into(),
            local: ep.is_local(),
            context_tokens: cat.map(|c| c.context_tokens).unwrap_or(0),
            can_think: cat.is_some_and(|c| c.max_thinking_budget > 0),
            endpoint_id: k.endpoint_id,
            model_id: k.model_id,
            family: k.family,
            tier: k.tier,
            source: "pool".into(),
            seen_unix: now,
            presence: Presence::Ready,
            class: Class::Unclassified,
            wide: false,
            measured: Measured::default(),
        });
    }
    for c in snap.iter().flat_map(|s| s.clis.iter()).filter(|c| c.installed) {
        for m in &c.models {
            if out.iter().any(|o| o.model_id == m.name) {
                continue;
            }
            out.push(Candidate {
                endpoint_id: format!("cli:{}", c.cli),
                model_id: m.name.clone(),
                family: String::new(),
                tier: String::new(),
                context_tokens: 0,
                can_think: false,
                cost_class: if c.cli == "ollama" { "local" } else { "subscription" }.into(),
                local: c.cli == "ollama",
                source: format!("cli:{}", c.cli),
                seen_unix: now,
                presence: Presence::Ready,
                class: Class::Unclassified,
                wide: false,
                measured: Measured::default(),
            });
        }
    }
    measure(cfg, &mut out);
    out
}

/// Fold a fresh discovery into the earlier inventory: seen ones are refreshed, ones not seen now become `gone`.
pub fn merge(previous: Vec<Candidate>, seen: Vec<Candidate>) -> Vec<Candidate> {
    let mut out = seen;
    // A served-model record comes from receipts, not from a source that lists models: keep it across a refresh.
    for c in out.iter_mut().filter(|c| c.measured.served_match.is_none()) {
        c.measured.served_match = previous.iter().find(|p| p.endpoint_id == c.endpoint_id && p.model_id == c.model_id).and_then(|p| p.measured.served_match);
    }
    for p in previous {
        if !out.iter().any(|c| c.endpoint_id == p.endpoint_id && c.model_id == p.model_id) {
            out.push(Candidate { presence: Presence::Gone, ..p });
        }
    }
    out
}

pub fn load(ws_dir: &Path) -> Vec<Candidate> {
    std::fs::read_to_string(ws_dir.join(FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save(ws_dir: &Path, list: &[Candidate]) -> std::io::Result<()> {
    let path = ws_dir.join(FILE);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(list).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, path)
}

pub fn render(list: &[Candidate]) -> String {
    if list.is_empty() {
        return "no models discovered (configure an endpoint, or run `delphi capabilities --refresh`)\n".into();
    }
    let mut o = format!("{:<18} {:<28} {:<10} {:<10} {:>9} {:<13} {:<12} {}\n", "endpoint", "model", "family", "tier", "context", "cost", "class", "state");
    for c in list {
        let ctx = if c.context_tokens == 0 { "-".to_string() } else { format!("{}k", c.context_tokens / 1000) };
        let dash = |s: &str| if s.is_empty() { "-".to_string() } else { s.to_string() };
        let class = format!("{}{}", c.class.as_str(), if c.wide { "+wide" } else { "" });
        o.push_str(&format!("{:<18} {:<28} {:<10} {:<10} {:>9} {:<13} {:<12} {}\n", c.endpoint_id, c.model_id, dash(&c.family), dash(&c.tier), ctx, c.cost_class, class, if c.is_ready() { "ready" } else { "gone" }));
    }
    o
}

/// `delphi capabilities --models`: discover, fold into the saved inventory, save, print.
pub fn run_inventory(cfg: &Config, ws_dir: &Path, snap: Option<&Snapshot>, now: i64, json: bool) -> std::io::Result<String> {
    let rules = ClassRules::load(ws_dir).map_err(std::io::Error::other)?;
    let mut list = merge(load(ws_dir), discover(cfg, snap, now));
    classify(&rules, &mut list);
    designate_emergency(cfg, &mut list);
    list.sort_by(|a, b| (a.presence != Presence::Ready, &a.endpoint_id, &a.model_id).cmp(&(b.presence != Presence::Ready, &b.endpoint_id, &b.model_id)));
    save(ws_dir, &list)?;
    Ok(if json { serde_json::to_string_pretty(&list).unwrap_or_default() + "\n" } else { format!("{}\n{}", crate::emergency::line(cfg), render(&list)) })
}

/// The shipped rules (`model_classes.toml`).
pub const SHIPPED_RULES: &str = include_str!("model_classes.toml");
/// A workspace's own ratified replacement, beside its `config.toml`.
pub const RULES_OVERRIDE: &str = "model-classes.toml";

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Rule {
    pub id: String,
    pub class: String,
    pub local: Option<bool>,
    pub tier: Option<String>,
    pub can_think: Option<bool>,
    pub min_context: Option<u64>,
}

impl Rule {
    fn holds(&self, c: &Candidate) -> bool {
        self.local.is_none_or(|v| v == c.local) && self.tier.as_deref().is_none_or(|t| t == c.tier) && self.can_think.is_none_or(|v| v == c.can_think) && self.min_context.is_none_or(|n| c.context_tokens >= n)
    }
}

/// A rule key that names a cost (`specs/056` §10.3): price never sets class, so such a rule is refused, not ignored.
fn is_cost_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    ["cost", "price", "multiplier", "usd", "premium", "billing"].iter().any(|w| k.contains(w))
}

/// The retrieval record a model needs to be `wide`: every planted string found at this fraction of its budget or more (§10.2).
pub const WIDE_RECALL_FRACTION: f64 = 0.8;

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
struct WideRule {
    min_context_ratio: Option<f64>,
}

/// What a role needs (`specs/056` §3.1).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RoleRule {
    pub floor: String,
    pub preferred: String,
    #[serde(default)]
    pub plan_making: bool,
    #[serde(default)]
    pub plan_following: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ClassRules {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    wide: WideRule,
    #[serde(default, rename = "rule")]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub roles: std::collections::BTreeMap<String, RoleRule>,
}

impl ClassRules {
    pub fn parse(text: &str) -> Result<ClassRules, String> {
        let raw: toml::Value = toml::from_str(text).map_err(|e| format!("class rules: {e}"))?;
        for rule in raw.get("rule").and_then(|r| r.as_array()).into_iter().flatten() {
            let id = rule.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            if let Some(key) = rule.as_table().into_iter().flat_map(|t| t.keys()).find(|k| is_cost_key(k)) {
                return Err(format!("class rules: rule `{id}` conditions on `{key}`, a cost; a price, multiplier or cost class never sets a class (specs/056 §10.3), it is a cost input to ranking only. Use measured or capability attributes (local, tier, can_think, min_context)"));
            }
        }
        let r: ClassRules = raw.try_into().map_err(|e| format!("class rules: {e}"))?;
        for rule in &r.rules {
            match Class::parse(&rule.class) {
                Some(Class::Unclassified | Class::Emergency) | None => return Err(format!("class rules: rule `{}` has class `{}`; use deep, steady, quick or local (the emergency model is set in config.toml, not by a rule)", rule.id, rule.class)),
                Some(_) => {}
            }
        }
        for (id, role) in &r.roles {
            for (what, v) in [("floor", &role.floor), ("preferred", &role.preferred)] {
                if !matches!(Class::parse(v), Some(Class::Deep | Class::Steady | Class::Quick | Class::Local)) {
                    return Err(format!("class rules: role `{id}` has {what} `{v}`; use deep, steady, quick or local"));
                }
            }
        }
        Ok(r)
    }

    pub fn shipped() -> ClassRules {
        ClassRules::parse(SHIPPED_RULES).expect("the shipped class rules parse (a test pins this)")
    }

    /// The workspace's replacement if it has one, else the shipped rules.
    pub fn load(ws_dir: &Path) -> Result<ClassRules, String> {
        match std::fs::read_to_string(ws_dir.join(RULES_OVERRIDE)) {
            Ok(t) => ClassRules::parse(&t),
            Err(_) => Ok(ClassRules::shipped()),
        }
    }

    /// Place one model, by attributes alone.
    pub fn class_of(&self, c: &Candidate) -> Class {
        self.rules.iter().find(|r| r.holds(c)).and_then(|r| Class::parse(&r.class)).unwrap_or(Class::Unclassified)
    }
}

/// Set `class` and `wide` on every candidate. A model no rule places stays `unclassified`.
///
/// The quality classes (`deep`, `steady`, `quick`) come from the rules, which read the operator's tier as the prior; no measured
/// attribute moves them, and no cost does (§10.2, §10.3). `wide` needs a measured prompt budget at least `[wide] min_context_ratio`
/// times its family's median and a retrieval record at [`WIDE_RECALL_FRACTION`] or more. A carrier that reported serving another
/// model makes the candidate's family `unknown` and its class `unclassified`.
pub fn classify(rules: &ClassRules, list: &mut [Candidate]) {
    for c in list.iter_mut().filter(|c| c.measured.served_match == Some(false)) {
        c.family = crate::seat::UNKNOWN.into();
    }
    let ratio = rules.wide.min_context_ratio.unwrap_or(0.0);
    let medians: Vec<(String, u64)> = {
        let mut fams: Vec<String> = list.iter().filter(|c| !c.family.is_empty() && c.family != crate::seat::UNKNOWN).map(|c| c.family.clone()).collect();
        fams.sort();
        fams.dedup();
        fams.into_iter()
            .filter_map(|f| {
                let mut v: Vec<u64> = list.iter().filter(|c| c.family == f && c.is_ready() && c.measured.prompt_budget_tokens > 0).map(|c| c.measured.prompt_budget_tokens).collect();
                v.sort_unstable();
                (v.len() >= 3).then(|| (f, v[v.len() / 2]))
            })
            .collect()
    };
    for c in list.iter_mut() {
        if c.measured.served_match == Some(false) {
            c.class = Class::Unclassified;
            c.wide = false;
            continue;
        }
        c.class = rules.class_of(c);
        let budget = c.measured.prompt_budget_tokens;
        let recalled = c.measured.recall_fraction.is_some_and(|f| f >= WIDE_RECALL_FRACTION);
        c.wide = ratio > 0.0 && recalled && budget > 0 && medians.iter().find(|(f, _)| *f == c.family).is_some_and(|(_, m)| budget as f64 >= ratio * *m as f64);
    }
}

/// Why the measured attributes rule this candidate out of one dispatch under `discovered` (`specs/056` §10.2 use 1), or `None`.
/// `prompt_tokens` is what the request needs (0: not known); `needs_json` is a seat that must answer a schema. An empty record is
/// never a pass. Spec 131's margin is not built, so the budget is compared as measured.
pub fn measured_ineligible(m: &Measured, prompt_tokens: u64, needs_json: bool) -> Option<String> {
    if prompt_tokens > 0 && m.prompt_budget_tokens > 0 && prompt_tokens > m.prompt_budget_tokens {
        return Some(format!("its measured prompt budget ({} tokens) is smaller than the {} needed", m.prompt_budget_tokens, prompt_tokens));
    }
    if needs_json && !m.json_valid() {
        let rec = if m.json_in_text.is_empty() { "no record".to_string() } else { format!("`{}`", m.json_in_text) };
        return Some(format!("this seat must answer a schema and its JSON-in-text record is {rec}, not every sample valid"));
    }
    None
}

/// Mark the configured emergency model (`[routing] emergency_*`, `specs/056` §3.5). Call after [`classify`]; it overrides the rules,
/// so an emergency model is never also `local` or `quick` and therefore never in the eligible set.
pub fn designate_emergency(cfg: &Config, list: &mut [Candidate]) {
    for c in list.iter_mut().filter(|c| crate::emergency::is_emergency(cfg, &c.endpoint_id, &c.model_id)) {
        c.class = Class::Emergency;
        c.wide = false;
    }
}

/// The ready candidates no rule placed (`delphi doctor` warns on these).
pub fn unclassified(list: &[Candidate]) -> Vec<&Candidate> {
    list.iter().filter(|c| c.is_ready() && c.class == Class::Unclassified).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hostmap::{CliEntry, Item};

    const CFG: &str = r#"
[models.alpha]
fast = "a-small"
workhorse = "a-mid"
frontier = "a-top"
[models.catalog."a-small"]
max_output_tokens = 8000
context_tokens = 100000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 5.0
[models.catalog."a-mid"]
max_output_tokens = 8000
context_tokens = 200000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 5.0
[models.catalog."a-top"]
max_output_tokens = 8000
context_tokens = 200000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 5.0
max_thinking_budget = 32000
[[endpoints]]
id = "api"
family = "alpha"
path = "api"
credential = "env:K"
"#;

    fn snap(models: &[&str]) -> Snapshot {
        Snapshot { clis: vec![CliEntry { cli: "codex".into(), installed: true, models: models.iter().map(|m| Item { name: (*m).into(), detail: String::new(), source: "cli".into() }).collect(), ..Default::default() }], ..Default::default() }
    }

    #[test]
    fn pool_and_cli_lists_both_become_candidates_and_all_start_unclassified() {
        let cfg = Config::parse(CFG).unwrap();
        let l = discover(&cfg, Some(&snap(&["x-one"])), 10);
        assert_eq!(l.len(), 4);
        let top = l.iter().find(|c| c.model_id == "a-top").unwrap();
        assert_eq!((top.tier.as_str(), top.context_tokens, top.can_think, top.cost_class.as_str()), ("frontier", 200000, true, "metered"));
        let cli = l.iter().find(|c| c.model_id == "x-one").unwrap();
        assert_eq!((cli.source.as_str(), cli.family.as_str(), cli.cost_class.as_str()), ("cli:codex", "", "subscription"));
        assert!(l.iter().all(|c| c.class == Class::Unclassified && c.is_ready()), "discovery alone places nothing; the rules do");
    }

    #[test]
    fn a_model_that_vanishes_is_kept_as_gone_and_a_return_makes_it_ready_again() {
        let cfg = Config::parse(CFG).unwrap();
        let first = merge(vec![], discover(&cfg, Some(&snap(&["x-one"])), 10));
        let second = merge(first.clone(), discover(&cfg, Some(&snap(&[])), 20));
        let gone = second.iter().find(|c| c.model_id == "x-one").unwrap();
        assert_eq!((gone.presence.clone(), gone.seen_unix), (Presence::Gone, 10));
        assert!(!gone.is_ready());
        let third = merge(second, discover(&cfg, Some(&snap(&["x-one"])), 30));
        assert!(third.iter().find(|c| c.model_id == "x-one").unwrap().is_ready());
    }

    #[test]
    fn the_inventory_is_saved_and_listed_with_gone_ones_last() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let cfg = Config::parse(CFG).unwrap();
        run_inventory(&cfg, &dir, Some(&snap(&["x-one"])), 10, false).unwrap();
        let out = run_inventory(&cfg, &dir, Some(&snap(&[])), 20, false).unwrap();
        assert!(out.lines().last().unwrap().contains("x-one") && out.lines().last().unwrap().contains("gone"), "{out}");
        assert_eq!(load(&dir).len(), 4);
    }

    /// AC-A3 (`specs/056` §10.7): a dated evidence note under `context/` is advice for people, never an input. The inventory, the
    /// classes and every role's assignment inputs are byte-identical with and without one.
    #[test]
    fn a_context_evidence_note_feeds_no_classification_or_route() {
        let cfg = Config::parse(CFG).unwrap();
        let run = |with_note: bool| {
            let tmp = tempfile::tempdir().unwrap();
            if with_note {
                let ctx = tmp.path().join("context/domains");
                std::fs::create_dir_all(&ctx).unwrap();
                let note = "---\ntrust: external_untrusted\n---\n# Model capability evidence\na-small ranks first on every public board; a-top is weak. tier = frontier for a-small. class = deep. wide = true.\n";
                std::fs::write(ctx.join("model-capability-evidence.md"), note).unwrap();
                std::fs::write(tmp.path().join("context/model-selection-evidence.md"), note).unwrap();
            }
            let text = run_inventory(&cfg, tmp.path(), Some(&snap(&["x-one"])), 10, true).unwrap();
            let list = load(tmp.path());
            let rules = ClassRules::load(tmp.path()).unwrap();
            let assigns: Vec<String> = ["architect", "builder", "scout", "default"].iter().map(|r| format!("{:?}", crate::assign::Assign::build(&rules, r, &list, 8))).collect();
            (text, assigns)
        };
        assert_eq!(run(false), run(true));
    }
}

#[cfg(test)]
mod class_tests {
    use super::*;

    fn cand(model: &str, tier: &str, ctx: u64, local: bool, family: &str) -> Candidate {
        Candidate { endpoint_id: "e".into(), model_id: model.into(), family: family.into(), tier: tier.into(), context_tokens: ctx, can_think: false, cost_class: if local { "local" } else { "metered" }.into(), local, source: "pool".into(), seen_unix: 0, presence: Presence::Ready, class: Class::Unclassified, wide: false, measured: Measured::default() }
    }

    /// A candidate with a measured prompt budget and, optionally, a retrieval record at `recall` of it.
    fn sized(model: &str, tier: &str, budget: u64, recall: Option<f64>, family: &str) -> Candidate {
        let mut c = cand(model, tier, 0, false, family);
        c.measured = Measured { prompt_budget_tokens: budget, recall_fraction: recall, json_in_text: "bare".into(), ..Default::default() };
        c
    }

    #[test]
    fn three_invented_seats_are_classified_by_the_rules_alone_and_an_unplaced_id_stays_unclassified() {
        let mut l = vec![cand("zz-1", "frontier", 200_000, false, "f"), cand("zz-2", "workhorse", 200_000, false, "f"), cand("zz-3", "fast", 100_000, false, "f"), cand("zz-4", "", 0, false, ""), cand("zz-5", "frontier", 8_000, true, "g")];
        classify(&ClassRules::shipped(), &mut l);
        let got: Vec<Class> = l.iter().map(|c| c.class).collect();
        assert_eq!(got, [Class::Deep, Class::Steady, Class::Quick, Class::Unclassified, Class::Local]);
        assert_eq!(unclassified(&l).len(), 1);
    }

    /// AC-A1: a 600k budget with a retrieval record at 0.8 is `wide`; the same budget with no record, or a declared window alone, is not.
    #[test]
    fn wide_needs_a_measured_budget_and_a_retrieval_record_not_a_declared_window() {
        let mut l: Vec<Candidate> = ["a", "b", "c", "i", "j"].iter().map(|m| sized(m, "workhorse", 200_000, Some(0.9), "f")).collect();
        l.extend([sized("d", "frontier", 600_000, Some(0.8), "f"), sized("e", "frontier", 600_000, None, "f"), sized("g", "frontier", 600_000, Some(0.5), "f"), cand("h", "frontier", 5_000_000, false, "f"), sized("s", "frontier", 600_000, Some(1.0), "solo")]);
        classify(&ClassRules::shipped(), &mut l);
        assert_eq!(l.iter().map(|c| c.wide).collect::<Vec<_>>(), [false, false, false, false, false, true, false, false, false, false], "a family of fewer than three has no median");
        assert!(l.iter().all(|c| c.class == Class::Deep || c.tier != "frontier"), "wide is an extra mark; the class stays the tier prior");
    }

    /// AC-A1: a carrier that served another model makes the family `unknown` and the model `unclassified`.
    #[test]
    fn a_served_model_mismatch_makes_the_family_unknown_and_the_model_unclassified() {
        let mut l = vec![sized("a", "frontier", 600_000, Some(1.0), "f"), sized("b", "frontier", 200_000, Some(1.0), "f")];
        l[0].measured.served_match = Some(false);
        l[1].measured.served_match = Some(true);
        classify(&ClassRules::shipped(), &mut l);
        assert_eq!((l[0].family.as_str(), l[0].class, l[0].wide), (crate::seat::UNKNOWN, Class::Unclassified, false));
        assert_eq!((l[1].family.as_str(), l[1].class), ("f", Class::Deep));
    }

    /// AC-A2: a rule naming a cost fails to parse with a sentence, and no cost attribute moves a class.
    #[test]
    fn a_cost_condition_is_refused_and_no_cost_value_changes_a_class() {
        for key in ["cost_class = \"subscription\"", "multiplier = 1.0", "price_category = \"low\"", "max_input_usd_per_mtok = 3.0", "premium = false"] {
            let e = ClassRules::parse(&format!("[[rule]]\nid = \"cheap\"\n{key}\nclass = \"deep\"\n")).unwrap_err();
            assert!(e.contains("rule `cheap`") && e.contains("never sets a class"), "{key}: {e}");
        }
        let rules = ClassRules::parse("[[rule]]\nid = \"big\"\nmin_context = 500000\nclass = \"deep\"\n").unwrap();
        assert_eq!(rules.class_of(&cand("zz", "", 600_000, false, "")), Class::Deep, "a new vendor is a row of attributes");
        let base = || vec![sized("p", "frontier", 600_000, Some(0.9), "f"), sized("q", "workhorse", 200_000, Some(0.9), "f"), sized("r", "fast", 100_000, None, "f"), cand("t", "", 0, false, "")];
        let mut want = base();
        classify(&ClassRules::shipped(), &mut want);
        for cost in ["metered", "subscription", "local", "low", "standard", "premium", ""] {
            let mut l = base();
            l.iter_mut().for_each(|c| c.cost_class = cost.into());
            classify(&ClassRules::shipped(), &mut l);
            assert_eq!(l.iter().map(|c| (c.class, c.wide)).collect::<Vec<_>>(), want.iter().map(|c| (c.class, c.wide)).collect::<Vec<_>>(), "cost {cost}");
        }
        assert!(ClassRules::parse("[[rule]]\nid = \"x\"\nclass = \"best\"\n").is_err());
        assert!(ClassRules::parse("[[rule]]\nid = \"x\"\nclass = \"unclassified\"\n").is_err());
        assert_eq!(ClassRules::shipped().version, 1);
    }

    #[test]
    fn the_measured_attributes_rule_a_pair_out_only_with_a_record_and_an_empty_one_is_no_pass() {
        let m = Measured { prompt_budget_tokens: 100_000, json_in_text: "bare".into(), ..Default::default() };
        assert!(measured_ineligible(&m, 90_000, true).is_none());
        assert!(measured_ineligible(&m, 120_000, false).unwrap().contains("measured prompt budget (100000 tokens)"));
        assert!(measured_ineligible(&Measured::default(), 5_000_000, false).is_none(), "no budget record: the declared window decides, as before");
        assert!(measured_ineligible(&Measured::default(), 0, true).unwrap().contains("no record"));
        assert!(measured_ineligible(&Measured { json_in_text: "nested-string".into(), ..Default::default() }, 0, true).unwrap().contains("nested-string"));
        assert!(measured_ineligible(&Measured { json_in_text: "untested".into(), ..Default::default() }, 0, false).is_none());
    }

    /// The seed table fills the attributes of a carrier's rows, with where they came from; another carrier's model gets none.
    #[test]
    fn the_seed_table_is_the_first_source_of_the_measured_attributes() {
        let seed = crate::model_catalog::Seed::shipped();
        let row = seed.models.iter().find(|m| m.carrier == "copilot" && m.tail_tested_tokens > 0).unwrap();
        let m = Measured::from_seed(&seed, "copilot", &row.id);
        assert_eq!((m.prompt_budget_tokens, m.carrier.as_str(), m.carrier_version.as_str(), m.measured_on.as_str()), (row.prompt_budget_tokens, "copilot", seed.copilot_cli.as_str(), seed.measured.as_str()));
        assert!(m.recall_fraction.is_some_and(|f| f > 0.0 && f < WIDE_RECALL_FRACTION), "a tail at about 90k tokens is no record near the budget");
        assert_eq!(Measured::from_seed(&seed, "api", &row.id), Measured::default());
        let untested = seed.models.iter().find(|m| m.tail_tested_tokens == 0).unwrap();
        assert_eq!(Measured::from_seed(&seed, &untested.carrier, &untested.id).recall_fraction, None);
    }

    #[test]
    fn a_workspace_file_replaces_the_shipped_rules() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(ClassRules::load(dir.path()).unwrap(), ClassRules::shipped());
        std::fs::write(dir.path().join(RULES_OVERRIDE), "[[rule]]\nid = \"all\"\nclass = \"quick\"\n").unwrap();
        assert_eq!(ClassRules::load(dir.path()).unwrap().rules.len(), 1);
        std::fs::write(dir.path().join(RULES_OVERRIDE), "not toml [").unwrap();
        assert!(ClassRules::load(dir.path()).is_err());
    }
}
