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
            if let Some((id, score)) = nearest(&p.text, rules).filter(|(_, s)| *s >= DUPLICATE_AT) {
                out.push(format!("it repeats [{}] ({:.0}% of the same words); use `merge` with target {}", id, score * 100.0, id));
            }
            if rules.len() >= MAX_RULES {
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
    let pin = ratify::edit_pinned(home, ws_dir, scope, "heuristics.md", |cur| edited(cur, p)).map_err(|e| match e {
        RatifyError::Stale { .. } => "the rules file has edits that were never ratified; review them with /ratify first".to_string(),
        other => other.to_string(),
    })?;
    Ok(Accepted { scope, file: if scope == Scope::Operator { "operator/heuristics.md".into() } else { "local/heuristics.md".into() }, pin })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop(op: &str, scope: &str, text: &str, target: &str) -> LiveProposal {
        LiveProposal { mission: "m1".into(), id: "p1".into(), op: op.into(), scope: scope.into(), text: text.into(), rationale: "two builds failed".into(), target: target.into(), tier: "L3".into(), task_id: None, timestamp: "t".into() }
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
