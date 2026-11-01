//! `delphi check <kind> <path>`: the deterministic helpers behind the built-in skills (`skills/CATALOG.md`, D-02).
//!
//! Each kind reads files and prints a verdict; none touches a workspace, the network or the daemon, so an agent
//! (or a human) can run one anywhere. A skill calls a check instead of re-deriving the rule in prose: the model
//! spends tokens on judgement and the binary spends none on arithmetic, graph walks or schema rules.
//!
//! The kinds that judge one document's text live in `delphi_engine::checks`, shared with the engine's `check` tool.
//! The kinds that need a directory (`kb`, `tui`, `skill`) live here.

use crate::frontmatter::{self, get_str};
use crate::lint;
use crate::report::{Level, Report};
use delphi_engine::checks;
use regex::Regex;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use unicode_width::UnicodeWidthStr;

/// Every kind `delphi check` accepts. `lint` verifies that a `SKILL.md` names only these.
pub const KINDS: [&str; 11] = ["dag", "handoff", "provenance", "rules", "review-md", "spec", "kb", "tui", "skill", "fact", "record"];

/// Run one check. `target` is a file (a directory for `kb`, `skill` and `tui`); `repo` is the root against which
/// `dag` resolves its `spec:` links.
pub fn run(kind: &str, target: &Path, repo: Option<&Path>) -> Result<Report, String> {
    let mut r = Report::default();
    match kind {
        "kb" => kb(target, &mut r)?,
        "tui" => tui(target, &mut r)?,
        "skill" => skill(target, &mut r)?,
        k if checks::TEXT_KINDS.contains(&k) => {
            let text = fs::read_to_string(target).map_err(|e| format!("cannot read {}: {}", target.display(), e))?;
            // Without --repo there is nothing to resolve spec links against, so they are not judged.
            let o = checks::check_text(k, &text, &|p| repo.is_none_or(|root| root.join(p).is_file()))?;
            o.fails.iter().for_each(|m| r.fail(k, "", m));
            o.warns.iter().for_each(|m| r.warn(k, "", m));
            o.notes.iter().for_each(|m| r.push(Level::Ok, k, "", m));
        }
        other => return Err(format!("unknown check `{}` (one of: {})", other, KINDS.join(", "))),
    }
    if r.findings.iter().all(|f| f.level != Level::Fail) {
        r.push(Level::Ok, kind, target.display().to_string(), "passes");
    }
    Ok(r)
}

// ── fact: a claim over stored snapshots (specs/040 §7.2) ───────────────────

/// `delphi check fact <claim.json> --home <dir> [--offline]`: the deterministic part of the fact path. The input names the claim and the
/// ids of snapshots the daemon already stored; there is no model here, and nothing is written. The only network call is the DOI lookup at
/// doi.org for a stored journal-publisher page that carries a DOI (`specs/040` §30), the same one the daemon makes; `offline` skips it
/// and judges such a page on its text alone. The entailment judge and the contradiction search are the daemon's (`fact_run`), so a pass
/// here is "may go on".
pub fn fact(input: &Path, home: &Path, offline: bool) -> Result<Report, String> {
    let resolver = delphi_engine::fact_run::HandleResolver::default();
    fact_with(input, home, if offline { None } else { Some(&resolver) })
}

fn fact_with(input: &Path, home: &Path, doi: Option<&dyn delphi_engine::fact_run::DoiResolver>) -> Result<Report, String> {
    use delphi_engine::fact_check::Submission;
    use delphi_engine::fact_fetch::SnapshotStore;
    use delphi_engine::fact_sources::SourceList;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        claim: String,
        needed_for: String,
        #[serde(default)]
        kind: Option<delphi_engine::fact_check::ClaimKind>,
        snapshots: Vec<String>,
    }
    let text = fs::read_to_string(input).map_err(|e| format!("cannot read {}: {}", input.display(), e))?;
    let i: Input = serde_json::from_str(&text).map_err(|e| format!("{}: {}", input.display(), e))?;
    let sources = match fs::read_to_string(home.join("config.toml")) {
        Ok(t) => delphi_adapter::config::Config::parse(&t).map(|c| c.validator.sources).unwrap_or_default(),
        Err(_) => Default::default(),
    };
    let store = SnapshotStore::in_home(home);
    let mut snaps = Vec::new();
    for id in &i.snapshots {
        snaps.push(store.get(id).ok_or_else(|| format!("snapshot {} is not stored here, or its bytes do not match its hash", id.chars().take(16).collect::<String>()))?);
    }
    let sub = Submission::over_snapshots(i.claim, i.needed_for, i.kind.unwrap_or_default(), &snaps);
    let o = delphi_engine::fact_run::check_stored(&sub, &snaps, &SourceList::new(sources), doi).to_outcome();
    let mut r = Report::default();
    o.fails.iter().for_each(|m| r.fail("fact", "", m));
    o.warns.iter().for_each(|m| r.warn("fact", "", m));
    o.notes.iter().for_each(|m| r.push(Level::Ok, "fact", "", m));
    if r.findings.iter().all(|f| f.level != Level::Fail) {
        r.push(Level::Ok, "fact", input.display().to_string(), "passes");
    }
    Ok(r)
}

// ── record: an event exists in a mission's log, as the daemon wrote it (specs/040 §21.4, §21.6) ──────────────────

/// Event kinds that carry an agent's own words: a record of one proves that the agent wrote it, not that it is true.
const AGENT_WORDS: [&str; 4] = ["task.output", "task.result", "learn.note", "learn.proposed"];

/// `delphi check record <mission-dir> --event <seq> [--hash <h>]`: the event is in the log, the log's hash chain
/// verifies (against the head the daemon keeps in `state.db`, so a cut tail shows), and, if given, the event's hash
/// matches. Pure code: no model, no network, nothing written.
pub fn record(mission_dir: &Path, seq: u64, hash: Option<&str>) -> Result<Report, String> {
    let mut r = Report::default();
    let id = mission_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let events = delphi_core::log::replay(mission_dir).map_err(|e| format!("cannot read the log in {}: {}", mission_dir.display(), e))?;
    let at = format!("{}#{}", id, seq);
    let Some(e) = events.iter().find(|e| e.seq == seq) else {
        r.fail("record", &at, "no such event in this mission's log");
        return Ok(r);
    };
    let stored = mission_dir.parent().and_then(Path::parent).and_then(|local| delphi_core::ledger::Ledger::stored_chain_head(&local.join("state.db"), &id));
    let chain = delphi_core::log::verify_chain(mission_dir).map_err(|e| format!("cannot read the log in {}: {}", mission_dir.display(), e))?.against_stored_head(stored);
    if let Some(b) = &chain.broken {
        r.fail("record", &at, format!("the log's chain is broken: {b}"));
        return Ok(r);
    }
    if let Some(want) = hash {
        match e.h.as_deref() {
            Some(h) if h == want => {}
            Some(_) => r.fail("record", &at, "the event's hash is not the one given"),
            None => r.fail("record", &at, "the event has no hash to compare (it is from before the chain)"),
        }
    }
    let chained = e.h.is_some() && chain.broken.is_none();
    let how = if chained { "recorded by the daemon, chain verified" } else { "recorded by the daemon (not chained)" };
    if AGENT_WORDS.contains(&e.kind.as_str()) {
        r.warn("record", &at, format!("{} `{}` event: an agent wrote this; the record shows that it was said, not that it is true", how, e.kind));
    } else {
        r.push(Level::Ok, "record", &at, format!("{how}: `{}`", e.kind));
    }
    if r.findings.iter().all(|f| f.level != Level::Fail) {
        r.push(Level::Ok, "record", &at, "passes");
    }
    Ok(r)
}

// ── kb: stale claims and broken links ───────────────────────────────────────

fn kb(root: &Path, r: &mut Report) -> Result<(), String> {
    let tree = lint::Tree::from_repo(root).map_err(|e| format!("cannot read {}: {}", root.display(), e))?;
    r.findings.extend(lint::run(&tree).findings.into_iter().filter(|f| ["links", "frontmatter", "rule-caps"].contains(&f.check.as_str())));
    let superseded: BTreeSet<&String> = tree
        .files
        .iter()
        .filter(|(k, p)| k.ends_with(".md") && fs::read_to_string(p).ok().and_then(|t| lint::frontmatter_of(&t)).map(|fm| get_str(&fm, "status") == Some("superseded")).unwrap_or(false))
        .map(|(k, _)| k)
        .collect();
    let link = Regex::new(r"\]\(([^)\s#]+)").unwrap();
    for (path, phys) in tree.files.iter().filter(|(k, _)| k.ends_with(".md") && !superseded.contains(k)) {
        let dir = path.rsplit_once('/').map(|d| d.0).unwrap_or("");
        let text = fs::read_to_string(phys).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            for c in link.captures_iter(line) {
                let joined = if dir.is_empty() { c[1].to_string() } else { format!("{}/{}", dir, &c[1]) };
                if let Some(n) = lint::normalize(&joined) {
                    if superseded.contains(&n) {
                        r.warn("kb", format!("{}:{}", path, i + 1), format!("links to `{}`, which is superseded", n));
                    }
                }
            }
        }
    }
    Ok(())
}

// ── tui: character-cell invariants (skills/tui-design) ──────────────────────

fn is_emoji(c: char) -> bool {
    // ✓ ✔ ✗ ✖ and the ❯ prompt are the sanctioned glyphs; other pictographs vary in width by terminal.
    !matches!(c, '\u{2713}' | '\u{2714}' | '\u{2717}' | '\u{2716}' | '\u{276F}') && matches!(c, '\u{1F300}'..='\u{1FAFF}' | '\u{2600}'..='\u{27BF}' | '\u{2B50}' | '\u{2B55}' | '\u{FE0F}')
}

fn tui(target: &Path, r: &mut Report) -> Result<(), String> {
    let files: Vec<_> = if target.is_dir() {
        walkdir::WalkDir::new(target).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "rs")).map(|e| e.into_path()).collect()
    } else {
        vec![target.to_path_buf()]
    };
    let badge = Regex::new("⟦[^⟧]*⟧").unwrap();
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        for (i, line) in text.lines().enumerate() {
            let at = format!("{}:{}", path.display(), i + 1);
            if let Some(c) = line.chars().find(|c| is_emoji(*c)) {
                r.fail("tui", &at, format!("emoji `{}` (INV zero-emoji: width varies by terminal; use ○ ● ► ❯ ✔ ✖)", c));
            }
            if let Some(c) = line.chars().find(|c| matches!(c, '\u{258C}' | '\u{2590}')) {
                r.fail("tui", &at, format!("vertical half-block `{}` (INV-AVATAR-01)", c));
            }
            let t = line.trim();
            if (t.contains('┌') && t.contains('┐')) || (t.contains('└') && t.contains('┘')) {
                r.fail("tui", &at, "box border enclosing a region (borderless layout; use whitespace and `─` hairlines)");
            }
            // Only literal badges: a format template (`⟦{}⟧`) has no width until it is filled.
            for b in badge.find_iter(line).filter(|b| !b.as_str().contains(['{', '}', '\'', '"', '\\'])) {
                let w = UnicodeWidthStr::width(b.as_str());
                if w != 5 {
                    r.fail("tui", &at, format!("badge `{}` is {} cells wide (INV-AVATAR-05: exactly 5)", b.as_str(), w));
                }
            }
        }
    }
    Ok(())
}

// ── skill: a package an agent or operator wrote ─────────────────────────────

fn skill(dir: &Path, r: &mut Report) -> Result<(), String> {
    let text = fs::read_to_string(dir.join("SKILL.md")).map_err(|e| format!("cannot read {}/SKILL.md: {}", dir.display(), e))?;
    let doc = frontmatter::parse(&text).map_err(|e| format!("SKILL.md: {}", e))?;
    let Some(fm) = doc.frontmatter else {
        r.fail("skill", "SKILL.md", "missing frontmatter");
        return Ok(());
    };
    let canon = dir.canonicalize().ok();
    let name = canon.as_ref().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
    let custom = canon.as_ref().map(|p| p.to_string_lossy().contains("/custom/") || p.to_string_lossy().contains("/operator/skills/")).unwrap_or(false);
    lint::skill_errors(&fm, &name, custom).into_iter().for_each(|e| r.fail("skill", "", e));
    lint::skill_package_errors(&doc.body, &fm, &|rel| dir.join(rel).is_file()).into_iter().for_each(|e| r.fail("skill", "", e));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_fact_refuses_unstored_snapshots_and_submitted_quotes() {
        let home = tempfile::tempdir().unwrap();
        let (_d, p) = tmp("c.json", &format!(r#"{{"claim":"X is Y.","needed_for":"q","snapshots":["{}"]}}"#, "0".repeat(64)));
        assert!(fact(&p, home.path(), true).err().unwrap().contains("not stored"));
        let (_d2, p2) = tmp("c.json", r#"{"claim":"X is Y.","needed_for":"q","snapshots":[],"quote":"X is Y."}"#);
        assert!(fact(&p2, home.path(), true).is_err(), "a submitted quote is refused by the parser");
    }

    fn tmp(name: &str, body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(name);
        fs::write(&p, body).unwrap();
        (d, p)
    }

    fn fails(kind: &str, body: &str) -> Vec<String> {
        let (_d, p) = tmp("f", body);
        run(kind, &p, None).unwrap().findings.into_iter().filter(|f| f.level == Level::Fail).map(|f| f.message).collect()
    }

    #[test]
    fn record_finds_a_daemon_event_verifies_the_chain_and_the_hash_and_says_when_an_agent_wrote_it() {
        let d = tempfile::tempdir().unwrap();
        let mdir = d.path().join("local/missions/m1");
        let mut log = delphi_core::log::EventLog::open(&mdir, Default::default()).unwrap();
        log.append(delphi_core::event::EventEnvelope::new("m1", "validator.verdict", serde_json::json!({"verdict": "would_ratify"}))).unwrap();
        log.append(delphi_core::event::EventEnvelope::new("m1", "task.output", serde_json::json!({"text": "I say so"}))).unwrap();
        drop(log);
        let ev = delphi_core::log::replay(&mdir).unwrap();
        let h = ev[0].h.clone().unwrap();
        let msgs = |r: Report| r.findings.into_iter().map(|f| (f.level, f.message)).collect::<Vec<_>>();
        let ok = msgs(record(&mdir, ev[0].seq, Some(&h)).unwrap());
        assert!(ok.iter().any(|(l, m)| *l == Level::Ok && m.contains("chain verified")), "{ok:?}");
        assert!(msgs(record(&mdir, ev[0].seq, Some("0000")).unwrap()).iter().any(|(l, m)| *l == Level::Fail && m.contains("hash")));
        assert!(msgs(record(&mdir, 99, None).unwrap()).iter().any(|(l, m)| *l == Level::Fail && m.contains("no such event")));
        let agent = msgs(record(&mdir, ev[1].seq, None).unwrap());
        assert!(agent.iter().any(|(l, m)| *l == Level::Warn && m.contains("an agent wrote this")), "{agent:?}");
        // Editing an event breaks the chain, and the check fails.
        let seg = mdir.join("events/000001.jsonl");
        let text = std::fs::read_to_string(&seg).unwrap().replace("would_ratify", "ratified_ok");
        std::fs::write(&seg, text).unwrap();
        assert!(msgs(record(&mdir, ev[0].seq, None).unwrap()).iter().any(|(l, m)| *l == Level::Fail && m.contains("chain is broken")));
    }

    #[test]
    fn text_kinds_reach_the_engine_and_dag_links_resolve_against_the_repo() {
        assert!(fails("provenance", "no frontmatter\n")[0].contains("missing"));
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("specs")).unwrap();
        fs::write(d.path().join("specs/001-x.md"), "x").unwrap();
        let (_t, p) = tmp("plan.yaml", "milestones:\n  - {id: m1, spec: 'specs/001-x.md#L3'}\n  - {id: m2, spec: specs/999-y.md}\n");
        let f: Vec<_> = run("dag", &p, Some(d.path())).unwrap().findings.into_iter().filter(|f| f.level == Level::Fail).collect();
        assert_eq!(f.len(), 1, "{:?}", f);
        assert!(f[0].message.contains("999-y"));
        // No --repo: links are not judged.
        assert!(run("dag", &p, None).unwrap().findings.iter().all(|f| f.level != Level::Fail));
    }

    #[test]
    fn tui_invariants() {
        assert!(fails("tui", "let s = \"⟦•─•⟧ ○ ● ► ❯ ✔ ✖\";\n").is_empty());
        assert!(fails("tui", "let s = \"🤖 ready\";\n")[0].contains("emoji"));
        assert!(fails("tui", "let s = \"▌body\";\n")[0].contains("INV-AVATAR-01"));
        assert!(fails("tui", "let s = \"⟦••⟧\";\n")[0].contains("exactly 5"));
        assert!(fails("tui", "let top = \"┌────┐\";\n")[0].contains("border"));
        assert!(fails("tui", "let s = \"⭐ ok\";\n")[0].contains("emoji"));
    }

    #[test]
    fn kb_flags_broken_links_and_superseded_targets() {
        let d = tempfile::tempdir().unwrap();
        let fm = |status: &str| format!("---\nid: x\ntitle: X\nauthor: architect\ntimestamp: \"2026-10-25T00:00:00Z\"\nsource_type: internal_generated\nconfidence: medium\nstatus: {}\nsuperseded_by: new\nsource_mission: m\ntags: [t]\n---\n", status);
        fs::create_dir(d.path().join("context")).unwrap();
        fs::write(d.path().join("context/old.md"), fm("superseded")).unwrap();
        fs::write(d.path().join("context/new.md"), format!("{}see [old](old.md) and [gone](gone.md)\n", fm("active"))).unwrap();
        let r = run("kb", d.path(), None).unwrap();
        assert!(r.findings.iter().any(|f| f.level == Level::Fail && f.message.contains("broken link")));
        assert!(r.findings.iter().any(|f| f.level == Level::Warn && f.message.contains("superseded")));
    }

    /// The shipped templates are the starting point agents copy, so each must pass the check it is paired with.
    #[test]
    fn shipped_templates_pass_their_checks() {
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (kind, file) in [
            ("dag", "task-decomposition/templates/task-dag.yaml"),
            ("dag", "roadmap-sequencing/templates/milestones.yaml"),
            ("handoff", "handoff-packaging/templates/handoff-envelope.yaml"),
            ("spec", "spec-authoring/templates/spec-skeleton.md"),
        ] {
            let r = run(kind, &skills.join(file), Some(&repo)).unwrap();
            let bad: Vec<_> = r.findings.iter().filter(|f| f.level == Level::Fail).map(|f| f.message.clone()).collect();
            assert!(bad.is_empty(), "{}: {:?}", file, bad);
        }
        let rule = fs::read_to_string(skills.join("lesson-promotion/templates/lesson-rule.md")).unwrap();
        let file = format!("---\nactive_rules: 1\n---\n{}", rule.split("<!--").next().unwrap());
        assert!(lint::cap_errors(&file).is_empty(), "{:?}", lint::cap_errors(&file));
    }

    #[test]
    fn every_built_in_skill_package_is_valid() {
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let mut n = 0;
        for e in fs::read_dir(&skills).unwrap().flatten().filter(|e| e.path().join("SKILL.md").is_file()) {
            let r = run("skill", &e.path(), None).unwrap();
            let bad: Vec<_> = r.findings.iter().filter(|f| f.level == Level::Fail).map(|f| f.message.clone()).collect();
            assert!(bad.is_empty(), "{}: {:?}", e.path().display(), bad);
            n += 1;
        }
        assert!(n >= 43, "expected the full catalog, found {} packages", n);
    }

    #[test]
    fn unknown_kind_is_an_error() {
        assert!(run("nope", Path::new("."), None).is_err());
    }
}
