//! `delphi route`: which canonical role or bundle agent the router would offer for a task, where it runs, what the task looks
//! like it wants and whether the ranking is sure enough to adopt it (`specs/098` §8), and the measures of `specs/041` §9. A dry
//! run: no model is called, no mission or workspace is created, nothing is dispatched and no daemon is started.

use delphi_engine::agent_registry::{self as ar, Labeled, Query};
use delphi_engine::route_any as ra;
use delphi_engine::workspace::WorkspacePaths;
use serde_json::Value;

const VALUE_FLAGS: [&str; 8] = ["--workspace", "--home", "--domain", "--role", "--limit", "--eval", "--adopt-eval", "--threshold"];

/// The task in the arguments: every word that is neither a flag nor a flag's value. `main` has already taken the command word
/// out, so nothing is skipped (`specs/041` §13 item 1: skipping the first word dropped a task's first word and, after `--home`, read the
/// home's path as the task).
fn task_of(args: &[String]) -> String {
    let mut task = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if VALUE_FLAGS.contains(&a.as_str()) {
            it.next();
        } else if !a.starts_with("--") {
            task.push(a.clone());
        }
    }
    task.join(" ")
}

pub fn run(args: &[String], home: &std::path::Path, ws_dir: &std::path::Path) -> i32 {
    let flag = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let task = task_of(args);
    if args.iter().any(|a| a == "--corrections") {
        return corrections(ws_dir, args.iter().any(|a| a == "--json"));
    }
    let ws = WorkspacePaths { home: home.to_path_buf(), dir: ws_dir.to_path_buf() };
    let reg = ar::registry(&ws);
    let emb = match delphi_engine::embed::open_default(Some(home)) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("delphi route: the embedding model is not used: {}", e);
            None
        }
    };
    let json = args.iter().any(|a| a == "--json");
    let packs = delphi_engine::mcp_packs::Packs::new(WorkspacePaths { home: home.to_path_buf(), dir: ws_dir.to_path_buf() });
    let roles = match ra::role_rows(&packs) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("delphi route: the canonical roles are not readable ({e}); only bundle agents are ranked");
            Vec::new()
        }
    };
    let settings = std::fs::read_to_string(ws_dir.join("config.toml")).map(|t| ra::settings_in(&t)).unwrap_or_default();
    let threshold = match flag("--threshold").map(|t| t.parse::<f32>()) {
        None => settings.adopt_threshold,
        Some(Ok(t)) if (0.0..=1.0).contains(&t) => t,
        _ => {
            eprintln!("delphi route: --threshold is a number from 0 to 1");
            return 2;
        }
    };
    if let Some(path) = flag("--eval") {
        return eval(&reg, &path, emb.as_deref(), json);
    }
    if let Some(path) = flag("--adopt-eval") {
        return adopt_eval(&reg, &roles, &path, threshold, emb.as_deref(), json);
    }
    if task.trim().is_empty() {
        eprintln!("delphi route: give the task in words, for example: delphi route \"who can review our pricing strategy?\"");
        return 2;
    }
    let limit = match flag("--limit").map(|l| l.parse::<usize>()) {
        None => 3,
        Some(Ok(n)) if (1..=5).contains(&n) => n,
        _ => {
            eprintln!("delphi route: --limit is 1 to 5");
            return 2;
        }
    };
    let domain = flag("--domain");
    let role = flag("--role");
    let q = Query { text: &task, domain: domain.as_deref(), role: role.as_deref(), limit };
    let mut found = ra::route(&reg.entries, &roles, &q, threshold, emb.as_deref());
    ra::name_roles(&mut found, &roles, &ra::named_in(&task, &roles, Some(&packs.theme_resolver())));
    if json {
        let mut out = ra::to_json(&found, &settings, reg.entries.len());
        let parts: Vec<Value> = found.candidates.iter().map(|c| serde_json::json!({"target": c.target, "lexical": c.lexical, "vector": c.vector, "penalty": c.penalty})).collect();
        out["ranking_parts"] = Value::Array(parts);
        println!("{out}");
        return 0;
    }
    println!("{} canonical roles and {} bundle agents in the registry; the vector part is {}.", roles.len(), reg.entries.len(), if emb.is_some() { "on (confidence blends it in; the default threshold was set on words alone)" } else { "off (no embedding model)" });
    println!("adopt threshold {:.2} (recommended); the task looks like: {}", threshold, found.trigger);
    if found.status == "no_specialist" {
        println!("no specialist: nothing scores {:.2} or more.", ar::TAU);
        match &found.fallback {
            Some(r) => println!("fallback: dispatch the canonical role `{}`.", r),
            None => println!("fallback: dispatch the canonical role that fits."),
        }
        return 0;
    }
    for (i, c) in found.candidates.iter().enumerate() {
        println!("{}. {}  (runs as {}, {}; locus {})  confidence {:.2}{}  [score {:.2}; lexical {:.2}, vector {}, penalty {:.2}]", i + 1, c.target, c.role, c.domain, c.locus, c.confidence, if c.named { " adopt (named)" } else if c.adopt { " adopt" } else { "" }, c.score, c.lexical, c.vector.map_or("-".into(), |v| format!("{:.2}", v)), c.penalty);
        println!("   {}", c.summary.chars().take(200).collect::<String>());
    }
    0
}

/// Measure how `adopt = true` does on a labeled set of `{"task", "expect"}` (`expect` is a canonical role id, or `none` for a task
/// that should adopt nothing) at `threshold`: the way to replace the provisional default with one set on your own tasks.
fn adopt_eval(reg: &ar::Registry, roles: &[ra::RoleRow], path: &str, threshold: f32, emb: Option<&dyn delphi_engine::embed::Embedder>, json: bool) -> i32 {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi route: cannot read {}: {}", path, e);
            return 2;
        }
    };
    let labeled = match ra::parse_labeled(&text) {
        Ok(l) if !l.is_empty() => l,
        Ok(_) => {
            eprintln!("delphi route: {} has no labeled tasks", path);
            return 2;
        }
        Err(e) => {
            eprintln!("delphi route: {} is not a JSON list of {{\"task\", \"expect\"}}: {}", path, e);
            return 2;
        }
    };
    let e = ra::evaluate_adopt(&reg.entries, roles, &labeled, threshold, emb);
    if json {
        println!("{}", serde_json::json!({"n": e.n, "threshold": threshold, "adopted": e.adopted, "correct": e.correct, "precision": e.precision, "recall": e.recall, "wrong": e.wrong.iter().map(|(t, w, g)| serde_json::json!({"task": t, "expect": w, "got": g})).collect::<Vec<_>>()}));
        return 0;
    }
    println!("{} labeled tasks at threshold {:.2}: adopted {}, right {}; precision {:.3}, recall {:.3}", e.n, threshold, e.adopted, e.correct, e.precision, e.recall);
    for (task, want, got) in &e.wrong {
        println!("wrong: \"{}\" wanted {} got {}", task, want, got);
    }
    0
}

fn eval(reg: &ar::Registry, path: &str, emb: Option<&dyn delphi_engine::embed::Embedder>, json: bool) -> i32 {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi route: cannot read {}: {}", path, e);
            return 2;
        }
    };
    let rows: Vec<Value> = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("delphi route: {} is not a JSON list of {{\"task\", \"target\"}}: {}", path, e);
            return 2;
        }
    };
    let labeled: Vec<Labeled> = rows.iter().filter_map(|r| Some(Labeled { task: r["task"].as_str()?.to_string(), target: r["target"].as_str()?.to_string() })).collect();
    if labeled.len() != rows.len() {
        eprintln!("delphi route: every entry needs a string `task` and `target`");
        return 2;
    }
    let started = std::time::Instant::now();
    let e = ar::eval(reg, &labeled, emb);
    let per = started.elapsed().as_secs_f64() * 1000.0 / labeled.len().max(1) as f64;
    if json {
        println!("{}", serde_json::json!({"n": e.n, "recall_at_1": e.recall1, "recall_at_3": e.recall3, "recall_at_5": e.recall5, "no_specialist": e.no_specialist, "unknown_targets": e.unknown_targets, "ms_per_query": per, "tokens_domain_index": e.tokens_domain_index, "tokens_flat_list": e.tokens_flat_list, "misses": e.misses.iter().map(|(t, w, g)| serde_json::json!({"task": t, "labeled": w, "got": g})).collect::<Vec<_>>()}));
        return 0;
    }
    println!("{} labeled tasks over {} agents (vector part {})", e.n, reg.entries.len(), if emb.is_some() { "on" } else { "off" });
    println!("recall@1 {:.3}   recall@3 {:.3}   recall@5 {:.3}", e.recall1, e.recall3, e.recall5);
    println!("no_specialist {}   latency {:.2} ms/query", e.no_specialist, per);
    println!("orchestrator tokens: domain index ~{}, flat list uncapped ~{}", e.tokens_domain_index, e.tokens_flat_list);
    for t in &e.unknown_targets {
        println!("label names an agent no active bundle has: {}", t);
    }
    for (task, want, got) in &e.misses {
        println!("miss: \"{}\" wanted {} got [{}]", task, want, got.join(", "));
    }
    0
}

/// `delphi route --corrections` (`specs/041` §13 item 2): how often the operator named a role other than the one the ranking chose, read
/// from the `route.decided` records of every mission. Read-only; it prints counts and role names, never a task, and proposes
/// nothing on its own: the operator decides what, if anything, to change.
fn corrections(ws_dir: &std::path::Path, json: bool) -> i32 {
    let mut events = Vec::new();
    for base in [ws_dir.join("missions"), ws_dir.join("local/missions")] {
        let Ok(rd) = std::fs::read_dir(&base) else { continue };
        for e in rd.flatten() {
            if let Ok(evs) = delphi_core::log::replay(&e.path()) {
                events.extend(evs.into_iter().filter(|x| x.kind == delphi_engine::host_rpc::ROUTE_EVENT));
            }
        }
    }
    let c = delphi_engine::route_any::corrections(&events);
    if json {
        let pairs: Vec<Value> = c.pairs.iter().map(|(r, n, k)| serde_json::json!({"ranked": r, "named": n, "count": k})).collect();
        println!("{}", serde_json::json!({"decided": c.decided, "named": c.named, "research_default": c.research, "corrections": pairs}));
        return 0;
    }
    println!("{} routing decisions logged; the operator named a role in {}; research went to the scout by default in {}.", c.decided, c.named, c.research);
    if c.pairs.is_empty() {
        println!("no named role differed from the ranking's choice.");
    }
    for (ranked, named, k) in &c.pairs {
        println!("{k:>4}  the ranking chose {ranked}, the operator named {named}");
    }
    if !c.pairs.is_empty() {
        println!("A pair that recurs suggests the named role's skills do not describe tasks like these. Any change is yours to make (a skill description, a bundle agent's tags, [routing] adopt_threshold); nothing here changes routing.");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|x| x.to_string()).collect()
    }

    /// `specs/041` §13 item 1: no word of the task is lost and no flag's value becomes one.
    #[test]
    fn the_task_is_every_word_that_is_not_a_flag_or_its_value() {
        assert_eq!(task_of(&v(&["summarize", "this", "file"])), "summarize this file");
        assert_eq!(task_of(&v(&["--home", "/tmp/jobs/x", "--json", "zzqx"])), "zzqx");
        assert_eq!(task_of(&v(&["review", "the", "plan", "--limit", "3", "--workspace", "w"])), "review the plan");
        assert_eq!(task_of(&v(&["--corrections"])), "");
    }

    #[test]
    fn corrections_count_a_named_role_that_differs_from_the_ranking_and_print_no_task() {
        let ev = |facts: serde_json::Value| delphi_core::event::EventEnvelope::new("m", delphi_engine::host_rpc::ROUTE_EVENT, facts);
        let events = vec![
            ev(serde_json::json!({"task_chars": 9, "target": "role:strategist", "named": true, "ranked": "role:advocate", "adopt": true, "threshold": 0.55})),
            ev(serde_json::json!({"task_chars": 9, "target": "role:strategist", "named": true, "ranked": "role:advocate", "adopt": true, "threshold": 0.55})),
            ev(serde_json::json!({"task_chars": 9, "target": "role:architect", "named": true, "ranked": "role:architect", "adopt": true, "threshold": 0.55})),
            ev(serde_json::json!({"task_chars": 9, "target": "role:builder", "adopt": true, "threshold": 0.55})),
            ev(serde_json::json!({"task_chars": 9, "target": "role:scout", "research_default": true, "ranked": "role:writer", "adopt": true, "threshold": 0.55})),
        ];
        let c = delphi_engine::route_any::corrections(&events);
        assert_eq!((c.decided, c.named, c.research), (5, 3, 1));
        assert_eq!(c.pairs, vec![("role:advocate".to_string(), "role:strategist".to_string(), 2)]);
    }
}
