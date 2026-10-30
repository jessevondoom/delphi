//! Ratifying a rule proposal (`specs/004` §4.2, `specs/031` slice L2b).
//!
//! A proposal made with `suggest_rule` is a `learn.proposed` event ([`crate::learn::LiveProposal`]). Nothing here runs by
//! itself: the operator's own command calls [`accept`], which checks the proposal, edits the scope's `heuristics.md` by
//! compare-and-swap ([`crate::ratify::edit_pinned`]) and returns what changed. The new rule applies from the next task.
//!
//! The checks are deterministic and make no model call: the no-relaxation linter, the workspace's `[privacy]` names for
//! operator scope, a lexical near-duplicate test against the rules already in the file, and the file's caps (25 rules,
//! 6 KB, 350 characters a rule). Embedding similarity is not used yet.

use crate::distill::Privacy;
use crate::learn::{LiveProposal, MAX_PROPOSAL_TEXT};
use crate::ratify::{self, RatifyError, Scope};
use std::collections::BTreeSet;
use std::path::Path;

pub const MAX_RULES: usize = 25;
pub const MAX_BYTES: usize = 6144;
/// Jaccard overlap of the words of two rules at or above which an `add` is refused as a duplicate.
pub const DUPLICATE_AT: f64 = 0.6;
/// Word overlap at or above which an existing rule is *shown* beside an `add` as a supersession candidate (`specs/050` ML-2).
pub const SUPERSESSION_AT: f64 = 0.3;
/// Most rules one `add` may name as replaced.
pub const MAX_SUPERSEDES: usize = 3;

/// A rule id as this file numbers them: `W-` or `O-` and digits.
pub fn is_rule_id(id: &str) -> bool {
    id.strip_prefix("W-").or_else(|| id.strip_prefix("O-")).is_some_and(|n| (1..=6).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
}

/// An active rule a proposed `add` may replace: named by the proposal, or close to it by shared words. Shown, never applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub named: bool,
    pub overlap: f64,
}

impl Candidate {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({"id": self.id, "named": self.named, "overlap": (self.overlap * 100.0).round() / 100.0})
    }
}

/// The rules in `rules` that `p` names in `supersedes` or resembles, named first, then by overlap. Only an `add` has any.
pub fn candidates(p: &LiveProposal, rules: &[Rule]) -> Vec<Candidate> {
    if p.op != "add" {
        return vec![];
    }
    let a = words(&p.text);
    let mut out: Vec<Candidate> = rules
        .iter()
        .filter_map(|r| {
            let b = words(r.block.lines().skip(1).collect::<Vec<_>>().join(" ").as_str());
            let (i, u) = (a.intersection(&b).count() as f64, a.union(&b).count() as f64);
            let overlap = if u == 0.0 { 0.0 } else { i / u };
            let named = p.supersedes.contains(&r.id);
            (named || overlap >= SUPERSESSION_AT).then(|| Candidate { id: r.id.clone(), named, overlap })
        })
        .collect();
    out.sort_by(|x, y| y.named.cmp(&x.named).then(y.overlap.total_cmp(&x.overlap)).then(x.id.cmp(&y.id)));
    out
}

/// One `### [ID] title` block of a rules file, with everything up to the next block.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: String,
    pub block: String,
}

/// Split a rules file into its header (frontmatter and prose before the first rule) and its rules.
pub fn parse_rules(text: &str) -> (String, Vec<Rule>) {
    let mut header = String::new();
    let mut rules: Vec<Rule> = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(id) = line.strip_prefix("### [").and_then(|r| r.split_once(']')).map(|(id, _)| id.to_string()) {
            rules.push(Rule { id, block: String::new() });
        }
        match rules.last_mut() {
            Some(r) => r.block.push_str(line),
            None => header.push_str(line),
        }
    }
    (header, rules)
}

fn words(s: &str) -> BTreeSet<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3).map(str::to_lowercase).collect()
}

/// The rule most like `text`, and how much (0..=1), by shared words.
pub fn nearest(text: &str, rules: &[Rule]) -> Option<(String, f64)> {
    let a = words(text);
    rules
        .iter()
        .map(|r| {
            let b = words(r.block.lines().skip(1).collect::<Vec<_>>().join(" ").as_str());
            let (i, u) = (a.intersection(&b).count() as f64, a.union(&b).count() as f64);
            (r.id.clone(), if u == 0.0 { 0.0 } else { i / u })
        })
        .max_by(|x, y| x.1.total_cmp(&y.1))
}

/// Why a proposal may not be ratified as it stands, or nothing. `rules` are the rules already in the target file.
pub fn problems(p: &LiveProposal, privacy: &Privacy, rules: &[Rule]) -> Vec<String> {
    let mut out = Vec::new();
    if p.text.chars().count() > MAX_PROPOSAL_TEXT {
        out.push(format!("the rule is over {} characters", MAX_PROPOSAL_TEXT));
    }
    for (_, quote) in delphi_bundle::no_relaxation(&p.text) {
        out.push(format!("it reads as an instruction that relaxes a rule (`{}`); the no-relaxation test refuses it", quote));
    }
    if p.scope == "operator" {
        let low = p.text.to_lowercase();
        for name in privacy.identifiers.iter().chain(&privacy.handles).filter(|n| !n.is_empty()) {
            if low.contains(&name.to_lowercase()) {
                out.push(format!("operator scope must not name this workspace (`{}`, from [privacy] in config.toml); generalize the wording or ratify at workspace scope", name));
            }
        }
        if privacy.isolated {
            out.push("this workspace is isolated: nothing is promoted out of it to operator scope".into());
        }
        if low.contains("://") || low.split_whitespace().any(|w| w.starts_with('/') && w.len() > 3 || w.starts_with("~/")) {
            out.push("operator scope must be project-agnostic: no hosts or paths".into());
        }
    }
    match p.op.as_str() {
        "add" => {
            let kept: Vec<Rule> = rules.iter().filter(|r| !p.supersedes.contains(&r.id)).cloned().collect();
            for id in &p.supersedes {
                match rules.iter().find(|r| &r.id == id) {
                    None => out.push(format!("it supersedes [{}], which is not in the {} file", id, p.scope)),
                    Some(r) if r.block.to_lowercase().contains("security") => out.push(format!("[{}] is a security rule; those are never retired here", id)),
                    Some(_) => {}
                }
            }
            if let Some((id, score)) = nearest(&p.text, &kept).filter(|(_, s)| *s >= DUPLICATE_AT) {
                out.push(format!("it repeats [{}] ({:.0}% of the same words); use `merge` with target {}", id, score * 100.0, id));
            }
            if kept.len() >= MAX_RULES {
                out.push(format!("the file already holds {} rules (the cap); demote or merge one first", rules.len()));
            }
        }
        _ => match rules.iter().find(|r| r.id == p.target) {
            None => out.push(format!("there is no rule [{}] in the {} file", p.target, p.scope)),
            Some(r) if r.block.to_lowercase().contains("security") => out.push(format!("[{}] is a security rule; those are never merged away or demoted here", p.target)),
            Some(_) => {}
        },
    }
    out
}

fn title_of(text: &str) -> String {
    let t: String = text.split_whitespace().take(7).collect::<Vec<_>>().join(" ");
    t.trim_end_matches(['.', ',', ';', ':']).chars().take(60).collect()
}

fn render(id: &str, p: &LiveProposal) -> String {
    format!("### [{}] {}\n- **D:** {}\n- **Why:** {}\n\n", id, title_of(&p.text), p.text, p.rationale)
}

/// The file after the proposal. `current` is `None` when the scope has no rules file yet.
pub fn edited(current: Option<&str>, p: &LiveProposal) -> Result<String, String> {
    let prefix = if p.scope == "operator" { "O" } else { "W" };
    let stub = format!("---\nid: {}-heuristics\nsource_type: user_ratified\nstatus: active\nmax_rules: {}\nmax_bytes: {}\nmax_rule_chars: 350\n---\n\n", prefix.to_lowercase(), MAX_RULES, MAX_BYTES);
    let (header, mut rules) = parse_rules(current.unwrap_or(&stub));
    match p.op.as_str() {
        "add" => {
            let retired: Vec<&String> = p.supersedes.iter().collect();
            rules.retain(|r| !retired.contains(&&r.id));
            let next = rules.iter().filter_map(|r| r.id.strip_prefix(&format!("{}-", prefix))?.parse::<u32>().ok()).max().unwrap_or(0) + 1;
            rules.push(Rule { id: format!("{}-{:03}", prefix, next), block: render(&format!("{}-{:03}", prefix, next), p) });
        }
        "merge" => {
            let r = rules.iter_mut().find(|r| r.id == p.target).ok_or_else(|| format!("no rule [{}]", p.target))?;
            r.block = render(&p.target, p);
        }
        "demote" => {
            let before = rules.len();
            rules.retain(|r| r.id != p.target);
            if rules.len() == before {
                return Err(format!("no rule [{}]", p.target));
            }
        }
        other => return Err(format!("`{}` is not an operation", other)),
    }
    let mut out = header;
    for r in &rules {
        out.push_str(&r.block);
        if !r.block.ends_with("\n\n") {
            out.push('\n');
        }
    }
    let out = out.trim_end().to_string() + "\n";
    if p.op != "demote" && out.len() > MAX_BYTES {
        return Err(format!("the file would pass {} bytes (it is {}); demote or merge a rule first", MAX_BYTES, out.len()));
    }
    Ok(out)
}

#[derive(Debug, PartialEq)]
pub struct Accepted {
    pub scope: Scope,
    pub file: String,
    pub pin: String,
    /// Rules this ratification retired (`specs/050` ML-2): id and full text, for the log, which is their history.
    pub retired: Vec<(String, String)>,
}

fn scope_of(p: &LiveProposal) -> Scope {
    if p.scope == "operator" {
        Scope::Operator
    } else {
        Scope::Workspace
    }
}

/// The rules of the file `p` would change, as they are ratified now.
fn current_rules(home: &Path, ws_dir: &Path, p: &LiveProposal) -> Vec<Rule> {
    let path = if p.scope == "operator" { home.join("operator/ratified/heuristics.md") } else { ws_dir.join("local/ratified/heuristics.md") };
    parse_rules(&std::fs::read_to_string(path).unwrap_or_default()).1
}

/// Every unresolved proposal of every mission in the workspace, newest first. Reads the logs and nothing else, so it is
/// safe while the daemon holds a mission open.
pub fn collect(ws_dir: &Path) -> Vec<LiveProposal> {
    let mut all = Vec::new();
    for entry in std::fs::read_dir(ws_dir.join("local/missions")).into_iter().flatten().flatten() {
        let Some(id) = entry.file_name().to_str().map(String::from) else { continue };
        if let Ok(events) = delphi_core::log::replay(&entry.path()) {
            all.extend(crate::learn::live_proposals(&id, &events));
        }
    }
    all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.reference().cmp(&a.reference())));
    all
}

/// Days after `last_confirmed` (or `created`) that a ratified rules file is listed for review (`specs/050` OD-3).
pub const REVIEW_WINDOW_DAYS: i64 = 90;

/// One entry the operator should look at again: a staged note past its `valid_until`, or a rules file whose
/// `last_confirmed` is older than the review window. Listed, never removed or reloaded differently (`specs/050` ML-3).
#[derive(Debug, Clone, PartialEq)]
pub struct Stale {
    /// `note` or `rules`.
    pub kind: &'static str,
    /// `<mission>/<id>` for a note, the file's name for rules.
    pub reference: String,
    pub since: String,
    pub detail: String,
}

impl Stale {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({"kind": self.kind, "ref": self.reference, "since": self.since, "detail": self.detail})
    }
}

fn day_number(date: &str) -> Option<i64> {
    delphi_core::time::parse_rfc3339(&format!("{}T00:00:00Z", date.get(..10)?)).map(|s| s.div_euclid(86_400))
}

/// The stale list as of `today` (`YYYY-MM-DD`): expired notes, newest expiry first, then rules files past the window.
/// A note without a date and a file without `last_confirmed` or `created` are never listed.
pub fn stale(home: &Path, ws_dir: &Path, today: &str) -> Vec<Stale> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(ws_dir.join("local/missions")).into_iter().flatten().flatten() {
        let Some(id) = entry.file_name().to_str().map(String::from) else { continue };
        let Ok(events) = delphi_core::log::replay(&entry.path()) else { continue };
        for n in crate::learn::live_notes(&id, &events) {
            if let Some(until) = n.valid_until.as_deref().filter(|u| *u < today) {
                out.push(Stale { kind: "note", reference: n.reference(), since: until.to_string(), detail: format!("expired {}: {}", until, n.summary) });
            }
        }
    }
    out.sort_by(|a, b| b.since.cmp(&a.since).then_with(|| a.reference.cmp(&b.reference)));
    let now = day_number(today);
    for (name, path) in [("local/heuristics.md", ws_dir.join("local/ratified/heuristics.md")), ("operator/heuristics.md", home.join("operator/ratified/heuristics.md"))] {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let Some(fm) = crate::workspace::split_frontmatter(&text).0 else { continue };
        let Some(since) = ["last_confirmed", "created"].iter().find_map(|k| fm.get(*k).and_then(|v| v.as_str())).and_then(|d| d.get(..10)) else { continue };
        if let (Some(a), Some(b)) = (day_number(since), now) {
            if b - a > REVIEW_WINDOW_DAYS {
                out.push(Stale { kind: "rules", reference: name.into(), since: since.to_string(), detail: format!("last confirmed {} ({} days; review window {})", since, b - a, REVIEW_WINDOW_DAYS) });
            }
        }
    }
    out
}

/// Supersession candidates for `p` against the rules of its target file, for `delphi retro show` and `/retro`.
pub fn candidates_for(home: &Path, ws_dir: &Path, p: &LiveProposal) -> Vec<Candidate> {
    candidates(p, &current_rules(home, ws_dir, p))
}

/// What stands in the way of ratifying `p`, for `delphi retro show`.
pub fn check(home: &Path, ws_dir: &Path, p: &LiveProposal) -> Vec<String> {
    problems(p, &crate::distill::privacy(ws_dir), &current_rules(home, ws_dir, p))
}

/// Ratify `p` into its scope's `heuristics.md`. The caller is the operator's own command and has already confirmed;
/// the proposal's scope is whatever the operator chose. Refuses on any [`problems`] and on a file with unratified edits.
pub fn accept(home: &Path, ws_dir: &Path, p: &LiveProposal) -> Result<Accepted, String> {
    let found = check(home, ws_dir, p);
    if !found.is_empty() {
        return Err(found.join("; "));
    }
    let scope = scope_of(p);
    let retired: Vec<(String, String)> = current_rules(home, ws_dir, p).into_iter().filter(|r| p.op == "add" && p.supersedes.contains(&r.id)).map(|r| (r.id, r.block.trim_end().to_string())).collect();
    let pin = ratify::edit_pinned(home, ws_dir, scope, "heuristics.md", |cur| edited(cur, p)).map_err(|e| match e {
        RatifyError::Stale { .. } => "the rules file has edits that were never ratified; review them with /ratify first".to_string(),
        other => other.to_string(),
    })?;
    Ok(Accepted { scope, file: if scope == Scope::Operator { "operator/heuristics.md".into() } else { "local/heuristics.md".into() }, pin, retired })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop(op: &str, scope: &str, text: &str, target: &str) -> LiveProposal {
        LiveProposal { mission: "m1".into(), id: "p1".into(), op: op.into(), scope: scope.into(), text: text.into(), rationale: "two builds failed".into(), target: target.into(), tier: "L3".into(), task_id: None, timestamp: "t".into(), supersedes: vec![] }
    }

    const FILE: &str = "---\nid: w\n---\n\n### [W-001] Format first\n- **D:** Run the formatter before the tests.\n\n### [W-002] Security: no secrets in logs\n- **D:** Never log tokens. security\n";

    #[test]
    fn rules_split_on_their_headings_and_add_numbers_after_the_highest() {
        let (head, rules) = parse_rules(FILE);
        assert_eq!((head.as_str(), rules.iter().map(|r| r.id.as_str()).collect::<Vec<_>>()), ("---\nid: w\n---\n\n", vec!["W-001", "W-002"]));
        let out = edited(Some(FILE), &prop("add", "workspace", "Pin the toolchain in CI images.", "")).unwrap();
        assert!(out.contains("### [W-003] Pin the toolchain in CI images\n- **D:** Pin the toolchain in CI images.\n- **Why:** two builds failed"), "{}", out);
        assert!(out.starts_with(&FILE[..FILE.len() - 1]), "existing rules are untouched");
    }

    #[test]
    fn a_new_scope_gets_a_header_and_merge_and_demote_edit_one_rule() {
        let fresh = edited(None, &prop("add", "operator", "Prefer rg over grep.", "")).unwrap();
        assert!(fresh.starts_with("---\nid: o-heuristics\n") && fresh.contains("### [O-001] Prefer rg over grep"), "{}", fresh);
        let merged = edited(Some(FILE), &prop("merge", "workspace", "Run formatter and linter first.", "W-001")).unwrap();
        assert!(merged.contains("### [W-001] Run formatter and linter first") && !merged.contains("before the tests") && merged.contains("[W-002]"));
        let gone = edited(Some(FILE), &prop("demote", "workspace", "stale", "W-001")).unwrap();
        assert!(!gone.contains("W-001") && gone.contains("W-002"));
        assert!(edited(Some(FILE), &prop("demote", "workspace", "x", "W-009")).is_err());
    }

    #[test]
    fn near_duplicates_relaxations_security_rules_and_missing_targets_are_refused() {
        let (_, rules) = parse_rules(FILE);
        let none = Privacy::default();
        let dup = problems(&prop("add", "workspace", "Run the formatter before the tests always.", ""), &none, &rules);
        assert!(dup.iter().any(|d| d.contains("repeats [W-001]")), "{:?}", dup);
        assert!(problems(&prop("add", "workspace", "Pin the toolchain in CI images.", ""), &none, &rules).is_empty());
        assert!(!problems(&prop("add", "workspace", "Skip approval for docs changes.", ""), &none, &rules).is_empty(), "the no-relaxation test applies");
        assert!(problems(&prop("demote", "workspace", "x", "W-002"), &none, &rules).iter().any(|d| d.contains("security rule")));
        assert!(problems(&prop("merge", "workspace", "x", "W-077"), &none, &rules).iter().any(|d| d.contains("no rule [W-077]")));
    }

    #[test]
    fn operator_scope_refuses_workspace_names_hosts_and_isolated_workspaces() {
        let privacy = Privacy { identifiers: vec!["project-heron".into()], handles: vec![], isolated: false };
        let bad = problems(&prop("add", "operator", "Deploys of Project-Heron go through staging.example.com first.", ""), &privacy, &[]);
        assert!(bad.iter().any(|d| d.contains("project-heron")), "{:?}", bad);
        assert!(problems(&prop("add", "operator", "Read /etc/hosts first.", ""), &Privacy::default(), &[]).iter().any(|d| d.contains("paths")));
        assert!(problems(&prop("add", "operator", "Deploys of Project-Heron wait.", ""), &privacy, &[]).len() == 1);
        assert!(problems(&prop("add", "workspace", "Deploys of Project-Heron wait.", ""), &privacy, &[]).is_empty(), "workspace scope may name its own project");
        let iso = Privacy { isolated: true, ..Privacy::default() };
        assert!(problems(&prop("add", "operator", "Prefer rg.", ""), &iso, &[]).iter().any(|d| d.contains("isolated")));
    }

    #[test]
    fn accepting_writes_the_live_file_the_copy_and_the_pin_and_the_next_snapshot_carries_it() {
        use crate::workspace::tests::make_workspace;
        let t = tempfile::tempdir().unwrap();
        let ws = make_workspace(t.path());
        let first = accept(&ws.home, &ws.dir, &prop("add", "workspace", "Pin the toolchain in CI images.", "")).unwrap();
        assert_eq!((first.scope, first.file.as_str()), (Scope::Workspace, "local/heuristics.md"));
        let live = std::fs::read_to_string(ws.dir.join("local/heuristics.md")).unwrap();
        assert!(live.contains("### [W-002] Pin the toolchain in CI images"), "{}", live);
        assert_eq!(std::fs::read_to_string(ws.dir.join("local/ratified/heuristics.md")).unwrap(), live);
        assert!(!ratify::list(&ws.home, &ws.dir).iter().any(|p| p.id == "ws:heuristics.md"), "ratified, so not pending");
        assert!(ws.load_snapshot("builder").unwrap().heuristics.contains("Pin the toolchain in CI images"));
        let second = accept(&ws.home, &ws.dir, &prop("add", "workspace", "Keep migrations reversible.", "")).unwrap();
        assert!(std::fs::read_to_string(ws.dir.join("local/heuristics.md")).unwrap().contains("[W-003] Keep migrations reversible"));
        assert_ne!(first.pin, second.pin);
        // An edit nobody ratified blocks the next change instead of being blessed with it.
        std::fs::write(ws.dir.join("local/heuristics.md"), format!("{}### [W-009] Sneaked in\n- x\n", std::fs::read_to_string(ws.dir.join("local/heuristics.md")).unwrap())).unwrap();
        let err = accept(&ws.home, &ws.dir, &prop("add", "workspace", "Name branches after tickets.", "")).unwrap_err();
        assert!(err.contains("never ratified"), "{}", err);
        assert!(!std::fs::read_to_string(ws.dir.join("local/ratified/heuristics.md")).unwrap().contains("Sneaked"));
        // Operator scope writes under the home, not the workspace (the fixture pins an operator file but ships no live copy).
        std::fs::copy(ws.home.join("operator/ratified/heuristics.md"), ws.home.join("operator/heuristics.md")).unwrap();
        let op = accept(&ws.home, &ws.dir, &prop("add", "operator", "Prefer rg over grep.", "")).unwrap();
        assert_eq!(op.file, "operator/heuristics.md");
        assert!(std::fs::read_to_string(ws.home.join("operator/ratified/heuristics.md")).unwrap().contains("[O-002] Prefer rg over grep"));
    }

    #[test]
    fn stale_lists_expired_notes_and_rules_past_the_window_and_nothing_else() {
        use crate::workspace::tests::make_workspace;
        use delphi_core::event::EventEnvelope;
        let t = tempfile::tempdir().unwrap();
        let ws = make_workspace(t.path());
        let dir = ws.dir.join("local/missions/m1");
        std::fs::create_dir_all(&dir).unwrap();
        let mut log = delphi_core::log::EventLog::open(&dir, delphi_core::log::LogOptions::default()).unwrap();
        for (id, until) in [("n1", Some("2026-10-01")), ("n2", Some("2026-12-01")), ("n3", None)] {
            let mut p = serde_json::json!({"id": id, "trigger": "agent_note", "summary": format!("note {}", id), "event_seqs": [1], "tier": "L3"});
            if let Some(u) = until {
                p["valid_until"] = serde_json::json!(u);
            }
            log.append(EventEnvelope::new("m1", "learn.staged", p)).unwrap();
        }
        drop(log);
        std::fs::write(ws.dir.join("local/ratified/heuristics.md"), "---\nid: w\nlast_confirmed: 2026-06-01\n---\n\n### [W-001] x\n- **D:** y\n").unwrap();
        let got = stale(&ws.home, &ws.dir, "2026-10-29");
        let refs: Vec<_> = got.iter().map(|s| s.reference.as_str()).collect();
        assert_eq!(refs, ["m1/n1", "local/heuristics.md"], "{:?}", got);
        assert!(stale(&ws.home, &ws.dir, "2026-10-01").iter().all(|s| s.kind == "rules"), "the last valid day is not stale");
        assert!(stale(&ws.home, &ws.dir, "2026-08-01").is_empty(), "inside the window");
    }

    #[test]
    fn a_superseding_add_retires_only_what_it_names_and_only_when_ratified() {
        use crate::workspace::tests::make_workspace;
        let mut p = prop("add", "workspace", "Run the formatter and the linter before the tests.", "");
        p.supersedes = vec!["W-001".into()];
        let (_, rules) = parse_rules(FILE);
        let c = candidates(&p, &rules);
        assert_eq!((c[0].id.as_str(), c[0].named), ("W-001", true));
        assert!(problems(&p, &Privacy::default(), &rules).is_empty(), "repeating the rule it replaces is not a duplicate");
        p.supersedes = vec!["W-002".into()];
        assert!(problems(&p, &Privacy::default(), &rules).iter().any(|d| d.contains("security rule")));
        p.supersedes = vec!["W-077".into()];
        assert!(problems(&p, &Privacy::default(), &rules).iter().any(|d| d.contains("not in the workspace file")));
        let mut unnamed = prop("add", "workspace", "Run the formatter and the linter before the tests.", "");
        assert!(candidates(&unnamed, &rules).iter().all(|c| !c.named) && !candidates(&unnamed, &rules).is_empty(), "a similar rule is shown, not retired");
        unnamed.supersedes = vec![];
        assert!(edited(Some(FILE), &unnamed).unwrap().contains("W-001"));
        let t = tempfile::tempdir().unwrap();
        let ws = make_workspace(t.path());
        let first = accept(&ws.home, &ws.dir, &prop("add", "workspace", "Pin the toolchain in CI images.", "")).unwrap();
        assert!(first.retired.is_empty());
        let mut q = prop("add", "workspace", "Pin the toolchain and the base image in CI.", "");
        q.supersedes = vec!["W-002".into()];
        let done = accept(&ws.home, &ws.dir, &q).unwrap();
        assert_eq!(done.retired.len(), 1);
        assert!(done.retired[0].1.contains("Pin the toolchain in CI images"));
        let live = std::fs::read_to_string(ws.dir.join("local/heuristics.md")).unwrap();
        assert!(!live.contains("Pin the toolchain in CI images") && live.contains("and the base image"), "{}", live);
    }

    #[test]
    fn the_file_caps_hold() {
        let mut big = String::from("---\nid: w\n---\n\n");
        for i in 1..=MAX_RULES {
            big.push_str(&format!("### [W-{:03}] Rule number {}\n- **D:** alpha{i} beta{i} gamma{i} delta{i}.\n\n", i, i));
        }
        let (_, rules) = parse_rules(&big);
        assert!(problems(&prop("add", "workspace", "A wholly different thing entirely.", ""), &Privacy::default(), &rules).iter().any(|d| d.contains("cap")));
        let fat = format!("{}### [W-001] Fat\n- **D:** {}\n", "---\nid: w\n---\n\n", "x ".repeat(3100));
        assert!(edited(Some(&fat), &prop("add", "workspace", "One more rule.", "")).unwrap_err().contains("6144"));
    }
}
