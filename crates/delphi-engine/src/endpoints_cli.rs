//! `delphi endpoints test [id] [--probe]` (`specs/009` §6). Without `--probe` it only prints the cached
//! results (offline, free). With it, a minimal token-capped suite goes to allowed endpoints only.
//!
//! Runs through `delphi-core endpoints …`. Probe spend is outside any mission, so it is not in the usage
//! ledger; the command prints what it spent.

use crate::boot::{effective_config, http_model_client};
use crate::capabilities::{load, probe, save, Capability};
use crate::mission::SecretRegistry;
use delphi_adapter::config::{AccessPath, Config, SharedConfig};
use std::path::Path;

const TIERS: [&str; 3] = ["fast", "workhorse", "frontier"];

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn mark(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "yes",
        Some(false) => "NO",
        None => "-",
    }
}

fn print_row(c: &Capability, at: i64) {
    let age = (at - c.probed_at).max(0);
    let when = if c.is_fresh(at) { format!("{}m ago", age / 60) } else { format!("STALE ({}h ago)", age / 3600) };
    println!(
        "{:<16} {:<28} auth={:<12} tools={:<4} reasoning={:<4} signatures={:<4} usage={:<4} {}{}",
        c.endpoint_id,
        c.model_id,
        c.auth.chars().take(12).collect::<String>(),
        mark(c.tool_calling),
        mark(c.reasoning),
        mark(c.signature_round_trip),
        mark(c.usage_reported),
        when,
        if c.degraded { "  [degraded: compat gateway]" } else { "" }
    );
    for n in &c.notes {
        println!("    note: {}", n);
    }
}

pub fn run(ws_dir: &Path, args: &[String]) -> i32 {
    if args.first().map(String::as_str) != Some("test") {
        eprintln!("usage: delphi endpoints test [<endpoint id>] [--probe]");
        return 2;
    }
    let live = match std::fs::read_to_string(ws_dir.join("config.toml")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi endpoints: cannot read config.toml: {}", e);
            return 1;
        }
    };
    let cfg = match effective_config(ws_dir, &live).and_then(|(text, _)| Config::parse(&text).map_err(|e| e.join("; "))) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("delphi endpoints: {}", e);
            return 1;
        }
    };
    let only = args.get(1).filter(|a| !a.starts_with("--")).cloned();
    let cache = ws_dir.join("local/endpoint-capabilities.json");
    let allowed = cfg.allowed();
    let targets: Vec<_> = cfg.endpoints.iter().filter(|e| allowed.contains(&e.id) && only.as_ref().is_none_or(|o| &e.id == o)).collect();
    if let Some(o) = &only {
        if targets.is_empty() {
            eprintln!("delphi endpoints: `{}` is not an allowed endpoint (see allowed_endpoints in config.toml)", o);
            return 1;
        }
    }
    if !args.iter().any(|a| a == "--probe") {
        let cached = load(&cache);
        let mut shown = 0;
        for c in cached.iter().filter(|c| targets.iter().any(|t| t.id == c.endpoint_id)) {
            print_row(c, now());
            shown += 1;
        }
        if shown == 0 {
            println!("no cached probe results; run `delphi endpoints test --probe` (it sends a few capped requests to the allowed endpoints)");
        }
        return 0;
    }

    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("delphi endpoints: {}", e);
            return 1;
        }
    };
    let shared = SharedConfig::new(cfg.clone());
    let secrets = SecretRegistry::new();
    let results = rt.block_on(async {
        let client = http_model_client(shared, secrets);
        let mut out = Vec::new();
        for ep in &targets {
            if ep.path == AccessPath::Harness {
                println!("{:<16} skipped: harness endpoints are probed by the sandboxed harness check, not here", ep.id);
                continue;
            }
            let mut models: Vec<String> = TIERS.iter().filter_map(|t| cfg.resolve_model(ep, t)).collect();
            models.sort();
            models.dedup();
            if models.is_empty() {
                println!("{:<16} skipped: no model is configured for its family", ep.id);
            }
            for m in models {
                let cap = probe(&*client, &cfg, ep, &m, now()).await;
                print_row(&cap, cap.probed_at);
                out.push(cap);
            }
        }
        out
    });
    if let Err(e) = save(&cache, &results) {
        eprintln!("delphi endpoints: could not save results: {}", e);
        return 1;
    }
    let (tokens, cost): (u64, f64) = results.iter().fold((0, 0.0), |(t, c), r| (t + r.tokens_used, c + r.cost_usd.unwrap_or(0.0)));
    println!("probed {} endpoint x model pair(s); {} tokens, about ${:.4} (outside the mission ledger)", results.len(), tokens, cost);
    if results.iter().any(|r| r.auth != "ok") {
        1
    } else {
        0
    }
}
