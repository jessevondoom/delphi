//! `delphi pages show|check|dispute|search|list|pin` (`specs/055` §6): the readable view of the library's claims.
//!
//! A page is generated, never edited and never trusted text: it carries a banner and a bibliography table the daemon
//! writes, and the daemon's mechanical check (`specs/057` §3.1) rejects a page that cites what the library does not hold
//! or quotes what a source does not say. `dispute` is the operator's
//! "this is wrong" on a sentence (refused inside an agent seat): it files a contested mark on the cited claim.

use delphi_engine::library::bib::Format;
use delphi_engine::library::pages::{Finding, Scope};
use delphi_engine::library::Library;
use serde_json::json;
use std::path::Path;

const USAGE: &str = "usage:
  delphi pages show <topic words...> [--draft <file>] [--style apa|mla|chicago|plain] [--home <dir>]
  delphi pages check <file> [--home <dir>]
  delphi pages dispute <claim> --note <what is wrong> [--home <dir>]
  delphi pages search <question words...> [--internal | --external] [--json] [--home <dir>]
  delphi pages expand <question words...> [--json] [--home <dir>]
  delphi pages list [--json] [--home <dir>]
  delphi pages pin|unpin <key> [--home <dir>]
  delphi pages new <title words...> [--home <dir>]
  delphi pages topics [--json] [--home <dir>]
  delphi pages topic <slug> [--version <stamp>] [--json] [--home <dir>]
  delphi pages versions <slug> [--json] [--home <dir>]
  delphi pages rebuild <slug> [--home <dir>]
  delphi pages seeds --count [--json] [--home <dir>] [--workspace <name>]";

fn value_of(args: &[String], name: &str) -> Result<Option<String>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => args.get(i + 1).filter(|v| !v.starts_with("--")).cloned().map(Some).ok_or_else(|| format!("{} needs a value", name)),
    }
}

fn positional(args: &[String]) -> Vec<&String> {
    args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !["--draft", "--style", "--note", "--home", "--workspace", "--version"].contains(&args[*i - 1].as_str()))).map(|(_, a)| a).collect()
}

fn report(findings: &[Finding]) {
    eprintln!("the page is rejected: {} finding(s)", findings.len());
    for f in findings {
        eprintln!("  {}", f);
    }
}

/// May this page be printed (`specs/142` §2.5 item 4 (d))? Not when it carries more of a held source than may leave this machine: a page
/// printed goes wherever the terminal's output goes. Says why on standard error when it may not.
fn printable(home: &Path, markdown: &str) -> bool {
    emit(home, markdown, &mut std::io::sink()).map_err(|e| eprintln!("delphi pages: the page is not printed: {}", e)).is_ok()
}

/// Write `markdown` to `out` when it may leave, and nothing when it may not.
fn emit(home: &Path, markdown: &str, out: &mut dyn std::io::Write) -> Result<(), String> {
    delphi_engine::held_text::check_in_home(home, markdown, &[])?;
    out.write_all(markdown.as_bytes()).map_err(|e| e.to_string())
}

fn search(home: &Path, ws_dir: &Path, query: &str, external: Option<bool>, as_json: bool) -> i32 {
    use delphi_core::spawn::{connect_or_spawn, SpawnOptions};
    let Some(exe) = crate::chat::daemon_exe() else {
        eprintln!("delphi pages search: cannot locate this program to start the daemon");
        return 4;
    };
    let mut attached = match connect_or_spawn(&SpawnOptions { home, workspace: ws_dir, daemon_exe: &exe, extra_args: vec![], prefix_args: vec!["__core".into()], own_version: Some(env!("CARGO_PKG_VERSION")) }) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("delphi pages search: {}", e);
            return 4;
        }
    };
    let r = attached.client.call("pages.search", json!({"query": query, "external": external}));
    if attached.spawned {
        let _ = attached.client.call("daemon.shutdown", json!({}));
    }
    match r {
        Ok(v) if !printable(home, v["markdown"].as_str().unwrap_or("")) => 1,
        Ok(v) if as_json => {
            println!("{}", v);
            0
        }
        Ok(v) => {
            print!("{}", v["markdown"].as_str().unwrap_or(""));
            for n in v["notes"].as_array().into_iter().flatten().filter_map(|n| n.as_str()) {
                eprintln!("{}", n);
            }
            eprintln!("{} · {}{}", v["path"].as_str().unwrap_or(""), v["outcome"].as_str().unwrap_or(""), if v["cached"] == true { " · cached" } else { "" });
            if v["outcome"] == "claims_only" { 1 } else { 0 }
        }
        Err(e) => {
            eprintln!("delphi pages search: {}", e);
            1
        }
    }
}

fn expand(home: &Path, ws_dir: &Path, query: &str, as_json: bool) -> i32 {
    use delphi_core::spawn::{connect_or_spawn, SpawnOptions};
    let Some(exe) = crate::chat::daemon_exe() else {
        eprintln!("delphi pages expand: cannot locate this program to start the daemon");
        return 4;
    };
    let mut attached = match connect_or_spawn(&SpawnOptions { home, workspace: ws_dir, daemon_exe: &exe, extra_args: vec![], prefix_args: vec!["__core".into()], own_version: Some(env!("CARGO_PKG_VERSION")) }) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("delphi pages expand: {}", e);
            return 4;
        }
    };
    let r = attached.client.call("pages.expand", json!({"query": query}));
    if attached.spawned {
        let _ = attached.client.call("daemon.shutdown", json!({}));
    }
    match r {
        Ok(v) if !printable(home, v["markdown"].as_str().unwrap_or("")) => 1,
        Ok(v) if as_json => {
            println!("{}", v);
            0
        }
        Ok(v) => {
            print!("{}", v["markdown"].as_str().unwrap_or(""));
            for n in v["notes"].as_array().into_iter().flatten().filter_map(|n| n.as_str()) {
                eprintln!("{}", n);
            }
            eprintln!("{} · {}{}", v["path"].as_str().unwrap_or(""), v["outcome"].as_str().unwrap_or(""), if v["cached"] == true { " · cached" } else { "" });
            if v["outcome"] == "claims_only" { 1 } else { 0 }
        }
        Err(e) => {
            eprintln!("delphi pages expand: {}", e);
            1
        }
    }
}

/// What `delphi pages rebuild` rebuilds.
pub(crate) enum Target<'a> {
    Slug(&'a str),
    Due,
    All,
}

/// `delphi pages rebuild <slug> | --due | --all` (`specs/117` §2 item 2, `specs/143` §2.8): the daemon rebuilds topic pages as the job
/// `pages` started by the operator. Before anything runs it asks the daemon what the job would meet at the bridge: `off` refuses and
/// says why; past the window, the daily cap or the weekly budget it asks for the typed confirmation (a terminal, no agent seat, the
/// word typed back), and without it stays within the limits; past the ceiling nothing runs. Exit codes: 0 built · 1 a page failed or
/// is the claims alone · 2 off or usage · 3 a limit · 4 the daemon could not start.
fn rebuild(home: &Path, ws_dir: &Path, target: Target<'_>) -> i32 {
    crate::bridge_cmd::with_daemon(home, ws_dir, "pages rebuild", |client| rebuild_on(client, target))
}

fn rebuild_on(client: &mut delphi_core::client::Client, target: Target<'_>) -> i32 {
    let confirmed = match crate::bridge_cmd::preflight(client, "pages", "pages rebuild") {
        Ok(c) => c,
        Err(code) => return code,
    };
    let params = match target {
        Target::Slug(slug) => json!({"slug": slug, "confirmed": confirmed}),
        Target::Due => json!({"due": true, "confirmed": confirmed}),
        Target::All => json!({"all": true, "confirmed": confirmed}),
    };
    let passed = |client: &mut delphi_core::client::Client| crate::bridge_cmd::say_passed(client, "pages", confirmed);
    match (target_is_one(&params), client.call("pages.rebuild", params)) {
        (true, Ok(v)) => {
            println!("{} version {} · {} · {}", v["slug"].as_str().unwrap_or(""), v["version"], v["outcome"].as_str().unwrap_or(""), v["change"].as_str().unwrap_or(""));
            passed(client);
            if v["outcome"] == "claims_only" { 1 } else { 0 }
        }
        (false, Ok(v)) => {
            let built = v["built"].as_array().cloned().unwrap_or_default();
            for b in &built {
                println!("{} version {} · {} · {}", b["slug"].as_str().unwrap_or(""), b["version"], b["outcome"].as_str().unwrap_or(""), b["change"].as_str().unwrap_or(""));
            }
            for f in v["failed"].as_array().into_iter().flatten() {
                eprintln!("{}: {}", f["slug"].as_str().unwrap_or(""), f["error"].as_str().unwrap_or(""));
            }
            println!("{} of {} topics rebuilt", built.len(), v["topics"]);
            passed(client);
            if let Some(why) = v["stopped"].as_str() {
                eprintln!("delphi pages rebuild: stopped: {}", why);
                return 3;
            }
            if v["failed"].as_array().is_some_and(|f| !f.is_empty()) || built.iter().any(|b| b["outcome"] == "claims_only") { 1 } else { 0 }
        }
        (_, Err(e)) => {
            eprintln!("delphi pages rebuild: {}", e);
            if e.to_string().contains("[bridge.jobs] pages") { 2 } else { 1 }
        }
    }
}

fn target_is_one(params: &serde_json::Value) -> bool {
    params["slug"].is_string()
}

/// `delphi pages seeds --count` (`specs/139` §4 slice A, AC-17): for each topic, how many spans each row of §2.1 would give its seed, and
/// what the caps keep. Reads only: no page, record or index is written, and no model is called.
fn seeds_count(lib: &Library, home: &Path, ws: &Path, as_json: bool) -> i32 {
    use delphi_engine::topic_seed::{assemble, count_json, Assemble, Row};
    let ids = delphi_engine::query_guard::identifiers(ws);
    let ctx = delphi_engine::library::topics::DueCtx { ws: Some(ws), now: now_unix(), index: false };
    let topics = lib.topic_pages(&ctx);
    let lines: Vec<serde_json::Value> = topics.iter().map(|t| count_json(&t.slug, &assemble(lib, home, ws, &t.side, &Assemble { identifiers: &ids, search: true }))).collect();
    if as_json {
        println!("{}", json!({"topics": lines}));
        return 0;
    }
    if lines.is_empty() {
        println!("no topics yet: a research run makes one, or `delphi pages new <title>`");
        return 0;
    }
    for l in &lines {
        let rows: Vec<String> = Row::ALL
            .iter()
            .map(|r| {
                let row = &l["rows"][r.number() - 1];
                let (o, k) = (row["offered"].as_u64().unwrap_or(0), row["kept"].as_u64().unwrap_or(0));
                if o == k { format!("{} {}", r.name(), k) } else { format!("{} {} of {}", r.name(), k, o) }
            })
            .collect();
        let named = l["named_sources"].as_u64().unwrap_or(0);
        let waiting = if l["spans"] == 0 { "  (waiting for material)" } else { "" };
        println!("{}\n  {}{}\n  = {} spans, {} bytes, answer {} characters{}", l["slug"].as_str().unwrap_or(""), rows.join(" · "), if named > 0 { format!(" · {} source(s) named only", named) } else { String::new() }, l["spans"], l["bytes"], l["answer_chars"], waiting);
    }
    0
}

fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn cli(args: &[String], home: &Path, ws_dir: Option<&Path>) -> i32 {
    let lib = Library::in_home(home);
    let rest = &args[args.len().min(1)..];
    let pos = positional(rest);
    let (draft, style, note) = match (value_of(rest, "--draft"), value_of(rest, "--style"), value_of(rest, "--note")) {
        (Ok(d), Ok(s), Ok(n)) => (d, s, n),
        (d, s, n) => {
            eprintln!("delphi pages: {}", d.err().or(s.err()).or(n.err()).unwrap_or_default());
            return 2;
        }
    };
    match args.first().map(String::as_str) {
        Some("show") if !pos.is_empty() => {
            let Some(style) = style.as_deref().map_or(Some(Format::Apa), Format::parse).filter(|f| !matches!(f, Format::CslJson | Format::BibTex)) else {
                eprintln!("delphi pages: --style is apa, mla, chicago or plain");
                return 2;
            };
            let draft = match draft.map(|p| std::fs::read_to_string(&p).map_err(|e| format!("{}: {}", p, e))).transpose() {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("delphi pages: {}", e);
                    return 2;
                }
            };
            let topic = pos.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
            match lib.build_page(&topic, draft.as_deref(), style) {
                Ok(p) if !printable(home, &p.markdown) => 1,
                Ok(p) => {
                    print!("{}", p.markdown);
                    if !p.flags.is_empty() {
                        eprintln!("unverified figure (in no cited material): {}", p.flags.join(", "));
                    }
                    for f in &p.figure_failures {
                        eprintln!("figure dropped: {}", f);
                    }
                    if p.withheld > 0 {
                        eprintln!("{} claim(s) rest on a private source and are left off", p.withheld);
                    }
                    0
                }
                Err(f) => {
                    report(&f);
                    1
                }
            }
        }
        Some("check") if pos.len() == 1 => match std::fs::read_to_string(pos[0]) {
            Ok(t) => {
                let checked = lib.check_page_text(&t, Scope::External);
                if checked.findings.is_empty() {
                    println!("the page passes the mechanical check");
                    if !checked.flags.is_empty() {
                        println!("unverified figure (in no cited material): {}", checked.flags.join(", "));
                    }
                    0
                } else {
                    report(&checked.findings);
                    1
                }
            }
            Err(e) => {
                eprintln!("delphi pages check: {}: {}", pos[0], e);
                2
            }
        },
        Some("search") if !pos.is_empty() => {
            let (internal, external) = (rest.iter().any(|a| a == "--internal"), rest.iter().any(|a| a == "--external"));
            if internal && external {
                eprintln!("delphi pages search: --internal and --external are alternatives");
                return 2;
            }
            let Some(ws) = ws_dir else { return 4 };
            let mode = if external { Some(true) } else if internal { Some(false) } else { None };
            search(home, ws, &pos.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "), mode, rest.iter().any(|a| a == "--json"))
        }
        Some("expand") if !pos.is_empty() => {
            let Some(ws) = ws_dir else { return 4 };
            expand(home, ws, &pos.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "), rest.iter().any(|a| a == "--json"))
        }
        Some("list") => {
            let ttl = delphi_adapter::config::PagesConfig::default().external_ttl_secs;
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let pages = lib.pages(now, ttl);
            if rest.iter().any(|a| a == "--json") {
                println!("{}", json!(pages.iter().map(|p| p.to_json()).collect::<Vec<_>>()));
            } else if pages.is_empty() {
                println!("no cached pages");
            }
            for p in pages.iter().filter(|_| !rest.iter().any(|a| a == "--json")) {
                println!("{}  {}{}  {}{}", p.key, if p.pinned { "pinned " } else { "" }, p.outcome, p.query, p.stale.as_ref().map(|s| format!("  ({})", s)).unwrap_or_default());
            }
            0
        }
        Some(c @ ("pin" | "unpin")) if pos.len() == 1 => {
            let ttl = delphi_adapter::config::PagesConfig::default().external_ttl_secs;
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            match lib.pin_page(pos[0], c == "pin", now, ttl) {
                Ok(_) => {
                    println!("{} {}", pos[0], if c == "pin" { "pinned" } else { "unpinned" });
                    0
                }
                Err(e) => {
                    eprintln!("delphi pages {}: {}", c, e);
                    1
                }
            }
        }
        Some("new") if !pos.is_empty() => {
            if crate::xenia_terminal::inside_an_agent_seat() {
                eprintln!("delphi pages new: refused; a topic is the operator's to make and this process was started by an agent seat");
                return 2;
            }
            let title = pos.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
            match lib.topic_create(&title, delphi_engine::library::topics::Origin::Operator, now_unix()) {
                Ok(e) => {
                    println!("topic {} ({}); its page is built when the daemon is idle, or now with `delphi pages rebuild {}`", e.slug, e.title, e.slug);
                    0
                }
                Err(e) => {
                    eprintln!("delphi pages new: {}", e);
                    1
                }
            }
        }
        Some("topics") => {
            let ctx = delphi_engine::library::topics::DueCtx { ws: ws_dir, now: now_unix(), index: true };
            let topics = lib.topic_pages(&ctx);
            if rest.iter().any(|a| a == "--json") {
                println!("{}", json!(topics.iter().map(|t| t.to_json()).collect::<Vec<_>>()));
                return 0;
            }
            if topics.is_empty() {
                println!("no topics yet: a research run makes one, or `delphi pages new <title>`");
            }
            for t in &topics {
                let built = t.built.map(|b| format!("version {}, built {}{}", t.version, delphi_engine::library::page_meta::ymd(b), t.outcome.as_deref().filter(|o| *o == "seed").map(|_| ", seed draft").unwrap_or_default())).unwrap_or_else(|| if t.waiting { delphi_engine::topic_seed::WAITING.into() } else { "not built".into() });
                println!("{}  {}  {}{}", t.slug, built, t.title, t.due.as_ref().map(|d| format!("  (due: {})", d)).unwrap_or_default());
            }
            0
        }
        Some("topic") if pos.len() == 1 => {
            let version = match value_of(rest, "--version") {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("delphi pages topic: {}", e);
                    return 2;
                }
            };
            let ctx = delphi_engine::library::topics::DueCtx { ws: ws_dir, now: now_unix(), index: true };
            let Some((e, md)) = lib.topic_get(pos[0], version.as_deref(), &ctx) else {
                eprintln!("delphi pages topic: no topic `{}`{}", pos[0], version.map(|v| format!(" with a version `{}`", v)).unwrap_or_default());
                return 1;
            };
            if rest.iter().any(|a| a == "--json") {
                let mut v = e.to_json();
                v["markdown"] = json!(md);
                println!("{}", v);
                return 0;
            }
            if let Some(c) = &e.change {
                println!("changed: {}", c);
            }
            if md.is_empty() {
                println!("{} is not built yet", e.slug);
            }
            print!("{}", md);
            0
        }
        Some("versions") if pos.len() == 1 => {
            let Some(vs) = lib.topic_versions(pos[0]) else {
                eprintln!("delphi pages versions: no topic `{}`", pos[0]);
                return 1;
            };
            if rest.iter().any(|a| a == "--json") {
                println!("{}", json!(vs));
                return 0;
            }
            if vs.is_empty() {
                println!("no earlier version of {}", pos[0]);
            }
            for v in &vs {
                println!("{}  {}", v["version"].as_str().unwrap_or(""), v["change"].as_str().unwrap_or(""));
            }
            0
        }
        Some("seeds") if pos.is_empty() && rest.iter().any(|a| a == "--count") => {
            let Some(ws) = ws_dir else { return 4 };
            seeds_count(&lib, home, ws, rest.iter().any(|a| a == "--json"))
        }
        Some("rebuild") => {
            let Some(ws) = ws_dir else { return 4 };
            let (due, all) = (rest.iter().any(|a| a == "--due"), rest.iter().any(|a| a == "--all"));
            match (pos.as_slice(), due, all) {
                ([slug], false, false) => rebuild(home, ws, Target::Slug(slug)),
                ([], true, false) => rebuild(home, ws, Target::Due),
                ([], false, true) => rebuild(home, ws, Target::All),
                _ => {
                    eprintln!("usage: delphi pages rebuild <slug> | --due | --all");
                    2
                }
            }
        }
        Some("dispute") if pos.len() == 1 => {
            if crate::xenia_terminal::inside_an_agent_seat() {
                eprintln!("delphi pages dispute: refused; a dispute is the operator's to file and this process was started by an agent seat");
                return 2;
            }
            match lib.dispute_claim(pos[0], note.as_deref().unwrap_or("")) {
                Ok(c) => {
                    println!("{} contested: {}", pos[0], delphi_engine::library::claims::status(&c).label());
                    0
                }
                Err(e) => {
                    eprintln!("delphi pages dispute: {}", e);
                    1
                }
            }
        }
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A held source written as the library keeps one (`specs/142` §2.5 item 2): its record with the `text_held` mark and its text.
    pub(crate) fn held_source(home: &Path, id: &str, text: &str) {
        let dir = home.join("library/sources").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("meta.json"), json!({"type": "webpage", "title": "Held", "licence": "unknown", "capture_level": "full", "text_held": true, "status": "not validated"}).to_string()).unwrap();
        std::fs::write(dir.join("source.md"), text).unwrap();
    }

    /// `specs/142` AC-9, a page printed: a page that carries 400 characters of a held source no record of it names is not printed, and
    /// nothing reaches the output; a page that quotes 200 characters of it is printed whole.
    #[test]
    fn a_page_that_carries_a_held_source_past_the_bound_is_not_printed() {
        let home = tempfile::tempdir().unwrap();
        let text: String = (0..300).map(|i| format!("Held sentence {i:03} runs on and on. ")).collect();
        held_source(home.path(), "held-a", &text);
        let long = format!("# Tides\n\nA note copied this: {}\n", text.chars().skip(50).take(400).collect::<String>());
        let mut out = Vec::new();
        let err = emit(home.path(), &long, &mut out).unwrap_err();
        assert!(err.contains("s-held-a") && out.is_empty(), "{err}");
        let short = format!("# Tides\n\nA short quote: {}\n", text.chars().skip(50).take(200).collect::<String>());
        emit(home.path(), &short, &mut out).unwrap();
        assert_eq!(out, short.as_bytes());
    }

    fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(tree(&p));
            } else {
                out.push((p.display().to_string(), std::fs::read(&p).unwrap_or_default()));
            }
        }
        out.sort();
        out
    }

    /// `specs/139` AC-17: `delphi pages seeds --count` counts each topic's spans by row and writes nothing.
    #[test]
    fn pages_seeds_count_counts_by_row_and_writes_nothing() {
        use delphi_engine::research_record::{Asker, Begin, Topic};
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let t = Topic::begin(&ws, &Begin { question: "Why do tides lag?", asked_by: Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-1" }).unwrap();
        t.finish("r-1", "done", Some(&json!({"answer": "Tides lag.\n\n## Why\n\nThe estuary is shallow.", "claims": []})), &[], None, 0).unwrap();
        let lib = Library::in_home(home.path());
        lib.topics_from_research(&ws, 1);
        lib.topic_create("Lighthouses", delphi_engine::library::topics::Origin::Operator, 1).unwrap();
        let before = tree(home.path());
        assert_eq!(cli(&["seeds".into(), "--count".into(), "--json".into()], home.path(), Some(&ws)), 0);
        assert_eq!(cli(&["seeds".into(), "--count".into()], home.path(), Some(&ws)), 0);
        assert_eq!(tree(home.path()), before, "the count writes nothing");
        assert_eq!(cli(&["seeds".into()], home.path(), Some(&ws)), 2, "only the count exists");
        let seed = delphi_engine::topic_seed::assemble(&lib, home.path(), &ws, &lib.topic_side("why-do-tides-lag").unwrap(), &delphi_engine::topic_seed::Assemble { identifiers: &[], search: true });
        let line = delphi_engine::topic_seed::count_json("why-do-tides-lag", &seed);
        assert_eq!((line["rows"][0]["kept"].clone(), line["spans"].clone()), (json!(2), json!(2)));
    }

    #[test]
    fn pages_expand_usage_on_missing_args() {
        let d = tempfile::tempdir().unwrap();
        let code = cli(&["expand".to_string()], d.path(), None);
        assert_eq!(code, 2);
    }
}
