//! `delphi kb extract` and `delphi kb attest` (`specs/068` §6.5, §6.6): key facts from a summary page, checked against the
//! stored source by the daemon, then judged one card at a time by the operator. Nothing here validates by itself.

use crate::library::extract::{Candidate, ATTESTED_LABEL, MAX_PER_PAGE};
use crate::library::Library;
use serde_json::Value;
use std::io::BufRead;
use std::path::Path;

const USAGE_EXTRACT: &str = "usage: delphi kb extract <page-id> (--auto | <candidates.json | ->)   (--auto takes the sentences a held source says word for word; else a JSON list of {\"claim\", \"source\", \"quote\", \"locator\"})";

fn flat(s: &str, n: usize) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if one.chars().count() > n { format!("{}…", one.chars().take(n - 1).collect::<String>()) } else { one }
}

fn candidates(text: &str) -> Result<Vec<Candidate>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {}", e))?;
    let items = v.as_array().ok_or("expected a JSON list")?;
    let get = |o: &Value, k: &str| o[k].as_str().unwrap_or("").to_string();
    Ok(items.iter().map(|o| Candidate { claim: get(o, "claim"), source_id: get(o, "source"), quote: get(o, "quote"), locator: get(o, "locator") }).collect())
}

/// Propose key facts from a page. The page body is never read here: each candidate stands on its own quote.
pub fn extract_cmd(home: &Path, ws: &Path, args: &[String]) -> i32 {
    let rest: Vec<&String> = args.iter().filter(|a| !a.starts_with("--") || a.as_str() == "-").collect();
    let auto = args.iter().any(|a| a == "--auto");
    let (id, src) = match (&rest[..], auto) {
        ([id], true) => (*id, None),
        ([id, src], false) => (*id, Some(*src)),
        _ => {
            eprintln!("{}", USAGE_EXTRACT);
            return 2;
        }
    };
    if !ws.join(crate::kb_import::DIR).join(format!("{}.md", id)).is_file() || id.ends_with(".bib") || id.contains('/') || id.contains("..") {
        eprintln!("delphi kb extract: no imported page with id `{}`", flat(id, 80));
        return 1;
    }
    let lib = Library::in_home(home);
    let cands = match src {
        None => {
            let page = std::fs::read_to_string(ws.join(crate::kb_import::DIR).join(format!("{}.md", id))).unwrap_or_default();
            let found = lib.auto_candidates(crate::workspace::split_frontmatter(&page).1);
            if found.is_empty() {
                println!("No sentence of page:{} appears word for word in a source the library holds; add the sources (`delphi sources add`) or give a candidates file", id);
                return 0;
            }
            found
        }
        Some(src) => {
            let mut text = String::new();
            let read = if src.as_str() == "-" { std::io::Read::read_to_string(&mut std::io::Read::take(std::io::stdin(), 1 << 20), &mut text).map(|_| ()) } else { std::fs::read_to_string(src.as_str()).map(|t| text = t) };
            if let Err(e) = read {
                eprintln!("delphi kb extract: {}: {}", src, e);
                return 1;
            }
            match candidates(&text) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("delphi kb extract: {}", e);
                    return 1;
                }
            }
        }
    };
    let r = lib.extract_candidates(id, &cands);
    println!("Candidates for page:{}  {} added · {} already known · {} rejected (up to {} open per page)", id, r.accepted.len(), r.existing.len(), r.rejected.len(), MAX_PER_PAGE);
    for (i, why) in &r.rejected {
        println!("  #{} rejected: {}", i + 1, flat(why, 120));
    }
    if !r.accepted.is_empty() {
        println!("  Not validated yet. Check them: delphi kb attest {}", id);
    }
    0
}

/// One card per open candidate, grouped by source: `y` confirm, `n` reject, `s` skip, `q` stop. The daemon has already
/// confirmed the quote is in the source, so the operator judges only meaning and truth.
pub fn attest_cmd(home: &Path, args: &[String]) -> i32 {
    let lib = Library::in_home(home);
    let page = args.iter().find(|a| !a.starts_with("--")).cloned();
    let mut cards: Vec<(String, Value)> = Vec::new();
    for c in lib.claims() {
        let Some(origin) = c["origin"].as_str().and_then(|o| o.strip_prefix("page:")) else { continue };
        if page.as_deref().is_some_and(|p| p != origin) {
            continue;
        }
        if lib.open_candidates(origin).iter().any(|o| o["id"] == c["id"]) {
            let source = c["anchors"][0]["source_id"].as_str().unwrap_or("?").to_string();
            cards.push((source, c));
        }
    }
    cards.sort_by(|a, b| a.0.cmp(&b.0).then(a.1["id"].as_str().cmp(&b.1["id"].as_str())));
    if cards.is_empty() {
        println!("nothing to check{}", page.map(|p| format!(" for page:{}", p)).unwrap_or_default());
        return 0;
    }
    let total = cards.len();
    let (mut yes, mut no) = (0, 0);
    let mut last = String::new();
    let mut input = std::io::stdin().lock().lines();
    for (n, (source, c)) in cards.iter().enumerate() {
        if *source != last {
            println!("\nSource {}", flat(source, 80));
            last = source.clone();
        }
        let a = &c["anchors"][0];
        let id = c["id"].as_str().unwrap_or("");
        println!("[{}/{}] {}\n  claim: {}\n  quote: \"{}\"  ({})\n  y confirm · n reject · s skip · q stop  → on y: {}", n + 1, total, id, flat(c["text"].as_str().unwrap_or(""), 300), flat(a["quote"].as_str().unwrap_or(""), 300), flat(a["locator"].as_str().unwrap_or(""), 60), ATTESTED_LABEL);
        let key = match input.next() {
            Some(Ok(l)) => l.trim().to_lowercase(),
            _ => break,
        };
        let done = match key.as_str() {
            "y" => lib.attest_claim(id).map(|_| yes += 1),
            "n" => lib.reject_claim(id).map(|_| no += 1),
            "q" => break,
            _ => Ok(()),
        };
        if let Err(e) = done {
            eprintln!("  {}", e);
        }
    }
    println!("\n{} confirmed · {} rejected · {} left for later", yes, no, total - yes - no);
    0
}
