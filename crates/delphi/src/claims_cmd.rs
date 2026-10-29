//! `delphi claims add|anchor|list|show|validate|pull` (`specs/055` §4, §5): claims in the central library.
//!
//! A claim's status is derived by the daemon, never typed: `add` makes a claim that reads *not validated*, and only
//! `validate` (the operator, in a terminal, refused inside an agent seat) records a validation, which counts only while an
//! anchor still holds. Capture and anchors go through `delphi-engine`; this file reads arguments and prints.

use delphi_engine::library::claims::{self, AnchorSpec, ClaimOpts, Validator};
use delphi_engine::library::Library;
use serde_json::Value;
use std::path::Path;

const USAGE: &str = "usage:
  delphi claims add <text> [--kind fact|episode|procedure|attributed] [--scope personal|project|team|org] [--project <name>]
    [--valid-until YYYY-MM-DD] [--supersedes <claim>] [--source <id> --locator <where> --quote <passage>] [--home <dir>]
  delphi claims anchor <claim> --source <id> --quote <passage> [--locator <where>] [--prefix <t>] [--suffix <t>] [--home <dir>]
  delphi claims list [--json] [--home <dir>]
  delphi claims show <claim> [--json] [--home <dir>]
  delphi claims validate <claim> [--home <dir>]
  delphi claims pull <words...> [--project <name>] [--json] [--home <dir>]";

const WITH_VALUE: [&str; 13] = ["--kind", "--scope", "--project", "--valid-until", "--supersedes", "--source", "--locator", "--quote", "--prefix", "--suffix", "--home", "--workspace", "--limit"];

fn clean(s: &str, n: usize) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if one.chars().count() > n { format!("{}…", one.chars().take(n - 1).collect::<String>()) } else { one }
}

fn value_of(args: &[String], name: &str) -> Result<Option<String>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => args.get(i + 1).filter(|v| !v.starts_with("--")).cloned().map(Some).ok_or_else(|| format!("{} needs a value", name)),
    }
}

fn positional(args: &[String]) -> Vec<&String> {
    args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !WITH_VALUE.contains(&args[*i - 1].as_str()))).map(|(_, a)| a).collect()
}

fn operator_only(cmd: &str) -> bool {
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi claims {}: refused; a claim's standing is the operator's to record and this process was started by an agent seat", cmd);
        return false;
    }
    true
}

pub fn cli(args: &[String], home: &Path) -> i32 {
    let lib = Library::in_home(home);
    let rest = &args[args.len().min(1)..];
    match args.first().map(String::as_str) {
        Some("add") => add(&lib, rest),
        Some("anchor") => anchor(&lib, rest),
        Some("list") => list(&lib, rest),
        Some("show") => show(&lib, rest),
        Some("validate") => validate(&lib, rest),
        Some("pull") => pull(&lib, rest),
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}

fn spec_from(args: &[String]) -> Result<Option<AnchorSpec>, String> {
    let (source, quote) = (value_of(args, "--source")?, value_of(args, "--quote")?);
    match (source, quote) {
        (None, None) => Ok(None),
        (Some(source_id), Some(quote)) => Ok(Some(AnchorSpec { source_id, quote, locator: value_of(args, "--locator")?, prefix: value_of(args, "--prefix")?, suffix: value_of(args, "--suffix")? })),
        _ => Err("an anchor needs both --source and --quote".into()),
    }
}

fn add(lib: &Library, args: &[String]) -> i32 {
    if !operator_only("add") {
        return 2;
    }
    let pos = positional(args);
    let [text] = pos[..] else {
        eprintln!("{}", USAGE);
        return 2;
    };
    let parsed = (|| -> Result<(ClaimOpts, Vec<AnchorSpec>), String> {
        let opts = ClaimOpts { kind: value_of(args, "--kind")?, scope: value_of(args, "--scope")?, project: value_of(args, "--project")?, valid_until: value_of(args, "--valid-until")?, supersedes: value_of(args, "--supersedes")? };
        Ok((opts, spec_from(args)?.into_iter().collect()))
    })();
    let (opts, specs) = match parsed {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi claims add: {}", e);
            return 2;
        }
    };
    match lib.add_claim(text, &opts, &specs) {
        Ok(c) => {
            println!("{} recorded: {} ({} anchor(s))", c["id"].as_str().unwrap_or("?"), claims::status(&c).label(), c["anchors"].as_array().map_or(0, Vec::len));
            0
        }
        Err(e) => {
            eprintln!("delphi claims add: {}", e);
            1
        }
    }
}

fn anchor(lib: &Library, args: &[String]) -> i32 {
    if !operator_only("anchor") {
        return 2;
    }
    let pos = positional(args);
    let ([id], Ok(Some(spec))) = (&pos[..], spec_from(args)) else {
        eprintln!("{}", USAGE);
        return 2;
    };
    match lib.add_anchor(id, &spec) {
        Ok(c) => {
            println!("{} anchored into {} ({} anchor(s)); status {}", id, spec.source_id, c["anchors"].as_array().map_or(0, Vec::len), claims::status(&c).label());
            0
        }
        Err(e) => {
            eprintln!("delphi claims anchor: {}", e);
            1
        }
    }
}

fn row(c: &Value) -> String {
    format!("{}  {:<12} {:<9} {}", c["id"].as_str().unwrap_or("?"), claims::status(c).as_str(), c["kind"].as_str().unwrap_or(""), clean(c["text"].as_str().unwrap_or(""), 80))
}

fn list(lib: &Library, args: &[String]) -> i32 {
    let all = lib.claims();
    if args.iter().any(|a| a == "--json") {
        let v: Vec<Value> = all.into_iter().map(|mut c| {
            c["status"] = Value::from(claims::status(&c).as_str());
            c
        }).collect();
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return 0;
    }
    if all.is_empty() {
        println!("no claims yet (`delphi claims add`)");
    }
    for c in &all {
        println!("{}", row(c));
    }
    0
}

fn show(lib: &Library, args: &[String]) -> i32 {
    let pos = positional(args);
    let [id] = pos[..] else {
        eprintln!("{}", USAGE);
        return 2;
    };
    let Some(mut c) = lib.claim(id) else {
        eprintln!("delphi claims show: no claim `{}`", clean(id, 80));
        return 1;
    };
    let st = claims::status(&c);
    c["status"] = Value::from(st.as_str());
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&c).unwrap_or_default());
        return 0;
    }
    println!("{}  {} ({})", id, st.as_str(), st.label());
    println!("  {}", clean(c["text"].as_str().unwrap_or(""), 400));
    for k in ["kind", "scope", "project", "created", "last_confirmed", "valid_until", "supersedes", "superseded_by"] {
        if let Some(v) = c[k].as_str() {
            println!("  {:<14} {}", k, clean(v, 120));
        }
    }
    for a in c["anchors"].as_array().into_iter().flatten() {
        println!("  anchor         {} v{} {} [{}]", a["source_id"].as_str().unwrap_or("?"), a["version"].as_str().unwrap_or("?"), clean(a["locator"].as_str().unwrap_or(""), 60), a["state"].as_str().unwrap_or("active"));
    }
    for m in c["contested_by"].as_array().into_iter().flatten() {
        println!("  contested      {} ({})", m["reason"].as_str().unwrap_or("?"), m["source"].as_str().unwrap_or(""));
    }
    let by = c["cited_by"].as_array().map_or(0, Vec::len);
    if by > 0 {
        println!("  relied on by   {} mission(s)", by);
    }
    0
}

fn validate(lib: &Library, args: &[String]) -> i32 {
    if !operator_only("validate") {
        return 2;
    }
    let pos = positional(args);
    let [id] = pos[..] else {
        eprintln!("{}", USAGE);
        return 2;
    };
    match lib.validate_claim(id, Validator::Operator) {
        Ok(c) => {
            println!("{} validated by the operator ({})", id, claims::status(&c).label());
            0
        }
        Err(e) => {
            eprintln!("delphi claims validate: {}", e);
            1
        }
    }
}

/// What a mission start would be handed for these words: pointers, no text. It records nothing.
fn pull(lib: &Library, args: &[String]) -> i32 {
    let query = positional(args).iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
    let project = match value_of(args, "--project") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi claims pull: {}", e);
            return 2;
        }
    };
    let pulled = lib.pull(&query, project.as_deref(), claims::PULL_MAX);
    if args.iter().any(|a| a == "--json") {
        let v: Vec<Value> = pulled.iter().map(|p| serde_json::json!({"uri": p.pointer().uri, "status": p.status.as_str(), "label": p.status.label(), "kind": p.kind, "origin": p.origin, "score": p.score})).collect();
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return 0;
    }
    if pulled.is_empty() {
        println!("no claims match");
    }
    for p in &pulled {
        println!("{}  {:<10} {:<9} origin {}  score {:.2}", p.pointer().uri, p.status.label(), p.kind, clean(&p.origin, 40), p.score);
    }
    0
}
