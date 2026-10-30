//! `delphi route`: which bundle agent the router would offer for a task, and the measures of `specs/041` §9. A dry run:
//! no model is called, no mission or workspace is created, nothing is dispatched.

use delphi_engine::agent_registry::{self as ar, Labeled, Query};
use delphi_engine::workspace::WorkspacePaths;
use serde_json::Value;

const VALUE_FLAGS: [&str; 6] = ["--workspace", "--home", "--domain", "--role", "--limit", "--eval"];

pub fn run(args: &[String], home: &std::path::Path, ws_dir: &std::path::Path) -> i32 {
    let flag = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let mut task = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if VALUE_FLAGS.contains(&a.as_str()) {
            it.next();
        } else if !a.starts_with("--") {
            task.push(a.clone());
        }
    }
    let task = task.join(" ");
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
    if let Some(path) = flag("--eval") {
        return eval(&reg, &path, emb.as_deref(), json);
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
    let found = ar::search(&reg.entries, &q, emb.as_deref());
    if json {
        let c: Vec<Value> = found.candidates.iter().map(|c| serde_json::json!({"target": c.entry.target, "host_role": c.entry.host_role, "domain": c.entry.domain, "summary": c.entry.summary, "score": c.score, "lexical": c.lexical, "vector": c.vector, "penalty": c.penalty})).collect();
        println!("{}", serde_json::json!({"status": if found.no_specialist { "no_specialist" } else { "ok" }, "candidates": c, "fallback": found.fallback, "agents": reg.entries.len()}));
        return 0;
    }
    println!("{} bundle agents in the registry; the vector part is {}.", reg.entries.len(), if emb.is_some() { "on" } else { "off (no embedding model)" });
    if found.no_specialist {
        println!("no specialist: nothing scores {:.2} or more.", ar::TAU);
        match &found.fallback {
            Some(r) => println!("fallback: dispatch the canonical role `{}`.", r),
            None => println!("fallback: dispatch the canonical role that fits."),
        }
        return 0;
    }
    for (i, c) in found.candidates.iter().enumerate() {
        println!("{}. {}  (runs as {}, {})  score {:.2}  [lexical {:.2}, vector {}, penalty {:.2}]", i + 1, c.entry.target, c.entry.host_role, c.entry.domain, c.score, c.lexical, c.vector.map_or("-".into(), |v| format!("{:.2}", v)), c.penalty);
        println!("   {}", c.entry.summary);
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
