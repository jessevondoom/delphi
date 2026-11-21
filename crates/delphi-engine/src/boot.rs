//! Assemble an [`Engine`] from a workspace on disk: `config.toml` (endpoints, access policy, models,
//! projects, enabled roles, budget, approval timeout), credentials, the usage ledger and the model client.

use crate::approval::Broker;
use crate::engine::{Engine, Settings};
use crate::mission::SecretRegistry;
use crate::model::{HttpModelClient, ModelClient};
use crate::probe::GitProbe;
use crate::service::EngineService;
use crate::workspace::WorkspacePaths;
use delphi_adapter::config::{Config, SharedConfig};
use delphi_adapter::credential::{CredentialStore, Resolver};
use delphi_adapter::http::{build_client, RetryPolicy};
use delphi_core::hub::Hub;
use delphi_core::ledger::Ledger;
use delphi_core::lock::{self, access_blocks, hash};
use delphi_core::provenance::OriginRules;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(p),
    }
}

/// `projects = ["~/code/my-project"]` → (`my-project`, canonical path). Names are the directory name,
/// lowercased, for `@project/<name>/…`.
pub fn project_roots(root: &toml::Value) -> Vec<(String, PathBuf)> {
    root.get("workspace")
        .and_then(|w| w.get("projects"))
        .and_then(|p| p.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(expand_home)
                .filter_map(|p| {
                    let canon = std::fs::canonicalize(&p).ok()?;
                    let name = canon.file_name()?.to_string_lossy().to_lowercase();
                    Some((name, canon))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Operational settings from `config.toml`, with the defaults of `crates/delphi/config.example.toml`.
pub fn settings_from(root: &toml::Value) -> Settings {
    let mut s = Settings { loop_cfg: crate::loopctl::LoopConfig::from_root(root), ..Settings::default() };
    // `specs/119` §14 item 3: retraction lookup, an archived copy when a page is gone, and citation chaining are on for every
    // workspace whose config does not set them; each stays a key the operator can turn off below. `Settings::default()` leaves them
    // off, so an engine built without a config (a test's) never asks the archive or the citation index.
    s.retractions = true;
    s.archive.enabled = true;
    s.chain.enabled = true;
    if let Some(v) = root.get("budget").and_then(|b| b.get("mission_budget_usd")).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))) {
        s.mission_budget_usd = v;
    }
    // Spec 123 §4: a Council's token ceiling (0 turns it off), and the money ceiling it keeps for workspaces that set one.
    if let Some(v) = root.get("budget").and_then(|b| b.get("council_budget_tokens")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.council_budget_tokens = v as u64;
    }
    if let Some(v) = root.get("budget").and_then(|b| b.get("council_budget_usd")).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))).filter(|v| *v >= 0.0) {
        s.council_budget_usd = v;
    }
    if let Some(v) = root.get("hitl").and_then(|b| b.get("approval_timeout_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.approval_timeout = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("away").and_then(|b| b.get("approval_park_max_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.approval_park_max = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("session").and_then(|b| b.get("summary")).and_then(|v| v.as_str()).and_then(crate::summary::SummaryMode::parse) {
        s.session_summary = v;
    }
    if let Some(v) = root.get("session").and_then(|b| b.get("conversation_tokens")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.conversation_tokens = v as usize;
    }
    if let Some(v) = root.get("optimistic").and_then(|b| b.get("review_wait_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.decision_review_wait = Duration::from_secs(v as u64);
    }
    if let Some(sc) = root.get("seats") {
        s.require_families = sc.get("require_families").and_then(|v| v.as_integer()).filter(|v| *v >= 1).map(|v| v.min(8) as u32);
        if let Some(n) = sc.get("max_foreign_seats").and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
            s.max_foreign_seats = n.min(64) as u32;
        }
        if let Some(n) = sc.get("max_foreign_tokens").and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
            s.max_foreign_tokens = n as u64;
        }
        // Spec 132 §5.4 item 2: pinned (`lock::PINNED_KEYS`), so this is the ratified copy's value. Anything but "two" keeps "all".
        s.spread_all = sc.get("spread").and_then(|v| v.as_str()) != Some("two");
    }
    // Spec 135 §6: a review's commands (a list of argv lists, at most 8, each non-empty) and its diff cap (1,000 to 200,000
    // characters). A value of the wrong shape keeps the default.
    if let Some(fg) = root.get("forge") {
        if let Some(list) = fg.get("review_commands").and_then(|v| v.as_array()) {
            let parsed: Option<Vec<Vec<String>>> = list.iter().map(|a| a.as_array().map(|w| w.iter().map(|x| x.as_str().map(String::from)).collect::<Option<Vec<_>>>()).unwrap_or(None).filter(|w| !w.is_empty())).collect();
            if let Some(cmds) = parsed.filter(|c| c.len() <= 8) {
                s.review_commands = Some(cmds);
            }
        }
        if let Some(n) = fg.get("review_diff_chars").and_then(|v| v.as_integer()).filter(|v| (1_000..=200_000).contains(v)) {
            s.review_diff_chars = n as usize;
        }
    }
    // Spec 104 §12 A4: `[access]` is one of the pinned blocks, so this is the ratified copy's value; a live edit does not count.
    if let Some(n) = root.get("access").and_then(|a| a.get("standing_foreign_tokens")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.standing_foreign_tokens = n as u64;
    }
    if let Some(n) = root.get("council").and_then(|c| c.get("stall_secs")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.council_stall_secs = n.min(86_400) as u64;
    }
    if let Some(b) = root.get("council").and_then(|c| c.get("earlier_rulings")).and_then(|v| v.as_bool()) {
        s.council_earlier_rulings = b;
    }
    if let Some(n) = root.get("research").and_then(|r| r.get("thin_below")).and_then(|v| v.as_integer()).filter(|v| *v >= 1) {
        s.thin_below = n.min(50) as usize;
    }
    // `specs/114` §3 item 3: when a new question continues a topic Delphi has. A share of the question's content words (above 0, at
    // most 1) and a retrieval score (0 to 1); anything else keeps the default.
    let bar = |key: &str| root.get("research").and_then(|r| r.get(key)).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)));
    if let Some(x) = bar("continue_overlap").filter(|x| *x > 0.0 && *x <= 1.0) {
        s.continue_bar.overlap = x;
    }
    if let Some(x) = bar("continue_score").filter(|x| (0.0..=1.0).contains(x)) {
        s.continue_bar.score = x;
    }
    // Due topics (`specs/112` §5.2): the days after which a topic's newest run is old, and the bounds a record's own figure is
    // held to. Each is 1 to 3650 days; anything else keeps the default, and bounds the wrong way round keep both defaults.
    if let Some(r) = root.get("research") {
        let days = |k: &str| r.get(k).and_then(|v| v.as_integer()).filter(|v| (1..=3650).contains(v)).map(|v| v as u64);
        let d = crate::research_record::Refresh::default();
        let (min, max) = match (days("refresh_days_min").unwrap_or(d.min), days("refresh_days_max").unwrap_or(d.max)) {
            (min, max) if min <= max => (min, max),
            _ => (d.min, d.max),
        };
        s.research_refresh = crate::research_record::Refresh { days: days("refresh_days").unwrap_or(d.days), min, max };
    }
    // Archived copies (`specs/101` §5, §8; on by default, `specs/119` §14 item 3): a boolean, off only when it is exactly `false`; the
    // endpoint must be an https URL, else the pinned default stays. Any other value of either keeps the default.
    if let Some(r) = root.get("research") {
        if let Some(b) = r.get("archive_fallback").and_then(|v| v.as_bool()) {
            s.archive.enabled = b;
        }
        if let Some(e) = r.get("archive_endpoint").and_then(|v| v.as_str()).map(str::trim).filter(|e| e.starts_with("https://") && e.len() <= 300 && delphi_adapter::reqwest::Url::parse(e).is_ok()) {
            s.archive.endpoint = e.to_string();
        }
    }
    // Citation chaining (`specs/101` §6, §8; on by default, `specs/119` §14 item 3): a boolean, off only when it is exactly `false`; the endpoint must be an https URL with no
    // credentials, query or fragment, else the pinned default stays; the bounds are counts in a range, and a value of the wrong kind or
    // outside the range keeps the default.
    if let Some(r) = root.get("research") {
        if let Some(b) = r.get("citation_chaining").and_then(|v| v.as_bool()) {
            s.chain.enabled = b;
        }
        if let Some(e) = r.get("citation_endpoint").and_then(|v| v.as_str()).map(str::trim).filter(|e| e.len() <= 300 && crate::research_chain::lookup_url(e, crate::research_chain::Direction::References, "10.1000/x").is_some()) {
            s.chain.endpoint = e.to_string();
        }
        let count = |key: &str, max: i64| r.get(key).and_then(|v| v.as_integer()).filter(|v| (1..=max).contains(v)).map(|n| n as usize);
        if let Some(n) = count("chain_depth", 3) {
            s.chain.depth = n;
        }
        if let Some(n) = count("chain_fanout", 10) {
            s.chain.fanout = n;
        }
        if let Some(n) = count("chain_max_per_seed", 50) {
            s.chain.max_per_seed = n;
        }
        if let Some(n) = count("chain_max_calls", 200) {
            s.chain.max_calls = n;
        }
        // Reading a chained work as a page (`specs/101` §6 item 8): the pinned DOI-resolution endpoint, under the same rule as the index
        // endpoint (https, no credentials, query or fragment), and the page cap.
        if let Some(e) = r.get("doi_endpoint").and_then(|v| v.as_str()).map(str::trim).filter(|e| e.len() <= 300 && crate::research_chain::resolve_url(e, "10.1000/x").is_some()) {
            s.chain.doi_endpoint = e.to_string();
        }
        if let Some(n) = count("chain_max_pages", 50) {
            s.chain.max_pages = n;
        }
    }
    // The caps of the wider fetch (`specs/098` §7.5 item 7a, slice S1d): counts of 1..=100, and a value of the wrong kind keeps the
    // default. The fetch itself is always on; an old `ladder` key is ignored (§13.2).
    if let Some(r) = root.get("research") {
        if let Some(n) = r.get("fetch_per_host").and_then(|v| v.as_integer()).filter(|v| (1..=100).contains(v)) {
            s.wider_fetch.per_host = n as usize;
        }
        if let Some(n) = r.get("fetch_per_run").and_then(|v| v.as_integer()).filter(|v| (1..=100).contains(v)) {
            s.wider_fetch.per_run = n as usize;
        }
    }
    // `[research.limits]` (`specs/112` §4 item 6): the one limits table of a research run. Each key is a count inside its range; a
    // value outside it, or of the wrong kind, keeps the default (the host runner's figure of before the table).
    if let Some(t) = root.get("research").and_then(|r| r.get("limits")) {
        let l = &mut s.research_limits;
        let count = |k: &str, lo: i64, hi: i64| t.get(k).and_then(|v| v.as_integer()).filter(|v| (lo..=hi).contains(v));
        for (i, d) in ["quick", "survey", "deep"].iter().enumerate() {
            if let Some(n) = count(&format!("pages_{d}"), 1, 40) {
                l.pages[i] = n as usize;
            }
            if let Some(n) = count(&format!("steps_{d}"), 1, 200) {
                l.steps[i] = n as u32;
            }
        }
        if let Some(n) = count("job_secs", 30, 3600) {
            l.job_secs = n as u64;
        }
        if let Some(n) = count("concurrent", 1, 8) {
            l.concurrent = n as usize;
        }
        if let Some(n) = count("page_chars", 1000, 200_000) {
            l.page_chars = n as usize;
        }
        if let Some(d) = t.get("depth").and_then(|v| v.as_str()).and_then(delphi_core::research::Depth::parse) {
            l.depth = d;
        }
        if let Some(n) = count("pictures_per_source", 0, 50) {
            l.pictures_per_source = n as usize;
        }
        if let Some(n) = count("pictures_per_run", 0, 200) {
            l.pictures_per_run = n as usize;
        }
    }
    // `[research] retractions` and `retractions_stale_days` (`specs/101` §4, §8; on by default, `specs/119` §14 item 3): a boolean, off only when exactly `false`; the age is 1 to 3650
    // days. A value of the wrong kind keeps the default.
    if let Some(b) = root.get("research").and_then(|r| r.get("retractions")).and_then(|v| v.as_bool()) {
        s.retractions = b;
    }
    // `[research] images` (`specs/118` §3 item 1): a boolean, on unless exactly `false`. A value of the wrong kind keeps the default.
    if let Some(b) = root.get("research").and_then(|r| r.get("images")).and_then(|v| v.as_bool()) {
        s.research_images = b;
    }
    // `[research] svg` (`specs/118` §11 decision 1): a boolean, off unless exactly `true`. A value of the wrong kind keeps the default.
    if let Some(b) = root.get("research").and_then(|r| r.get("svg")).and_then(|v| v.as_bool()) {
        s.research_svg = b;
    }
    if let Some(n) = root.get("research").and_then(|r| r.get("retractions_stale_days")).and_then(|v| v.as_integer()).filter(|v| (1..=3650).contains(v)) {
        s.retractions_stale_days = n;
    }
    // The corroboration numbers (`specs/098` §18): each ignored when it is not the right kind of value, so a typo keeps the default.
    if let Some(r) = root.get("research") {
        if let Some(n) = r.get("young_domain_days").and_then(|v| v.as_integer()).filter(|v| (0..=3650).contains(v)) {
            s.support.young_domain_days = n as u32;
        }
        if let Some(n) = r.get("burst_hours").and_then(|v| v.as_integer()).filter(|v| (0..=720).contains(v)) {
            s.support.burst_hours = n as u32;
        }
        if let Some(x) = r.get("similar_above").and_then(|v| v.as_float()).filter(|v| *v > 0.0 && *v <= 1.0) {
            s.support.similar_above = x;
        }
        if let Some(b) = r.get("unfamiliar_counts").and_then(|v| v.as_bool()) {
            s.support.unfamiliar_counts = b;
        }
    }
    // The confidence ladder (`specs/100` §3.2): counts of 1..=50 and on/off switches; a value of the wrong kind keeps the default.
    if let Some(t) = root.get("research").and_then(|r| r.get("tiers")) {
        let count = |k: &str| t.get(k).and_then(|v| v.as_integer()).filter(|v| (1..=50).contains(v)).map(|v| v as usize);
        let switch = |k: &str| t.get(k).and_then(|v| v.as_bool());
        let d = &mut s.tiers;
        d.corroborated_origins = count("corroborated_origins").unwrap_or(d.corroborated_origins);
        d.substantiated_trusted = count("substantiated_trusted").unwrap_or(d.substantiated_trusted);
        d.vetted_origins = count("vetted_origins").unwrap_or(d.vetted_origins);
        d.vetted_trusted = count("vetted_trusted").unwrap_or(d.vetted_trusted);
        d.corroborated_on = switch("corroborated_enabled").unwrap_or(d.corroborated_on);
        d.substantiated_on = switch("substantiated_enabled").unwrap_or(d.substantiated_on);
        d.vetted_on = switch("vetted_enabled").unwrap_or(d.vetted_on);
        d.fast_lane_on = switch("fast_lane_enabled").unwrap_or(d.fast_lane_on);
        s.tiers = s.tiers.clone().ordered();
    }
    s.hooks = crate::hooks::parse(root).0;
    if let Some(r) = root.get("relay") {
        if r.get("step_up").and_then(|v| v.as_str()) == Some("totp") {
            s.relay_totp_credential = r.get("totp_credential").and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty()).map(String::from);
        }
    }
    if let Some(o) = root.get("optimistic") {
        if let Some(a) = o.get("approver").and_then(|v| v.as_str()).and_then(crate::provisional::ApproverKind::parse) {
            s.approver = a;
        }
        if let Some(a) = o.get("second_seat").and_then(|v| v.as_str()).and_then(crate::provisional::SecondSeat::parse) {
            s.second_seat = a;
        }
        if let Some(n) = o.get("max_decisions").and_then(|v| v.as_integer()).filter(|v| *v > 0) {
            s.max_decisions = n as u32;
        }
        if let Some(n) = o.get("max_approver_calls").and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
            s.max_approver_calls = n as u32;
        }
        if let Some(a) = o.get("unseconded").and_then(|v| v.as_str()).and_then(crate::provisional::Unseconded::parse) {
            s.unseconded = a;
        }
    }
    if let Some(agents_val) = root.get("agents") {
        if let Some(list) = agents_val.get("forbidden").and_then(|v| v.as_array()) {
            s.forbidden_agents = list.iter().filter_map(|x| x.as_str().map(String::from)).collect();
        }
        if let Some(fleet) = agents_val.get("fleet").and_then(|v| v.as_str()) {
            s.fleet = fleet.to_string();
        }
        let interactive: Vec<String> = agents_val.get("interactive")
            .and_then(|v| v.as_array())
            .map(|list| list.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let enabled: Vec<String> = agents_val.get("enabled")
            .and_then(|v| v.as_array())
            .map(|list| list.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        if !interactive.is_empty() {
            s.interactive_agents = interactive.clone();
            s.enabled_agents = interactive;
        } else if !enabled.is_empty() {
            s.interactive_agents = enabled.clone();
            s.enabled_agents = enabled;
        }
    }
    if let Some(v) = root.get("routing").and_then(|r| r.get("degrade_prompt_secs")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.degrade_wait = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("routing").and_then(|r| r.get("checkpoint_model")).and_then(|v| v.as_bool()) {
        s.checkpoint_model = v;
    }
    if let Some(v) = root.get("routing").and_then(|r| r.get("checkpoint_wait_sec")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.checkpoint_wait = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("routing").and_then(|r| r.get("max_park_secs")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.park_max = Some(Duration::from_secs(v as u64));
    }
    if let Some(v) = root.get("harness").and_then(|h| h.get("subscription_budget_tokens")).and_then(|v| v.as_integer()).filter(|v| *v > 0) {
        s.harness_token_cap = v as u64;
    }
    s.harness_claude = crate::harness_opts::parse(root, crate::harness::Cli::Claude).0;
    s.harness_agy = crate::harness_opts::parse(root, crate::harness::Cli::Agy).0;
    s.harness_codex = crate::harness_opts::parse(root, crate::harness::Cli::Codex).0;
    s.harness_copilot = crate::harness_opts::parse(root, crate::harness::Cli::Copilot).0;
    s.learning = crate::learn::LearningSettings::parse(root).0;
    s.history = crate::history_record::Settings::parse(root).0;
    if let Some(v) = root.get("harness").and_then(|h| h.get("first_event_secs")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.harness_first_event = Duration::from_secs(v as u64);
    }
    if let Some(v) = root.get("harness").and_then(|h| h.get("connector_settle_secs")).and_then(|v| v.as_integer()).filter(|v| *v >= 0) {
        s.connector_settle = Duration::from_secs(v as u64);
    }
    s
}

fn now_rfc3339() -> String {
    delphi_core::time::now_rfc3339()
}

/// Re-read `config.toml` through the ratification rule and swap it into the running engine. Called after the
/// operator ratifies an endpoint change, so it takes effect without a restart. Returns the new notices.
pub fn reload_config(engine: &Engine) -> Result<Vec<String>, String> {
    let live = std::fs::read_to_string(engine.ws.dir.join("config.toml")).map_err(|e| format!("cannot read config.toml: {}", e))?;
    let (text, notices) = effective_config(&engine.ws.dir, &live)?;
    let cfg = Config::parse(&text).map_err(|errs| format!("config.toml has problems:\n  - {}", errs.join("\n  - ")))?;
    engine.cfg.set(cfg);
    Ok(notices)
}

pub struct Booted {
    pub engine: Arc<Engine>,
    pub service: Arc<EngineService>,
    pub default_mission: String,
    /// Things the operator should know on attach (for example unratified endpoint changes).
    pub notices: Vec<String>,
}

/// The `config.toml` the engine will actually use. Endpoint, access and credential settings
/// (`[[endpoints]]`, `[access]`, `[auth.*]`) are trusted **only from the ratified copy** (`specs/009` §5):
/// if the live file's blocks differ from the pin, the last ratified ones stay in force, and with none
/// available no endpoints are active, so a local edit can't redirect traffic or inject a credential helper.
/// Everything else (models, prices, projects, budget) comes from the live file.
pub fn effective_config(ws_dir: &Path, live_text: &str) -> Result<(String, Vec<String>), String> {
    let pinned = lock::read(&ws_dir.join("local/ratified.lock")).ok().and_then(|l| l.files.get(crate::ratify::CONFIG_KEY).cloned());
    let live_blocks = access_blocks(live_text);
    let mut notices = Vec::new();
    let trusted_blocks: String = if pinned.as_deref() == Some(hash(live_blocks.as_bytes()).as_str()) {
        live_blocks
    } else {
        match std::fs::read_to_string(ws_dir.join("local/ratified").join(crate::ratify::CONFIG_COPY)) {
            Ok(copy) if pinned.as_deref() == Some(hash(copy.as_bytes()).as_str()) => {
                notices.push("config.toml has unratified endpoint/access changes; the last ratified settings stay in force (/ratify to review).".into());
                copy
            }
            _ => {
                notices.push("config.toml's endpoints are not ratified, so no endpoint is active until you /ratify.".into());
                String::new()
            }
        }
    };
    let mut live: toml::Value = toml::from_str(live_text).map_err(|e| format!("config.toml is not valid TOML: {}", e))?;
    let table = live.as_table_mut().ok_or("config.toml is not a table")?;
    for k in delphi_core::lock::ACCESS_KEYS {
        table.remove(k);
    }
    // A pinned key of a live table (`[seats] spread`, `specs/132` §5.4): the live value is dropped, the ratified one put back, and the
    // rest of that table stays live.
    for (t, k) in delphi_core::lock::PINNED_KEYS {
        if let Some(toml::Value::Table(sub)) = table.get_mut(t) {
            sub.remove(k);
        }
    }
    if let Ok(toml::Value::Table(t)) = toml::from_str::<toml::Value>(&trusted_blocks) {
        for (k, v) in t {
            if let Some((_, key)) = delphi_core::lock::PINNED_KEYS.iter().find(|(p, _)| *p == k) {
                if let Some(x) = v.get(key) {
                    if let toml::Value::Table(sub) = table.entry(k).or_insert_with(|| toml::Value::Table(Default::default())) {
                        sub.insert(key.to_string(), x.clone());
                    }
                }
                continue;
            }
            table.insert(k, v);
        }
    }
    Ok((toml::to_string(&live).map_err(|e| e.to_string())?, notices))
}

/// The credential store behind OAuth logins: the same resolver the model client uses, and the secret store that
/// holds refresh tokens (for signing a connector out).
pub fn credential_store(cfg: SharedConfig, secrets: Arc<SecretRegistry>) -> (Arc<CredentialStore>, Arc<delphi_adapter::oauth2::SystemSecretStore>) {
    let reg = secrets.clone();
    let mut resolver = Resolver::system(Arc::new(move |s| reg.register(s)));
    let store = Arc::new(delphi_adapter::oauth2::SystemSecretStore::new(resolver.runner()));
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        resolver = resolver.with_gcloud_expiry(Arc::new(delphi_adapter::oauth2::GoogleTokenInfo::new(build_client(), handle.clone())));
        resolver = resolver.with_oauth(Arc::new(delphi_adapter::oauth2::Oauth2Provider::new(cfg, store.clone(), build_client(), handle)));
    }
    (Arc::new(CredentialStore::new(resolver)), store)
}

/// Access tokens for connectors come from the connector's `[auth.<login>]` block, exactly as model endpoints get theirs.
struct OauthTokens {
    creds: Arc<CredentialStore>,
    store: Arc<delphi_adapter::oauth2::SystemSecretStore>,
}

impl crate::connectors::TokenSource for OauthTokens {
    fn token(&self, connector: &str, login: &str) -> Result<String, String> {
        match self.creds.get(&format!("connector-{}", connector), &format!("oauth2:{}", login)) {
            Ok(delphi_adapter::credential::Resolved::Token(t)) => Ok(t.expose().to_string()),
            Ok(_) => Err("not a token credential".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn forget(&self, _connector: &str, login: &str) {
        use delphi_adapter::oauth2::SecretStore;
        let _ = self.store.delete(&delphi_adapter::oauth2::refresh_entry(login));
    }
}

/// Who a Google access token belongs to: the OpenID userinfo email.
struct GoogleIdentity;

impl crate::connectors::Identity for GoogleIdentity {
    fn who(&self, kind: &str, token: &str) -> Result<String, String> {
        if kind != "google" {
            return Err(format!("no identity lookup for `{}`", kind));
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        rt.block_on(async {
            let c = delphi_adapter::reqwest::Client::builder().redirect(delphi_adapter::reqwest::redirect::Policy::none()).timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
            let r = c.get("https://openidconnect.googleapis.com/v1/userinfo").bearer_auth(token).send().await.map_err(|e| e.without_url().to_string())?;
            if !r.status().is_success() {
                return Err(format!("the identity lookup failed (HTTP {})", r.status().as_u16()));
            }
            let v: serde_json::Value = serde_json::from_slice(&r.bytes().await.map_err(|e| e.without_url().to_string())?).map_err(|e| e.to_string())?;
            v["email"].as_str().map(String::from).ok_or_else(|| "the identity lookup returned no email".to_string())
        })
    }
}

/// The connector host for a workspace (`specs/010`).
pub fn connector_host(cfg: SharedConfig, secrets: Arc<SecretRegistry>, ws_dir: &Path) -> Arc<crate::connectors::ConnectorHost> {
    let (creds, store) = credential_store(cfg.clone(), secrets);
    crate::connectors::ConnectorHost::new(cfg, ws_dir, Arc::new(OauthTokens { creds, store }), Arc::new(crate::connectors::HttpFactory), Some(Arc::new(GoogleIdentity)))
}

/// The production model client: credentials (including `oauth2:` when a runtime is available), the
/// platform envelopes, retries, and redaction through `secrets`. Must be called inside a Tokio runtime
/// for `oauth2:` credentials to work.
pub fn http_model_client(cfg: SharedConfig, secrets: Arc<SecretRegistry>) -> Arc<HttpModelClient> {
    let reg = secrets.clone();
    let mut resolver = Resolver::system(Arc::new(move |s| reg.register(s)));
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let store = Arc::new(delphi_adapter::oauth2::SystemSecretStore::new(resolver.runner()));
        resolver = resolver.with_gcloud_expiry(Arc::new(delphi_adapter::oauth2::GoogleTokenInfo::new(build_client(), handle.clone())));
        resolver = resolver.with_oauth(Arc::new(delphi_adapter::oauth2::Oauth2Provider::new(cfg.clone(), store, build_client(), handle)));
    }
    let creds = Arc::new(CredentialStore::new(resolver));
    let (redact_secrets, register_secrets) = (secrets.clone(), secrets);
    Arc::new(HttpModelClient {
        cfg,
        creds,
        http: build_client(),
        retry: RetryPolicy::default(),
        idle_timeout: Duration::from_secs(120),
        redact: Arc::new(move |s| redact_secrets.redact(s)),
        register: Arc::new(move |s| register_secrets.register(s)),
    })
}

/// Build the engine for `ws_dir` (`$DELPHI_HOME/workspaces/<name>`), publishing through `hub`.
pub fn build(home: &Path, ws_dir: &Path, hub: Hub) -> Result<Booted, String> {
    build_with_model(home, ws_dir, hub, None)
}

/// As [`build`], optionally with a model client supplied (tests).
pub fn build_with_model(home: &Path, ws_dir: &Path, hub: Hub, model: Option<Arc<dyn ModelClient>>) -> Result<Booted, String> {
    let live_text = std::fs::read_to_string(ws_dir.join("config.toml")).map_err(|e| format!("cannot read {}/config.toml: {}", ws_dir.display(), e))?;
    let (text, mut notices) = effective_config(ws_dir, &live_text)?;
    let cfg = Config::parse(&text).map_err(|errs| format!("config.toml has problems:\n  - {}", errs.join("\n  - ")))?;
    let root: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;
    let secrets = SecretRegistry::new();
    let cfg = SharedConfig::new(cfg);
    let model: Arc<dyn ModelClient> = match model {
        Some(m) => m,
        None => {
            http_model_client(cfg.clone(), secrets.clone())
        }
    };
    std::fs::create_dir_all(ws_dir.join("local")).map_err(|e| e.to_string())?;
    let ledger = Ledger::open(&ws_dir.join("local/state.db")).map_err(|e| format!("usage ledger: {}", e))?;
    let rules = {
        let custom: Vec<String> = root.get("workspace").and_then(|w| w.get("untrusted_paths")).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        if custom.is_empty() {
            OriginRules::with_defaults()
        } else {
            OriginRules { untrusted_paths: custom }
        }
    };
    let connectors = connector_host(cfg.clone(), secrets.clone(), ws_dir);
    let (timers, timer_notice) = crate::timers::TimerStore::open(&ws_dir.join("local/timers.json"));
    notices.extend(timer_notice);
    notices.extend(crate::harness_opts::problems(&root));
    notices.extend(crate::learn::LearningSettings::parse(&root).1);
    notices.extend(crate::history_record::Settings::parse(&root).1);
    // A hook left out for being malformed is Delphi less strict than the operator meant: say so on attach.
    notices.extend(crate::hooks::parse(&root).1.into_iter().map(|p| format!("hooks: {}", p)));
    let pool = Arc::new(crate::pool::Pool::open(Some(ws_dir.join("local/pool-state.json")), cfg.get().routing.tuning.clone(), crate::pool::system_clock()));
    // Spec 103 M3: what a relay and a mailbox are sent passes the daemon's filter first, so agent-written text of a task that read a private
    // entry never reaches them. A task that read nothing private is sent exactly what it always was.
    crate::private_read::install_relay_filter(&hub, ws_dir);
    let engine = Arc::new(Engine {
        research_ask: Default::default(),
        validator: Default::default(),
        ws: WorkspacePaths { home: home.to_path_buf(), dir: ws_dir.to_path_buf() },
        cfg,
        hub,
        broker: Broker::new_unique(),
        model,
        secrets,
        ledger: Arc::new(Mutex::new(ledger)),
        settings: {
            let st = settings_from(&root);
            delphi_core::knowledge_status::set_tier_params(st.tiers.clone());
            // The roll-back latch (`specs/100` §7.3) is a file in the validator's state directory: it survives a restart, only the operator or the
            // audit writes it, and it is re-read about once a second so a roll-back from another process applies to this daemon.
            delphi_core::knowledge_status::set_tier_latch_file(crate::tier_audit::latch_file(&crate::fact_mode::state_dir(ws_dir)));
            st
        },
        probe: Arc::new(GitProbe::new()),
        projects: project_roots(&root),
        rules,
        clock: Arc::new(now_rfc3339),
        tools: Arc::new(crate::tools::Registry::builtin().with_validator(delphi_adapter::config::ValidatorMode::Advisory)),
        exec: Arc::new(crate::exec::ExecEnv::new(ws_dir, crate::exec::ExecConfig::from_root(&root, &std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()), delphi_sandbox::Sandbox::detect().ok())),
        injected: Default::default(),
        seat_deadlines: Default::default(),
        bundle_seen: Default::default(),
        xenia_seen: Default::default(),
        org_cache: Default::default(),
        schemas: Default::default(),
        name_live: Default::default(),
        connectors,
        pool,
        clock_unix: crate::pool::system_clock(),
        degraded: Default::default(),
        optimistic: Default::default(),
        decisions_changed: Default::default(),
        timers: Arc::new(timers),
        inventory: Arc::new(crate::inventory::Inventory::open(ws_dir.join("local/harness-tools.json"))),
        toolless: Default::default(),
        operator_prompts: Default::default(),
        readonly_tasks: Default::default(),
        readonly_handoffs: Default::default(),
        approver_calls: Default::default(),
        pace: Default::default(),
        totp: delphi_core::totp::Verifier::persistent(ws_dir.join("local/totp-state")),
        session_introduced: Default::default(),
        quarantined: Default::default(),
        unquarantined: Default::default(),
        solo_mode: Default::default(),
        grant: Default::default(),
        places: Default::default(),
    });
    let day = &now_rfc3339()[..10];
    let default_mission = format!("{}-session", day);
    let service = EngineService::new(engine.clone(), &default_mission);
    service.set_notices(notices.clone());
    Ok(Booted { engine, service, default_mission, notices })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::CONFIG;
    use crate::workspace::tests::make_workspace;
    use std::fs;

    #[test]
    fn harness_blocks_become_cli_options_and_a_typo_is_a_notice() {
        let root: toml::Value = toml::from_str("[harness]\nsubscription_budget_tokens = 9000\nfirst_event_secs = 45\n[harness.claude]\neffort = \"low\"\npartial_messages = true\n[harness.agy]\nmode = \"yolo\"\n").unwrap();
        let s = settings_from(&root);
        assert_eq!((s.harness_token_cap, s.harness_claude.effort.as_deref(), s.harness_claude.partial_messages, s.harness_agy.mode.clone()), (9000, Some("low"), true, None));
        assert_eq!(s.harness_first_event, Duration::from_secs(45));
        assert_eq!(settings_from(&toml::from_str("[harness]\n").unwrap()).harness_first_event, Duration::from_secs(90), "the default");
        let bad = crate::harness_opts::problems(&root);
        assert!(bad.len() == 1 && bad[0].contains("[harness.agy] mode"), "{:?}", bad);
    }


    #[test]
    fn the_council_stall_window_defaults_to_two_minutes_and_zero_turns_it_off() {
        assert_eq!(settings_from(&toml::from_str("").unwrap()).council_stall_secs, 120);
        assert_eq!(settings_from(&toml::from_str("[council]\nstall_secs = 300\n").unwrap()).council_stall_secs, 300);
        assert_eq!(settings_from(&toml::from_str("[council]\nstall_secs = 0\n").unwrap()).council_stall_secs, 0);
        assert_eq!(settings_from(&toml::from_str("[council]\nstall_secs = -5\n").unwrap()).council_stall_secs, 120, "a negative value is ignored");
        assert_eq!(settings_from(&toml::from_str("[council]\nstall_secs = 99999999\n").unwrap()).council_stall_secs, 86_400, "and a huge one is capped at a day");
        assert!(settings_from(&toml::from_str("").unwrap()).council_earlier_rulings, "spec 123 §9: earlier rulings are shown by default");
        assert!(!settings_from(&toml::from_str("[council]\nearlier_rulings = false\n").unwrap()).council_earlier_rulings);
    }

    #[test]
    fn a_council_token_ceiling_defaults_to_400k_and_the_money_ceiling_is_still_read() {
        let s = settings_from(&toml::from_str("").unwrap());
        assert_eq!((s.council_budget_tokens, s.council_budget_usd), (400_000, 20.0));
        let s = settings_from(&toml::from_str("[budget]\ncouncil_budget_tokens = 0\ncouncil_budget_usd = 7\n").unwrap());
        assert_eq!((s.council_budget_tokens, s.council_budget_usd), (0, 7.0), "0 turns the token ceiling off");
        assert_eq!(settings_from(&toml::from_str("[budget]\ncouncil_budget_tokens = -1\n").unwrap()).council_budget_tokens, 400_000, "a negative value is ignored");
    }

    #[test]
    fn the_wider_fetch_has_small_caps_and_a_bad_value_keeps_the_default() {
        use crate::research_observed::WiderFetch;
        // The caps of the wider fetch (specs/098 §7.5 item 7a, slice S1d): two pages a host, ten a run.
        assert_eq!(settings_from(&toml::from_str("").unwrap()).wider_fetch, WiderFetch::default());
        assert_eq!(settings_from(&toml::from_str("").unwrap()).wider_fetch, WiderFetch { per_host: 2, per_run: 10 });
        let set = settings_from(&toml::from_str("[research]\nfetch_per_host = 3\nfetch_per_run = 20\n").unwrap()).wider_fetch;
        assert_eq!(set, WiderFetch { per_host: 3, per_run: 20 });
        for bad in ["fetch_per_host = 0", "fetch_per_host = 1000", "fetch_per_run = -1", "fetch_per_run = \"many\""] {
            let w = settings_from(&toml::from_str(&format!("[research]\n{bad}\n")).unwrap()).wider_fetch;
            assert_eq!(w, WiderFetch::default(), "{bad}");
        }
        // An old config that still carries the removed `ladder` key reads without complaint and changes nothing (§13.2).
        for old in ["ladder = true", "ladder = false", "ladder = \"yes\""] {
            let s = settings_from(&toml::from_str(&format!("[research]\n{old}\n")).unwrap());
            assert_eq!((s.wider_fetch, s.thin_below), (WiderFetch::default(), 3), "{old}");
        }
    }

    // `specs/112` §4 item 6: the limits of the two runners are one table, and its defaults are the host runner's figures.
    #[test]
    fn the_research_limits_are_one_table_whose_defaults_are_the_host_runners_and_a_bad_value_keeps_the_default() {
        use crate::research_run::Limits;
        use delphi_core::research::Depth;
        let d = settings_from(&toml::from_str("").unwrap()).research_limits;
        assert_eq!(d, Limits::default());
        assert_eq!((d.pages, d.steps, d.job_secs, d.concurrent, d.page_chars, d.depth), ([3, 6, 10], [5, 15, 30], 600, 2, 24_000, Depth::Survey), "what the host runner had before the table");
        assert_eq!((d.job_secs, d.concurrent), (crate::research_jobs::JOB_TIMEOUT.as_secs(), crate::research_jobs::MAX_CONCURRENT));
        assert_eq!((d.pictures_per_source, d.pictures_per_run), (6, 24), "the console job's picture caps: the host runner kept none");
        let set = settings_from(&toml::from_str("[research.limits]\npages_quick = 2\npages_deep = 20\nsteps_survey = 40\njob_secs = 900\nconcurrent = 1\npage_chars = 8000\ndepth = \"deep\"\npictures_per_source = 0\npictures_per_run = 3\n").unwrap()).research_limits;
        assert_eq!((set.pages, set.steps, set.job_secs, set.concurrent, set.page_chars, set.depth, set.pictures_per_source, set.pictures_per_run), ([2, 6, 20], [5, 40, 30], 900, 1, 8000, Depth::Deep, 0, 3));
        assert_eq!((set.pages_at(Depth::Deep), set.steps_at(Depth::Survey)), (20, 40));
        for bad in ["pages_quick = 0", "pages_deep = 500", "steps_quick = \"many\"", "job_secs = 5", "concurrent = 0", "concurrent = 99", "page_chars = 10", "depth = \"forever\"", "pictures_per_run = -1"] {
            assert_eq!(settings_from(&toml::from_str(&format!("[research.limits]\n{bad}\n")).unwrap()).research_limits, Limits::default(), "{bad}");
        }
    }

    // `specs/118` §3 item 1: research keeps pictures unless `[research] images = false`; a value of the wrong kind keeps the default.
    #[test]
    fn research_keeps_pictures_unless_images_is_false() {
        let of = |text: &str| settings_from(&toml::from_str(text).unwrap()).research_images;
        assert!(of(""));
        assert!(!of("[research]\nimages = false\n"));
        assert!(of("[research]\nimages = true\n"));
        assert!(of("[research]\nimages = \"no\"\n"), "a string is not a boolean: the default");
    }

    // `specs/118` §11 decision 1: SVG capture is off unless `[research] svg = true`; a value of the wrong kind keeps the default.
    #[test]
    fn research_draws_svg_pictures_only_when_svg_is_true() {
        let of = |text: &str| settings_from(&toml::from_str(text).unwrap()).research_svg;
        assert!(!of(""));
        assert!(of("[research]\nsvg = true\n"));
        assert!(!of("[research]\nsvg = false\n"));
        assert!(!of("[research]\nsvg = \"yes\"\n"), "a string is not a boolean: the default");
    }

    // `specs/112` §5.2: when a research topic is due. Ninety days unless set; a bad value keeps the default.
    #[test]
    fn a_topic_is_refreshed_after_ninety_days_unless_set_and_a_bad_value_keeps_the_default() {
        use crate::research_record::Refresh;
        let of = |text: &str| settings_from(&toml::from_str(text).unwrap()).research_refresh;
        assert_eq!((of(""), Refresh::default()), (Refresh { days: 90, min: 7, max: 730 }, Refresh { days: 90, min: 7, max: 730 }));
        assert_eq!(of("[research]\nrefresh_days = 30\nrefresh_days_min = 1\nrefresh_days_max = 365\n"), Refresh { days: 30, min: 1, max: 365 });
        assert_eq!(of("[research]\nthin_below = 4\nrefresh_days = 180\n"), Refresh { days: 180, min: 7, max: 730 });
        for bad in ["refresh_days = 0", "refresh_days = -5", "refresh_days = 99999", "refresh_days = \"often\"", "refresh_days_min = 0", "refresh_days_max = 99999"] {
            assert_eq!(of(&format!("[research]\n{bad}\n")), Refresh::default(), "{bad}");
        }
        assert_eq!(of("[research]\nrefresh_days_min = 400\nrefresh_days_max = 30\n"), Refresh::default(), "bounds the wrong way round keep both defaults");
    }

    #[test]
    fn archived_copies_are_on_unless_turned_off_the_endpoint_must_be_https_and_a_bad_value_keeps_the_default() {
        use crate::library::ArchiveConfig;
        // `[research] archive_fallback` and `archive_endpoint` (specs/101 §5, §8; specs/119 §14 item 3): on for a workspace that does
        // not set it, the one pinned endpoint, and off in an engine built with no config.
        let unset = ArchiveConfig { enabled: true, ..ArchiveConfig::default() };
        assert_eq!(settings_from(&toml::from_str("").unwrap()).archive, unset);
        assert!(!ArchiveConfig::default().enabled && ArchiveConfig::default().endpoint == "https://archive.org/wayback/available");
        let on = settings_from(&toml::from_str("[research]\narchive_fallback = true\narchive_endpoint = \"https://archive.example.net/wayback/available\"\n").unwrap()).archive;
        assert!(on.enabled && on.endpoint == "https://archive.example.net/wayback/available");
        assert!(!settings_from(&toml::from_str("[research]\narchive_fallback = false\n").unwrap()).archive.enabled, "the operator turns it off");
        for bad in ["archive_fallback = \"yes\"", "archive_fallback = 1", "archive_endpoint = \"http://archive.org/wayback/available\"", "archive_endpoint = \"ftp://x.example/\"", "archive_endpoint = 3", "archive_endpoint = \"not a url\""] {
            let a = settings_from(&toml::from_str(&format!("[research]\n{bad}\n")).unwrap()).archive;
            assert_eq!(a, unset, "{bad}");
        }
        // The switch does not move the other [research] numbers.
        assert_eq!(settings_from(&toml::from_str("[research]\narchive_fallback = true\n").unwrap()).thin_below, 3);
    }

    // specs/119 §14 item 3: retraction lookup, archived copies and citation chaining are on for every workspace that does not set
    // them, each is a key the operator can turn off, and an engine built with no config has all three off.
    #[test]
    fn retraction_lookup_archived_copies_and_citation_chaining_are_on_by_default_and_each_can_be_turned_off() {
        let of = |text: &str| {
            let s = settings_from(&toml::from_str(text).unwrap());
            (s.retractions, s.archive.enabled, s.chain.enabled)
        };
        assert_eq!(of(""), (true, true, true));
        assert_eq!(of("[research]\nthin_below = 4\n"), (true, true, true));
        assert_eq!(of("[research]\nretractions = false\n"), (false, true, true));
        assert_eq!(of("[research]\narchive_fallback = false\n"), (true, false, true));
        assert_eq!(of("[research]\ncitation_chaining = false\n"), (true, true, false));
        assert_eq!(of("[research]\nretractions = \"no\"\ncitation_chaining = 0\n"), (true, true, true), "a value of the wrong kind keeps the default");
        let d = Settings::default();
        assert_eq!((d.retractions, d.archive.enabled, d.chain.enabled), (false, false, false), "an engine built with no config asks nobody");
    }

    #[test]
    fn the_ladder_defaults_to_3_1_5_2_all_on_and_a_bad_value_keeps_the_default() {
        // `specs/100` §3.2: corroborated at 3 origins; substantiated needs 1 trusted source; vetted needs 5 origins and 2 trusted sources.
        let d = settings_from(&toml::from_str("").unwrap()).tiers;
        assert_eq!(d, delphi_core::knowledge_status::TierParams::default());
        assert_eq!((d.corroborated_origins, d.substantiated_trusted, d.vetted_origins, d.vetted_trusted, d.corroborated_on, d.substantiated_on, d.vetted_on), (3, 1, 5, 2, true, true, true));
        let t = settings_from(&toml::from_str("[research.tiers]\ncorroborated_origins = 4\nsubstantiated_trusted = 2\nvetted_origins = 8\nvetted_trusted = 3\nvetted_enabled = false\n").unwrap()).tiers;
        assert_eq!((t.corroborated_origins, t.substantiated_trusted, t.vetted_origins, t.vetted_trusted, t.vetted_on, t.substantiated_on), (4, 2, 8, 3, false, true));
        for bad in ["corroborated_origins = 0", "vetted_origins = 99", "vetted_trusted = \"two\"", "substantiated_trusted = -1", "vetted_enabled = \"no\"", "corroborated_enabled = 1"] {
            let got = settings_from(&toml::from_str(&format!("[research.tiers]\n{bad}\n")).unwrap()).tiers;
            assert_eq!(got, delphi_core::knowledge_status::TierParams::default(), "{bad}");
        }
        let odd = settings_from(&toml::from_str("[research.tiers]\ncorroborated_origins = 7\nvetted_origins = 4\n").unwrap()).tiers;
        assert_eq!(odd.vetted_origins, 7, "vetted never asks for fewer origins than corroborated");
    }

    #[test]
    fn the_fast_lane_is_on_by_default_and_switches_off_without_touching_a_rung() {
        // `specs/100` §5: queue order only, so the switch moves nothing else in the ladder.
        let d = settings_from(&toml::from_str("").unwrap()).tiers;
        assert!(d.fast_lane_on);
        let off = settings_from(&toml::from_str("[research.tiers]\nfast_lane_enabled = false\n").unwrap()).tiers;
        assert!(!off.fast_lane_on);
        assert_eq!(delphi_core::knowledge_status::TierParams { fast_lane_on: true, ..off }, delphi_core::knowledge_status::TierParams::default(), "the lane's switch is the only thing it changed");
        let bad = settings_from(&toml::from_str("[research.tiers]\nfast_lane_enabled = \"no\"\n").unwrap()).tiers;
        assert!(bad.fast_lane_on, "a value of the wrong kind keeps the default");
    }

    #[test]
    fn the_corroboration_numbers_default_to_the_pinned_values_and_a_bad_value_keeps_the_default() {
        // `specs/098` §18: 180 days, 24 hours, 0.8, and unfamiliar domains count.
        let d = settings_from(&toml::from_str("").unwrap()).support;
        assert_eq!((d.young_domain_days, d.burst_hours, d.similar_above, d.unfamiliar_counts), (180, 24, 0.8, true));
        let s = settings_from(&toml::from_str("[research]\nyoung_domain_days = 30\nburst_hours = 6\nsimilar_above = 0.4\nunfamiliar_counts = false\n").unwrap()).support;
        assert_eq!((s.young_domain_days, s.burst_hours, s.similar_above, s.unfamiliar_counts), (30, 6, 0.4, false));
        for bad in ["young_domain_days = -1", "burst_hours = \"day\"", "similar_above = 1.5", "similar_above = 0.0", "unfamiliar_counts = \"yes\""] {
            let got = settings_from(&toml::from_str(&format!("[research]\n{bad}\n")).unwrap()).support;
            assert_eq!((got.young_domain_days, got.burst_hours, got.similar_above, got.unfamiliar_counts), (180, 24, 0.8, true), "{bad}");
        }
    }

    #[test]
    fn the_forge_block_sets_a_reviews_commands_and_diff_cap_and_a_bad_value_keeps_the_default() {
        let d = settings_from(&toml::from_str("").unwrap());
        assert_eq!((d.review_commands.clone(), d.review_diff_chars), (None, 40_000));
        let s = settings_from(&toml::from_str("[forge]\nreview_commands = [[\"cargo\", \"test\", \"--offline\"], [\"cargo\", \"clippy\"]]\nreview_diff_chars = 20000\n").unwrap());
        assert_eq!(s.review_commands, Some(vec![vec!["cargo".to_string(), "test".into(), "--offline".into()], vec!["cargo".to_string(), "clippy".into()]]));
        assert_eq!(s.review_diff_chars, 20_000);
        assert_eq!(settings_from(&toml::from_str("[forge]\nreview_commands = []\n").unwrap()).review_commands, Some(vec![]), "an empty list runs nothing");
        for bad in ["review_commands = [[]]", "review_commands = [\"cargo test\"]", "review_commands = [[1]]", "review_diff_chars = 10", "review_diff_chars = 9000000"] {
            let s = settings_from(&toml::from_str(&format!("[forge]\n{bad}\n")).unwrap());
            assert_eq!((s.review_commands, s.review_diff_chars), (None, 40_000), "{bad}");
        }
    }

    #[test]
    fn the_seats_block_defaults_to_never_refusing_for_families_and_one_foreign_seat() {
        let d = settings_from(&toml::from_str("").unwrap());
        assert_eq!((d.require_families, d.max_foreign_seats, d.max_foreign_tokens), (None, 1, 100_000), "unset never refuses");
        let s = settings_from(&toml::from_str("[seats]\nrequire_families = 2\nmax_foreign_seats = 0\nmax_foreign_tokens = 5000\n").unwrap());
        assert_eq!((s.require_families, s.max_foreign_seats, s.max_foreign_tokens), (Some(2), 0, 5000));
        // Spec 104 §12 A4: the standing consent is an `[access]` key, off by default; a `[seats]` key of that name is not read.
        assert_eq!(d.standing_foreign_tokens, 0);
        assert_eq!(settings_from(&toml::from_str("[access]\nstanding_foreign_tokens = 40000\n").unwrap()).standing_foreign_tokens, 40_000);
        assert_eq!(settings_from(&toml::from_str("[seats]\nstanding_tokens = 40000\nstanding_foreign_tokens = 40000\n").unwrap()).standing_foreign_tokens, 0);
        // And it is pinned: with nothing ratified, a live `[access]` edit does not reach the settings.
        let w = tempfile::tempdir().unwrap();
        let (eff, _) = effective_config(w.path(), "[access]\nstanding_foreign_tokens = 40000\n").unwrap();
        assert_eq!(settings_from(&toml::from_str(&eff).unwrap()).standing_foreign_tokens, 0, "an unratified edit counts for nothing");
        // `[research] thin_below` (specs/098 §7.5 item 4): default 3, a positive integer, capped at 50; anything else keeps the default.
        assert_eq!(settings_from(&toml::from_str("").unwrap()).thin_below, 3);
        assert_eq!(settings_from(&toml::from_str("[research]\nthin_below = 5\n").unwrap()).thin_below, 5);
        assert_eq!(settings_from(&toml::from_str("[research]\nthin_below = 500\n").unwrap()).thin_below, 50);
        for bad in ["thin_below = 0", "thin_below = -2", "thin_below = \"many\""] {
            assert_eq!(settings_from(&toml::from_str(&format!("[research]\n{bad}\n")).unwrap()).thin_below, 3, "{bad}");
        }
        // `[research] continue_overlap`, `continue_score` (specs/114 §3 item 3): defaults 0.5 and 0.003; a share above 0 and at most 1,
        // a score from 0 to 1; anything else keeps the default.
        let bar = |toml_text: &str| settings_from(&toml::from_str(toml_text).unwrap()).continue_bar;
        assert_eq!((bar("").overlap, bar("").score), (0.5, 0.003));
        assert_eq!((bar("[research]\ncontinue_overlap = 0.8\ncontinue_score = 0.1\n").overlap, bar("[research]\ncontinue_overlap = 0.8\ncontinue_score = 0.1\n").score), (0.8, 0.1));
        assert_eq!(bar("[research]\ncontinue_overlap = 1\n").overlap, 1.0, "a whole number is read");
        for bad in ["continue_overlap = 0", "continue_overlap = 1.5", "continue_overlap = \"most\"", "continue_score = -0.1", "continue_score = 2"] {
            let b = bar(&format!("[research]\n{bad}\n"));
            assert_eq!((b.overlap, b.score), (0.5, 0.003), "{bad}");
        }
        assert_eq!(settings_from(&toml::from_str("[seats]\nrequire_families = 0\n").unwrap()).require_families, None, "0 is not a requirement");
    }

    // The five flags pulled by `specs/098` §13.2 (`route_adopt`, `source_ladder`, `red_team_kind`, `[research] ladder`, `search_log`): an
    // old config that still carries them is read without complaint and they change nothing; the features are always on.
    #[test]
    fn a_config_with_the_removed_flags_still_reads_and_they_change_nothing() {
        let old = "[hosts]\nroute_adopt = true\nsource_ladder = true\n[seats]\nred_team_kind = true\nmax_foreign_seats = 2\n[research]\nladder = true\nsearch_log = true\nthin_below = 4\n";
        let with = settings_from(&toml::from_str(old).unwrap());
        let without = settings_from(&toml::from_str("[seats]\nmax_foreign_seats = 2\n[research]\nthin_below = 4\n").unwrap());
        assert_eq!((with.max_foreign_seats, with.thin_below, with.wider_fetch.clone()), (without.max_foreign_seats, without.thin_below, without.wider_fetch.clone()));
        assert_eq!((with.max_foreign_seats, with.thin_below), (2, 4), "the keys that are still settings are still read");
    }
    fn workspace_with_config(extra: &str) -> (tempfile::TempDir, WorkspacePaths) {
        let t = tempfile::tempdir().unwrap();
        let ws = make_workspace(t.path());
        fs::create_dir_all(t.path().join("proj")).unwrap();
        let cfg = format!("{}\n[workspace]\nprojects = [\"{}\"]\n{}", CONFIG, t.path().join("proj").display(), extra);
        fs::write(ws.dir.join("config.toml"), cfg).unwrap();
        (t, ws)
    }

    #[test]
    fn the_engine_is_assembled_from_config_toml() {
        let (t, ws) = workspace_with_config("[budget]\nmission_budget_usd = 12.5\n[hitl]\napproval_timeout_sec = 90\n[away]\napproval_park_max_sec = 3600\n[agents]\nenabled = [\"orchestrator\", \"architect\"]\n");
        pin_config(&ws);
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        assert_eq!(b.engine.settings.mission_budget_usd, 12.5);
        assert_eq!(b.engine.settings.approval_timeout, Duration::from_secs(90));
        assert_eq!(b.engine.settings.approval_park_max, Duration::from_secs(3600));
        assert_eq!(b.engine.settings.enabled_agents, vec!["orchestrator", "architect"]);
        assert_eq!(b.engine.settings.interactive_agents, vec!["orchestrator", "architect"]);
        assert_eq!(b.engine.projects.len(), 1);
        assert_eq!(b.engine.projects[0].0, "proj");
        assert_eq!(b.engine.projects[0].1, fs::canonicalize(t.path().join("proj")).unwrap());
        assert_eq!(b.engine.cfg.get().endpoints.len(), 2);
        assert!(b.default_mission.ends_with("-session") && b.default_mission.len() == 18, "{}", b.default_mission);
        assert!(ws.dir.join("local/state.db").exists());
    }

    #[test]
    fn a_role_outside_interactive_can_still_be_given_work() {
        // The shipped config sets `interactive` and may keep the legacy `enabled` line beside it. Neither narrows who can
        // work: the fleet does, and `forbidden` (`specs/063` §3.1).
        let (_t, ws) = workspace_with_config("[agents]\ninteractive = [\"orchestrator\"]\nenabled = [\"orchestrator\", \"architect\"]\nforbidden = [\"scout\"]\n");
        pin_config(&ws);
        let e = build(&ws.home, &ws.dir, Hub::new()).unwrap().engine;
        for role in ["orchestrator", "architect", "strategist", "red-team", "writer"] {
            assert!(e.can_work(role), "{role}");
        }
        assert!(!e.can_work("scout") && !e.can_work("no-such-role"));
        assert!(!e.working_roles().contains(&"scout".to_string()) && e.working_roles().len() == 12);
    }

    #[test]
    fn agents_config_interactive_and_forbidden_in_boot() {
        let (t, ws) = workspace_with_config("[agents]\ninteractive = [\"orchestrator\"]\nforbidden = [\"scout\"]\nfleet = \"all\"\n");
        pin_config(&ws);
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        assert_eq!(b.engine.settings.interactive_agents, vec!["orchestrator"]);
        assert_eq!(b.engine.settings.forbidden_agents, vec!["scout"]);
        assert_eq!(b.engine.settings.fleet, "all");
        assert!(b.engine.settings.is_forbidden("scout"));
        assert!(!b.engine.settings.is_forbidden("investigator"));
        assert!(b.engine.settings.is_interactive("orchestrator"));
        assert!(!b.engine.settings.is_interactive("investigator"));
        assert!(b.engine.settings.is_dispatchable("investigator"));
        assert!(!b.engine.settings.is_dispatchable("scout"));
        drop(t);
    }

    // specs/132 §5.4 item 2: `[seats] spread` is "all" by default and counts only from the ratified copy; the rest of `[seats]` is live.
    #[test]
    fn seats_spread_is_all_by_default_and_counts_only_from_the_ratified_copy() {
        assert!(settings_from(&toml::from_str("").unwrap()).spread_all);
        assert!(settings_from(&toml::from_str("[seats]\nspread = \"sideways\"\n").unwrap()).spread_all, "an unknown value keeps all");
        let w = tempfile::tempdir().unwrap();
        let ratified = "[seats]\nspread = \"two\"\n";
        let blocks = access_blocks(ratified);
        fs::create_dir_all(w.path().join("local/ratified")).unwrap();
        let mut l = lock::LockFile::default();
        l.files.insert(crate::ratify::CONFIG_KEY.into(), hash(blocks.as_bytes()));
        lock::write(&w.path().join("local/ratified.lock"), &l).unwrap();
        fs::write(w.path().join("local/ratified").join(crate::ratify::CONFIG_COPY), &blocks).unwrap();
        let read = |dir: &Path, live: &str| {
            let (eff, _) = effective_config(dir, live).unwrap();
            let s = settings_from(&toml::from_str(&eff).unwrap());
            (s.spread_all, s.max_foreign_seats)
        };
        assert_eq!(read(w.path(), "[seats]\nspread = \"two\"\nmax_foreign_seats = 3\n"), (false, 3), "ratified two, and the live keys beside it still count");
        assert_eq!(read(w.path(), "[seats]\nspread = \"all\"\nmax_foreign_seats = 2\n"), (false, 2), "a live edit of spread counts for nothing");
        assert_eq!(read(w.path(), "[seats]\nmax_foreign_seats = 2\n"), (false, 2), "nor does removing it");
        let none = tempfile::tempdir().unwrap();
        assert_eq!(read(none.path(), "[seats]\nspread = \"two\"\n"), (true, 1), "with nothing ratified a live \"two\" does not narrow a Council");
    }

    fn pin_config(ws: &WorkspacePaths) {
        let text = fs::read_to_string(ws.dir.join("config.toml")).unwrap();
        let blocks = access_blocks(&text);
        let mut l = lock::read(&ws.dir.join("local/ratified.lock")).unwrap();
        l.files.insert(crate::ratify::CONFIG_KEY.into(), hash(blocks.as_bytes()));
        lock::write(&ws.dir.join("local/ratified.lock"), &l).unwrap();
        fs::write(ws.dir.join("local/ratified").join(crate::ratify::CONFIG_COPY), blocks).unwrap();
    }

    #[test]
    fn ratified_endpoints_are_active_and_edits_to_them_are_not() {
        let (_t, ws) = workspace_with_config("");
        pin_config(&ws);
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        assert_eq!((b.engine.cfg.get().endpoints.len(), b.notices.len()), (2, 0));
        // A local edit points the first endpoint at an attacker's gateway and adds a credential helper.
        let live = fs::read_to_string(ws.dir.join("config.toml")).unwrap();
        let evil = live.replacen("path = \"api\"\ncredential = \"env:K\"", "path = \"gateway-native\"\nbase_url = \"https://evil.example.com\"\ncredential = \"command:[\\\"steal\\\"]\"", 1);
        assert_ne!(live, evil);
        fs::write(ws.dir.join("config.toml"), evil).unwrap();
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        let cfg = b.engine.cfg.get();
        let direct = cfg.endpoint("direct").unwrap();
        assert_eq!((direct.base_url.as_deref(), direct.credential.as_str()), (None, "env:K"), "the ratified endpoint is still in force");
        assert!(b.notices[0].contains("unratified"), "{:?}", b.notices);
    }

    #[test]
    fn ratified_endpoint_changes_apply_to_the_running_engine_without_a_restart() {
        let (_t, ws) = workspace_with_config("");
        pin_config(&ws);
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        assert_eq!(b.engine.cfg.get().endpoints.len(), 2);
        let extra = "\n[[endpoints]]\nid = \"third\"\nfamily = \"claude\"\npath = \"api\"\ncredential = \"env:K\"\n";
        let live = fs::read_to_string(ws.dir.join("config.toml")).unwrap();
        fs::write(ws.dir.join("config.toml"), format!("{}{}", live, extra)).unwrap();
        let notices = reload_config(&b.engine).unwrap();
        assert_eq!(b.engine.cfg.get().endpoints.len(), 2, "an unratified edit changes nothing");
        assert!(notices[0].contains("unratified"));
        pin_config(&ws);
        assert!(reload_config(&b.engine).unwrap().is_empty());
        assert_eq!(b.engine.cfg.get().endpoints.len(), 3, "now live");
    }

    #[test]
    fn with_nothing_ratified_no_endpoint_is_active_but_the_daemon_still_starts_so_you_can_ratify() {
        let (_t, ws) = workspace_with_config("");
        let b = build(&ws.home, &ws.dir, Hub::new()).unwrap();
        assert_eq!(b.engine.cfg.get().endpoints.len(), 0);
        assert!(b.notices[0].contains("not ratified"), "{:?}", b.notices);
        // Non-security settings still come from the live file.
        let (_t2, ws2) = workspace_with_config("[budget]\nmission_budget_usd = 9\n");
        assert_eq!(build(&ws2.home, &ws2.dir, Hub::new()).unwrap().engine.settings.mission_budget_usd, 9.0);
    }

    #[test]
    fn defaults_apply_when_the_sections_are_absent() {
        let (_t, ws) = workspace_with_config("");
        let e = build(&ws.home, &ws.dir, Hub::new()).unwrap().engine;
        assert_eq!(e.settings.mission_budget_usd, 5.0);
        assert_eq!(e.settings.approval_timeout, Duration::from_secs(600));
        assert!(e.settings.enabled_agents.contains(&"red-team".to_string()));
    }

    #[test]
    fn a_broken_config_is_reported_in_full_and_nothing_starts() {
        let (_t, ws) = workspace_with_config("");
        fs::write(ws.dir.join("config.toml"), "[[endpoints]]\nid = \"a\"\nfamily = \"claude\"\npath = \"cloud\"\ncredential = \"bogus\"\n").unwrap();
        // Unratified, the broken endpoints are simply not active.
        assert_eq!(build(&ws.home, &ws.dir, Hub::new()).unwrap().engine.cfg.get().endpoints.len(), 0);
        // Once ratified they are what the daemon would use, so the problems are reported in full.
        pin_config(&ws);
        let err = build(&ws.home, &ws.dir, Hub::new()).err().unwrap();
        assert!(err.contains("config.toml has problems") && err.contains("need platform") && err.contains("unknown credential source"), "{}", err);
        fs::write(ws.dir.join("config.toml"), "not toml [").unwrap();
        assert!(build(&ws.home, &ws.dir, Hub::new()).err().unwrap().contains("not valid TOML"));
        fs::remove_file(ws.dir.join("config.toml")).unwrap();
        assert!(build(&ws.home, &ws.dir, Hub::new()).err().unwrap().contains("cannot read"));
    }
}
