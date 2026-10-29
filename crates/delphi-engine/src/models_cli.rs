//! `delphi models [--probe]` (`specs/002` §4.1): the model pool, in preference order, with where each
//! `(endpoint, model)` pair stands. Without `--probe` it only reads the saved state (offline, free). With
//! it, every pool entry gets one capped call (the CLI's own probe for a harness), and the answer is
//! recorded. Probe spend is outside any mission, so it is not in the usage ledger.
//!
//! Runs through `delphi-core models …`. The daemon keeps the live state in memory and mirrors it to
//! `local/pool-state.json`; this command reads and updates that file, so a probe made here is picked up
//! by a running daemon the next time it restarts (it does not reread the file while running).

use crate::boot::{effective_config, http_model_client};
use crate::capabilities::{probe_alive, probe_harness};
use crate::mission::SecretRegistry;
use crate::model::ModelError;
use crate::pool::{describe, system_clock, Entry, Pool, State};
use delphi_adapter::config::{AccessPath, Config, SharedConfig};
use std::path::Path;

/// `delphi capabilities --models [--json]` (`specs/056` M0): every discovered model with its attributes and class.
pub fn inventory(ws_dir: &Path, home: &Path, json: bool) -> i32 {
    let live = match std::fs::read_to_string(ws_dir.join("config.toml")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi capabilities: cannot read config.toml: {}", e);
            return 1;
        }
    };
    let cfg = match effective_config(ws_dir, &live).and_then(|(text, _)| Config::parse(&text).map_err(|e| e.join("; "))) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("delphi capabilities: {}", e);
            return 1;
        }
    };
    match crate::models::run_inventory(&cfg, ws_dir, crate::hostmap::load(home).as_ref(), (system_clock())(), json) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(e) => {
            eprintln!("delphi capabilities: cannot save the inventory: {e}");
            1
        }
    }
}

pub fn run(ws_dir: &Path, args: &[String]) -> i32 {
    if let Some(bad) = args.iter().find(|a| *a != "--probe") {
        eprintln!("delphi models: unknown argument `{}`\nusage: delphi models [--probe]", bad);
        return 2;
    }
    let live = match std::fs::read_to_string(ws_dir.join("config.toml")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi models: cannot read config.toml: {}", e);
            return 1;
        }
    };
    let cfg = match effective_config(ws_dir, &live).and_then(|(text, _)| Config::parse(&text).map_err(|e| e.join("; "))) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("delphi models: {}", e);
            return 1;
        }
    };
    let entries = cfg.pool_entries();
    if entries.is_empty() {
        println!("{}", NONE_CONFIGURED);
        return 0;
    }
    let path = ws_dir.join("local/pool-state.json");
    let clock = system_clock();
    let pool = Pool::open(Some(path), cfg.routing.tuning.clone(), clock.clone());

    let mut failed = false;
    if args.iter().any(|a| a == "--probe") {
        let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("delphi models: {}", e);
                return 1;
            }
        };
        let shared = SharedConfig::new(cfg.clone());
        rt.block_on(async {
            let client = http_model_client(shared, SecretRegistry::new());
            for k in &entries {
                let Some(ep) = cfg.endpoint(&k.endpoint_id) else { continue };
                let (ok, note) = if ep.path == AccessPath::Harness {
                    let program = ep.harness.as_deref().and_then(crate::harness::Cli::from_harness).map_or("claude", |c| c.program());
                    let cap = probe_harness(&cfg, ep, program, clock()).await;
                    (cap.auth == "ok", cap.auth)
                } else {
                    match probe_alive(&*client, &cfg, ep, &k.model_id).await {
                        Ok(()) => (true, "ok".to_string()),
                        Err(ModelError::Exhausted(x)) => {
                            pool.record_exhausted(&k.endpoint_id, &k.model_id, &x);
                            (false, format!("exhausted: {}", x))
                        }
                        Err(ModelError::Throttled(m)) => {
                            pool.record_throttle(&k.endpoint_id, &k.model_id, None);
                            (false, format!("throttled: {}", m.chars().take(100).collect::<String>()))
                        }
                        Err(e) => (false, format!("failed: {}", e.to_string().chars().take(100).collect::<String>())),
                    }
                };
                if ok {
                    pool.record_success(&k.endpoint_id, &k.model_id);
                } else {
                    failed = true;
                }
                println!("probe {:<16} {:<28} {}", k.endpoint_id, k.model_id, note);
            }
        });
        println!();
    }

    print!("{}", render(&cfg, &pool.snapshot(), clock()));
    if failed {
        1
    } else {
        0
    }
}

/// What `delphi models` and `/models` say when nothing is configured (`specs/005e` §6.6), instead of an empty table.
pub const NONE_CONFIGURED: &str = "no model is configured for this workspace — run delphi setup";

/// The table `delphi models` prints: the pool in preference order, then the targets.
pub fn render(cfg: &Config, state: &[Entry], now: i64) -> String {
    if cfg.pool_entries().is_empty() {
        return format!("{}\n", NONE_CONFIGURED);
    }
    let mut out = format!("{:>2}  {:<16} {:<28} {:<10} state\n", "#", "endpoint", "model", "tier");
    let find = |endpoint_id: &str, model_id: &str| state.iter().find(|e| e.endpoint_id == endpoint_id && e.model_id == model_id);
    for (i, k) in cfg.pool_entries().iter().enumerate() {
        let line = find(&k.endpoint_id, &k.model_id).map(|e| describe(e, now)).unwrap_or_else(|| "available".into());
        out.push_str(&format!("{:>2}  {:<16} {:<28} {:<10} {}\n", i + 1, k.endpoint_id, k.model_id, k.tier, line));
    }
    if !cfg.routing.targets.is_empty() {
        out.push_str("\ntargets:\n");
        for (role, model) in &cfg.routing.targets {
            let up = cfg.endpoints_serving(model).iter().any(|k| find(&k.endpoint_id, &k.model_id).is_none_or(|e| e.state == State::Available));
            out.push_str(&format!("  {:<14} -> {}{}\n", role, model, if up { "" } else { "  (unavailable now; a similar model would be used)" }));
        }
    }
    out.push_str(&format!("\n{}\n", if cfg.routing.pool.is_some() { "order from [routing.pool]" } else { "no [routing.pool]: every configured tier, family by family, highest tier first" }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::Entry;

    const NOW: i64 = 1_790_000_000;

    fn cfg() -> Config {
        Config::parse(&format!("{}\n[routing.targets]\nred-team = \"gem-pro\"\n", crate::testkit::CONFIG.replace("[access]", "[models.gemini]\nfrontier = \"gem-pro\"\n[models.catalog.\"gem-pro\"]\nmax_output_tokens = 64000\ninput_usd_per_mtok = 2.0\noutput_usd_per_mtok = 12.0\n[[endpoints]]\nid = \"gem\"\nfamily = \"gemini\"\npath = \"api\"\ncredential = \"env:K\"\n[access]").replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"direct\", \"backup\", \"gem\"]"))).unwrap()
    }

    fn out_of_quota(endpoint_id: &str, model_id: &str, until: Option<i64>) -> Entry {
        Entry { endpoint_id: endpoint_id.into(), model_id: model_id.into(), state: State::Exhausted, since: NOW, until, reason: "quota reached".into(), throttles: vec![], probe_interval_secs: 900, next_probe_at: until.is_none().then_some(NOW + 900) }
    }

    #[test]
    fn an_empty_pool_says_no_model_is_configured_instead_of_printing_an_empty_table() {
        let none = Config::parse(crate::testkit::CONFIG.split("[[endpoints]]").next().unwrap()).unwrap();
        let text = render(&none, &[], NOW);
        assert_eq!(text, "no model is configured for this workspace — run delphi setup\n");
    }

    #[test]
    fn a_fresh_pool_lists_every_slot_available_and_the_targets_up() {
        let text = render(&cfg(), &[], NOW);
        assert!(text.lines().next().unwrap().contains("endpoint") && text.contains("direct") && text.contains("backup") && text.contains("gem-pro"), "{}", text);
        assert!(!text.contains("exhausted") && text.contains("red-team") && !text.contains("unavailable now"), "{}", text);
        assert!(text.contains("no [routing.pool]"));
    }

    #[test]
    fn an_exhausted_slot_shows_why_and_when_and_a_target_with_no_slot_left_says_so() {
        let state = [out_of_quota("gem", "gem-pro", Some(NOW + 136 * 3600)), out_of_quota("direct", "sonnet-x", None)];
        let text = render(&cfg(), &state, NOW);
        let gem = text.lines().find(|l| l.contains(" gem ")).unwrap();
        assert!(gem.contains("exhausted: quota reached") && gem.contains("in 5d 16h"), "{}", gem);
        let direct = text.lines().find(|l| l.contains("direct") && l.contains("sonnet-x")).unwrap();
        assert!(direct.contains("no reset time known, next probe in 15m"), "{}", direct);
        assert!(text.contains("red-team       -> gem-pro  (unavailable now; a similar model would be used)"), "{}", text);
        // The same model on another endpoint is untouched.
        assert!(text.lines().filter(|l| l.contains("backup")).all(|l| l.ends_with("available")), "{}", text);
    }
}
