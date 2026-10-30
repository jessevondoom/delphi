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
            Class::Unclassified => "unclassified",
        }
    }
    pub fn parse(s: &str) -> Option<Class> {
        Some(match s.trim() {
            "deep" => Class::Deep,
            "steady" => Class::Steady,
            "quick" => Class::Quick,
            "local" => Class::Local,
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
    /// Also far wider than its family's median context (`specs/056` §2.2).
    #[serde(default)]
    pub wide: bool,
}

impl Candidate {
    pub fn is_ready(&self) -> bool {
        self.presence == Presence::Ready
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
            });
        }
    }
    out
}

/// Fold a fresh discovery into the earlier inventory: seen ones are refreshed, ones not seen now become `gone`.
pub fn merge(previous: Vec<Candidate>, seen: Vec<Candidate>) -> Vec<Candidate> {
    let mut out = seen;
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
    list.sort_by(|a, b| (a.presence != Presence::Ready, &a.endpoint_id, &a.model_id).cmp(&(b.presence != Presence::Ready, &b.endpoint_id, &b.model_id)));
    save(ws_dir, &list)?;
    Ok(if json { serde_json::to_string_pretty(&list).unwrap_or_default() + "\n" } else { render(&list) })
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
    pub cost_class: Option<String>,
    pub min_context: Option<u64>,
}

impl Rule {
    fn holds(&self, c: &Candidate) -> bool {
        self.local.is_none_or(|v| v == c.local)
            && self.tier.as_deref().is_none_or(|t| t == c.tier)
            && self.can_think.is_none_or(|v| v == c.can_think)
            && self.cost_class.as_deref().is_none_or(|v| v == c.cost_class)
            && self.min_context.is_none_or(|n| c.context_tokens >= n)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
struct WideRule {
    min_context_ratio: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ClassRules {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    wide: WideRule,
    #[serde(default, rename = "rule")]
    pub rules: Vec<Rule>,
}

impl ClassRules {
    pub fn parse(text: &str) -> Result<ClassRules, String> {
        let r: ClassRules = toml::from_str(text).map_err(|e| format!("class rules: {e}"))?;
        for rule in &r.rules {
            match Class::parse(&rule.class) {
                Some(Class::Unclassified) | None => return Err(format!("class rules: rule `{}` has class `{}`; use deep, steady, quick or local", rule.id, rule.class)),
                Some(_) => {}
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
pub fn classify(rules: &ClassRules, list: &mut [Candidate]) {
    let ratio = rules.wide.min_context_ratio.unwrap_or(0.0);
    let medians: Vec<(String, u64)> = {
        let mut fams: Vec<String> = list.iter().filter(|c| !c.family.is_empty()).map(|c| c.family.clone()).collect();
        fams.sort();
        fams.dedup();
        fams.into_iter()
            .filter_map(|f| {
                let mut v: Vec<u64> = list.iter().filter(|c| c.family == f && c.is_ready() && c.context_tokens > 0).map(|c| c.context_tokens).collect();
                v.sort_unstable();
                (v.len() >= 3).then(|| (f, v[v.len() / 2]))
            })
            .collect()
    };
    for c in list.iter_mut() {
        c.class = rules.class_of(c);
        c.wide = ratio > 0.0 && medians.iter().find(|(f, _)| *f == c.family).is_some_and(|(_, m)| c.context_tokens as f64 >= ratio * *m as f64);
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
}

#[cfg(test)]
mod class_tests {
    use super::*;

    fn cand(model: &str, tier: &str, ctx: u64, local: bool, family: &str) -> Candidate {
        Candidate { endpoint_id: "e".into(), model_id: model.into(), family: family.into(), tier: tier.into(), context_tokens: ctx, can_think: false, cost_class: if local { "local" } else { "metered" }.into(), local, source: "pool".into(), seen_unix: 0, presence: Presence::Ready, class: Class::Unclassified, wide: false }
    }

    #[test]
    fn three_invented_seats_are_classified_by_the_rules_alone_and_an_unplaced_id_stays_unclassified() {
        let mut l = vec![cand("zz-1", "frontier", 200_000, false, "f"), cand("zz-2", "workhorse", 200_000, false, "f"), cand("zz-3", "fast", 100_000, false, "f"), cand("zz-4", "", 0, false, ""), cand("zz-5", "frontier", 8_000, true, "g")];
        classify(&ClassRules::shipped(), &mut l);
        let got: Vec<Class> = l.iter().map(|c| c.class).collect();
        assert_eq!(got, [Class::Deep, Class::Steady, Class::Quick, Class::Unclassified, Class::Local]);
        assert_eq!(unclassified(&l).len(), 1);
    }

    #[test]
    fn a_window_several_times_its_familys_median_is_also_wide() {
        let mut l = vec![cand("a", "fast", 200_000, false, "f"), cand("b", "workhorse", 200_000, false, "f"), cand("c", "frontier", 1_000_000, false, "f"), cand("d", "frontier", 1_000_000, false, "solo")];
        classify(&ClassRules::shipped(), &mut l);
        assert_eq!(l.iter().map(|c| c.wide).collect::<Vec<_>>(), [false, false, true, false], "a family of fewer than three has no median");
    }

    #[test]
    fn a_new_vendor_is_a_row_in_the_data_file_and_a_bad_class_is_refused() {
        let rules = ClassRules::parse("[[rule]]\nid = \"big-sub\"\ncost_class = \"subscription\"\nmin_context = 500000\nclass = \"deep\"\n").unwrap();
        let mut c = cand("zz", "", 600_000, false, "");
        c.cost_class = "subscription".into();
        assert_eq!(rules.class_of(&c), Class::Deep);
        assert!(ClassRules::parse("[[rule]]\nid = \"x\"\nclass = \"best\"\n").is_err());
        assert!(ClassRules::parse("[[rule]]\nid = \"x\"\nclass = \"unclassified\"\n").is_err());
        assert_eq!(ClassRules::shipped().version, 1);
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
