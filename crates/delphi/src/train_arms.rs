//! `delphi train arms init`: build the arm template homes for a training study (`specs/037` §2).
//!
//! An arm is a staffing decision made by configuration: which models answer each tier, which endpoints are allowed, which
//! roles prefer which family. This builds one **template home** per arm from the operator's own home, without touching it:
//!
//! - only the workspace's configuration and ratified files are copied (the release, the team layer, the agents and skills);
//!   no missions, no state, no worktrees, and none of the operator's own lessons, heuristics or profile, so an arm is a clean
//!   baseline and two arms differ only by their staffing;
//! - the endpoints' tier models, `[access]` and `role_family` are rewritten as the arm says, and the pinned hash of those
//!   blocks (`config.toml#access` in `ratified.lock`) is **re-sealed** the way `/ratify` would, so the home loads without a
//!   prompt. The copy is a throwaway test home; the operator's real ratification is never edited;
//! - each home is then loaded the way the daemon loads it, and the models its pool would use are listed, so a typo in a model
//!   id or an endpoint the arm needs but does not allow shows up here and not mid-study;
//! - with `--avail avail.json` (from `dev/train/avail.py --live`) an arm whose models are exhausted or unlisted is skipped
//!   with the reason and the time it resets, instead of failing runs later.
//!
//! It writes `<out>/arms.toml` (for `delphi train run --arms-file`) with `expect_models` derived from each arm's models.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use delphi_core::lock;
use serde_json::Value as Json;

#[derive(Debug, Clone, PartialEq)]
pub struct ArmDef {
    pub name: String,
    pub note: String,
    /// Per endpoint id: tier (`fast`, `workhorse`, `frontier`) to model.
    pub models: BTreeMap<String, BTreeMap<String, String>>,
    /// Roles' preferred family, replacing the home's own; absent keeps the home's, and an empty table clears it.
    pub role_family: Option<BTreeMap<String, String>>,
}

const TIERS: [&str; 3] = ["fast", "workhorse", "frontier"];

/// Read `arms-defs.toml`: `[[arm]]` with `name`, `note`, `[arm.models.<endpoint>]` tier tables, optional `[arm.role_family]`.
pub fn parse_defs(path: &Path) -> Result<Vec<ArmDef>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let t: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("{}: {}", path.display(), e.message()))?;
    let mut out: Vec<ArmDef> = Vec::new();
    for (i, a) in t.get("arm").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
        let at = format!("{}: arm #{}", path.display(), i + 1);
        let a = a.as_table().ok_or(format!("{} is not a table", at))?;
        let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(format!("{}: `name` must be lowercase letters, digits and `-`", at));
        }
        if out.iter().any(|d| d.name == name) {
            return Err(format!("{}: two arms are named `{}`", at, name));
        }
        let mut models = BTreeMap::new();
        for (ep, tiers) in a.get("models").and_then(|v| v.as_table()).into_iter().flatten() {
            let tiers = tiers.as_table().ok_or(format!("{}: models.{} is not a table", at, ep))?;
            let mut m = BTreeMap::new();
            for (tier, model) in tiers {
                let model = model.as_str().filter(|s| !s.is_empty()).ok_or(format!("{}: models.{}.{} must be a model id", at, ep, tier))?;
                if !TIERS.contains(&tier.as_str()) {
                    return Err(format!("{}: models.{}.{} is not a tier (fast, workhorse, frontier)", at, ep, tier));
                }
                m.insert(tier.clone(), model.to_string());
            }
            if m.len() != 3 {
                return Err(format!("{}: models.{} must give all three tiers", at, ep));
            }
            models.insert(ep.clone(), m);
        }
        if models.is_empty() {
            return Err(format!("{}: `{}` needs at least one [arm.models.<endpoint>]", at, name));
        }
        let role_family = a.get("role_family").and_then(|v| v.as_table()).map(|t| t.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect());
        out.push(ArmDef { name: name.into(), note: a.get("note").and_then(|v| v.as_str()).unwrap_or("").into(), models, role_family });
    }
    if out.is_empty() {
        return Err(format!("{}: no [[arm]] tables", path.display()));
    }
    Ok(out)
}

/// The model ids a run of this arm may report: each alias and its `claude-` form (the Claude CLI may report either), and
/// agy and Ollama ids as they are.
pub fn expect_models(def: &ArmDef) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for tiers in def.models.values() {
        for m in tiers.values() {
            // An alias (no dash) may be reported in its long form; a full id is reported as it is.
            let mut forms = vec![m.clone()];
            if !m.contains('-') {
                forms.push(format!("claude-{}", m));
            }
            for form in forms {
                if !v.contains(&form) {
                    v.push(form);
                }
            }
        }
    }
    v.sort();
    v
}

// ── availability ────────────────────────────────────────────────────────────────────────────────────────────────

/// Why this arm cannot run now, from an `avail.json`: a model it needs that is exhausted (with the reset time), unlisted or
/// unauthenticated. `None` means every model it needs is available, or not known to be otherwise.
pub fn unavailable(avail: &Json, def: &ArmDef) -> Option<String> {
    let models: Vec<&Json> = avail["models"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let mut problems = Vec::new();
    for tiers in def.models.values() {
        for m in tiers.values() {
            let rows: Vec<&&Json> = models.iter().filter(|r| r["model"].as_str() == Some(m.as_str())).collect();
            for r in rows {
                match r["status"].as_str() {
                    Some("exhausted") => problems.push(format!("{} is exhausted{}", m, r["until"].as_i64().map(|u| format!(" until {}", utc(u))).unwrap_or_default())),
                    Some("unlisted") | Some("unauthenticated") | Some("unreachable") => problems.push(format!("{} is {}", m, r["status"].as_str().unwrap_or(""))),
                    _ => {}
                }
            }
        }
    }
    problems.sort();
    problems.dedup();
    (!problems.is_empty()).then(|| problems.join("; "))
}

fn utc(ts: i64) -> String {
    let (days, rem) = (ts.div_euclid(86_400), ts.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let (mut y, doy) = (yoe + era * 400, doe - (365 * yoe + yoe / 4 - yoe / 100));
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!("{:04}-{:02}-{:02} {:02}:{:02}Z", y, m, d, rem / 3600, rem % 3600 / 60)
}

// ── building a home ─────────────────────────────────────────────────────────────────────────────────────────────

/// Paths (relative to the workspace) an arm home does not take: the operator's work and state, and the workspace's own
/// learning, so the arm starts from the release and team layers only.
const SKIP: [&str; 14] = [
    "local/missions", "local/state.db", "local/state.db-shm", "local/state.db-wal", "local/pool-state.json", "local/proposals", "local/timers.json",
    "local/memory", "local/staged", "local/ratified/memory", "local/ratified/heuristics.md", "local/ratified/specs", "worktrees", "scratch",
];

fn copy_filtered(src: &Path, dst: &Path, rel: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("{}: {}", dst.display(), e))?;
    for e in fs::read_dir(src).map_err(|e| format!("{}: {}", src.display(), e))?.flatten() {
        let r = rel.join(e.file_name());
        let rs = r.to_string_lossy().replace('\\', "/");
        if SKIP.contains(&rs.as_str()) || rs == "team/specs" {
            continue;
        }
        let (from, to) = (e.path(), dst.join(e.file_name()));
        let ft = e.file_type().map_err(|e| e.to_string())?;
        if ft.is_symlink() {
            continue; // a home holds plain files; a link could point out of it
        } else if ft.is_dir() {
            copy_filtered(&from, &to, &r)?;
        } else {
            fs::copy(&from, &to).map_err(|e| format!("{}: {}", to.display(), e))?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Built {
    pub name: String,
    /// `endpoint/model` pairs the home's pool would use, in preference order.
    pub pool: Vec<String>,
    pub problems: Vec<String>,
}

/// Rewrite the endpoints and access blocks of `config` as `def` says. Endpoints the arm does not name are removed, so the arm
/// cannot reach a model it did not declare.
fn rewrite_config(config: &str, def: &ArmDef) -> Result<String, String> {
    let mut v: toml::Value = toml::from_str(config).map_err(|e| format!("config.toml: {}", e))?;
    let t = v.as_table_mut().ok_or("config.toml is not a table")?;
    let (families, order) = {
        let eps = t.get_mut("endpoints").and_then(|e| e.as_array_mut()).ok_or("config.toml declares no [[endpoints]]")?;
        for want in def.models.keys() {
            if !eps.iter().any(|e| e.get("id").and_then(|i| i.as_str()) == Some(want.as_str())) {
                return Err(format!("the home has no endpoint `{}` (it has: {})", want, eps.iter().filter_map(|e| e.get("id").and_then(|i| i.as_str())).collect::<Vec<_>>().join(", ")));
            }
        }
        eps.retain(|e| e.get("id").and_then(|i| i.as_str()).is_some_and(|id| def.models.contains_key(id)));
        for e in eps.iter_mut() {
            let id = e.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
            let tiers = &def.models[&id];
            let mut m = toml::value::Table::new();
            for tier in TIERS {
                m.insert(tier.into(), toml::Value::String(tiers[tier].clone()));
            }
            e.as_table_mut().ok_or("an endpoint is not a table")?.insert("models".into(), toml::Value::Table(m));
        }
        let families: Vec<String> = eps.iter().filter_map(|e| e.get("family").and_then(|f| f.as_str()).map(String::from)).collect();
        let mut order = toml::value::Table::new();
        for e in eps.iter() {
            if let (Some(f), Some(id)) = (e.get("family").and_then(|f| f.as_str()), e.get("id").and_then(|i| i.as_str())) {
                if let Some(a) = order.entry(f.to_string()).or_insert_with(|| toml::Value::Array(vec![])).as_array_mut() {
                    a.push(toml::Value::String(id.to_string()));
                }
            }
        }
        (families, order)
    };
    let ids: Vec<String> = def.models.keys().cloned().collect();
    let access = t.entry("access").or_insert_with(|| toml::Value::Table(Default::default())).as_table_mut().ok_or("[access] is not a table")?;
    access.insert("allowed_endpoints".into(), toml::Value::Array(ids.iter().map(|i| toml::Value::String(i.clone())).collect()));
    access.insert("order".into(), toml::Value::Table(order));
    // A role preferring a family the arm has no endpoint for would be a config error: keep only what the arm can serve.
    let wanted: BTreeMap<String, String> = match &def.role_family {
        Some(m) => m.clone(),
        None => access.get("role_family").and_then(|r| r.as_table()).map(|t| t.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default(),
    };
    let rf: toml::value::Table = wanted.into_iter().filter(|(_, f)| families.contains(f)).map(|(k, f)| (k, toml::Value::String(f))).collect();
    if rf.is_empty() {
        access.remove("role_family");
    } else {
        access.insert("role_family".into(), toml::Value::Table(rf));
    }
    toml::to_string_pretty(&v).map_err(|e| e.to_string())
}

/// Build one arm's home under `out_home` from `source_home`'s `workspace`. The source is only read.
pub fn build_arm(source_home: &Path, workspace: &str, def: &ArmDef, out_home: &Path) -> Result<Built, String> {
    let src_ws = source_home.join("workspaces").join(workspace);
    if !src_ws.join("config.toml").is_file() {
        return Err(format!("{} has no workspace `{}`", source_home.display(), workspace));
    }
    let ws = out_home.join("workspaces").join(workspace);
    copy_filtered(&src_ws, &ws, Path::new(""))?;
    // The configuration, rewritten; then its access blocks sealed as `/ratify` would.
    let new = rewrite_config(&fs::read_to_string(ws.join("config.toml")).map_err(|e| e.to_string())?, def)?;
    fs::write(ws.join("config.toml"), &new).map_err(|e| e.to_string())?;
    let blocks = lock::access_blocks(&new);
    lock::write_atomic(&ws.join("local/ratified").join("config.access.toml"), blocks.as_bytes()).map_err(|e| e.to_string())?;
    let lock_path = ws.join("local/ratified.lock");
    let mut lk = lock::read(&lock_path).map_err(|e| format!("the source workspace has no usable ratified.lock: {}", e))?;
    lk.files.insert("config.toml#access".into(), lock::hash(blocks.as_bytes()));
    // Drop lock entries for files this home does not carry (the workspace's own learning and specs).
    lk.files.retain(|k, _| k == "config.toml#access" || ws.join("local/ratified").join(k).is_file());
    lock::write(&lock_path, &lk).map_err(|e| e.to_string())?;
    // A clean operator layer: a valid, empty lock and nothing ratified in it.
    let op = out_home.join("operator");
    fs::create_dir_all(op.join("ratified")).map_err(|e| e.to_string())?;
    lock::write(&op.join("ratified.lock"), &lock::LockFile::default()).map_err(|e| e.to_string())?;

    // Load it the way the daemon does, and say what it would route to.
    let (effective, notices) = delphi_engine::boot::effective_config(&ws, &new)?;
    let mut problems: Vec<String> = notices;
    let pool = match delphi_adapter::config::Config::parse(&effective) {
        Ok(cfg) => {
            problems.extend(cfg.problems());
            cfg.pool_entries().iter().map(|k| format!("{}/{}", k.endpoint_id, k.model_id)).collect()
        }
        Err(e) => {
            problems.extend(e);
            Vec::new()
        }
    };
    let want: Vec<String> = def.models.iter().flat_map(|(ep, t)| t.values().map(move |m| format!("{}/{}", ep, m))).collect();
    for p in &pool {
        if !want.contains(p) {
            problems.push(format!("the pool would use {} though the arm declares only {}", p, want.join(", ")));
        }
    }
    Ok(Built { name: def.name.clone(), pool, problems })
}

/// Is `path` the directory `home` or somewhere under it? `path` need not exist yet: its nearest existing ancestor is resolved.
fn inside(path: &Path, home: &Path) -> bool {
    let Ok(home) = home.canonicalize() else { return false };
    let mut probe = path.to_path_buf();
    let mut tail = PathBuf::new();
    loop {
        if let Ok(real) = probe.canonicalize() {
            return real.join(&tail).starts_with(&home);
        }
        let Some(name) = probe.file_name().map(|n| n.to_owned()) else { return false };
        tail = Path::new(&name).join(&tail);
        if !probe.pop() {
            return false;
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// `delphi train arms init --source-home H --workspace W --defs F --out DIR [--avail avail.json] [--only a,b]`.
pub fn cli(args: &[String]) -> i32 {
    let (Some(src), Some(defs), Some(out)) = (flag(args, "--source-home"), flag(args, "--defs"), flag(args, "--out")) else {
        eprintln!("delphi train arms init --source-home <your home> --workspace <name> --defs <arms-defs.toml> --out <new dir> [--avail avail.json] [--only a,b]\n  builds one template home per arm; your home is only read");
        return 2;
    };
    let (src, out) = (PathBuf::from(src), PathBuf::from(out));
    let workspace = flag(args, "--workspace").unwrap_or_else(|| "default".into());
    if let Err(e) = crate::train::check_work_home(&out) {
        eprintln!("delphi train arms init: {}", e);
        return 2;
    }
    // Homes are built fresh: a directory that already holds anything (even a previous run's) would be merged into.
    if out.exists() && fs::read_dir(&out).map(|mut r| r.next().is_some()).unwrap_or(true) {
        eprintln!("delphi train arms init: {} already holds files; give a new directory", out.display());
        return 2;
    }
    if inside(&out, &src) {
        eprintln!("delphi train arms init: --out must not be, or be inside, the source home");
        return 2;
    }
    let defs = match parse_defs(Path::new(&defs)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("delphi train arms init: {}", e);
            return 2;
        }
    };
    let avail: Option<Json> = flag(args, "--avail").and_then(|p| fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok());
    if flag(args, "--avail").is_some() && avail.is_none() {
        eprintln!("delphi train arms init: could not read --avail (run `python3 -I dev/train/avail.py --live --json avail.json`)");
        return 2;
    }
    let only: Option<Vec<String>> = flag(args, "--only").map(|s| s.split(',').map(String::from).collect());
    if let Err(e) = crate::train::prepare_work_home(&out) {
        eprintln!("delphi train arms init: {}", e);
        return 2;
    }
    let mut toml_out = String::from("# Made by `delphi train arms init`. Template homes are throwaway copies: nothing here is your ratified setup.\n\n");
    let (mut built, mut skipped, mut failed) = (0, 0, 0);
    for def in defs.iter().filter(|d| only.as_ref().is_none_or(|o| o.contains(&d.name))) {
        if let Some(why) = avail.as_ref().and_then(|a| unavailable(a, def)) {
            println!("skip  {:<22} {}", def.name, why);
            skipped += 1;
            continue;
        }
        match build_arm(&src, &workspace, def, &out.join("homes").join(&def.name)) {
            Ok(b) if b.problems.is_empty() => {
                println!("built {:<22} pool: {}", b.name, b.pool.join(", "));
                toml_out.push_str(&format!("[[arm]]\nname = \"{}\"\ntemplate_home = \"homes/{}\"\nexpect_models = [{}]\n\n", def.name, def.name, expect_models(def).iter().map(|m| format!("\"{}\"", m)).collect::<Vec<_>>().join(", ")));
                built += 1;
            }
            Ok(b) => {
                println!("FAIL  {:<22} {}", b.name, b.problems.join("; "));
                failed += 1;
            }
            Err(e) => {
                println!("FAIL  {:<22} {}", def.name, e);
                failed += 1;
            }
        }
    }
    if built > 0 {
        let _ = fs::write(out.join("arms.toml"), toml_out);
    }
    println!("\n{} built, {} skipped, {} failed; arms file: {}", built, skipped, failed, if built > 0 { out.join("arms.toml").display().to_string() } else { "none".into() });
    if avail.is_none() {
        println!("(no --avail given: arms were not checked against what the account offers or has left)");
    }
    i32::from(failed > 0 || built == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_engine::workspace::WorkspacePaths;

    const BUILDER: &str = include_str!("../../../agents/builder/AGENT.md");

    const CONFIG: &str = "[workspace]\nprojects = [\"/somewhere\"]\n\n[[endpoints]]\nid = \"claude-sub\"\nfamily = \"claude\"\npath = \"harness\"\nharness = \"claude\"\ncredential = \"harness-login\"\ncost_model = \"subscription\"\nmodels = { fast = \"haiku\", workhorse = \"sonnet\", frontier = \"opus\" }\n\n[[endpoints]]\nid = \"agy-sub\"\nfamily = \"gemini\"\npath = \"harness\"\nharness = \"agy\"\ncredential = \"harness-login\"\ncost_model = \"subscription\"\nmodels = { fast = \"flash-low\", workhorse = \"flash-medium\", frontier = \"argon-medium\" }\n\n[access]\nallowed_endpoints = [\"claude-sub\", \"agy-sub\"]\nrole_family = { red-team = \"gemini\" }\n[access.order]\nclaude = [\"claude-sub\"]\ngemini = [\"agy-sub\"]\n";

    /// A source home as the operator's looks: a ratified workspace with learning, missions and a personal operator layer.
    fn source(root: &Path) -> PathBuf {
        let home = root.join("source");
        let ws = home.join("workspaces/w");
        let put = |p: PathBuf, text: &str| {
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        put(ws.join("config.toml"), CONFIG);
        let blocks = lock::access_blocks(CONFIG);
        put(ws.join("local/ratified/config.access.toml"), &blocks);
        put(ws.join("local/ratified/agents/builder/AGENT.md"), BUILDER);
        put(ws.join("local/ratified/heuristics.md"), "---\nid: w\n---\n### [W-001] Personal rule\n");
        put(ws.join("local/ratified/memory/agents/builder/lessons.md"), "### [L-1] a lesson\n");
        put(ws.join("local/missions/m1/events/x.jsonl"), "{}\n");
        put(ws.join("local/state.db"), "x");
        put(ws.join("team/specs/011-x.md"), "spec");
        let mut lk = lock::LockFile::default();
        lk.files.insert("config.toml#access".into(), lock::hash(blocks.as_bytes()));
        lk.files.insert("agents/builder/AGENT.md".into(), lock::hash(BUILDER.as_bytes()));
        lk.files.insert("heuristics.md".into(), "h".into());
        lk.files.insert("memory/agents/builder/lessons.md".into(), "h".into());
        lock::write(&ws.join("local/ratified.lock"), &lk).unwrap();
        put(home.join("operator/ratified/profile.md"), "I am the operator and I like terse answers.");
        put(home.join("operator/ratified.lock"), "{\"files\": {\"profile.md\": \"x\"}}");
        home
    }

    fn def(name: &str, ep: &str, m: [&str; 3]) -> ArmDef {
        ArmDef { name: name.into(), note: String::new(), models: [(ep.to_string(), TIERS.iter().zip(m).map(|(t, m)| (t.to_string(), m.to_string())).collect())].into(), role_family: None }
    }

    #[test]
    fn an_arm_home_routes_to_exactly_its_models_loads_unprompted_and_leaves_the_source_alone() {
        let t = tempfile::tempdir().unwrap();
        let src = source(t.path());
        let before = fs::read_to_string(src.join("workspaces/w/config.toml")).unwrap();
        let out = t.path().join("homes/solo-haiku");
        let b = build_arm(&src, "w", &def("solo-haiku", "claude-sub", ["haiku", "haiku", "haiku"]), &out).unwrap();
        assert!(b.problems.is_empty(), "{:?}", b.problems);
        assert!(b.pool.iter().all(|p| p == "claude-sub/haiku") && !b.pool.is_empty(), "{:?}", b.pool);
        // The gemini endpoint is gone, and the role preference for it with it (it would be a config error).
        let cfg = fs::read_to_string(out.join("workspaces/w/config.toml")).unwrap();
        assert!(!cfg.contains("agy-sub") && !cfg.contains("role_family") && cfg.contains("haiku"), "{}", cfg);
        // The daemon's own loader accepts the sealed access blocks and the ratified agent manifest.
        let ws = WorkspacePaths::new(&out, "w");
        assert!(ws.load_manifest("builder").is_ok());
        assert!(delphi_engine::boot::effective_config(&out.join("workspaces/w"), &cfg).unwrap().1.is_empty(), "no notice about unratified access changes");
        // A clean baseline: no missions, state, personal learning or specs; an empty valid operator layer.
        let w = out.join("workspaces/w");
        for gone in ["local/missions", "local/state.db", "local/ratified/memory", "local/ratified/heuristics.md", "team/specs"] {
            assert!(!w.join(gone).exists(), "{} was copied", gone);
        }
        let lk = lock::read(&w.join("local/ratified.lock")).unwrap();
        assert!(lk.files.contains_key("agents/builder/AGENT.md") && lk.files.contains_key("config.toml#access") && !lk.files.contains_key("heuristics.md") && !lk.files.keys().any(|k| k.starts_with("memory/")));
        assert!(lock::read(&out.join("operator/ratified.lock")).unwrap().files.is_empty() && !out.join("operator/ratified/profile.md").exists());
        assert_eq!(fs::read_to_string(src.join("workspaces/w/config.toml")).unwrap(), before, "the source home is only read");
        assert!(src.join("operator/ratified/profile.md").exists());
    }

    #[test]
    fn a_mixed_arm_keeps_both_endpoints_and_the_cross_family_reviewer() {
        let t = tempfile::tempdir().unwrap();
        let src = source(t.path());
        let mut d = def("team-mixed", "claude-sub", ["haiku", "sonnet", "fable"]);
        d.models.insert("agy-sub".into(), TIERS.iter().zip(["flash-low", "flash-medium", "flash-high"]).map(|(t, m)| (t.to_string(), m.to_string())).collect());
        let b = build_arm(&src, "w", &d, &t.path().join("homes/mixed")).unwrap();
        assert!(b.problems.is_empty(), "{:?}", b.problems);
        for want in ["claude-sub/fable", "claude-sub/haiku", "agy-sub/flash-high", "agy-sub/flash-low"] {
            assert!(b.pool.contains(&want.to_string()), "{} in {:?}", want, b.pool);
        }
        let cfg = fs::read_to_string(t.path().join("homes/mixed/workspaces/w/config.toml")).unwrap();
        assert!(cfg.contains("role_family") && cfg.contains("red-team"), "the reviewer stays on the other family: {}", cfg);
    }

    #[test]
    fn an_arm_naming_an_endpoint_the_home_lacks_fails_with_what_it_has() {
        let t = tempfile::tempdir().unwrap();
        let src = source(t.path());
        let e = build_arm(&src, "w", &def("x", "ollama", ["a", "b", "c"]), &t.path().join("homes/x")).unwrap_err();
        assert!(e.contains("no endpoint `ollama`") && e.contains("claude-sub, agy-sub"), "{}", e);
        assert!(build_arm(&src, "nowhere", &def("x", "claude-sub", ["a", "b", "c"]), &t.path().join("homes/y")).unwrap_err().contains("no workspace"));
    }

    #[test]
    fn expected_models_cover_a_claude_alias_and_its_long_form_and_agy_ids_as_they_are() {
        let mut d = def("mixed", "claude-sub", ["haiku", "sonnet", "fable"]);
        d.models.insert("agy-sub".into(), TIERS.iter().zip(["gemini-3.8-flash-low", "gemini-3.8-flash-medium", "gemini-3.8-flash-high"]).map(|(t, m)| (t.to_string(), m.to_string())).collect());
        let e = expect_models(&d);
        for want in ["haiku", "claude-haiku", "sonnet", "claude-sonnet", "fable", "claude-fable", "gemini-3.8-flash-low", "gemini-3.8-flash-high"] {
            assert!(e.contains(&want.to_string()), "{} in {:?}", want, e);
        }
        assert!(!e.contains(&"claude-gemini-3.8-flash-low".to_string()), "an id with a dash is already a full id");
    }

    #[test]
    fn availability_skips_an_arm_with_an_exhausted_or_unlisted_model_and_says_when_it_resets() {
        let avail = serde_json::json!({"models": [
            {"provider": "agy", "model": "argon-medium", "status": "exhausted", "until": 1793343010},
            {"provider": "agy", "model": "flash-low", "status": "available"},
            {"provider": "claude", "model": "haiku", "status": "available"},
            {"provider": "claude", "model": "ghost", "status": "unlisted"},
            {"provider": "claude", "model": "sonnet", "status": "unknown"}]});
        let argon = def("solo-argon", "agy-sub", ["argon-medium", "argon-medium", "argon-medium"]);
        assert_eq!(unavailable(&avail, &argon).as_deref(), Some("argon-medium is exhausted until 2026-10-30 06:50Z"));
        assert_eq!(unavailable(&avail, &def("solo-haiku", "claude-sub", ["haiku", "haiku", "haiku"])), None);
        assert_eq!(unavailable(&avail, &def("ghostly", "claude-sub", ["haiku", "ghost", "ghost"])).as_deref(), Some("ghost is unlisted"));
        assert_eq!(unavailable(&avail, &def("unchecked", "claude-sub", ["sonnet", "sonnet", "sonnet"])), None, "unknown is not a reason to skip");
        assert_eq!(utc(0), "1970-01-01 00:00Z");
        assert_eq!(utc(1_793_343_010), "2026-10-30 06:50Z");
    }

    #[test]
    fn defs_are_validated() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("defs.toml");
        let ok = "[[arm]]\nname = \"solo-haiku\"\nnote = \"x\"\n[arm.models.claude-sub]\nfast = \"haiku\"\nworkhorse = \"haiku\"\nfrontier = \"haiku\"\n";
        fs::write(&f, ok).unwrap();
        let d = parse_defs(&f).unwrap();
        assert_eq!((d[0].name.as_str(), d[0].models["claude-sub"]["fast"].as_str(), d[0].role_family.clone()), ("solo-haiku", "haiku", None));
        for (bad, want) in [
            (ok.replace("solo-haiku", "Solo"), "`name` must be lowercase"),
            (ok.replace("frontier = \"haiku\"\n", ""), "must give all three tiers"),
            (ok.replace("frontier", "ultra"), "is not a tier"),
            ("[[arm]]\nname = \"x\"\n".to_string(), "needs at least one"),
            (format!("{}{}", ok, ok), "two arms are named"),
            ("".to_string(), "no [[arm]] tables"),
        ] {
            fs::write(&f, bad).unwrap();
            assert!(parse_defs(&f).unwrap_err().contains(want), "{}", want);
        }
    }

    #[test]
    fn the_command_builds_arms_writes_an_arms_file_and_refuses_unsafe_paths() {
        let t = tempfile::tempdir().unwrap();
        let src = source(t.path());
        let defs = t.path().join("defs.toml");
        fs::write(&defs, "[[arm]]\nname = \"solo-haiku\"\n[arm.models.claude-sub]\nfast = \"haiku\"\nworkhorse = \"haiku\"\nfrontier = \"haiku\"\n\n[[arm]]\nname = \"solo-argon\"\n[arm.models.agy-sub]\nfast = \"argon\"\nworkhorse = \"argon\"\nfrontier = \"argon\"\n").unwrap();
        let avail = t.path().join("avail.json");
        fs::write(&avail, "{\"models\": [{\"model\": \"argon\", \"status\": \"exhausted\", \"until\": 1793343010}]}").unwrap();
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let out = t.path().join("arms-out");
        let args = a(&["--source-home", src.to_str().unwrap(), "--workspace", "w", "--defs", defs.to_str().unwrap(), "--out", out.to_str().unwrap(), "--avail", avail.to_str().unwrap()]);
        assert_eq!(cli(&args), 0);
        let arms = fs::read_to_string(out.join("arms.toml")).unwrap();
        assert!(arms.contains("name = \"solo-haiku\"") && arms.contains("homes/solo-haiku") && arms.contains("\"claude-haiku\"") && !arms.contains("solo-argon"), "{}", arms);
        assert!(!out.join("homes/solo-argon").exists(), "an exhausted arm is skipped, not built");
        // The arms file is what `train run --arms-file` reads.
        let parsed = crate::train_run::parse_arms(&out.join("arms.toml")).unwrap();
        assert_eq!((parsed.len(), parsed[0].name.as_str()), (1, "solo-haiku"));
        // The output directory must be fresh, and never the source.
        assert_eq!(cli(&args), 2, "a directory that holds things is refused");
        let inside = a(&["--source-home", src.to_str().unwrap(), "--workspace", "w", "--defs", defs.to_str().unwrap(), "--out", src.join("arms").to_str().unwrap()]);
        assert_eq!(cli(&inside), 2, "inside the source home");
        assert_eq!(cli(&a(&["--defs", defs.to_str().unwrap()])), 2);
    }
}
