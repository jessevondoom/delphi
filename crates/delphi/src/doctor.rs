//! `delphi doctor` (`specs/006` §5): eight read-only, offline checks reporting ok / warn / fail / skipped.

use crate::home::{self, hash, is_protected, read_lock, Workspace};
use crate::lint::{self, Mode, Tree};
use crate::report::{Level, Report};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Logical overlay of a workspace (`specs/006` §3.1): base < core for Zone 1; team; local; operator
/// files under `operator/`; user theme packs under `themes/`.
pub fn overlay(ws: &Workspace, home: &Path) -> Tree {
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    let add = |root: PathBuf, prefix: &str, files: &mut BTreeMap<String, PathBuf>| {
        for e in walkdir::WalkDir::new(&root).into_iter().flatten().filter(|e| !e.file_type().is_dir()) {
            let rel = e.path().strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            if rel == "manifest.lock" {
                continue;
            }
            files.insert(format!("{}{}", prefix, rel), e.path().to_path_buf());
        }
    };
    add(ws.dir.join("base"), "", &mut files);
    let builtin = files.clone();
    add(home.join("themes"), "themes/", &mut files);
    add(ws.dir.join("core"), "", &mut files);
    add(ws.dir.join("team"), "", &mut files);
    add(ws.dir.join("local/memory"), "memory/", &mut files);
    add(ws.dir.join("local/missions"), "missions/", &mut files);
    let local_heur = ws.dir.join("local/heuristics.md");
    if local_heur.exists() {
        files.insert("heuristics.md".into(), local_heur);
    }
    let op = home.join("operator");
    for f in ["profile.md", "heuristics.md"] {
        if op.join(f).exists() {
            files.insert(format!("operator/{}", f), op.join(f));
        }
    }
    for d in ["core", "skills", "memory"] {
        add(op.join(d), &format!("operator/{}/", d), &mut files);
    }
    // Files still byte-identical to the release seed (any zone) may link to unseeded design records.
    let seed: BTreeMap<String, &[u8]> = crate::seed::files().into_iter().map(|f| (f.logical, f.bytes)).collect();
    let seeded: BTreeSet<String> = files.iter().filter(|(k, p)| seed.get(*k).map(|b| fs::read(p).map(|x| x == *b).unwrap_or(false)).unwrap_or(false)).map(|(k, _)| k.clone()).collect();
    Tree { mode: Mode::Workspace, files, seeded, builtin }
}

/// `doctor` options beyond the workspace.
#[derive(Default)]
pub struct Options {
    /// `--require-providers`: no configured endpoint is a failure, not a warning (`specs/006` §5).
    pub require_providers: bool,
}

pub fn run(home: &Path, ws: &Workspace, opts: &Options) -> Report {
    let mut r = Report::default();
    let tree = overlay(ws, home);

    // 1. Binary ↔ daemon version .
    let socks = fs::read_dir(home.join("run")).map(|d| d.flatten().filter(|e| e.path().extension().map(|x| x == "sock").unwrap_or(false)).count()).unwrap_or(0);
    if socks == 0 {
        r.push(Level::Skipped, "1 daemon-version", "", "no daemon running");
    } else {
        r.push(Level::Warn, "1 daemon-version", "", "a daemon socket is present; `delphi daemon status` shows which release it runs");
    }

    // 2. Zone 1 manifest lock integrity (workspace and operator release snapshots).
    check_manifest_lock(&ws.dir.join("base"), "base", &mut r);
    check_manifest_lock(&home.join("operator/base"), "operator/base", &mut r);
    match fs::read_to_string(ws.dir.join("config.toml")).map_err(|e| e.to_string()).and_then(|t| toml::from_str::<toml::Value>(&t).map_err(|e| e.to_string())) {
        Ok(_) => {}
        Err(e) => r.fail("3 taint-audit", "config.toml", format!("unreadable or invalid TOML: {}", e)),
    }

    // 3. Taint audit: protected files match ratified.lock (drift = pending proposal), ratified
    //    copies are intact, overrides are narrow-only, and the active theme is ratified.
    check_ratified(ws, home, &tree, &mut r);
    check_overrides(ws, home, &mut r);
    check_zone_layout(ws, home, &mut r);

    // 4 + 5. Rule caps and links on the workspace overlay (lint checks reused).
    let lint_report = lint::run(&tree);
    let mut caps = 0;
    let mut links = 0;
    for f in lint_report.findings {
        match f.check.as_str() {
            "rule-caps" => {
                caps += 1;
                r.push(f.level, "4 rule-caps", f.location, f.message);
            }
            "links" | "abs-paths" => {
                if f.level != Level::Skipped {
                    links += 1;
                    r.push(f.level, "5 links", f.location, f.message);
                }
            }
            // Schema, theme and canonical-name findings (specs/001 §1.2, §6.4 rule 3).
            other => r.push(f.level, &format!("3 {}", other), f.location, f.message),
        }
    }
    if caps == 0 {
        r.push(Level::Ok, "4 rule-caps", "", "heuristics and lessons within the Rule of 25, 6 KB and 350-character caps");
    }
    if links == 0 {
        r.push(Level::Ok, "5 links", "", "all links resolve (links to unseeded design records skipped)");
    }

    // 6. Retrieval index: the FTS5 tables against the chunks, and the vectors against the installed model.
    if ws.dir.join("local/state.db").exists() {
        match delphi_engine::kb::health(home, &ws.dir) {
            Err(e) => r.fail("6 retrieval-index", "local/state.db", format!("{}; `delphi kb reindex` rebuilds it from the Markdown", e)),
            Ok(h) => match (&h.installed_model, &h.indexed_model) {
                (None, _) => r.push(Level::Ok, "6 retrieval-index", "", format!("{} document(s), {} chunk(s); no embedding model, so search is lexical only", h.documents, h.chunks)),
                (Some(m), indexed) if indexed.as_deref() != Some(m.as_str()) && h.chunks > 0 => {
                    r.push(Level::Warn, "6 retrieval-index", "", format!("the index has no vectors from the installed model {}; run `delphi kb index`", m))
                }
                (Some(m), _) if h.vectors < h.chunks => r.push(Level::Warn, "6 retrieval-index", "", format!("{} of {} chunk(s) have no vector yet (model {}); run `delphi kb index`", h.chunks - h.vectors, h.chunks, m)),
                (Some(m), _) => r.push(
                    Level::Ok,
                    "6 retrieval-index",
                    "",
                    format!("{} document(s), {} chunk(s), all with vectors from {}; {}", h.documents, h.chunks, m, h.reranker.as_ref().map(|r| format!("results reranked by {}", r)).unwrap_or_else(|| "results are not reranked".into())),
                ),
            },
        }
    } else {
        r.push(Level::Skipped, "6 retrieval-index", "", "no state.db yet");
    }

    // 7. Model access: offline, from config only (probing is `--probe`).
    let cfg = fs::read_to_string(ws.dir.join("config.toml")).unwrap_or_default();
    let endpoints = toml::from_str::<toml::Value>(&cfg).ok().and_then(|v| v.get("endpoints").and_then(|e| e.as_array()).map(|a| a.len())).unwrap_or(0);
    if endpoints == 0 && opts.require_providers {
        r.fail("7 model-access", "", "no endpoints configured and --require-providers was given (specs/009)");
    } else if endpoints == 0 {
        r.warn("7 model-access", "", "no endpoints configured: no model is configured for this workspace — run delphi setup (or `delphi endpoints add`, specs/009)");
    } else {
        r.push(Level::Ok, "7 model-access", "", format!("{} endpoint(s) configured; `delphi endpoints test --probe` tests them", endpoints));
        // Cached probe results (specs/009 §6): read offline, never refreshed here.
        let cache = fs::read_to_string(ws.dir.join("local/endpoint-capabilities.json")).ok().and_then(|t| serde_json::from_str::<Vec<serde_json::Value>>(&t).ok()).unwrap_or_default();
        if cache.is_empty() {
            r.warn("7 model-access", "local/endpoint-capabilities.json", "no probe results yet: run `delphi endpoints test --probe` to learn what each endpoint really supports");
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        for c in &cache {
            let name = format!("{} / {}", c["endpoint_id"].as_str().unwrap_or("?"), c["model_id"].as_str().unwrap_or("?"));
            if c["auth"].as_str() != Some("ok") {
                r.warn("7 model-access", name, format!("last probe: {}", c["auth"].as_str().unwrap_or("unknown")));
            } else if now - c["probed_at"].as_i64().unwrap_or(0) >= 24 * 3600 {
                r.warn("7 model-access", name, "probe results are over 24 hours old: re-run `delphi endpoints test --probe`");
            } else if c["signature_round_trip"] == serde_json::Value::Bool(false) {
                r.warn("7 model-access", name, "reasoning signatures did not survive a round trip; Delphi will drop them for this endpoint");
            }
        }
    }

    // 7b. Connectors (specs/010 §9), offline: what is configured and whether its tool definitions are ratified.
    check_connectors(ws, &cfg, &mut r);

    // 8. Hygiene: clean projects, orphaned worktrees.
    check_hygiene(ws, home, &mut r);

    // 12. Added org bundles (specs/024 §4.6): the lock, the cache entry it points at, review dates.
    check_bundles(ws, home, &mut r);

    // 13. Joined Xenia stores (specs/011 §4.1): fewer than three admins, a freshness statement past half its life,
    //     tampering found by the last sync, contested records. Offline: it reads what the last sync accepted.
    check_xenia(home, delphi_xenia_now(), &mut r);

    // 10. Subscription harnesses (specs/009 §7B): installed and signed in, from the CLIs' own status commands.
    check_harnesses(&cfg, &run_cli, opts.require_providers, &mut r);
    check_coverage_drift(&cfg, &help_page, &mut r);
    check_mcp_policy(&cfg, &delphi_engine::policy::local_policy(&std::env::current_exe().unwrap_or_default()), &std::env::current_exe().unwrap_or_default(), &mut r);

    // 11. Model pool (specs/002 §4.1): which models are out of quota right now, from the daemon's saved state.
    check_pool(ws, &cfg, &mut r);

    // 9. Sandbox (D-24): optional. Reported, never a warning unless the config asks for it and it is missing.
    let wanted = toml::from_str::<toml::Value>(&cfg).ok().and_then(|v| v.get("exec").and_then(|e| e.get("sandbox")).and_then(|s| s.as_str().map(String::from))).unwrap_or_else(|| "off".into());
    match (delphi_sandbox::detect(), wanted.as_str()) {
        (Some(mode), "prefer" | "required") => r.push(Level::Ok, "9 sandbox", "", format!("{} is active: commands run sandboxed and need no approval card", mode.name())),
        (Some(mode), _) => r.push(Level::Ok, "9 sandbox", "", format!("off by choice ({} is available; set `[exec] sandbox = \"prefer\"` to use it): commands ask for approval, like Claude Code", mode.name())),
        (None, "required") => r.fail("9 sandbox", "", "`[exec] sandbox = \"required\"` but no OS sandbox is available (sandbox-exec on macOS, bubblewrap on Linux): no command will run"),
        (None, "prefer") => r.warn("9 sandbox", "", "`[exec] sandbox = \"prefer\"` but no OS sandbox is available: commands run directly and ask for approval"),
        (None, _) => r.push(Level::Ok, "9 sandbox", "", "off (no OS sandbox is available either): commands ask for approval"),
    }
    // 14. The central error log (specs/039): the effective level and where it came from, the file, its size and the drops.
    check_error_log(home, &mut r);

    // Checks are pushed as they are cheap to run; show them in their numbered order.
    r.findings.sort_by_key(|f| f.check.split(' ').next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(u32::MAX));
    r
}

/// `14 error-log`: read-only; says what is recorded and where, and warns when records were lost.
fn check_error_log(home: &Path, r: &mut Report) {
    let st = delphi_core::elog::status(home);
    let level = st.settings.level.as_str();
    let size = if st.exists { format!("{} bytes in {} file(s)", st.bytes, st.rotated_files + 1) } else { "not created yet (nothing has been logged)".to_string() };
    let msg = format!("level {} (from {}); {}; cap {} bytes x {} file(s); {} record(s) reported dropped", level, st.settings.source, size, st.settings.max_bytes, st.settings.files, st.dropped_reported);
    let loc = st.path.display().to_string();
    if st.settings.level == delphi_core::elog::Level::Off {
        r.push(Level::Ok, "14 error-log", loc, format!("off ({}): errors are not recorded; `DELPHI_LOG=error` or `[logging] level` turns it on", msg));
    } else if !st.writable {
        r.warn("14 error-log", loc, format!("the log directory is not writable, so records are dropped: {}", msg));
    } else if st.dropped_reported > 0 {
        r.warn("14 error-log", loc, format!("records were dropped (disk full or the directory was unwritable at the time): {}", msg));
    } else {
        r.push(Level::Ok, "14 error-log", loc, msg);
    }
}

/// (exit code, stdout) of a program, or `None` if it cannot be started.
type CliRun = Option<(i32, String)>;

/// Run a program with arguments and capture (exit code, stdout); `None` if it cannot be started.
fn run_cli(program: &str, args: &[&str]) -> CliRun {
    let out = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    Some((out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// Does the organization's managed MCP policy let Delphi's tool bridge load (`specs/020` §7)? Only the policy files on this
/// machine can be read; a policy delivered from the admin console cannot, which the finding says.
fn check_mcp_policy(cfg_text: &str, policy: &delphi_engine::policy::McpPolicy, exe: &std::path::Path, r: &mut Report) {
    use delphi_engine::policy::McpPolicy;
    let Ok(v) = toml::from_str::<toml::Value>(cfg_text) else { return };
    let has_claude = v.get("endpoints").and_then(|e| e.as_array()).is_some_and(|l| l.iter().any(|e| e.get("path").and_then(|p| p.as_str()) == Some("harness") && e.get("harness").and_then(|h| h.as_str()) == Some("claude")));
    let bridge_on = v.get("harness").and_then(|h| h.get("claude")).and_then(|c| c.get("bridge")).and_then(|b| b.as_bool()).unwrap_or(true);
    if !has_claude || !bridge_on {
        return;
    }
    match (policy, delphi_engine::policy::explain(policy, exe)) {
        (McpPolicy::NoLocalPolicy, _) => r.push(Level::Ok, "10 harness", "mcp policy", "no managed Claude Code settings on this machine. An organization can also deliver them from its admin console, which cannot be read from here: `delphi endpoints test --probe` shows whether the Delphi tool bridge and the account's connectors actually load"),
        (_, None) => r.push(Level::Ok, "10 harness", "mcp policy", "managed Claude Code settings are present and do not keep the Delphi tool bridge out"),
        (_, Some(why)) => r.warn("10 harness", "mcp policy", why),
    }
}

/// For every `path = "harness"` endpoint: is its CLI installed, and is it signed in? Claude has
/// `claude auth status`; `agy` has no status command, so its sign-in is proven by
/// `delphi endpoints test --probe` (one tiny call). Account details such as the email are never printed.
fn check_harnesses(cfg_text: &str, run: &dyn Fn(&str, &[&str]) -> CliRun, require: bool, r: &mut Report) {
    let Ok(v) = toml::from_str::<toml::Value>(cfg_text) else { return };
    let Some(list) = v.get("endpoints").and_then(|e| e.as_array()) else { return };
    let missing = |r: &mut Report, id: &str, msg: String| if require { r.fail("10 harness", id.to_string(), msg) } else { r.warn("10 harness", id.to_string(), msg) };
    for e in list.iter().filter(|e| e.get("path").and_then(|p| p.as_str()) == Some("harness")) {
        let id = e.get("id").and_then(|i| i.as_str()).unwrap_or("?");
        match e.get("harness").and_then(|h| h.as_str()) {
            Some("claude") => match run("claude", &["--version"]) {
                None => missing(r, id, "the `claude` program was not found on PATH; install Claude Code, or this endpoint is skipped (API endpoints take over if any)".into()),
                Some((_, ver)) => match run("claude", &["auth", "status", "--json"]).and_then(|(_, out)| serde_json::from_str::<serde_json::Value>(&out).ok()) {
                    Some(s) if s["loggedIn"] == true => {
                        // The ratified profile (`specs/020`): what Delphi will ask this CLI to do. Whether an organization's own
                        // MCP policy lets the bridge load can only be seen by starting the CLI: `endpoints test --probe`.
                        let p = v.get("harness").and_then(|h| h.get("claude"));
                        let (bridge, approvals, connectors) = (p.and_then(|p| p.get("bridge")).and_then(|b| b.as_bool()).unwrap_or(true), p.and_then(|p| p.get("approvals")).and_then(|a| a.as_str()).unwrap_or("delegate"), p.and_then(|p| p.get("claudeai_connectors")).and_then(|b| b.as_bool()).unwrap_or(true));
                        r.push(Level::Ok, "10 harness", id.to_string(), format!("claude {} is signed in ({}, {} plan); the approval bridge needs no sandbox. Profile: Delphi tool bridge {}, approvals by {}, account connectors {}. `delphi endpoints test --probe` confirms the bridge loads under your organization's MCP policy", ver.trim(), s["authMethod"].as_str().unwrap_or("?"), s["subscriptionType"].as_str().unwrap_or("?"), if bridge { "on" } else { "off (roles run text-only)" }, approvals, if connectors { "on" } else { "off" }))
                    }
                    Some(_) => missing(r, id, "`claude` is installed but not signed in: run `claude auth login`".into()),
                    None => r.warn("10 harness", id.to_string(), "`claude auth status` gave no usable answer; run `delphi endpoints test --probe`"),
                },
            },
            Some("agy") => match run("agy", &["--version"]) {
                None => missing(r, id, "the `agy` program was not found on PATH; install the Antigravity CLI".into()),
                Some((_, ver)) => {
                    // `[harness.agy]` (`specs/020` §8): with the bridge on (the default) a span carries Delphi's tools in a plugin and a
                    // hook gates its other tools; off, it is the old read-only, untrusted seat. Either way: no file edits or commands.
                    let bridge = v.get("harness").and_then(|h| h.get("agy")).and_then(|a| a.get("bridge")).and_then(|b| b.as_bool()).unwrap_or(true);
                    let what = if bridge { "Delphi's tools reach it through a plugin and a hook denies its other tools, and its output is untrusted until `delphi endpoints test --probe` has watched the gate hold on this version (after that it starts as trusted as any role, and drops to untrusted when it reads the web); it is never a Builder" } else { "the bridge is off: read-only roles only, nothing gates its own tools, output is untrusted" };
                    r.push(Level::Ok, "10 harness", id.to_string(), format!("agy {} is installed; it has no status command, so run `delphi endpoints test --probe` to prove it is signed in. {}", ver.trim(), what))
                }
            },
            Some("gemini") => r.warn("10 harness", id.to_string(), "the Gemini CLI no longer supports personal Google sign-in (it points to Antigravity); use `harness = \"agy\"`"),
            other => r.warn("10 harness", id.to_string(), format!("the `{}` harness is not implemented yet", other.unwrap_or("?"))),
        }
    }
}

/// A CLI's `--help` page for the words given (stdout and stderr together: some print usage to stderr); `None` if it cannot be started.
fn help_page(program: &str, words: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program).args(words).arg("--help").stdin(std::process::Stdio::null()).output().ok()?;
    Some(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// For every CLI a `path = "harness"` endpoint uses: does it show flags or commands that `coverage/<cli>.cov` has no
/// disposition for (`specs/043` §6)? The CLIs ship weekly, so this is the alarm that a release added surface. Read-only:
/// it runs `--help` pages only, never a command.
fn check_coverage_drift(cfg_text: &str, help: &dyn Fn(&str, &[&str]) -> Option<String>, r: &mut Report) {
    let Ok(v) = toml::from_str::<toml::Value>(cfg_text) else { return };
    let Some(list) = v.get("endpoints").and_then(|e| e.as_array()) else { return };
    let mut seen = std::collections::BTreeSet::new();
    for e in list.iter().filter(|e| e.get("path").and_then(|p| p.as_str()) == Some("harness")) {
        let Some(cli) = e.get("harness").and_then(|h| h.as_str()).filter(|c| delphi_engine::coverage::CLIS.contains(c)) else { continue };
        if !seen.insert(cli.to_string()) {
            continue;
        }
        let run = |words: &[&str]| help(cli, words);
        let Some(items) = delphi_engine::coverage::drift(cli, &run) else { continue }; // not installed: check 10 says so
        if items.is_empty() {
            r.push(Level::Ok, "10 harness", format!("{} surface", cli), "every flag and command in its --help has a disposition in the coverage manifest (specs/043)");
        } else {
            let shown: Vec<String> = items.iter().take(6).map(|i| i.describe()).collect();
            let more = if items.len() > 6 { format!(" and {} more", items.len() - 6) } else { String::new() };
            r.warn("10 harness", format!("{} surface", cli), format!("the installed {} shows {}{} with no disposition in crates/delphi-engine/coverage/{}.cov: it is newer than this Delphi knows. Nothing breaks (Delphi passes only the flags it uses); a Delphi update classifies it", cli, shown.join(", "), more, cli));
        }
    }
}

fn check_pool(ws: &Workspace, cfg_text: &str, r: &mut Report) {
    let Ok(cfg) = delphi_adapter::config::Config::parse(cfg_text) else { return }; // a bad config is reported by the checks above
    let configured = cfg.pool_entries().len();
    if configured == 0 {
        r.push(Level::Skipped, "11 model pool", "", "no model is configured yet");
        return;
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let stuck: Vec<_> = delphi_engine::pool::view(&ws.dir.join("local/pool-state.json"), now).into_iter().filter(|e| e.state == delphi_engine::pool::State::Exhausted).collect();
    if stuck.is_empty() {
        r.push(Level::Ok, "11 model pool", "", format!("{} model slot(s) configured, none known to be out of quota", configured));
    }
    for e in stuck {
        r.warn("11 model pool", format!("{}/{}", e.endpoint_id, e.model_id), delphi_engine::pool::describe(&e, now));
    }
}

fn check_manifest_lock(base: &Path, label: &str, r: &mut Report) {
    let path = base.join("manifest.lock");
    let lock = match read_lock(&path) {
        Ok(l) => l,
        Err(e) => return r.fail("2 manifest-lock", format!("{}/manifest.lock", label), e.to_string()),
    };
    let mut bad = 0;
    for (logical, h) in &lock {
        match fs::read(base.join(logical)) {
            Ok(b) if hash(&b) == *h => {}
            Ok(_) => {
                bad += 1;
                r.fail("2 manifest-lock", format!("{}/{}", label, logical), "release snapshot was modified (it is read-only)");
            }
            Err(_) => {
                bad += 1;
                r.fail("2 manifest-lock", format!("{}/{}", label, logical), "release snapshot file is missing");
            }
        }
    }
    let upstream: Vec<String> = walkdir::WalkDir::new(base.parent().unwrap_or(base))
        .into_iter()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".upstream-"))
        .map(|e| e.path().display().to_string())
        .collect();
    for u in &upstream {
        r.warn("2 manifest-lock", u.clone(), "unresolved upstream merge");
    }
    for e in walkdir::WalkDir::new(base).into_iter().flatten().filter(|e| !e.file_type().is_dir()) {
        let rel = e.path().strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
        if rel != "manifest.lock" && !lock.contains_key(&rel) {
            bad += 1;
            r.fail("2 manifest-lock", format!("{}/{}", label, rel), "file is not part of the release snapshot");
        }
    }
    let signed = fs::read_to_string(&path).map(|t| !t.contains("unsigned-dev-build")).unwrap_or(false);
    if label == "base" {
        match crate::release::status() {
            delphi_xenia::release_sig::Check::Invalid(why) => r.fail("2 manifest-lock", "", format!("this build's release signature is invalid: {why}")),
            delphi_xenia::release_sig::Check::Signed { kid, .. } if signed => r.push(Level::Ok, "2 manifest-lock", "", format!("signed release (key {kid})")),
            delphi_xenia::release_sig::Check::Signed { .. } => r.warn("2 manifest-lock", "", "this build is a signed release but the workspace is still at an older one; `delphi chat` or `delphi update` refreshes it"),
            delphi_xenia::release_sig::Check::Unsigned => {
                if !signed {
                    r.warn("2 manifest-lock", "", "development build: not a signed release");
                }
                if crate::update::config_requires_signed(base.parent().unwrap_or(base)) {
                    r.warn("2 manifest-lock", "", "this workspace requires signed releases (`[update] require_signed`) and this build is not one: `delphi update` will refuse");
                }
            }
        }
    }
    if bad == 0 {
        r.push(Level::Ok, "2 manifest-lock", "", format!("{} release files match {}/manifest.lock", lock.len(), label));
    }
}

fn check_ratified(ws: &Workspace, home: &Path, tree: &Tree, r: &mut Report) {
    let lock = match read_lock(&ws.dir.join("local/ratified.lock")) {
        Ok(l) => l,
        Err(e) => return r.fail("3 taint-audit", "local/ratified.lock", e.to_string()),
    };
    let mut drift = 0;
    let mut unratified = 0;
    let mut broken = 0;
    for (logical, phys) in &tree.files {
        if logical.starts_with("operator/") || !is_protected(logical) {
            continue;
        }
        let live = hash(&fs::read(phys).unwrap_or_default());
        match lock.get(logical) {
            Some(h) if *h == live => {}
            Some(_) => {
                drift += 1;
                r.warn("3 taint-audit", logical.clone(), "differs from the ratified copy: staged as a pending proposal; agents keep loading local/ratified/");
            }
            None => {
                unratified += 1;
                r.warn("3 taint-audit", logical.clone(), "not ratified: pending proposal; not loaded until the operator ratifies it");
            }
        }
    }
    // A ratified file whose live copy vanished is drift too (the overlay can't see it).
    for logical in lock.keys().filter(|k| !k.contains('#')) {
        if !tree.files.contains_key(logical) {
            drift += 1;
            r.warn("3 taint-audit", logical.clone(), "live file is missing: pending proposal to delete it; agents keep loading the ratified copy");
        }
    }
    // A lock that lost entries must not pass silently: every file `init` ratified is required.
    for req in required_workspace_entries() {
        if !lock.contains_key(&req) {
            broken += 1;
            r.fail("3 taint-audit", format!("local/ratified.lock: {}", req), "required entry missing from the lock");
        }
    }
    // The ratified copies are what agents load: each must exist and match its lock entry.
    broken += verify_copies(&lock, &ws.dir.join("local/ratified"), "local/ratified", r);
    // Approved specs must be ratified by exact hash (specs/003 §3.1.2, 006-A2).
    for (logical, phys) in tree.files.iter().filter(|(k, _)| k.starts_with("specs/") && k.ends_with(".md")) {
        let text = fs::read_to_string(phys).unwrap_or_default();
        let approved = crate::frontmatter::parse(&text).ok().and_then(|d| d.frontmatter).map(|m| crate::frontmatter::get_str(&m, "status") == Some("approved") || m.contains_key(serde_yaml::Value::String("approved_sections".into()))).unwrap_or(false);
        if approved && lock.get(logical) != Some(&hash(text.as_bytes())) {
            broken += 1;
            r.fail("3 taint-audit", logical.clone(), "marked approved but its exact hash is not ratified by the operator");
        }
    }
    let cfg = fs::read_to_string(ws.dir.join("config.toml")).unwrap_or_default();
    if lock.get("config.toml#access").map(|h| *h != hash(home::access_blocks(&cfg).as_bytes())).unwrap_or(true) {
        drift += 1;
        r.warn("3 taint-audit", "config.toml", "endpoint/access/auth blocks differ from the ratified version: Red-tier proposal (specs/009 §5)");
    }
    let theme = toml::from_str::<toml::Value>(&cfg).ok().and_then(|v| v.get("theme").and_then(|t| t.get("name")).and_then(|n| n.as_str()).map(String::from)).unwrap_or_else(|| "greek".into());
    if !lock.contains_key(&format!("themes/{}/theme.md", theme)) {
        broken += 1;
        r.fail("3 taint-audit", format!("themes/{}/theme.md", theme), "configured theme is not ratified (specs/001 §6.4); it will not load");
    }
    // Operator scope (001-A1): fail closed when the lock is unreadable.
    let op = home.join("operator");
    match read_lock(&op.join("ratified.lock")) {
        Err(e) => {
            broken += 1;
            r.fail("3 taint-audit", "operator/ratified.lock", format!("unreadable: {}", e));
        }
        Ok(op_lock) => {
            for req in home::operator_required_entries() {
                if !op_lock.contains_key(&req) {
                    broken += 1;
                    r.fail("3 taint-audit", format!("operator/ratified.lock: {}", req), "required entry missing from the lock");
                }
            }
            for (name, h) in &op_lock {
                let live = fs::read(op.join(name)).map(|b| hash(&b)).unwrap_or_default();
                if live != *h {
                    drift += 1;
                    r.warn("3 taint-audit", format!("operator/{}", name), "differs from the ratified copy: pending proposal");
                }
            }
            for (logical, _) in tree.files.iter().filter(|(k, _)| k.starts_with("operator/")) {
                let name = &logical["operator/".len()..];
                if !op_lock.contains_key(name) {
                    unratified += 1;
                    r.warn("3 taint-audit", logical.clone(), "not ratified in operator/ratified.lock: pending proposal; not loaded");
                }
            }
            broken += verify_copies(&op_lock, &op.join("ratified"), "operator/ratified", r);
        }
    }
    if drift == 0 && unratified == 0 && broken == 0 {
        r.push(Level::Ok, "3 taint-audit", "", "every protected file and ratified copy matches its lock");
    }
}

fn required_workspace_entries() -> Vec<String> {
    let mut v: Vec<String> = crate::seed::files().into_iter().map(|f| f.logical).filter(|l| is_protected(l)).collect();
    v.extend(["context/team-heuristics.md", "heuristics.md", "config.toml#access"].iter().map(|s| s.to_string()));
    v
}

/// Every lock entry (except pinned config blocks) must have an intact copy under `dir`, and `dir`
/// must hold nothing else: the path guard serves protected files from here.
fn verify_copies(lock: &BTreeMap<String, String>, dir: &Path, label: &str, r: &mut Report) -> usize {
    let mut broken = 0;
    for e in walkdir::WalkDir::new(dir).into_iter().flatten().filter(|e| !e.file_type().is_dir()) {
        let rel = e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
        // `config.access.toml` is the ratified copy of the pinned endpoint blocks (`config.toml#access`).
        if rel == "config.access.toml" {
            let ok = lock.get("config.toml#access").map(|h| fs::read(e.path()).map(|b| hash(&b) == *h).unwrap_or(false)).unwrap_or(false);
            if !ok {
                broken += 1;
                r.fail("3 taint-audit", format!("{}/{}", label, rel), "ratified endpoint config does not match its pin (tampered or planted)");
            }
        } else if !lock.contains_key(&rel) {
            broken += 1;
            r.fail("3 taint-audit", format!("{}/{}", label, rel), "file in the ratified directory has no lock entry (planted)");
        }
    }
    for (logical, h) in lock.iter().filter(|(k, _)| !k.contains('#')) {
        match fs::read(dir.join(logical)) {
            Ok(b) if hash(&b) == *h => {}
            Ok(_) => {
                broken += 1;
                r.fail("3 taint-audit", format!("{}/{}", label, logical), "ratified copy does not match the lock (tampered)");
            }
            Err(_) => {
                broken += 1;
                r.fail("3 taint-audit", format!("{}/{}", label, logical), "ratified copy is missing");
            }
        }
    }
    broken
}

/// Zone discipline (`specs/006` §3.1): `team/` may not shadow Zone 1 release paths (overrides live in
/// `agents/custom/` and `skills/custom/`, which get narrow-only checks), and no zone may contain
/// symlinks, which would let a live file stand in for something outside the Delphi home.
fn check_zone_layout(ws: &Workspace, home: &Path, r: &mut Report) {
    let team = ws.dir.join("team");
    for e in walkdir::WalkDir::new(&team).into_iter().flatten().filter(|e| !e.file_type().is_dir()) {
        let rel = e.path().strip_prefix(&team).unwrap().to_string_lossy().replace('\\', "/");
        let rel = rel.to_lowercase();
        let zone1 = rel == "governance.md"
            || rel == "spec.md"
            || rel == "context/heuristics.md"
            || rel.starts_with("themes/")
            || (rel.starts_with("agents/") && !rel.starts_with("agents/custom/"))
            || (rel.starts_with("skills/") && !rel.starts_with("skills/custom/"));
        if zone1 {
            r.fail("3 zones", format!("team/{}", rel), "team/ may not shadow a release path; overrides go in agents/custom/ or skills/custom/");
        }
    }
    let roots = [ws.dir.join("base"), ws.dir.join("core"), team, ws.dir.join("local"), home.join("operator"), home.join("themes")];
    for root in roots {
        // A dangling symlink at a zone root must not look like an absent zone.
        if fs::symlink_metadata(&root).is_err() {
            continue;
        }
        for e in walkdir::WalkDir::new(&root) {
            match e {
                Ok(e) if e.path_is_symlink() => r.fail("3 zones", e.path().display().to_string(), "symlinks are not allowed in Delphi zones"),
                Ok(_) => {}
                // Unreadable directories would otherwise drop their files from every check.
                Err(err) => r.fail("3 zones", err.path().map(|p| p.display().to_string()).unwrap_or_default(), format!("cannot read: {}", err)),
            }
        }
    }
}

/// Narrow-only (`specs/001` §2.3, 001-A1): `core/` and `operator/core/` overrides get the full manifest
/// checks, compared against the built-in in `base/`.
/// Each configured connector should have ratified pins for its tools; without them nothing is offered to agents.
/// Reads only the ratified copies. Probing the servers is `delphi connectors test <name>`.
fn check_connectors(ws: &Workspace, cfg: &str, r: &mut Report) {
    let Some(table) = toml::from_str::<toml::Value>(cfg).ok().and_then(|v| v.get("connectors").and_then(|c| c.as_table().cloned())) else { return };
    let lock = delphi_core::lock::read(&ws.dir.join("local/ratified.lock")).ok();
    for (name, c) in &table {
        let logical = format!("connectors/{}.pins.json", name);
        let ratified = fs::read(ws.dir.join("local/ratified").join(&logical)).ok();
        let intact = match (&ratified, lock.as_ref().and_then(|l| l.files.get(&logical))) {
            (Some(bytes), Some(want)) => &delphi_core::lock::hash(bytes) == want,
            _ => false,
        };
        let listed = c.get("tools").and_then(|t| t.as_table()).map(|t| t.len()).unwrap_or(0);
        let location = format!("[connectors.{}]", name);
        if intact {
            r.push(Level::Ok, "7b connectors", location, format!("{} tool(s) in the catalog, definitions ratified; `delphi connectors test {}` checks the server", listed, name));
        } else if ratified.is_some() {
            r.fail("7b connectors", location, "the ratified tool pins do not match the lock: the copy was changed outside ratification");
        } else {
            r.warn("7b connectors", location, format!("no ratified tool definitions, so agents are offered none: run `delphi connectors pin {}`, then /ratify", name));
        }
    }
}

fn check_overrides(ws: &Workspace, home: &Path, r: &mut Report) {
    let tree = overlay(ws, home);
    for (root, label) in [(ws.dir.join("core/agents"), "core/agents"), (home.join("operator/core/agents"), "operator/core/agents")] {
        let entries = match fs::read_dir(&root) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for e in entries.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            let path = format!("{}/{}/AGENT.md", label, id);
            let text = fs::read_to_string(e.path().join("AGENT.md")).unwrap_or_default();
            let (fails, warns) = lint::manifest_findings(&tree, &text, &id, Some(lint::builtin_fm(&tree, &id)));
            for f in fails {
                r.fail("3 overrides", path.clone(), f);
            }
            for w in warns {
                r.warn("3 overrides", path.clone(), w);
            }
        }
    }
}

fn delphi_xenia_now() -> i64 {
    delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(0)
}

/// Check 13: one finding per warning of each joined store, or one line saying all is well. Nothing is printed when no
/// store is joined.
pub(crate) fn check_xenia(home: &Path, now: i64, r: &mut Report) {
    use delphi_core::xenia_status::{self, Severity};
    let console_admin = delphi_core::xenia_admin_console::console_admin_enabled(home);
    // On is the notable state and gets its own line; off (the default) is said at the end of each healthy store's line.
    if console_admin && !xenia_status::list_stores(home).is_empty() {
        r.push(Level::Ok, "13 xenia", "console admin", "console admin signing is ON (operator/xenia.toml): the console may sign roster proposals that touch no admin, one card at a time, 5 an hour; admin changes stay in the terminal".to_string());
    }
    for (store, st) in xenia_status::all(home, &delphi_core::xenia_sync::VerifyConfig::at(now)) {
        let loc = store.id();
        match st {
            Err(e) => r.fail("13 xenia", loc, format!("the store cannot be read: {e}. {}", delphi_core::xenia_help::what_to_do("store-unreadable").unwrap_or_default())),
            Ok(st) => {
                for w in &st.warnings {
                    let text = format!("{}. {}", w.text.trim_end_matches('.'), delphi_core::xenia_help::what_to_do(w.id).unwrap_or_default());
                    match w.severity {
                        Severity::Fail => r.fail("13 xenia", loc.clone(), text),
                        Severity::Warn => r.warn("13 xenia", loc.clone(), text),
                    }
                }
                if st.warnings.is_empty() {
                    let f = st.freshness.as_ref();
                    r.push(Level::Ok, "13 xenia", loc, format!("{} admin(s), statement {}% of its life used, nothing contested or quarantined by the last sync{}", st.roster.as_ref().map(|x| x.admins).unwrap_or(0), f.and_then(|f| f.life_used_pct).unwrap_or(0), if console_admin { "" } else { "; console admin signing is off (set `[xenia] console_admin = true` in operator/xenia.toml to allow the limited console form)" }));
                }
            }
        }
    }
}

fn check_bundles(ws: &Workspace, home: &Path, r: &mut Report) {
    let lock = match delphi_core::bundles::read(&ws.dir) {
        Ok(l) => l,
        Err(e) => return r.fail("12 bundles", "local/bundles.lock", e),
    };
    let today = delphi_core::bundles::today();
    let active: Vec<_> = lock.bundle.iter().filter(|e| e.is_active()).collect();
    for e in &active {
        let cache = delphi_core::bundles::cache_dir(home, &e.org, &e.name, &e.hash);
        let loc = e.id();
        match fs::read(cache.join(".delphi-bundle/files.json")) {
            Err(_) => r.fail("12 bundles", loc.clone(), format!("cache entry {} is missing; add the bundle again from {}", cache.display(), e.source.path)),
            Ok(bytes) if delphi_bundle::bundle_hash(&bytes) != e.hash => r.fail("12 bundles", loc.clone(), "cache entry's files.json no longer matches the locked hash (the cache was changed)"),
            Ok(_) => {}
        }
        if e.is_stale(&today) {
            r.warn("12 bundles", loc, format!("{} is past its review date ({}); it ranks lower until its publisher releases a reviewed version", e.version, e.review_by));
        }
        // A newer version at the bundle's source (`specs/024` §3.5). Information: it is applied only by `delphi bundle update`.
        // A source that cannot be looked at (moved, a store) says nothing here; `/org` shows why.
        let check = delphi_bundle::source_check::check_in(home, &e.source.kind, &e.source.path, &e.hash, &e.publisher_kid);
        if let Some(line) = check.line() {
            r.warn("12 bundles", e.id(), format!("update available: {} -> {} at {}; `delphi bundle update {}` applies it", e.version, line, e.source.path, e.name));
        }
    }
    // How far each bundle's vectors are (`specs/027` §5.4): search is lexical for a bundle until they are complete, which
    // is normal for a while after an add and so is information, not a warning. A cache that failed its spot check is
    // worth a look: its publisher's vectors did not match what this install's model makes from the same text.
    let entries: Vec<_> = active.iter().map(|e| (*e).clone()).collect();
    let status = delphi_engine::bundle_vectors::statuses(home, &ws.dir, &entries);
    for e in &active {
        for v in status.get(&e.id()).map(|s| s.rejected.as_slice()).unwrap_or_default() {
            r.warn("12 bundles", e.id(), format!("the shipped vector cache {} failed its spot check and was discarded; the chunks are embedded locally instead", v.file));
        }
    }
    if !active.is_empty() && r.findings.iter().all(|f| !f.check.starts_with("12") || f.level == Level::Ok) {
        let vectors: Vec<String> = entries.iter().filter_map(|e| status.get(&e.id()).filter(|s| s.total > 0).map(|s| format!("{} {}", e.id(), s.label()))).collect();
        let pending = entries.iter().any(|e| status.get(&e.id()).is_some_and(|s| s.total > 0 && !s.complete()));
        let tail = if vectors.is_empty() { String::new() } else { format!("; vectors: {}{}", vectors.join(", "), if pending { " (search is lexical only for a bundle until its vectors are complete)" } else { "" }) };
        r.push(Level::Ok, "12 bundles", "local/bundles.lock", format!("{} added bundle(s), caches intact, none stale{}", active.len(), tail));
    }
}

fn check_hygiene(ws: &Workspace, home: &Path, r: &mut Report) {
    use std::os::unix::fs::PermissionsExt;
    let mut problems = 0;
    for p in home::projects(&ws.dir) {
        let loc = p.display().to_string();
        if !p.exists() {
            problems += 1;
            r.warn("8 hygiene", loc, "registered project path does not exist");
            continue;
        }
        if fs::symlink_metadata(p.join(".delphi")).is_ok() {
            problems += 1;
            r.fail("8 hygiene", loc.clone(), "project contains a `.delphi` entry; Delphi state belongs in the Delphi home (D-03)");
        }
        if !p.join(".git").exists() {
            problems += 1;
            r.warn("8 hygiene", loc.clone(), "registered project is not a git repository");
        }
        // Git worktree registrations whose directory is gone (`git worktree prune` candidates).
        if let Ok(entries) = fs::read_dir(p.join(".git/worktrees")) {
            for e in entries.flatten() {
                let gitdir = fs::read_to_string(e.path().join("gitdir")).unwrap_or_default();
                if !gitdir.trim().is_empty() && !e.path().join(gitdir.trim()).exists() {
                    problems += 1;
                    r.warn("8 hygiene", e.path().display().to_string(), "stale git worktree registration");
                }
            }
        }
    }
    if let Ok(entries) = fs::read_dir(ws.dir.join("worktrees")) {
        for e in entries.flatten() {
            if !e.path().join(".git").exists() {
                problems += 1;
                r.warn("8 hygiene", e.path().display().to_string(), "orphaned worktree directory");
            }
        }
    }
    let run = home.join("run");
    if let Ok(m) = fs::metadata(&run) {
        if m.permissions().mode() & 0o077 != 0 {
            problems += 1;
            r.fail("8 hygiene", run.display().to_string(), "run/ must be 0700");
        }
    }
    if let Ok(entries) = fs::read_dir(&run) {
        for e in entries.flatten() {
            if e.path().extension().map(|x| x == "sock").unwrap_or(false) {
                let mode = fs::symlink_metadata(e.path()).map(|m| m.permissions().mode() & 0o777).unwrap_or(0);
                if mode & 0o077 != 0 {
                    problems += 1;
                    r.fail("8 hygiene", e.path().display().to_string(), format!("socket mode {:o}; must be 0600", mode));
                }
            }
        }
    }
    if problems == 0 {
        r.push(Level::Ok, "8 hygiene", "", "projects clean; no orphaned worktrees; run/ private");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::{init, InitOptions};
    use std::process::Command;

    fn setup() -> (tempfile::TempDir, PathBuf, Workspace, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let proj = t.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        Command::new("git").args(["init", "-q"]).current_dir(&proj).status().unwrap();
        let ws = init(&InitOptions { home: home.clone(), workspace: "w".into(), projects: vec![proj.clone()], theme: "greek".into() }).unwrap();
        (t, home, ws, proj)
    }

    // specs/043 §6: a CLI whose --help shows something the manifest does not know is a warning; a CLI not installed is check 10's business.
    #[test]
    fn coverage_drift_warns_on_new_surface_and_is_quiet_when_classified() {
        let cfg = "[[endpoints]]\nid = \"c\"\nfamily = \"claude\"\npath = \"harness\"\nharness = \"claude\"\n";
        let newer = |_: &str, w: &[&str]| -> Option<String> { w.is_empty().then(|| "Options:\n  --print   print\n  --brand-new-flag   x\n".to_string()) };
        let mut r = Report::default();
        check_coverage_drift(cfg, &newer, &mut r);
        let f = r.findings.iter().find(|f| f.check == "10 harness" && f.location == "claude surface").expect("a finding");
        assert!(f.level == Level::Warn && f.message.contains("--brand-new-flag") && !f.message.contains("`--print`"), "{:?}", f.message);
        let none = |_: &str, _: &[&str]| -> Option<String> { None };
        let mut r2 = Report::default();
        check_coverage_drift(cfg, &none, &mut r2);
        assert!(r2.findings.iter().all(|f| f.location != "claude surface"));
        let quiet = |_: &str, w: &[&str]| -> Option<String> { w.is_empty().then(|| "Options:\n  --print   print\n".to_string()) };
        let mut r3 = Report::default();
        check_coverage_drift(cfg, &quiet, &mut r3);
        assert!(r3.findings.iter().any(|f| f.location == "claude surface" && f.level == Level::Ok));
    }

    // specs/006 §6 test 6 (doctor part): a fresh offline init passes doctor with exit 0.
    #[test]
    fn fresh_init_passes() {
        let (_t, home, ws, _p) = setup();
        let r = run(&home, &ws, &Options::default());
        r.print();
        assert_eq!(r.exit_code(), 0);
        assert!(r.findings.iter().any(|f| f.check.starts_with("3") && f.level == Level::Ok));
        assert!(r.findings.iter().any(|f| f.check.starts_with("5") && f.level == Level::Ok));
    }

    // specs/024 §4.6: doctor says when an added bundle's cache is gone or changed, and when it is past its review date.
    #[test]
    fn added_bundles_are_checked_for_their_cache_and_their_review_date() {
        let (_t, home, ws, _p) = setup();
        let r = run(&home, &ws, &Options::default());
        assert!(r.findings.iter().all(|f| !f.check.starts_with("12")), "no bundles, no finding");
        let files_json = b"{\"bundle\":1}";
        let hash = delphi_bundle::bundle_hash(files_json);
        let entry = delphi_core::bundles::Entry {
            org: "acme".into(),
            name: "plat".into(),
            version: "1.0.0".into(),
            hash: hash.clone(),
            publisher_kid: "unsigned".into(),
            provenance: "unsigned".into(),
            source: delphi_core::bundles::Source { kind: "directory".into(), path: "/src/plat".into() },
            added_at: "2026-10-25T00:00:00Z".into(),
            order: 10,
            status: "active".into(),
            review_by: "2999-01-01".into(),
            title: "T".into(),
            skill_roles: Default::default(),
            history: Vec::new(),
            inert_servers: Vec::new(),
            trust: String::new(),
            trust_hash: String::new(),
            trust_kid: String::new(),
            trust_log: Vec::new(),
        };
        delphi_core::bundles::write(&ws.dir, &delphi_core::bundles::Lock { bundle: vec![entry.clone()], ..Default::default() }).unwrap();
        let r = run(&home, &ws, &Options::default());
        let f = r.findings.iter().find(|f| f.check == "12 bundles").expect("a finding");
        assert!(f.level == Level::Fail && f.message.contains("missing"), "{:?}", f);
        assert_eq!(r.exit_code(), 1);
        let cache = delphi_core::bundles::cache_dir(&home, "acme", "plat", &hash);
        fs::create_dir_all(cache.join(".delphi-bundle")).unwrap();
        fs::write(cache.join(".delphi-bundle/files.json"), files_json).unwrap();
        let r = run(&home, &ws, &Options::default());
        assert!(r.findings.iter().any(|f| f.check == "12 bundles" && f.level == Level::Ok), "intact cache");
        fs::write(cache.join(".delphi-bundle/files.json"), b"{\"bundle\":2}").unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.check == "12 bundles" && f.level == Level::Fail && f.message.contains("no longer matches")));
        fs::write(cache.join(".delphi-bundle/files.json"), files_json).unwrap();
        let stale = delphi_core::bundles::Entry { review_by: "2020-01-01".into(), ..entry };
        delphi_core::bundles::write(&ws.dir, &delphi_core::bundles::Lock { bundle: vec![stale], ..Default::default() }).unwrap();
        let r = run(&home, &ws, &Options::default());
        assert!(r.findings.iter().any(|f| f.check == "12 bundles" && f.level == Level::Warn && f.message.contains("past its review date")));
        assert_eq!(r.exit_code(), 0, "stale is a warning");
    }

    // specs/002 §4.1: doctor says which models are out of quota, from the daemon's saved pool state.
    #[test]
    fn an_exhausted_model_is_a_warning_and_a_fresh_pool_is_ok() {
        let (_t, home, ws, _p) = setup();
        let r = run(&home, &ws, &Options::default());
        assert!(r.findings.iter().any(|f| f.check == "11 model pool" && f.level == Level::Skipped), "placeholders only: nothing configured");
        let mut cfg = fs::read_to_string(ws.dir.join("config.toml")).unwrap();
        cfg = cfg.replace("workhorse = \"<claude workhorse model id>\"", "workhorse = \"sonnet-x\"");
        cfg.push_str("\n[[endpoints]]\nid = \"direct\"\nfamily = \"claude\"\npath = \"api\"\ncredential = \"env:K\"\n");
        fs::write(ws.dir.join("config.toml"), cfg).unwrap();
        let r = run(&home, &ws, &Options::default());
        assert!(r.findings.iter().any(|f| f.check == "11 model pool" && f.level == Level::Ok && f.message.contains("1 model slot")), "{:?}", r.findings.iter().filter(|f| f.check.starts_with("11")).map(|f| &f.message).collect::<Vec<_>>());
        let until = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64 + 3 * 86_400;
        fs::write(ws.dir.join("local/pool-state.json"), format!(r#"{{"entries":[{{"endpoint_id":"direct","model_id":"sonnet-x","state":"exhausted","since":1,"until":{},"reason":"quota reached","throttles":[],"probe_interval_secs":900,"next_probe_at":null}}]}}"#, until)).unwrap();
        let r = run(&home, &ws, &Options::default());
        let f = r.findings.iter().find(|f| f.check == "11 model pool" && f.level == Level::Warn).expect("a warning for the exhausted pair");
        assert!(f.location == "direct/sonnet-x" && f.message.contains("quota reached") && (f.message.contains("in 2d") || f.message.contains("in 3d")), "{:?}", f);
        assert_eq!(r.exit_code(), 0, "a spent quota is a warning, not a failure");
    }

    // specs/010 §9 (offline part): a connector with no ratified tool definitions offers agents nothing, and doctor says so.
    #[test]
    fn a_connector_without_ratified_pins_warns_and_a_tampered_pin_file_fails() {
        let (_t, home, ws, _p) = setup();
        let mut cfg = fs::read_to_string(ws.dir.join("config.toml")).unwrap();
        cfg.push_str("\n[connectors.docs]\nurl = \"https://mcp.example.com/mcp\"\nhosts = [\"mcp.example.com\"]\nroles = [\"orchestrator\"]\ntools = { search = \"read\" }\n");
        fs::write(ws.dir.join("config.toml"), cfg).unwrap();
        let r = run(&home, &ws, &Options::default());
        assert!(r.findings.iter().any(|f| f.check == "7b connectors" && f.level == Level::Warn && f.message.contains("delphi connectors pin docs")), "{:?}", r.findings.iter().map(|f| (&f.check, &f.message)).collect::<Vec<_>>());
        // Ratified pins that match the lock are fine; a copy changed behind the lock's back fails.
        let pins = "{\"connector\": \"docs\", \"tools\": {}}\n";
        fs::create_dir_all(ws.dir.join("local/ratified/connectors")).unwrap();
        fs::write(ws.dir.join("local/ratified/connectors/docs.pins.json"), pins).unwrap();
        let mut lock = delphi_core::lock::read(&ws.dir.join("local/ratified.lock")).unwrap();
        lock.files.insert("connectors/docs.pins.json".into(), delphi_core::lock::hash(pins.as_bytes()));
        delphi_core::lock::write(&ws.dir.join("local/ratified.lock"), &lock).unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.check == "7b connectors" && f.level == Level::Ok));
        fs::write(ws.dir.join("local/ratified/connectors/docs.pins.json"), "{\"tools\": {\"evil\": {}}}").unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.check == "7b connectors" && f.level == Level::Fail));
    }

    // specs/006 §6 test 7
    #[test]
    fn drift_is_reported_as_pending_proposal() {
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("team/REVIEW.md"), "tampered").unwrap();
        fs::create_dir_all(ws.dir.join("core/agents/builder")).unwrap();
        fs::write(ws.dir.join("core/agents/builder/AGENT.md"), "override").unwrap();
        let r = run(&home, &ws, &Options::default());
        let drift: Vec<_> = r.findings.iter().filter(|f| f.check.starts_with("3") && f.level == Level::Warn).map(|f| f.location.clone()).collect();
        assert!(drift.contains(&"REVIEW.md".to_string()) && drift.contains(&"agents/builder/AGENT.md".to_string()), "{:?}", drift);
        assert!(fs::read_to_string(ws.dir.join("local/ratified/REVIEW.md")).unwrap().len() > 100);
    }

    // specs/006 §6 test 10
    #[test]
    fn clean_repo_rule_is_provenance_not_content() {
        let (_t, home, ws, proj) = setup();
        fs::write(proj.join("GOVERNANCE.md"), "a project may contain anything").unwrap();
        fs::create_dir_all(proj.join("agents/builder")).unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.check.starts_with("8") && f.level == Level::Ok));
        fs::create_dir_all(proj.join(".delphi")).unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
    }

    #[test]
    fn tampered_or_missing_ratified_copies_fail() {
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("local/ratified/GOVERNANCE.md"), "tampered").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::remove_file(home.join("operator/ratified.lock")).unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
    }

    #[test]
    fn lint_findings_and_widening_overrides_fail_doctor() {
        let (_t, home, ws, _p) = setup();
        let base = fs::read_to_string(ws.dir.join("base/agents/scout/AGENT.md")).unwrap();
        fs::create_dir_all(ws.dir.join("core/agents/scout")).unwrap();
        fs::write(ws.dir.join("core/agents/scout/AGENT.md"), base.replace("write: [", "write: [\"specs/**\", ")).unwrap();
        let r = run(&home, &ws, &Options::default());
        assert!(r.findings.iter().any(|f| f.check.starts_with("3") && f.level == Level::Fail && f.message.contains("widens")), "{:#?}", r.findings);
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("team/context/bad.md"), "no frontmatter\n").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
    }

    #[test]
    fn unratified_theme_and_require_providers() {
        let (_t, home, ws, _p) = setup();
        let cfg = fs::read_to_string(ws.dir.join("config.toml")).unwrap().replace("name = \"greek\"", "name = \"norse\"");
        fs::write(ws.dir.join("config.toml"), cfg).unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.message.contains("theme is not ratified")));
        let (_t, home, ws, _p) = setup();
        assert_eq!(run(&home, &ws, &Options { require_providers: true }).exit_code(), 1);
    }

    #[test]
    fn emptied_locks_planted_copies_and_bad_config_fail() {
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("local/ratified.lock"), "{\"files\": {}}").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::write(home.join("operator/ratified.lock"), "{}").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::create_dir_all(ws.dir.join("local/ratified/agents/custom/builder")).unwrap();
        fs::write(ws.dir.join("local/ratified/agents/custom/builder/AGENT.md"), "planted").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("config.toml"), "not = [valid").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::write(home.join("operator/base/GOVERNANCE.md"), "edited").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
    }

    #[test]
    fn core_override_cannot_attach_out_of_scope_skill() {
        let (_t, home, ws, _p) = setup();
        let base = fs::read_to_string(ws.dir.join("base/agents/scout/AGENT.md")).unwrap();
        fs::create_dir_all(ws.dir.join("core/agents/scout")).unwrap();
        fs::write(ws.dir.join("core/agents/scout/AGENT.md"), base.replace("authorized_skills:\n", "authorized_skills:\n  - evil\n")).unwrap();
        fs::create_dir_all(ws.dir.join("team/skills/custom/evil")).unwrap();
        fs::write(ws.dir.join("team/skills/custom/evil/SKILL.md"), "---\nname: evil\ndescription: d\nauthorized_agents: [scout]\ninput_schema: {type: object}\noutput_schema: {type: object}\nrequired_permissions: [write:worktree]\n---\nb\n").unwrap();
        let r = run(&home, &ws, &Options::default());
        assert!(r.findings.iter().any(|f| f.check == "3 overrides" && f.message.contains("evil")), "{:#?}", r.findings);
    }

    #[test]
    fn team_shadowing_symlinks_and_missing_live_files() {
        let (_t, home, ws, _p) = setup();
        fs::create_dir_all(ws.dir.join("team/agents/scout")).unwrap();
        fs::copy(ws.dir.join("base/agents/scout/AGENT.md"), ws.dir.join("team/agents/scout/AGENT.md")).unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.check == "3 zones" && f.level == Level::Fail));
        let (t, home, ws, _p) = setup();
        fs::write(t.path().join("elsewhere.md"), "x").unwrap();
        std::os::unix::fs::symlink(t.path().join("elsewhere.md"), ws.dir.join("team/context/link.md")).unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::remove_file(ws.dir.join("team/context/team-heuristics.md")).unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.location == "context/team-heuristics.md" && f.message.contains("missing")));
    }

    // An operator override and skill carry manifest/skill schemas, not §5 provenance frontmatter.
    #[test]
    fn case_variant_shadowing_and_unreadable_dirs_fail() {
        use std::os::unix::fs::PermissionsExt;
        let (_t, home, ws, _p) = setup();
        fs::create_dir_all(ws.dir.join("team/Agents/scout")).unwrap();
        fs::write(ws.dir.join("team/Agents/scout/AGENT.md"), "x").unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.check == "3 zones" && f.level == Level::Fail));
        let (_t, home, ws, _p) = setup();
        let d = ws.dir.join("team/context/locked");
        fs::create_dir_all(&d).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o000)).unwrap();
        let failed = run(&home, &ws, &Options::default()).exit_code();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(failed, 1);
    }

    #[test]
    fn operator_overrides_are_not_provenance_documents() {
        let (_t, home, ws, _p) = setup();
        let base = fs::read_to_string(ws.dir.join("base/agents/scout/AGENT.md")).unwrap();
        fs::create_dir_all(home.join("operator/core/agents/scout")).unwrap();
        fs::write(home.join("operator/core/agents/scout/AGENT.md"), base).unwrap();
        let r = run(&home, &ws, &Options::default());
        assert!(!r.findings.iter().any(|f| f.check == "3 frontmatter"), "{:#?}", r.findings);
        // A faithful copy of the built-in keeps its relative links resolving (counterpart path).
        assert!(!r.findings.iter().any(|f| f.check == "5 links" && f.level == Level::Fail), "{:#?}", r.findings);
    }

    #[test]
    fn modified_release_snapshot_fails() {
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("base/GOVERNANCE.md"), "edited").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("base/extra.md"), "planted").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
    }

    #[test]
    fn the_organizations_mcp_policy_is_reported_with_a_snippet_an_administrator_can_use() {
        use delphi_engine::policy::McpPolicy;
        let cfg = "[[endpoints]]\nid=\"c\"\nfamily=\"claude\"\npath=\"harness\"\nharness=\"claude\"\ncredential=\"harness-login\"\n";
        let exe = std::path::Path::new("/opt/delphi/bin/delphi");
        let run = |policy: McpPolicy, cfg: &str| {
            let mut r = Report::default();
            check_mcp_policy(cfg, &policy, exe, &mut r);
            r.findings.iter().map(|f| (f.level, f.message.clone())).collect::<Vec<_>>()
        };
        let none = run(McpPolicy::NoLocalPolicy, cfg);
        assert!(none.len() == 1 && none[0].0 == Level::Ok && none[0].1.contains("admin console") && none[0].1.contains("endpoints test --probe"), "{:?}", none);
        assert_eq!(run(McpPolicy::Allowed, cfg)[0].0, Level::Ok);
        let blocked = run(McpPolicy::NotListed, cfg);
        assert!(blocked[0].0 == Level::Warn && blocked[0].1.contains("allowedMcpServers") && blocked[0].1.contains("\"serverName\": \"delphi\"") && blocked[0].1.contains("/opt/delphi/bin/delphi"), "{:?}", blocked);
        assert_eq!(run(McpPolicy::Denied("x".into()), cfg)[0].0, Level::Warn);
        // Nothing to say without a Claude harness endpoint, or with the bridge turned off.
        assert!(run(McpPolicy::NotListed, "[[endpoints]]\nid=\"a\"\nfamily=\"claude\"\npath=\"api\"\ncredential=\"env:K\"\n").is_empty());
        assert!(run(McpPolicy::NotListed, &format!("{}\n[harness.claude]\nbridge = false\n", cfg)).is_empty());
    }

    #[test]
    fn harness_endpoints_are_checked_with_the_clis_own_commands() {
        let cfg = "[[endpoints]]\nid=\"c\"\nfamily=\"claude\"\npath=\"harness\"\nharness=\"claude\"\ncredential=\"harness-login\"\n[[endpoints]]\nid=\"a\"\nfamily=\"gemini\"\npath=\"harness\"\nharness=\"agy\"\ncredential=\"harness-login\"\n[[endpoints]]\nid=\"g\"\nfamily=\"gemini\"\npath=\"harness\"\nharness=\"gemini\"\ncredential=\"harness-login\"\n[[endpoints]]\nid=\"api\"\nfamily=\"claude\"\npath=\"api\"\ncredential=\"env:K\"\n";
        let run_ok = |p: &str, a: &[&str]| -> Option<(i32, String)> {
            match (p, a.first().copied()) {
                ("claude", Some("--version")) => Some((0, "2.1.310 (Claude Code)\n".into())),
                ("claude", Some("auth")) => Some((0, r#"{"loggedIn":true,"authMethod":"claude.ai","subscriptionType":"pro","email":"secret@example.com"}"#.into())),
                ("agy", Some("--version")) => Some((0, "1.4.6\n".into())),
                _ => None,
            }
        };
        let mut r = Report::default();
        check_harnesses(cfg, &run_ok, false, &mut r);
        let text: Vec<String> = r.findings.iter().map(|f| format!("{:?} {} {}", f.level, f.location, f.message)).collect();
        assert!(text.iter().any(|t| t.starts_with("Ok c") && t.contains("signed in") && t.contains("pro plan") && t.contains("Delphi tool bridge on, approvals by delegate, account connectors on")), "{:?}", text);
        let mut r2 = Report::default();
        check_harnesses(&format!("{}\n[harness.claude]\nbridge = false\napprovals = \"delphi\"\nclaudeai_connectors = false\n", cfg), &run_ok, false, &mut r2);
        assert!(r2.findings.iter().any(|f| f.location == "c" && f.message.contains("bridge off (roles run text-only), approvals by delphi, account connectors off")), "{:?}", r2.findings.iter().map(|f| f.message.clone()).collect::<Vec<_>>());
        assert!(text.iter().any(|t| t.starts_with("Ok a") && t.contains("endpoints test --probe") && t.contains("plugin and a hook denies its other tools")), "{:?}", text);
        assert!(text.iter().any(|t| t.starts_with("Warn g") && t.contains("Antigravity")), "{:?}", text);
        assert!(!text.iter().any(|t| t.contains("secret@example.com")), "the account email is never printed");
        assert_eq!(r.findings.len(), 3, "API endpoints are not harness checks");

        let run_out = |p: &str, a: &[&str]| -> Option<(i32, String)> {
            match (p, a.first().copied()) {
                ("claude", Some("--version")) => Some((0, "2.1.310\n".into())),
                ("claude", Some("auth")) => Some((0, r#"{"loggedIn":false}"#.into())),
                _ => None,
            }
        };
        let mut r = Report::default();
        check_harnesses(cfg, &run_out, true, &mut r);
        let level_of = |id: &str| r.findings.iter().find(|f| f.location == id).map(|f| f.level);
        assert_eq!((level_of("c"), level_of("a")), (Some(Level::Fail), Some(Level::Fail)), "not signed in and not installed fail under --require-providers");
        let mut r = Report::default();
        check_harnesses(cfg, &run_out, false, &mut r);
        assert!(r.findings.iter().filter(|f| f.location == "c" || f.location == "a").all(|f| f.level == Level::Warn));
    }

    // specs/011 §4.1 and §3.2: check 13 reads the joined stores and speaks only in warnings it can back.
    mod xenia {
        use super::*;
        use delphi_core::xenia_fixture::{now, Fx, DAY};
        use delphi_xenia::testkit::{EpochSpec, MemberSpec, TestOrg};

        fn two_admin_org() -> TestOrg {
            let mut t = TestOrg::empty();
            let (a, b, e) = (t.who("alice"), t.who("bob"), t.who("erin"));
            let genesis = EpochSpec { org: delphi_xenia::testkit::ORG.into(), store: delphi_xenia::testkit::STORE.into(), epoch: 1, prev_hash: None, members: vec![MemberSpec::new(&a, "alice").admin(), MemberSpec::new(&b, "bob").admin(), MemberSpec::new(&e, "erin")] };
            t.push_signed(genesis, &["alice", "bob"]);
            t
        }

        fn levels(home: &Path, at: i64) -> Vec<(Level, String)> {
            let mut r = Report::default();
            check_xenia(home, at, &mut r);
            r.findings.iter().map(|f| (f.level, f.message.clone())).collect()
        }

        #[test]
        fn no_joined_store_says_nothing() {
            let t = tempfile::tempdir().unwrap();
            assert!(levels(t.path(), now()).is_empty());
        }

        #[test]
        fn a_healthy_store_is_one_ok_line_and_each_problem_is_its_own_warning() {
            let t = tempfile::tempdir().unwrap();
            let (fx, _) = Fx::installed(t.path(), &[]);
            let home = fx.dir.join("home");
            let ok = levels(&home, now());
            assert_eq!(ok.len(), 1);
            assert_eq!(ok[0].0, Level::Ok);
            // Past half of the statement's life: a warning that says how much is used.
            let aging = levels(&home, now() + 4 * DAY);
            assert!(aging.iter().any(|(l, m)| *l == Level::Warn && m.contains("of its life is used")), "{aging:?}");
            // Lapsed: no endorsement counts.
            let lapsed = levels(&home, now() + 9 * DAY);
            assert!(lapsed.iter().any(|(l, m)| *l == Level::Warn && m.contains("no endorsement counts")), "{lapsed:?}");
        }

        #[test]
        fn loosened_or_damaged_pins_are_failures_that_name_the_file_and_never_a_panic() {
            use std::os::unix::fs::PermissionsExt;
            let t = tempfile::tempdir().unwrap();
            let (fx, _) = Fx::installed(t.path(), &[]);
            let home = fx.dir.join("home");
            let pins = fx.store.paths.state.join("pins.toml");
            std::fs::set_permissions(&pins, std::fs::Permissions::from_mode(0o644)).unwrap();
            let loose = levels(&home, now());
            assert!(loose.iter().any(|(l, m)| *l == Level::Fail && m.starts_with("pins.toml is not a private regular file")), "{loose:?}");
            std::fs::write(&pins, b"\xff garbage \x1b[2J").unwrap();
            let bad = levels(&home, now());
            assert!(bad.iter().any(|(l, m)| *l == Level::Fail && m.contains("pins.toml is damaged")), "{bad:?}");
        }

        #[test]
        fn fewer_than_three_admins_warns_and_does_not_fail() {
            let t = tempfile::tempdir().unwrap();
            let (fx, _) = Fx::installed_on(Fx::new(t.path(), two_admin_org()), &[]);
            let found = levels(&fx.dir.join("home"), now());
            assert!(found.iter().any(|(l, m)| *l == Level::Warn && m.contains("only 2 admin")), "{found:?}");
            assert!(found.iter().all(|(l, _)| *l != Level::Fail));
        }

        #[test]
        fn tampering_found_by_the_last_sync_is_a_failure() {
            let t = tempfile::tempdir().unwrap();
            let fx0 = Fx::with_cast(t.path());
            let m = delphi_core::xenia_fixture::records::memory_with(&fx0.id("mia"), &delphi_core::xenia_fixture::u(0), 1, "active", "v", "rust", "internal_empirical");
            let (fx, _) = Fx::installed_on(fx0, &[&m]);
            fx.remote.remove(&m.path);
            fx.commit("delete");
            let home = fx.dir.join("home");
            let store = delphi_core::xenia_status::open(&home, &delphi_core::xenia_status::list_stores(&home)[0]).unwrap();
            delphi_core::xenia_status::sync_and_record(&store, &delphi_core::xenia_sync::VerifyConfig::at(now())).unwrap();
            let found = levels(&home, now());
            assert!(found.iter().any(|(l, m)| *l == Level::Fail && m.contains("refused the store's head")), "{found:?}");
        }
    }
}
