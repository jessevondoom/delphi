//! The briefing behind `consult_orchestrator` (`specs/107` §2, as amended by A2): one deterministic call that reads the task. It reuses
//! the rankers that exist (`route_any` for the roster and the trigger, the project search for context pointers, the memory search for
//! memory pointers), needs no daemon, runs no model and dispatches nothing. Everything taken from project files or memory is marked
//! `untrusted_data: true` and carried as a pointer with a short preview, never as an instruction. The whole answer is at most 6 KiB.

use serde_json::{json, Value};

/// The whole answer is at most this many bytes of JSON.
pub const MAX_BYTES: usize = 6 * 1024;
pub const KINDS: [&str; 3] = ["software", "analysis", "document"];
/// The build verbs of the fixed rule (A2). A task with one of them, or a path or a source file, is software.
pub const BUILD_VERBS: [&str; 8] = ["build", "implement", "fix", "refactor", "add", "write code", "patch", "migrate"];
const SOURCE_EXTS: [&str; 24] = ["rs", "ts", "tsx", "js", "jsx", "mjs", "py", "go", "java", "kt", "c", "h", "cc", "cpp", "hpp", "cs", "rb", "php", "swift", "scala", "sh", "sql", "toml", "lua"];
/// Two roster roles closer than this in confidence are a toss-up, and the orchestrator run by Delphi is worth asking (§2 item 9).
pub const TIE: f32 = 0.05;
const MAX_ROSTER: usize = 5;
const MAX_POINTERS: usize = 6;
const MAX_MEMORY: usize = 3;
const PREVIEW: usize = 160;
const STOP: [&str; 40] = ["the", "and", "for", "with", "that", "this", "from", "into", "onto", "about", "what", "when", "where", "which", "while", "would", "should", "could", "there", "their", "them", "then", "than", "have", "has", "had", "are", "was", "were", "will", "can", "not", "but", "all", "any", "our", "your", "you", "its", "how"];

/// The words of `task`, lower-cased, as whole words (letters, digits, `-`).
fn words(task: &str) -> Vec<String> {
    task.to_lowercase().split(|c: char| !(c.is_alphanumeric() || c == '-')).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// A token that names a path or a source file: `src/lib.rs`, `crates/x/`, `main.py`. Not a URL, not `and/or`.
fn names_path(task: &str) -> bool {
    task.split_whitespace().map(|t| t.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | '(' | ')' | ',' | ';' | ':' | '.') && c != '/')).any(|t| {
        if t.contains("://") {
            return false;
        }
        let ext = t.rsplit_once('.').map(|(stem, e)| (stem, e.to_ascii_lowercase()));
        let source = ext.as_ref().is_some_and(|(stem, e)| !stem.is_empty() && SOURCE_EXTS.contains(&e.as_str()));
        let dir = t.contains('/') && (t.ends_with('/') || t.starts_with("./") || t.starts_with("../") || t.rsplit('/').next().is_some_and(|last| last.contains('.')));
        source || dir
    })
}

/// `kind`, given or inferred by the fixed rule (A2): `software` when the task has a build verb or names a path or a source file,
/// otherwise `analysis`. The second value says whether it was inferred.
pub fn kind_of(task: &str, given: Option<&str>) -> Result<(&'static str, bool), String> {
    if let Some(k) = given {
        return KINDS.iter().find(|x| **x == k).map(|x| (*x, false)).ok_or_else(|| "invalid arguments: `kind` must be software, analysis, or document".to_string());
    }
    let w = words(task);
    let joined = format!(" {} ", w.join(" "));
    let verb = BUILD_VERBS.iter().any(|v| joined.contains(&format!(" {v} ")));
    Ok((if verb || names_path(task) { "software" } else { "analysis" }, true))
}

/// The search words of a task: content words of three letters or more, in the order they appear, at most 12.
pub fn query_of(task: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for w in words(task) {
        if w.chars().count() >= 3 && !STOP.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
        if out.len() == 12 {
            break;
        }
    }
    out.join(" ")
}

fn flat(s: &str, max: usize) -> String {
    crate::host_hooks::flatten(s, max, "")
}

/// The escalation level and its reason (§2 item 3): an analysis or a document is never above Level 2 and never told it needs a spec;
/// Level 4 is offered only for software when the trigger is a Council or a Panel or the blast radius is system-wide.
pub fn level(kind: &str, trigger: &str, ambiguity: &str, blast: &str) -> (&'static str, &'static str) {
    if kind != "software" {
        return if ambiguity == "low" && blast == "isolated" {
            ("Level 1 (Direct Hunt)", "A task that names no software change, with low ambiguity: answer it directly.")
        } else {
            ("Level 2 (Scoped Pipeline)", "A task that names no software change: work it through with the roles below, check what you rely on, and no spec is needed.")
        };
    }
    if matches!(trigger, "council" | "panel") || blast == "system-wide" {
        ("Level 4 (Council Mode)", "A software change whose words ask for a decision between options or carry high stakes, or whose blast radius is system-wide: deliberate before execution.")
    } else if blast == "module" || ambiguity == "high" {
        ("Level 3 (Spec-First DAG)", "A module-scoped software change, or a highly ambiguous one: it needs an approved specification before code changes.")
    } else if ambiguity == "medium" {
        ("Level 2 (Scoped Pipeline)", "An isolated software change with moderate ambiguity: build it directly and validate.")
    } else {
        ("Level 1 (Direct Hunt)", "An isolated software change with low ambiguity: direct inspection or a single step.")
    }
}

/// One roster entry as the ranking gave it.
pub struct Seat {
    pub role: String,
    pub why: String,
    pub locus: String,
    pub confidence: f32,
    pub servable: bool,
    /// The operator named this role (`specs/107` §8.2): it answers.
    pub named: bool,
    /// The scout, taking a research task by default (§8.2 item 8).
    pub research: bool,
}

/// The worker subagent of `specs/106` for a role: `<role>-worker` when the role is servable and runs on the host (locus host or either).
pub fn worker_of(role: &str, locus: &str, servable: bool) -> Option<String> {
    (servable && matches!(locus, "host" | "either")).then(|| format!("{role}-worker"))
}

/// The roster (§2 item 1): the canonical roles at or above the adopt threshold, best first, at most five, with the roles the task
/// names leading (§8.2 item 4); the orchestrator alone when none reaches it and none is named.
pub fn roster(routed: &crate::route_any::Routed, roles: &[crate::mcp_packs::RoleInfo]) -> (Vec<Seat>, bool) {
    let mut out: Vec<Seat> = Vec::new();
    for c in routed.candidates.iter().filter(|c| c.kind == "role" && c.adopt) {
        if out.iter().any(|s| s.role == c.role) || out.len() == MAX_ROSTER {
            continue;
        }
        let info = roles.iter().find(|r| r.id == c.role);
        out.push(Seat { role: c.role.clone(), why: info.map(|r| r.purpose.clone()).unwrap_or_default(), locus: c.locus.clone(), confidence: c.confidence, servable: info.is_some_and(|r| r.servable), named: c.named, research: c.research });
    }
    if !out.is_empty() {
        return (out, false);
    }
    let o = roles.iter().find(|r| r.id == "orchestrator");
    (vec![Seat { role: "orchestrator".into(), why: "no role reached the adopt threshold for this task, so the orchestrator carries it alone".into(), locus: o.map(|r| r.locus.clone()).unwrap_or_else(|| "either".into()), confidence: 0.0, servable: o.is_some_and(|r| r.servable), named: false, research: false }], true)
}

/// Does the task ask who should do it or how (§2 item 9)?
fn asks_who_or_how(task: &str) -> bool {
    let w = words(task);
    w.iter().any(|x| x == "who" || x == "whom") || w.first().is_some_and(|x| x == "how") || task.to_lowercase().contains(" how ")
}

/// The suggestion that hands the top role the work (§8.3 item 1): its worker or its pack on the host, or `request_help` with it as the
/// target when it runs only inside Delphi.
fn hand_on(top: &Seat) -> Value {
    let who = if top.named {
        format!("the operator named the {}: the {} answers, not the orchestrator", top.role, top.role)
    } else if top.research {
        return json!({"tool": "get_agent", "role": top.role, "worker": worker_of(&top.role, &top.locus, top.servable), "why": format!("the task asks for research: the {} takes it by default, in this host. Start the {}-worker subagent, or adopt its pack with get_agent; it answers under its own display line", top.role, top.role)});
    } else {
        format!("the {} fits this task best: it does the work, not the orchestrator", top.role)
    };
    match worker_of(&top.role, &top.locus, top.servable) {
        Some(w) => json!({"tool": "get_agent", "role": top.role, "worker": w, "why": format!("{who}. Start the {w} subagent for its own context (a review, a second opinion, a long task), or adopt its pack with get_agent for a short answer; it answers under its own display line")}),
        None => json!({"tool": "request_help", "role": top.role, "suggested_target": top.role, "why": format!("{who}. It runs only inside Delphi: ask with request_help and suggested_target {}", top.role)}),
    }
}

/// The fixed-form suggestions (§2 item 9, as amended by §8.3 item 1), at most three, each naming a tool and why. When the roster is not
/// the orchestrator alone, the first hands the top role the work.
pub fn next(task: &str, seats: &[Seat], alone: bool, trigger: &str, pointers_above_floor: bool) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    if let Some(top) = seats.first().filter(|_| !alone) {
        out.push(hand_on(top));
    }
    let named = seats.iter().any(|s| s.named || s.research);
    let tie = !alone && !named && seats.len() >= 2 && seats.windows(2).any(|p| (p[0].confidence - p[1].confidence).abs() <= TIE);
    if tie || asks_who_or_how(task) {
        out.push(json!({"tool": "request_help", "why": if tie { "two roles fit this task almost equally: ask Delphi's own orchestrator who should do it" } else { "the task asks who should do it or how: Delphi's own orchestrator can answer that, on the operator's Delphi tokens" }}));
    }
    match trigger {
        "panel" => out.push(json!({"tool": "council_open", "why": "the task weighs options: hold a Panel of host seats (council_frame first)"})),
        "council" => out.push(json!({"tool": "request_council", "why": "the task carries high stakes: a formal Council, run by Delphi with a barrier it holds"})),
        _ => {}
    }
    if !pointers_above_floor {
        out.push(json!({"tool": "request_research", "why": "nothing in the project's specs, plans, context or research matched the task: ask Delphi's scout for sourced facts"}));
    }
    out.truncate(3);
    out
}

/// A context pointer passes the floor when its best line or its path shares a word with the task's search words.
pub fn above_floor(query: &str, path: &str, preview: &str) -> bool {
    let hay = format!("{} {}", path.to_lowercase(), preview.to_lowercase());
    query.split_whitespace().any(|w| hay.contains(w))
}

/// The project's instruction file (`AGENTS.md`, else `CLAUDE.md`) at `root`, with the commands its fenced blocks name (at most four,
/// flattened). A gate is never invented: no file, or no command in one, means none.
pub fn instruction_file(root: &std::path::Path) -> Option<(String, Vec<String>)> {
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let p = root.join(name);
        let Ok(m) = std::fs::symlink_metadata(&p) else { continue };
        if !m.is_file() || m.len() > 256 * 1024 {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let mut cmds: Vec<String> = Vec::new();
        let mut inside = false;
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("```") {
                inside = !inside;
                continue;
            }
            let cmd = t.split('#').next().unwrap_or("").trim();
            if inside && !cmd.is_empty() && cmd.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && cmd.len() <= 160 && cmds.len() < 4 {
                cmds.push(flat(cmd, 160));
            }
        }
        return Some((name.to_string(), cmds));
    }
    None
}

/// The constraints (§2 item 6), built from what is true of the project.
pub fn constraints(kind: &str, done_commands: &[String], file: Option<&(String, Vec<String>)>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    if kind == "software" {
        out.push(json!("No Spec, No Build: building software requires an approved specification (status: approved)"));
        if !done_commands.is_empty() {
            out.push(json!(format!("Never commit past a failing gate: {} (from done_when)", done_commands.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", "))));
        } else if let Some((name, cmds)) = file.filter(|(_, c)| !c.is_empty()) {
            out.push(json!({"text": format!("Never commit past a failing gate: {name} names {}", cmds.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", ")), "untrusted_data": true}));
        } else {
            out.push(json!("Never commit past a failing gate: no gate is known for this project; ask the operator or pass done_when (none is invented here)"));
        }
        out.push(json!("One slice per commit / pull request"));
    }
    out.push(json!("Preserve host sandbox and security boundaries"));
    out.push(json!("External agent attribution invariant: ⟦badge⟧ Name (role, via delphi)"));
    out
}

/// Cut the answer to [`MAX_BYTES`]: shorter previews first, then the last memory pointer, then the last context pointer.
pub fn fit(out: &mut Value) {
    let size = |v: &Value| v.to_string().len();
    if size(out) <= MAX_BYTES {
        return;
    }
    for key in [("execution_contract", "context_pointers"), ("memory", "")] {
        let list = if key.1.is_empty() { out.get_mut(key.0) } else { out.get_mut(key.0).and_then(|v| v.get_mut(key.1)) };
        for p in list.and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(s) = p.get("preview").and_then(Value::as_str) {
                p["preview"] = json!(flat(s, 60));
            }
        }
    }
    while size(out) > MAX_BYTES && out["memory"].as_array().is_some_and(|a| !a.is_empty()) {
        out["memory"].as_array_mut().unwrap().pop();
    }
    while size(out) > MAX_BYTES && out["execution_contract"]["context_pointers"].as_array().is_some_and(|a| !a.is_empty()) {
        out["execution_contract"]["context_pointers"].as_array_mut().unwrap().pop();
    }
}

/// The roster as JSON.
pub fn roster_json(seats: &[Seat], alone: bool) -> Vec<Value> {
    seats
        .iter()
        .map(|s| {
            let mut v = json!({"role": s.role, "why": flat(&s.why, 200), "locus": s.locus, "confidence": ((s.confidence * 100.0).round() / 100.0) as f64, "worker": worker_of(&s.role, &s.locus, s.servable)});
            if alone {
                v["alone"] = json!(true);
            }
            if s.named {
                v["named"] = json!(true);
            }
            if s.research {
                v["research_default"] = json!(true);
            }
            v
        })
        .collect()
}

/// A pointer from a search result (`search_project` or `search_memory`): path, line, a short preview, its tier, and the untrusted mark.
pub fn pointer(hit: &Value, status: Option<&str>) -> Value {
    let mut v = json!({"path": hit["path"], "line": hit["line"], "preview": flat(hit["preview"].as_str().unwrap_or(""), PREVIEW), "source_type": hit["source_type"], "untrusted_data": true});
    if let Some(s) = status {
        v["status"] = json!(s);
    }
    if let Some(l) = hit["layer"].as_str() {
        v["layer"] = json!(l);
    }
    v
}

pub const POINTERS: usize = MAX_POINTERS;
pub const MEMORY: usize = MAX_MEMORY;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_is_inferred_by_the_fixed_rule_and_a_given_kind_wins() {
        assert_eq!(kind_of("Fix the off-by-one in the pager", None).unwrap(), ("software", true));
        assert_eq!(kind_of("Look at crates/delphi/src/main.rs and say what it does", None).unwrap(), ("software", true));
        assert_eq!(kind_of("Why is main.py slow?", None).unwrap(), ("software", true));
        assert_eq!(kind_of("Write a report on the quarter's churn and/or retention", None).unwrap(), ("analysis", true));
        assert_eq!(kind_of("Summarize https://example.com/a/b.html for the board", None).unwrap(), ("analysis", true));
        assert_eq!(kind_of("Fix the report", Some("document")).unwrap(), ("document", false));
        assert!(kind_of("x", Some("poem")).is_err());
    }

    #[test]
    fn an_analysis_is_never_above_level_two_and_level_four_needs_a_trigger_or_a_system_wide_change() {
        assert_eq!(level("analysis", "council", "high", "system-wide").0, "Level 2 (Scoped Pipeline)");
        assert_eq!(level("software", "none", "high", "module").0, "Level 3 (Spec-First DAG)");
        assert_eq!(level("software", "panel", "low", "isolated").0, "Level 4 (Council Mode)");
        assert_eq!(level("software", "none", "low", "system-wide").0, "Level 4 (Council Mode)");
        assert_eq!(level("software", "none", "low", "isolated").0, "Level 1 (Direct Hunt)");
    }

    #[test]
    fn an_analysis_gets_no_spec_or_gate_line_and_a_gate_is_never_invented() {
        let a = constraints("analysis", &[], None).iter().map(Value::to_string).collect::<String>();
        assert!(!a.contains("No Spec") && !a.contains("gate"), "{a}");
        let none = constraints("software", &[], None).iter().map(Value::to_string).collect::<String>();
        assert!(none.contains("no gate is known") && !none.contains("cargo"), "{none}");
        let given = constraints("software", &["npm test".into()], None).iter().map(Value::to_string).collect::<String>();
        assert!(given.contains("`npm test` (from done_when)"), "{given}");
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("AGENTS.md"), "# Rules\n```sh\nmake check   # all\n```\n").unwrap();
        let f = instruction_file(t.path()).unwrap();
        let named = constraints("software", &[], Some(&f)).iter().map(Value::to_string).collect::<String>();
        assert!(named.contains("AGENTS.md names `make check`") && named.contains("untrusted_data"), "{named}");
    }

    #[test]
    fn the_worker_name_follows_spec_106_and_the_suggestions_follow_their_rules() {
        assert_eq!(worker_of("builder", "either", true).as_deref(), Some("builder-worker"));
        assert_eq!(worker_of("validator", "inside", true), None);
        assert_eq!(worker_of("x", "host", false), None);
        let s = |r: &str, c: f32| Seat { role: r.into(), why: String::new(), locus: "either".into(), confidence: c, servable: true, named: false, research: false };
        let tools = |v: Vec<Value>| v.iter().map(|x| x["tool"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(tools(next("Tune the cache", &[s("builder", 0.7), s("architect", 0.68)], false, "none", true)), ["get_agent", "request_help"]);
        assert_eq!(tools(next("Who should own the cache?", &[s("builder", 0.9)], false, "panel", false)), ["get_agent", "request_help", "council_open"]);
        assert_eq!(tools(next("Tune the cache", &[s("builder", 0.9)], false, "none", true)), ["get_agent"]);
        assert!(next("Tune the cache", &[s("orchestrator", 0.0)], true, "none", true).is_empty(), "alone: the orchestrator carries it");
        // §8.3 item 1: the hand-on names the worker; a named role says so and suppresses the tie rule; an inside role goes by request_help.
        let first = &next("Tune the cache", &[s("builder", 0.9)], false, "none", true)[0];
        assert_eq!((first["role"].as_str(), first["worker"].as_str()), (Some("builder"), Some("builder-worker")));
        let mut named = s("strategist", 0.3);
        named.named = true;
        let n = next("strategist review", &[named, s("architect", 0.31)], false, "none", true);
        assert_eq!(tools(n.clone()), ["get_agent"]);
        assert!(n[0]["why"].as_str().unwrap().contains("the operator named the strategist"), "{}", n[0]);
        let inside = Seat { role: "validator".into(), why: String::new(), locus: "inside".into(), confidence: 0.8, servable: true, named: false, research: false };
        let mut scout = s("scout", 0.2);
        scout.research = true;
        let r = &next("research churn benchmarks", &[scout], false, "research", true)[0];
        assert!(r["worker"] == "scout-worker" && r["why"].as_str().unwrap().contains("asks for research"), "{r}");
        let v = &next("Check the claims", &[inside], false, "none", true)[0];
        assert_eq!((v["tool"].as_str(), v["suggested_target"].as_str()), (Some("request_help"), Some("validator")));
        assert_eq!(query_of("Fix the flaky test in the pager and the pager's cache"), "fix flaky test pager cache");
    }
}
