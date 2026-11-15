//! Knowledge health (`specs/119` §5; slice B4): the state of the knowledge loop in one place.
//!
//! [`gather`] counts what the install holds today: sources by how they are kept, claims by status, pages, imported summaries
//! with no source listed, staged notes waiting and the oldest, ratified lessons per role, research requests in 30 days and how
//! many ran, and searches of the knowledge base that found nothing. Stores that later slices add (research topics of
//! `specs/112`, provisional notes of `specs/116`, due pages of `specs/117`) are counted when their folder exists and left out
//! when it does not; a store that is missing or unreadable is never an error.
//!
//! [`Health::lines`] is what `delphi kb health` and the doctor print. A line that marks a stalled loop says so in words and
//! names the command that helps (§5 item 2). [`write_daily`] appends the same figures to `local/health.jsonl`, at most once a day.
//!
//! Read-only apart from that one file, which is written only inside a workspace that already exists: nothing here creates the
//! Delphi home, a workspace or a library. It calls no model and no network, and prints no text of any source, claim or note.

use crate::research_asks::THIRTY_DAYS;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// The daily series, in the workspace.
pub const FILE: &str = "local/health.jsonl";
/// A staged note that has waited this long marks the review loop as stalled.
const STAGED_STALE_DAYS: i64 = 14;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Health {
    /// `YYYY-MM-DD`, UTC.
    pub date: String,
    /// `pointer`, `excerpts`, `full`, `private` (a private source is counted there, whatever its level), and `purged` when any.
    pub sources: BTreeMap<String, usize>,
    /// By derived status (`noted`, `checking`, `validated`, …).
    pub claims: BTreeMap<String, usize>,
    pub pages: usize,
    pub summaries: usize,
    pub summaries_without_source: usize,
    pub staged: usize,
    /// The day the oldest waiting note was staged.
    pub staged_oldest: Option<String>,
    /// Ratified rules in each role's lessons files (operator and workspace), for every role the workspace has.
    pub lessons: BTreeMap<String, usize>,
    pub research_asked: usize,
    pub research_ran: usize,
    pub searches_found_nothing: usize,
    /// Research topics (`specs/112`): how many, when the store exists.
    pub topics: Option<usize>,
    /// Provisional notes (`specs/116`): how many wait and the day of the oldest, when the store exists.
    pub provisional: Option<(usize, Option<String>)>,
}

/// One printed line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    /// The line marks a loop that is not turning; its text says so and names the command that helps.
    pub stalled: bool,
}

fn day_of(unix: i64) -> String {
    delphi_core::time::format_unix_millis(unix, 0).chars().take(10).collect()
}

fn count(map: &mut BTreeMap<String, usize>, key: &str) {
    *map.entry(key.to_string()).or_insert(0) += 1;
}

/// Regular files under `dir` (any depth) whose name ends in `suffix`, with each one's modification time in Unix seconds.
fn files_under(dir: &Path, suffix: &str, skip: &[&str]) -> Vec<i64> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let (p, name) = (e.path(), e.file_name().to_string_lossy().to_string());
            match e.file_type() {
                Ok(t) if t.is_dir() && !skip.contains(&name.as_str()) => stack.push(p),
                Ok(t) if t.is_file() && name.ends_with(suffix) => {
                    let at = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
                    out.push(at);
                }
                _ => {}
            }
        }
    }
    out
}

fn rules_in(path: &Path) -> usize {
    std::fs::read_to_string(path).map(|t| crate::workspace::rule_ids(crate::workspace::split_frontmatter(&t).1).len()).unwrap_or(0)
}

/// Count everything. `now` is Unix seconds. Reads only.
pub fn gather(home: &Path, ws_dir: &Path, now: i64) -> Health {
    let mut h = Health { date: day_of(now), ..Health::default() };

    // The library is the install's: sources and claims, and the pages built from them.
    let lib = crate::library::Library::in_home(home);
    for (_, meta) in lib.list() {
        let kept = if !meta["purged"].is_null() {
            "purged"
        } else if crate::library::is_private(&meta) {
            "private"
        } else {
            match meta["capture_level"].as_str() {
                Some("full") => "full",
                Some("excerpts") => "excerpts",
                _ => "pointer",
            }
        };
        count(&mut h.sources, kept);
    }
    for claim in lib.claims() {
        count(&mut h.claims, crate::library::claims::status(&claim).as_str());
    }
    h.pages = files_under(&lib.root().join("pages"), ".json", &[]).len();

    // Imported summary pages and their bibliographies (`specs/068`).
    let summaries = ws_dir.join(crate::kb_import::DIR);
    for e in std::fs::read_dir(&summaries).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(id) = name.strip_suffix(".md").filter(|n| !n.ends_with(".bib")) else { continue };
        h.summaries += 1;
        let bib = std::fs::read_to_string(summaries.join(format!("{id}.bib.md"))).unwrap_or_default();
        if !bib.lines().any(|l| l.strip_prefix("sources: ").is_some_and(|n| n.trim().parse::<usize>().is_ok_and(|n| n > 0))) {
            h.summaries_without_source += 1;
        }
    }

    // Staged notes that wait for the operator (`specs/031`).
    let (notes, _) = crate::learn::scan(ws_dir);
    h.staged = notes.len();
    h.staged_oldest = notes.iter().map(|n| n.timestamp.chars().take(10).collect::<String>()).filter(|d| d.len() == 10).min();

    // Ratified lessons, per role the workspace has.
    let ratified = ws_dir.join("local/ratified");
    for e in std::fs::read_dir(ratified.join("agents")).into_iter().flatten().flatten() {
        let role = e.file_name().to_string_lossy().to_string();
        if !e.path().join("AGENT.md").is_file() {
            continue;
        }
        let rel = format!("memory/agents/{role}/lessons.md");
        h.lessons.insert(role, rules_in(&home.join("operator/ratified").join(&rel)) + rules_in(&ratified.join(&rel)));
    }

    // Research asked for, and searches of the knowledge base that found nothing, in the last 30 days.
    let asks = crate::research_asks::counts_since(ws_dir, now - THIRTY_DAYS);
    (h.research_asked, h.research_ran) = (asks.asked, asks.ran);
    h.searches_found_nothing = std::fs::read_to_string(ws_dir.join("local/kb-misses.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["confirmed"] != true && v["t"].as_str().and_then(|t| t.parse::<i64>().ok()).is_some_and(|t| t >= now - THIRTY_DAYS))
        .count();

    // Stores of later slices: counted when they are there.
    let topics = ws_dir.join("research/topics");
    if topics.is_dir() {
        h.topics = Some(std::fs::read_dir(&topics).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).count());
    }
    let provisional = ws_dir.join("local/memory/provisional");
    if provisional.is_dir() {
        // The shelves a note leaves the tier for (`provisional_notes::SHELVES`: expired, rejected, kept) do not wait.
        let notes = files_under(&provisional, ".md", &crate::provisional_notes::SHELVES);
        h.provisional = Some((notes.len(), notes.iter().min().map(|t| day_of(*t))));
    }
    h
}

impl Health {
    pub fn sources_total(&self) -> usize {
        self.sources.values().sum()
    }

    pub fn claims_total(&self) -> usize {
        self.claims.values().sum()
    }

    /// The figures as one JSON object: the line of `local/health.jsonl`, and the read for the app. A store that is absent has no key.
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("date".into(), json!(self.date));
        o.insert("sources".into(), json!({"total": self.sources_total(), "kept": self.sources}));
        o.insert("claims".into(), json!({"total": self.claims_total(), "status": self.claims}));
        o.insert("pages".into(), json!(self.pages));
        o.insert("summaries".into(), json!({"total": self.summaries, "no_source": self.summaries_without_source}));
        o.insert("staged_notes".into(), json!({"waiting": self.staged, "oldest": self.staged_oldest}));
        o.insert("lessons".into(), json!(self.lessons));
        o.insert("research_30d".into(), json!({"asked": self.research_asked, "ran": self.research_ran}));
        o.insert("searches_found_nothing_30d".into(), json!(self.searches_found_nothing));
        if let Some(n) = self.topics {
            o.insert("topics".into(), json!({"total": n}));
        }
        if let Some((n, oldest)) = &self.provisional {
            o.insert("provisional_notes".into(), json!({"waiting": n, "oldest": oldest}));
        }
        Value::Object(o)
    }

    /// What `delphi kb health` and the doctor print, in a fixed order. `now` is Unix seconds, for the age of the oldest note.
    pub fn lines(&self, now: i64) -> Vec<Line> {
        let fact = |text: String| Line { text, stalled: false };
        let stalled = |text: String| Line { text, stalled: true };
        let setup = crate::research_setup::SETUP_COMMAND;
        let by = |m: &BTreeMap<String, usize>, order: &[&str]| -> String {
            let mut parts: Vec<String> = order.iter().filter(|k| m.contains_key(**k) || !matches!(**k, "purged")).map(|k| format!("{} {}", k, m.get(*k).copied().unwrap_or(0))).collect();
            parts.extend(m.iter().filter(|(k, _)| !order.contains(&k.as_str())).map(|(k, n)| format!("{k} {n}")));
            parts.join(", ")
        };
        let mut out = Vec::new();

        // Sources, by how they are kept.
        let total = self.sources_total();
        let kept = by(&self.sources, &["pointer", "excerpts", "full", "private", "purged"]);
        let with_text = total - self.sources.get("pointer").copied().unwrap_or(0) - self.sources.get("purged").copied().unwrap_or(0);
        out.push(if total == 0 {
            stalled(format!("0 sources: research has kept no source. See {setup}."))
        } else if with_text == 0 {
            stalled(format!("{total} sources ({kept}): none has text kept, so none can be searched or quoted. See delphi sources add <url> --level excerpts."))
        } else {
            fact(format!("{total} sources ({kept})"))
        });

        // Claims, by status.
        out.push(if self.claims_total() == 0 {
            stalled(format!("0 claims: research has kept no claim. See {setup}."))
        } else {
            let order = ["noted", "checking", "corroborated", "substantiated", "vetted", "validated", "contested", "disproved", "retired"];
            fact(format!("{} claims ({})", self.claims_total(), order.iter().filter_map(|k| self.claims.get(*k).map(|n| format!("{k} {n}"))).collect::<Vec<_>>().join(", ")))
        });

        if let Some(n) = self.topics {
            out.push(fact(format!("{n} research topics")));
        }

        out.push(if self.pages == 0 { stalled("0 pages: no page has been built from the library. See delphi pages search \"<question>\".".into()) } else { fact(format!("{} pages", self.pages)) });

        if self.summaries_without_source > 0 {
            out.push(stalled(format!("{} imported summaries, {} with no source listed: what they say cannot be traced. See delphi kb import <file> (import the page again with its links).", self.summaries, self.summaries_without_source)));
        } else {
            out.push(fact(format!("{} imported summaries, 0 with no source listed", self.summaries)));
        }

        let age = |day: &Option<String>| day.as_deref().and_then(|d| delphi_core::xenia_index::unix_of(&format!("{d}T00:00:00Z"))).map(|t| (now - t) / 86_400);
        match (self.staged, &self.staged_oldest) {
            (0, _) => out.push(fact("0 staged notes waiting".into())),
            (n, oldest) if age(oldest).is_some_and(|d| d >= STAGED_STALE_DAYS) => out.push(stalled(format!("{n} staged notes waiting, the oldest since {}: nothing has been reviewed for {} days. See delphi learn list.", oldest.as_deref().unwrap_or("?"), age(oldest).unwrap_or(0)))),
            (n, oldest) => out.push(fact(format!("{n} staged notes waiting, the oldest since {}", oldest.as_deref().unwrap_or("an unknown day")))),
        }
        if let Some((n, oldest)) = &self.provisional {
            out.push(fact(match oldest {
                Some(d) if *n > 0 => format!("{n} provisional notes waiting, the oldest since {d}"),
                _ => format!("{n} provisional notes waiting"),
            }));
        }

        let per_role = self.lessons.iter().map(|(r, n)| format!("{r} {n}")).collect::<Vec<_>>().join(", ");
        out.push(if self.lessons.values().all(|n| *n == 0) {
            stalled(format!("0 lessons for any role{}: nothing learned has been kept. See delphi learn list.", if per_role.is_empty() { String::new() } else { format!(" ({per_role})") }))
        } else {
            fact(format!("lessons per role: {per_role}"))
        });

        out.push(if self.research_asked > 0 && self.research_ran == 0 {
            stalled(format!("{} research requests in 30 days, 0 ran: no search engine was set up. See {setup}.", self.research_asked))
        } else {
            fact(format!("{} research requests in 30 days, {} ran", self.research_asked, self.research_ran))
        });

        out.push(fact(format!("{} searches of the knowledge base found nothing in 30 days", self.searches_found_nothing)));
        out
    }
}

/// Append today's figures to the workspace's `local/health.jsonl` unless today's line is already there. `Ok(true)` when a line
/// was written. Writes nothing, and says so, when the workspace has no `local/` directory: it never creates a workspace.
pub fn write_daily(ws_dir: &Path, h: &Health) -> Result<bool, String> {
    use std::io::Write;
    let path = ws_dir.join(FILE);
    if !path.parent().is_some_and(Path::is_dir) {
        return Ok(false);
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    if have.lines().rev().take(8).filter_map(|l| serde_json::from_str::<Value>(l).ok()).any(|v| v["date"] == h.date.as_str()) {
        return Ok(false);
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("{}: {}", FILE, e))?;
    writeln!(f, "{}", h.to_json()).map_err(|e| format!("{}: {}", FILE, e))?;
    Ok(true)
}

/// The daemon's part: is it time to look again? True at most once an hour per process; the caller then gathers and calls
/// [`write_daily`] on a blocking thread, which writes only when today's line is missing.
pub fn tick_due(now: i64) -> bool {
    use std::sync::atomic::{AtomicI64, Ordering::SeqCst};
    static CHECKED: AtomicI64 = AtomicI64::new(i64::MIN / 2);
    let last = CHECKED.load(SeqCst);
    now - last >= 3600 && CHECKED.compare_exchange(last, now, SeqCst, SeqCst).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const NOW: i64 = 1_800_000_000;

    /// A home with a workspace `w` that has a `local/` directory and two roles.
    fn home() -> (tempfile::TempDir, std::path::PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path().join("workspaces/w");
        for role in ["scout", "writer"] {
            fs::create_dir_all(ws.join(format!("local/ratified/agents/{role}"))).unwrap();
            fs::write(ws.join(format!("local/ratified/agents/{role}/AGENT.md")), "---\nid: x\n---\n").unwrap();
        }
        (t, ws)
    }

    fn source(home: &Path, id: &str, meta: Value) {
        let dir = home.join("library/sources").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("meta.json"), meta.to_string()).unwrap();
    }

    fn texts(h: &Health) -> Vec<String> {
        h.lines(NOW).into_iter().map(|l| l.text).collect()
    }

    // specs/119 AC-7: a workspace with sources and no claims prints the stalled-loop line, in the words of §5 item 2.
    #[test]
    fn sources_and_no_claims_prints_the_stalled_loop_line_with_the_command_that_helps() {
        let (t, ws) = home();
        for i in 0..3 {
            source(t.path(), &format!("s-{i:012x}"), json!({"capture_level": "pointer", "licence": "unknown"}));
        }
        let h = gather(t.path(), &ws, NOW);
        assert_eq!((h.sources_total(), h.claims_total()), (3, 0));
        let lines = h.lines(NOW);
        let claims = lines.iter().find(|l| l.text.starts_with("0 claims")).expect("the claims line");
        assert_eq!(claims.text, "0 claims: research has kept no claim. See delphi research setup.");
        assert!(claims.stalled);
        // All three are pointers: that is said too, with its own command.
        let sources = &lines[0];
        assert!(sources.stalled && sources.text.starts_with("3 sources (pointer 3, excerpts 0, full 0, private 0): none has text kept") && sources.text.contains("See delphi sources add"), "{}", sources.text);
    }

    #[test]
    fn everything_that_exists_today_is_counted() {
        let (t, ws) = home();
        source(t.path(), "s-000000000001", json!({"capture_level": "pointer"}));
        source(t.path(), "s-000000000002", json!({"capture_level": "excerpts"}));
        source(t.path(), "s-000000000003", json!({"capture_level": "full"}));
        source(t.path(), "s-000000000004", json!({"capture_level": "full", "private": true}));
        source(t.path(), "s-000000000005", json!({"capture_level": "full", "purged": "2026-11-01"}));
        let lib = crate::library::Library::in_home(t.path());
        fs::create_dir_all(lib.claims_dir()).unwrap();
        for (i, c) in [json!({"id": "c-01", "text": "a"}), json!({"id": "c-02", "text": "b"})].iter().enumerate() {
            fs::write(lib.claims_dir().join(format!("c-0{i}.json")), c.to_string()).unwrap();
        }
        fs::create_dir_all(lib.root().join("pages")).unwrap();
        fs::write(lib.root().join("pages/k1.json"), "{}").unwrap();
        fs::write(lib.root().join("pages/k1.md"), "page").unwrap();
        let sums = ws.join(crate::kb_import::DIR);
        fs::create_dir_all(&sums).unwrap();
        for (id, bib) in [("a", Some("sources: 2\n")), ("b", Some("sources: none\n")), ("c", None)] {
            fs::write(sums.join(format!("{id}.md")), "# page").unwrap();
            if let Some(b) = bib {
                fs::write(sums.join(format!("{id}.bib.md")), b).unwrap();
            }
        }
        let lessons = "---\nid: l\n---\n### [L-1] One\n- a\n### [L-2] Two\n- b\n";
        fs::create_dir_all(t.path().join("operator/ratified/memory/agents/scout")).unwrap();
        fs::write(t.path().join("operator/ratified/memory/agents/scout/lessons.md"), lessons).unwrap();
        fs::create_dir_all(ws.join("local/ratified/memory/agents/scout")).unwrap();
        fs::write(ws.join("local/ratified/memory/agents/scout/lessons.md"), "### [L-3] Three\n- c\n").unwrap();
        crate::research_asks::record(&ws, crate::research_asks::Outcome::Queued, NOW - 86_400);
        crate::research_asks::record(&ws, crate::research_asks::Outcome::NoEngine, NOW - 3600);
        crate::research_asks::record(&ws, crate::research_asks::Outcome::Queued, NOW - 40 * 86_400);
        let misses: String = [(NOW - 100, false), (NOW - 200, false), (NOW - 50 * 86_400, false), (NOW - 10, true)].iter().map(|(t, c)| format!("{}\n", json!({"t": t.to_string(), "query": "q", "results": 0, "confirmed": c}))).collect();
        fs::write(ws.join("local/kb-misses.jsonl"), misses).unwrap();

        let h = gather(t.path(), &ws, NOW);
        assert_eq!(h.sources, BTreeMap::from([("pointer".to_string(), 1), ("excerpts".to_string(), 1), ("full".to_string(), 1), ("private".to_string(), 1), ("purged".to_string(), 1)]));
        assert_eq!(h.claims_total(), 2);
        assert_eq!((h.pages, h.summaries, h.summaries_without_source), (1, 3, 2));
        assert_eq!(h.lessons, BTreeMap::from([("scout".to_string(), 3), ("writer".to_string(), 0)]));
        assert_eq!((h.research_asked, h.research_ran, h.searches_found_nothing), (2, 1, 2));
        assert_eq!((h.staged, h.topics, &h.provisional), (0, None, &None));
        let text = texts(&h).join("\n");
        for needle in [
            "5 sources (pointer 1, excerpts 1, full 1, private 1, purged 1)",
            "2 claims (",
            "1 pages",
            "3 imported summaries, 2 with no source listed: what they say cannot be traced. See delphi kb import",
            "0 staged notes waiting",
            "lessons per role: scout 3, writer 0",
            "2 research requests in 30 days, 1 ran",
            "2 searches of the knowledge base found nothing in 30 days",
        ] {
            assert!(text.contains(needle), "{needle}\n{text}");
        }
        assert!(!text.contains("topics") && !text.contains("provisional"), "a store that is not there is not reported: {text}");
        let j = h.to_json();
        assert_eq!((j["sources"]["total"].as_u64(), j["claims"]["total"].as_u64(), j["summaries"]["no_source"].as_u64(), j["research_30d"]["ran"].as_u64()), (Some(5), Some(2), Some(2), Some(1)));
        assert!(j.get("topics").is_none() && j.get("provisional_notes").is_none());
    }

    #[test]
    fn stores_of_later_slices_are_reported_when_present_and_left_out_when_not() {
        let (t, ws) = home();
        fs::create_dir_all(ws.join("research/topics/what-is-tokio-0a1b2c3d")).unwrap();
        fs::create_dir_all(ws.join("research/topics/raft-leader-4e5f6a7b")).unwrap();
        fs::create_dir_all(ws.join("local/memory/provisional/scout")).unwrap();
        fs::create_dir_all(ws.join("local/memory/provisional/expired")).unwrap();
        fs::write(ws.join("local/memory/provisional/scout/n1.md"), "note").unwrap();
        fs::write(ws.join("local/memory/provisional/expired/n0.md"), "old").unwrap();
        let h = gather(t.path(), &ws, NOW);
        assert_eq!(h.topics, Some(2));
        assert_eq!(h.provisional.as_ref().map(|p| p.0), Some(1), "expired notes do not wait");
        let text = texts(&h).join("\n");
        assert!(text.contains("2 research topics") && text.contains("1 provisional notes waiting"), "{text}");
        let j = h.to_json();
        assert_eq!((j["topics"]["total"].as_u64(), j["provisional_notes"]["waiting"].as_u64()), (Some(2), Some(1)));
    }

    #[test]
    fn each_stalled_loop_says_so_in_words_and_names_a_command() {
        let (t, ws) = home();
        crate::research_asks::record(&ws, crate::research_asks::Outcome::NoEngine, NOW - 60);
        let h = gather(t.path(), &ws, NOW);
        let lines = h.lines(NOW);
        let stalled: Vec<&Line> = lines.iter().filter(|l| l.stalled).collect();
        for want in [
            "0 sources: research has kept no source. See delphi research setup.",
            "0 claims: research has kept no claim. See delphi research setup.",
            "0 pages: no page has been built from the library. See delphi pages search \"<question>\".",
            "0 lessons for any role (scout 0, writer 0): nothing learned has been kept. See delphi learn list.",
            "1 research requests in 30 days, 0 ran: no search engine was set up. See delphi research setup.",
        ] {
            assert!(stalled.iter().any(|l| l.text == want), "{want}\n{:?}", lines);
        }
        for l in &stalled {
            assert!(l.text.contains(": ") && l.text.contains("See delphi "), "a stalled line says why and what helps: {}", l.text);
        }
        // An old staged note is a stalled review; a recent one is a fact.
        let mut h = Health { staged: 4, staged_oldest: Some(day_of(NOW - 30 * 86_400)), ..h };
        assert!(h.lines(NOW).iter().any(|l| l.stalled && l.text.starts_with("4 staged notes waiting, the oldest since ") && l.text.ends_with("See delphi learn list.")));
        h.staged_oldest = Some(day_of(NOW - 2 * 86_400));
        assert!(h.lines(NOW).iter().any(|l| !l.stalled && l.text.starts_with("4 staged notes waiting, the oldest since ")));
    }

    // It must never create the Delphi home, and a home with nothing in it is a set of zeros, not an error.
    #[test]
    fn nothing_is_created_and_an_empty_or_missing_home_is_all_zeros() {
        let t = tempfile::tempdir().unwrap();
        let (home, ws) = (t.path().join("no-home"), t.path().join("no-home/workspaces/w"));
        let h = gather(&home, &ws, NOW);
        assert_eq!((h.sources_total(), h.claims_total(), h.pages, h.summaries, h.staged, h.research_asked), (0, 0, 0, 0, 0, 0));
        assert!(h.lessons.is_empty());
        assert!(!h.lines(NOW).is_empty());
        assert_eq!(write_daily(&ws, &h), Ok(false), "no workspace: no file");
        assert!(!home.exists(), "nothing was created");
    }

    #[test]
    fn the_daily_file_gets_one_line_a_day() {
        let (t, ws) = home();
        let today = gather(t.path(), &ws, NOW);
        assert_eq!(write_daily(&ws, &today), Ok(true));
        assert_eq!(write_daily(&ws, &today), Ok(false), "today's line is there");
        source(t.path(), "s-000000000001", json!({"capture_level": "full"}));
        let tomorrow = gather(t.path(), &ws, NOW + 86_400);
        assert_eq!(write_daily(&ws, &tomorrow), Ok(true));
        let text = fs::read_to_string(ws.join(FILE)).unwrap();
        let rows: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], today.to_json());
        assert_ne!(rows[0]["date"], rows[1]["date"]);
        assert_eq!((rows[0]["sources"]["total"].as_u64(), rows[1]["sources"]["total"].as_u64()), (Some(0), Some(1)));
    }
}
