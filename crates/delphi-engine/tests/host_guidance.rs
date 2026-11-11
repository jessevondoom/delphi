//! Spec 098 slice S5: the host guidance (source ladder, reflex, repo rules) against `AGENTS.md` and the golden `CLAUDE.md` template.
//! Set `UPDATE_GOLDEN=1` to rewrite the golden files from the current text; review the diff before committing it.

use delphi_engine::host_guidance::{claude_md_template, ladder_section, pack_with_ladder, REPO_RULES};
use delphi_engine::mcp_hosts::{export_files, skill_md};
use std::path::PathBuf;

fn repo(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

fn agents_md() -> String {
    std::fs::read_to_string(repo("AGENTS.md")).expect("AGENTS.md at the repository root")
}

/// The commands AGENTS.md says to run before committing: the lines of the code block after "Run before committing:".
fn gate_commands(agents: &str) -> Vec<String> {
    let after = agents.split("Run before committing:").nth(1).expect("AGENTS.md says what to run before committing");
    let block = after.split("```").nth(1).expect("the commands are in a code block");
    block.lines().map(str::trim).filter(|l| !l.is_empty() && *l != "sh").map(String::from).collect()
}

#[test]
fn the_repo_rules_name_the_same_gate_commands_as_agents_md() {
    let cmds = gate_commands(&agents_md());
    assert_eq!(cmds.len(), 3, "AGENTS.md lists three gate commands: {cmds:?}");
    for c in &cmds {
        assert!(REPO_RULES.contains(c.as_str()), "the plugin text lacks the gate command AGENTS.md names: {c}");
    }
}

#[test]
fn the_repo_rules_name_the_same_trailer_as_agents_md() {
    let agents = agents_md();
    let start = agents.find("Implements-Spec:").expect("AGENTS.md names the trailer");
    let trailer: String = agents[start..].chars().take_while(|c| *c != '`').collect();
    assert_eq!(trailer, "Implements-Spec: specs/NNN-*.md");
    assert!(REPO_RULES.contains(&trailer), "the plugin text must give the trailer exactly as AGENTS.md does: {trailer}");
    // and it names no other trailer
    for line in REPO_RULES.lines().filter(|l| l.contains("Implements")) {
        assert!(line.contains("Implements-Spec:"), "{line}");
    }
}

#[test]
fn the_repo_rules_say_the_merge_rule_in_agents_mds_words() {
    let agents = agents_md();
    for rule in ["Agents never merge on their own", "Never run `gh pr merge` or `delphi merge --auto`", "never remove a gate label", "You may open PRs, draft specs and queue ready PRs", "Scratch runs need `--home <tmpdir>`", "One slice per pull request", "Never commit past a failing gate"] {
        assert!(agents.contains(rule), "AGENTS.md no longer says: {rule}");
        assert!(REPO_RULES.contains(rule), "the plugin text lacks: {rule}");
    }
}

#[test]
fn the_plugin_skill_and_every_generated_copy_carry_the_repo_rules() {
    assert!(skill_md("1.2.3").contains(REPO_RULES));
    let files = export_files("claude-code", "1.2.3").unwrap();
    let skill = files.iter().find(|(p, _)| p.ends_with("skills/delphi/SKILL.md")).expect("the delphi skill is exported");
    assert!(skill.1.contains(REPO_RULES) && skill.1.contains("Generated for Delphi release 1.2.3."));
    // the council skill is about Councils, not commits
    let council = files.iter().find(|(p, _)| p.ends_with("skills/delphi-council/SKILL.md")).unwrap();
    assert!(!council.1.contains("When this project has a repo"));
}

fn golden(name: &str, got: &str) {
    let path = repo("crates/delphi-engine/tests/fixtures").join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, got).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden file {name}; run with UPDATE_GOLDEN=1"));
    assert_eq!(got, want, "{name} differs; if the change is intended, rerun with UPDATE_GOLDEN=1 and review the diff");
}

#[test]
fn the_claude_md_template_matches_its_golden_files() {
    golden("claude_md_ladder_off.md", &claude_md_template(false));
    golden("claude_md_ladder_on.md", &claude_md_template(true));
}

#[test]
fn the_ladder_pack_text_matches_its_golden_files() {
    for role in ["scout", "investigator", "red-team"] {
        golden(&format!("ladder_{role}.md"), &ladder_section(role).unwrap());
    }
}

#[test]
fn the_scout_investigator_and_red_team_manifests_are_unchanged_and_still_say_untrusted_content_is_data() {
    // The ladder is served on top of the manifests with the flag on; the manifests themselves are not edited by this slice, because
    // a static file cannot be switched off by `[hosts] source_ladder` and the same files are read by seats inside Delphi.
    for role in ["scout", "investigator", "red-team"] {
        let text = std::fs::read_to_string(repo(&format!("agents/{role}/AGENT.md"))).unwrap();
        assert!(!text.contains("Source ladder"), "{role}: the ladder is served, not written into the manifest");
        assert_eq!(pack_with_ladder(role, text.clone(), false), text);
    }
}
