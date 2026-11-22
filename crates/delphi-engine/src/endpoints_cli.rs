//! `delphi endpoints test [id] [--probe]` (`specs/009` §6). Without `--probe` it only prints the cached
//! results (offline, free). With it, a minimal token-capped suite goes to allowed endpoints only.
//!
//! Runs through `delphi-core endpoints …`. Probe spend is outside any mission, so it is not in the usage
//! ledger; the command prints what it spent.
//!
//! The probe is the carrier conformance probe of `specs/132` §4.1 (slice C6): it reads every harness CLI's version, prints the calls
//! and their estimated cost before anything runs, runs only on the operator's yes (`--yes`, or `y` at a terminal), and writes one
//! record per harness endpoint, model and CLI version to `local/carrier-conformance.json`.

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
    run_with(ws_dir, args, &confirm)
}

/// [`run`], with the operator's yes asked by `yes` (tests answer it without a terminal).
fn run_with(ws_dir: &Path, args: &[String], yes: &dyn Fn() -> bool) -> i32 {
    if args.first().map(String::as_str) != Some("test") {
        eprintln!("usage: delphi endpoints test [<endpoint id>] [--probe [--yes] [--ladder]]");
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

    let ladder = args.iter().any(|a| a == "--ladder");
    // Before anything runs: every harness CLI's version, and what the probe will cost (§4.1 steps 1 and 2).
    let mut versions: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
    println!("The probe will make these calls (outside any mission's ledger):");
    for ep in &targets {
        if ep.path == AccessPath::Harness {
            let cli = ep.harness.as_deref().and_then(crate::harness::Cli::from_harness);
            let program = cli.map_or("claude", |c| c.program());
            let version = versions.entry(program.to_string()).or_insert_with(|| crate::capabilities::cli_version_bounded(program, 10)).clone();
            let model = TIERS.iter().find_map(|t| cfg.resolve_model(ep, t)).unwrap_or_default();
            let gate = cli == Some(crate::harness::Cli::Agy) && cfg.harness.get("agy").is_none_or(|p| p.bridge);
            let multiplier = if program == "copilot" { copilot_multiplier(ws_dir, version.as_deref(), &model) } else { None };
            println!("  {} [{}]", crate::conformance::estimate_line(&ep.id, program, &model, crate::capabilities::harness_probe_prompt_bytes(gate), multiplier), version.as_deref().unwrap_or("version unknown"));
            if ladder {
                println!("  {}", crate::conformance::ladder_estimate_line(&ep.id, program, multiplier));
            }
        } else {
            println!("  {:<16} api: up to 3 calls per configured model, each capped at 64 to 512 output tokens plus the model's minimum thinking budget", ep.id);
        }
    }
    // The input ladder spends far more than the rest (`specs/131` S6): it runs only on the operator's y at a terminal, never on `--yes`.
    if ladder {
        println!("The input ladder runs only on y at a terminal; `--yes` does not cover it.");
        if !yes() {
            println!("nothing was run.");
            return 2;
        }
    } else if !args.iter().any(|a| a == "--yes") && !yes() {
        println!("nothing was run. Pass `--yes` to run these calls, or answer y at a terminal.");
        return 2;
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
                let program = ep.harness.as_deref().and_then(crate::harness::Cli::from_harness).map_or("claude", |c| c.program());
                let version = versions.get(program).cloned().flatten();
                let agy_version = if program == "agy" { version.clone() } else { None };
                let (cap, seat, gate) = crate::capabilities::probe_harness_full(&cfg, ep, program, now(), std::time::Duration::from_secs(10), agy_version).await;
                let bridge = cfg.harness.get(program).is_none_or(|p| p.bridge);
                let rec = crate::conformance::from_probe(&cap, seat.as_ref(), gate.as_ref(), program, bridge, &ep.family, version.as_deref());
                // C3, the input ladder (`specs/131` S6), after the bare call answered; without it, the last ladder's row on this version stays.
                let steps = if ladder && cap.auth == "ok" { crate::capabilities::probe_ladder(&cfg, ep, program, ladder_nonce(), std::time::Duration::from_secs(180)).await } else { vec![] };
                let rec = crate::conformance::with_ladder(rec.clone(), (!steps.is_empty()).then_some(steps.as_slice()), crate::conformance::same(&crate::conformance::load(&ws_dir.join(crate::conformance::FILE)), &rec));
                if let Err(e) = crate::conformance::save(&ws_dir.join(crate::conformance::FILE), &rec) {
                    eprintln!("delphi endpoints: could not save the conformance record: {}", e);
                }
                println!("    conformance: {}", rec.rows.iter().map(|r| format!("{} {}", r.row, r.status.as_str())).collect::<Vec<_>>().join(", "));
                // What the CLI said it can do goes where the daemon reads it, so the orchestrator knows before the first span.
                if let Some(seat) = seat {
                    crate::inventory::Inventory::open(ws_dir.join("local/harness-tools.json")).record(seat);
                }
                // What the conformance probe found out about the gate, tied to the CLI's version: a span reads it to decide how far to trust the seat.
                if let Some(gate) = gate {
                    if let Err(e) = crate::capabilities::save_gate(&ws_dir.join("local/agy-gate.json"), &gate) {
                        eprintln!("delphi endpoints: could not save the gate record: {}", e);
                    }
                }
                print_row(&cap, cap.probed_at);
                out.push(cap);
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

/// Copilot's multiplier for `model`: the cached catalog when it was read on the installed version, else the shipped seed table. A cost
/// input for the line printed before the probe, never a choice.
fn copilot_multiplier(ws_dir: &Path, version: Option<&str>, model: &str) -> Option<f64> {
    use crate::model_catalog as mc;
    let cat = version.and_then(|v| mc::fresh(&ws_dir.join("local"), v, now()));
    cat.as_ref().and_then(|c| c.model(model)).and_then(|m| m.multiplier).or_else(|| mc::Seed::shipped().model("copilot", model).map(|m| m.multiplier).filter(|m| *m > 0.0))
}

/// A fresh seed for the ladder's code words, so no two runs share one.
fn ladder_nonce() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0) ^ ((std::process::id() as u64) << 32)
}

/// The operator's yes at a terminal. Anything else (a pipe, a script, an agent) is a no: the probe spends the operator's tokens.
fn confirm() -> bool {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("Run these calls? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::lock;

    // specs/132 AC-11: the cost is printed before any call, and nothing runs without the operator's yes.
    #[test]
    fn the_probe_runs_nothing_without_the_operators_yes() {
        let w = tempfile::tempdir().unwrap();
        let cfg = "[[endpoints]]\nid = \"direct\"\nfamily = \"claude\"\npath = \"api\"\ncredential = \"env:DELPHI_TEST_NO_SUCH_KEY\"\n[models.claude]\nfast = \"m\"\n";
        std::fs::create_dir_all(w.path().join("local/ratified")).unwrap();
        std::fs::write(w.path().join("config.toml"), cfg).unwrap();
        let blocks = lock::access_blocks(cfg);
        std::fs::write(w.path().join("local/ratified").join(crate::ratify::CONFIG_COPY), &blocks).unwrap();
        let mut l = lock::LockFile::default();
        l.files.insert(crate::ratify::CONFIG_KEY.into(), lock::hash(blocks.as_bytes()));
        lock::write(&w.path().join("local/ratified.lock"), &l).unwrap();
        let asked = std::cell::Cell::new(0);
        let no = || {
            asked.set(asked.get() + 1);
            false
        };
        assert_eq!(run_with(w.path(), &["test".into(), "--probe".into()], &no), 2);
        assert_eq!(asked.get(), 1, "the operator was asked once");
        assert!(!w.path().join("local/endpoint-capabilities.json").exists() && !w.path().join(crate::conformance::FILE).exists(), "nothing ran, nothing was written");
        // Without --probe nothing is asked and nothing is spent.
        assert_eq!(run_with(w.path(), &["test".into()], &no), 0);
        assert_eq!(asked.get(), 1);
        // specs/131 S6: the input ladder asks at the terminal even with --yes, and runs nothing on a no.
        assert_eq!(run_with(w.path(), &["test".into(), "--probe".into(), "--yes".into(), "--ladder".into()], &no), 2);
        assert_eq!(asked.get(), 2, "--yes does not cover the ladder");
        assert!(!w.path().join("local/endpoint-capabilities.json").exists() && !w.path().join(crate::conformance::FILE).exists());
    }
}
