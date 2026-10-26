//! Slice B7 (`specs/025` AC-2..5, 7..9): the plugin component table, the `.mcp.json` reader, import, the exports and the
//! marketplace file. Plugins are built in a temp dir so that a token-shaped value never sits in the repository.

use super::*;
use crate::import::{import, ImportOpts, Outcome};
use std::fs;
use std::path::PathBuf;

struct Fake;

impl Host for Fake {
    fn provenance(&self, text: &str) -> Vec<String> {
        let Some(fm) = ids::frontmatter_of(text) else { return vec!["no frontmatter".into()] };
        ["id", "title", "author", "timestamp", "source_type", "confidence", "status", "source_mission", "tags"].iter().filter(|k| !fm.contains_key(serde_yaml::Value::String((**k).into()))).map(|k| format!("missing required field `{}`", k)).collect()
    }

    fn skill_errors(&self, fm: &serde_yaml::Mapping, dir: &str) -> Vec<String> {
        if ids::get_str(fm, "name") == Some(dir) { Vec::new() } else { vec![format!("name must equal its directory `{}`", dir)] }
    }

    fn role_scope(&self, role: &str) -> Option<RoleScope> {
        let (write, tools): (Vec<String>, Vec<String>) = match role {
            "red-team" => (vec![], vec!["Read".into(), "Grep".into(), "Glob".into()]),
            _ => (vec!["missions/**".into()], vec!["Read".into(), "Grep".into(), "Glob".into(), "Write".into()]),
        };
        Some(RoleScope { read: vec!["*".into()], write, execute: vec![], tools })
    }
}

fn golden() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dev/bundles/acme-platform")
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() { copy(&e.path(), &dest) } else { fs::copy(e.path(), dest).unwrap(); }
    }
}

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

const SKILL: &str = "---\nname: triage\ndescription: Sort incoming reports by severity.\nallowed-tools: Bash(rm:*)\nmetadata:\n  delphi.authorized_agents: '[\"builder\"]'\n---\nUse templates/ or assets/form.md and read references/policy.md.\n";
const AGENT: &str = "---\nname: reviewer\ndescription: Reviews a diff.\ntools: Read, Grep, Bash, WebFetch\nmodel: opus\n---\nReview the diff and list the risks.\n";

/// A plain Claude Code plugin with every component the table in `specs/025` §2.3 names, except `bin/`.
fn plugin() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    put(r, ".claude-plugin/plugin.json", r#"{"name":"Review Kit","version":"2.1.0","description":"Review helpers.","author":{"name":"Zed Corp","email":"dev@zed.example"},"license":"MIT","keywords":["review"],"hooks":"./hooks/hooks.json","userConfig":{"region":{"type":"string","description":"where"}}}"#);
    put(r, "skills/triage/SKILL.md", SKILL);
    put(r, "skills/triage/assets/form.md", "# form\n");
    put(r, "skills/triage/references/policy.md", "# policy\n");
    put(r, "skills/triage/scripts/run.sh", "echo hi\n");
    put(r, "agents/reviewer.md", AGENT);
    put(r, "commands/ship.md", "run it\n");
    put(r, "hooks/hooks.json", "{\"hooks\":{}}\n");
    put(r, ".lsp.json", "{}\n");
    put(r, "output-styles/terse.md", "terse\n");
    put(r, "workflows/w.md", "w\n");
    put(r, "monitors/m.md", "m\n");
    put(r, "settings.json", "{}\n");
    put(r, "CLAUDE.md", "be nice\n");
    put(r, "README.md", "# Review Kit\n");
    put(r, ".mcp.json", r#"{"mcpServers":{"tickets":{"type":"http","url":"https://mcp.zed.example/mcp","headers":{"X-Team":"${TEAM}"}}}}"#);
    t
}

fn out() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let o = t.path().join("bundle");
    (t, o)
}

fn rows(i: &import::Imported) -> String {
    i.render_table()
}

#[test]
fn ac_2_a_plugin_with_every_ignored_component_imports_with_one_note_each_and_copies_none_of_them() {
    let p = plugin();
    let (_keep, o) = out();
    let i = import(p.path(), &o, &ImportOpts { skill_roles: vec!["scout".into()], ..ImportOpts::default() }, &Fake).unwrap();
    assert!(i.report.passed(), "{}", i.report.render("x"));
    let table = rows(&i);
    for c in ["commands/", "hooks/", ".lsp.json", "output-styles/", "workflows/", "monitors/", "settings.json", "CLAUDE.md"] {
        let hits: Vec<_> = i.rows.iter().filter(|r| r.component == c && r.outcome == Outcome::Ignored).collect();
        assert_eq!(hits.len(), 1, "one ignored row for {}:\n{}", c, table);
        assert!(!hits[0].why.is_empty());
        assert!(!o.join(c.trim_end_matches('/')).exists(), "{} was copied", c);
    }
    assert!(i.rows.iter().any(|r| r.component == "plugin.json hooks" && r.outcome == Outcome::Ignored), "{}", table);
    assert!(i.rows.iter().any(|r| r.component == "plugin.json userConfig"), "a non-sensitive userConfig passes with a note\n{}", table);
    assert!(i.rows.iter().any(|r| r.component.starts_with("mcp:tickets")), "{}", table);
    assert_eq!((i.org.as_str(), i.name.as_str()), ("zed-corp", "review-kit"));
}

#[test]
fn the_committed_review_kit_fixture_imports_with_a_host_role_and_validates() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dev/bundles/plugin-fixtures/review-kit");
    let (_k, o) = out();
    let opts = ImportOpts { skill_roles: vec!["scout".into()], agent_roles: [("reviewer".to_string(), "red-team".to_string())].into(), ..ImportOpts::default() };
    let i = import(&fixture, &o, &opts, &Fake).unwrap();
    assert!(i.report.passed(), "{}", i.report.render("x"));
    assert_eq!((i.org.as_str(), i.name.as_str()), ("zed-corp", "review-kit"));
    println!("{}", rows(&i));
    assert!(!o.join("commands").exists() && !o.join("hooks").exists());
    assert!(o.join("skills/triage/assets/form.md").is_file() && o.join("skills/triage/references/policy.md").is_file());
}

#[test]
fn ac_2_a_bin_directory_and_a_sensitive_option_refuse_the_import_before_anything_is_written() {
    let p = plugin();
    put(p.path(), "bin/tool", "#!/bin/sh\n");
    let (_k, o) = out();
    let e = import(p.path(), &o, &ImportOpts::default(), &Fake).unwrap_err();
    assert!(e[0].contains("bin/"), "{:?}", e);
    assert!(!o.exists());

    let p = plugin();
    put(p.path(), ".claude-plugin/plugin.json", r#"{"name":"x","userConfig":{"api_key":{"type":"string","sensitive":true}}}"#);
    let (_k, o) = out();
    let e = import(p.path(), &o, &ImportOpts::default(), &Fake).unwrap_err();
    assert!(e[0].contains("userConfig.api_key") && e[0].contains("sensitive"), "{:?}", e);
    assert!(!o.exists());

    // the same option inside a bundle's own plugin.json fails validation at step 13, with or without --claude
    let t = tempfile::tempdir().unwrap();
    copy(&golden(), t.path());
    put(t.path(), ".claude-plugin/plugin.json", r#"{"name":"acme-platform","channels":[{"userConfig":{"t":{"sensitive":true}}}]}"#);
    let r = validate(t.path(), &Options::default(), &Fake);
    assert!(r.fails.iter().any(|f| f.step == 13 && f.msg.contains("sensitive")), "{}", r.render("x"));
}

#[test]
fn ac_3_an_agent_imports_without_a_role_table_until_the_operator_names_a_host_role() {
    let p = plugin();
    let (_k, o) = out();
    let i = import(p.path(), &o, &ImportOpts::default(), &Fake).unwrap();
    let toml = fs::read_to_string(o.join("bundle.toml")).unwrap();
    assert!(!toml.contains("[agents."), "no role, no table (a table without a role fails step 4):\n{}", toml);
    assert!(i.report.notes.iter().any(|n| n.step == 9 && n.at == "agents/reviewer.md" && n.msg.contains("Claude Code only")), "{}", i.report.render("x"));

    let (_k, o) = out();
    let opts = ImportOpts { agent_roles: [("reviewer".to_string(), "red-team".to_string())].into(), ..ImportOpts::default() };
    let i = import(p.path(), &o, &opts, &Fake).unwrap();
    assert!(i.report.passed(), "{}", i.report.render("x"));
    assert_eq!(i.report.manifest.as_ref().unwrap().agents.get("reviewer").map(String::as_str), Some("red-team"));
    assert!(i.report.notes.iter().any(|n| n.at == "agents/reviewer.md" && n.msg.contains("Bash") && n.msg.contains("WebFetch")), "tools the host lacks are dropped: {}", i.report.render("x"));
    assert!(i.report.manifest.unwrap().touches_roles.contains(&"red-team".to_string()));

    let (_k, o) = out();
    let bad = ImportOpts { agent_roles: [("ghost".to_string(), "red-team".to_string())].into(), ..ImportOpts::default() };
    assert!(import(p.path(), &o, &bad, &Fake).unwrap_err()[0].contains("no agent `ghost`"));
    let (_k, o) = out();
    let bad = ImportOpts { agent_roles: [("reviewer".to_string(), "wizard".to_string())].into(), ..ImportOpts::default() };
    assert!(import(p.path(), &o, &bad, &Fake).unwrap_err()[0].contains("not a canonical role"));
}

#[test]
fn a_skill_never_names_its_own_roles_and_allowed_tools_is_ignored_with_a_note() {
    let p = plugin();
    let (_k, o) = out();
    let i = import(p.path(), &o, &ImportOpts::default(), &Fake).unwrap();
    let skill = fs::read_to_string(o.join("skills/triage/SKILL.md")).unwrap();
    assert!(!skill.contains("authorized_agents"), "the foreign list is dropped:\n{}", skill);
    assert!(i.report.notes.iter().any(|n| n.at == "skills/triage/SKILL.md" && n.msg.contains("allowed-tools")));
    assert!(i.report.notes.iter().any(|n| n.msg.contains("no authorized_agents")), "{}", i.report.render("x"));
    assert!(i.rows.iter().any(|r| r.why.contains("named roles of their own")), "{}", rows(&i));

    let (_k, o) = out();
    import(p.path(), &o, &ImportOpts { skill_roles: vec!["scout".into(), "writer".into()], ..ImportOpts::default() }, &Fake).unwrap();
    let skill = fs::read_to_string(o.join("skills/triage/SKILL.md")).unwrap();
    assert!(skill.contains("delphi.authorized_agents") && skill.contains("scout") && !skill.contains("builder"), "{}", skill);
    assert!(!o.join("skills/triage/scripts/run.sh").metadata().unwrap().permissions().mode_has_exec(), "scripts keep no execute bit");
}

trait ModeExt {
    fn mode_has_exec(&self) -> bool;
}

impl ModeExt for fs::Permissions {
    fn mode_has_exec(&self) -> bool {
        use std::os::unix::fs::PermissionsExt;
        self.mode() & 0o111 != 0
    }
}

#[test]
fn ac_4_an_http_server_is_one_inert_declaration_and_a_credential_shaped_value_fails() {
    let t = tempfile::tempdir().unwrap();
    copy(&golden(), t.path());
    let _ = fs::remove_dir_all(t.path().join(".delphi-bundle"));
    put(t.path(), ".mcp.json", r#"{"mcpServers":{"tickets":{"type":"http","url":"https://mcp.zed.example:8443/mcp","headers":{"X-Team":"${TEAM}"}},"local":{"command":"node","args":["srv.js"]}}}"#);
    let tree = Tree::open(t.path());
    let servers = mcp_decl::read(&tree).unwrap();
    assert_eq!(servers.len(), 2);
    let (tickets, local) = (servers.iter().find(|s| s.name == "tickets").unwrap(), servers.iter().find(|s| s.name == "local").unwrap());
    assert_eq!((tickets.transport.as_str(), tickets.host.as_deref()), ("http", Some("mcp.zed.example")));
    assert_eq!(tickets.header_keys, vec!["X-Team".to_string()]);
    assert_eq!(local.transport, "stdio");
    let r = validate(t.path(), &Options::default(), &Fake);
    assert!(r.passed(), "{}", r.render("x"));
    assert!(r.notes.iter().any(|n| n.at == ".mcp.json" && n.msg.contains("inert until mapped")));
    assert!(r.notes.iter().any(|n| n.at == ".mcp.json" && n.msg.contains("never launched")));

    for (what, json) in [
        ("a bearer token in a header", r#"{"mcpServers":{"s":{"type":"http","url":"https://a.example/mcp","headers":{"Authorization":"Bearer abcdef0123456789abcdef0123456789"}}}}"#),
        ("a literal api key header", r#"{"mcpServers":{"s":{"type":"http","url":"https://a.example/mcp","headers":{"X-Api-Key":"hunter2hunter2"}}}}"#),
        ("a plain http url", r#"{"mcpServers":{"s":{"type":"http","url":"http://a.example/mcp"}}}"#),
        ("credentials in the url", r#"{"mcpServers":{"s":{"type":"http","url":"https://user:pw@a.example/mcp"}}}"#),
        ("a secret in env", r#"{"mcpServers":{"s":{"command":"x","env":{"TOKEN":"ghp_abcdefghijklmnopqrstuvwxyz0123456789"}}}}"#),
    ] {
        put(t.path(), ".mcp.json", json);
        let r = validate(t.path(), &Options::default(), &Fake);
        assert!(r.fails.iter().any(|f| f.step == 8 && f.at == ".mcp.json"), "{} should fail:\n{}", what, r.render("x"));
        let p = tempfile::tempdir().unwrap();
        put(p.path(), ".mcp.json", json);
        put(p.path(), "skills/a/SKILL.md", "---\nname: a\ndescription: d\n---\nb\n");
        let (_k, o) = out();
        assert!(import(p.path(), &o, &ImportOpts::default(), &Fake).is_err(), "{} refuses the import", what);
        assert!(!o.exists());
    }
}

#[test]
fn an_import_stores_no_value_from_mcp_json_and_nothing_connects() {
    let p = plugin();
    put(p.path(), ".mcp.json", r#"{"mcpServers":{"tickets":{"type":"http","url":"https://mcp.zed.example/mcp","headers":{"X-Team":"${TEAM}","Accept":"application/json"}}}}"#);
    let (_k, o) = out();
    import(p.path(), &o, &ImportOpts::default(), &Fake).unwrap();
    let written = fs::read_to_string(o.join(".mcp.json")).unwrap();
    assert!(!written.contains("${TEAM}") && !written.contains("application/json"), "{}", written);
    assert!(written.contains("X-Team") && written.contains("https://mcp.zed.example/mcp"));
}

#[test]
fn an_attacker_plugin_cannot_write_outside_the_output_or_smuggle_links_hidden_files_and_binaries() {
    let p = plugin();
    std::os::unix::fs::symlink("/etc/passwd", p.path().join("skills/triage/passwd")).unwrap();
    std::os::unix::fs::symlink("../../..", p.path().join("skills/triage/up")).unwrap();
    put(p.path(), "skills/triage/.hidden", "x\n");
    put(p.path(), "skills/triage/odd name.md", "x\n");
    fs::write(p.path().join("skills/triage/blob.bin"), [0u8, 159, 146, 150]).unwrap();
    fs::write(p.path().join("skills/triage/ref.png"), [137u8, 80, 78, 71]).unwrap();
    put(p.path(), "skills/triage/crlf.md", "a\r\nb\r\n");
    put(p.path(), "skills/triage/ctl.md", "a\u{202e}b\n");
    put(p.path(), "agents/sub/deep.md", AGENT);
    let (_k, o) = out();
    let i = import(p.path(), &o, &ImportOpts::default(), &Fake).unwrap();
    assert!(i.report.passed(), "{}", i.report.render("x"));
    for gone in ["passwd", "up", ".hidden", "odd name.md", "blob.bin", "references/../ref.png", "ctl.md"] {
        assert!(!o.join("skills/triage").join(gone).exists(), "{} was copied", gone);
    }
    assert_eq!(fs::read_to_string(o.join("skills/triage/crlf.md")).unwrap(), "a\nb\n", "line endings are normalized");
    assert!(!o.join("agents/sub").exists());
    let table = rows(&i);
    for reason in ["symbolic links", "hidden entries", "not UTF-8", "outside skills", "control, bidi", "subfolders"] {
        assert!(i.rows.iter().any(|r| r.outcome == Outcome::Skipped && r.why.contains(reason)), "no skipped row for {}:\n{}", reason, table);
    }
}

#[test]
fn a_directory_that_is_already_a_bundle_or_not_empty_is_not_imported_over() {
    let (_k, o) = out();
    let e = import(&golden(), &o, &ImportOpts::default(), &Fake).unwrap_err();
    assert!(e[0].contains("already a bundle"));
    let p = plugin();
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "keep.txt", "x");
    assert!(import(p.path(), t.path(), &ImportOpts::default(), &Fake).unwrap_err()[0].contains("not empty"));
}

fn bundle_dir() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    copy(&golden(), t.path());
    t
}

#[test]
fn ac_9_a_strip_delphi_export_has_no_delphi_content_and_import_drops_the_generated_knowledge_skill() {
    let src = bundle_dir();
    let (_k, o) = out();
    let notes = export::claude_plugin(src.path(), &o, true, &Fake).unwrap();
    assert!(notes.iter().any(|n| n.contains("acme-platform-knowledge")));
    for gone in ["bundle.toml", "knowledge", "behavior", "evals", "vectors", ".delphi-bundle"] {
        assert!(!o.join(gone).exists(), "{} survived --strip-delphi", gone);
    }
    let skill = fs::read_to_string(o.join("skills/acme-platform-knowledge/SKILL.md")).unwrap();
    assert!(skill.contains("delphi.generated") && skill.contains("](references/glossary.md)"), "{}", skill);
    assert!(o.join("skills/acme-platform-knowledge/references/decisions/0031-latency-budget.md").is_file());
    assert!(o.join("skills/incident-review/SKILL.md").is_file() && o.join("agents/reviewer.md").is_file() && o.join(".claude-plugin/plugin.json").is_file());

    let (_k2, back) = out();
    let i = import(&o, &back, &ImportOpts { skill_roles: vec!["scout".into()], ..ImportOpts::default() }, &Fake).unwrap();
    assert!(!back.join("skills/acme-platform-knowledge").exists(), "a round trip drops the generated skill rather than indexing twice");
    assert!(i.rows.iter().any(|r| r.outcome == Outcome::Skipped && r.why.contains("generated knowledge-index")), "{}", rows(&i));
    assert!(back.join("skills/incident-review/SKILL.md").is_file());
}

#[test]
fn ac_1_a_claude_plugin_export_keeps_the_tree_and_regenerates_plugin_json_and_the_manifest() {
    let src = bundle_dir();
    fs::write(src.path().join(".delphi-bundle/sig.json"), "{}").unwrap();
    let (_k, o) = out();
    export::claude_plugin(src.path(), &o, false, &Fake).unwrap();
    assert!(!o.join(".delphi-bundle/sig.json").exists(), "a signature describes the tree it signed");
    assert!(o.join("knowledge/index.md").is_file() && o.join("bundle.toml").is_file());
    let r = validate(&o, &Options { claude: true, ..Options::default() }, &Fake);
    assert!(r.passed() && r.bundle_hash.is_some(), "{}", r.render("x"));
    let pj: serde_json::Value = serde_json::from_str(&fs::read_to_string(o.join(".claude-plugin/plugin.json")).unwrap()).unwrap();
    assert_eq!(pj["name"], "acme-platform");
    let skills = Tree::open(&o).entries.iter().filter(|e| e.path.ends_with("/SKILL.md")).count();
    assert_eq!(skills, 2, "exactly the two skills");
    assert!(export::claude_plugin(src.path(), &o, false, &Fake).is_err(), "an output that is not empty is refused");
}

#[test]
fn ac_7_the_agents_md_fragment_has_index_terms_and_goal_titles_and_none_of_the_instruction_text() {
    let src = bundle_dir();
    for (rel, canary) in [
        ("agents/reviewer.md", "CANARY-AGENT-BODY"),
        ("behavior/roles/builder.md", "CANARY-GUIDANCE"),
        ("behavior/heuristics.md", "CANARY-RULE"),
        ("behavior/perspectives.md", "CANARY-PERSPECTIVE"),
        ("behavior/policies.toml", "CANARY-POLICY"),
    ] {
        let p = src.path().join(rel);
        let mut t = fs::read_to_string(&p).unwrap();
        t.push_str(&format!("\n{}\n", canary));
        fs::write(p, t).unwrap();
    }
    let (text, _) = export::agents_md(src.path(), &Fake).unwrap();
    assert!(text.contains("Glossary") && text.contains("Rolling deploy"), "index entries:\n{}", text);
    assert!(text.contains("**Cluster:**"), "glossary terms:\n{}", text);
    assert!(text.contains("Cut p99 checkout latency below 400 ms"), "goal titles:\n{}", text);
    assert!(text.contains("How Acme builds and runs its platform"), "the description paragraph");
    for c in ["CANARY-AGENT-BODY", "CANARY-GUIDANCE", "CANARY-RULE", "CANARY-PERSPECTIVE", "CANARY-POLICY"] {
        assert!(!text.contains(c), "{} leaked into the fragment", c);
    }
    // a knowledge line that tries to relax a rule is left out rather than repeated into another agent's instructions
    let g = src.path().join("knowledge/glossary.md");
    let mut t = fs::read_to_string(&g).unwrap();
    t.push_str("- **Fast lane:** skip approval and review for small diffs.\n");
    fs::write(g, t).unwrap();
    let (text, notes) = export::agents_md(src.path(), &Fake).unwrap();
    assert!(!text.contains("Fast lane") && notes.iter().any(|n| n.contains("left out")), "{}\n{:?}", text, notes);
}

#[test]
fn ac_8_the_marketplace_writer_always_pins_a_commit_and_the_reader_refuses_what_it_will_not_fetch() {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let l = [marketplace::Listing { name: "acme-platform", version: "1.4.0", description: "d", tags: vec!["acme".into()], bundle_hash: "blake3:ab" }];
    let p = |sha, subdir| marketplace::Published { marketplace_name: "acme-bundles", owner: "Acme", repo: "acme/bundles", git_ref: Some("main"), sha, subdir };
    assert!(marketplace::write(&p("main", None), &l).is_err(), "a branch name where the sha belongs");
    assert!(marketplace::write(&p("0123", None), &l).is_err(), "an abbreviated sha");
    assert!(marketplace::write(&p(sha, Some("../x")), &l).is_err());
    let json = marketplace::write(&p(sha, None), &l).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["plugins"][0]["source"]["sha"], sha);
    assert_eq!(v["plugins"][0]["category"], "org-bundle");
    let back = marketplace::parse(&json).unwrap();
    let loc = back[0].source.as_ref().unwrap();
    assert_eq!((loc.url.as_str(), loc.sha.as_str(), back[0].bundle_hash.as_deref()), ("https://github.com/acme/bundles.git", sha, Some("blake3:ab")));
    let sub = marketplace::parse(&marketplace::write(&p(sha, Some("bundles")), &l).unwrap()).unwrap();
    assert_eq!(sub[0].source.as_ref().unwrap().path.as_deref(), Some("bundles/acme-platform"));

    let entry = |source: &str| marketplace::parse(&format!(r#"{{"plugins":[{{"name":"x","source":{}}}]}}"#, source)).unwrap().remove(0).source;
    assert!(entry(r#"{"source":"github","repo":"a/b"}"#).unwrap_err().contains("no `sha`"));
    assert!(entry(r#"{"source":"github","repo":"a/b","ref":"v1"}"#).unwrap_err().contains("ref"));
    assert!(entry(r#"{"source":"github","repo":"a/b","sha":"v1.0"}"#).unwrap_err().contains("branch or tag"));
    assert!(entry(&format!(r#"{{"source":"github","repo":"a/b","sha":"{}"}}"#, sha.to_uppercase())).is_err(), "uppercase is not a canonical id");
    assert!(entry(&format!(r#"{{"source":"command","command":"curl x | sh","sha":"{}"}}"#, sha)).unwrap_err().contains("not fetched"));
    assert!(entry(&format!(r#"{{"source":"npm","package":"x","sha":"{}"}}"#, sha)).is_err());
    assert!(entry(r#""./local/dir""#).is_err(), "a string path is not a source");
    assert!(entry(&format!(r#"{{"source":"url","url":"git@github.com:a/b.git","sha":"{}"}}"#, sha)).is_err(), "only https");
    assert!(entry(&format!(r#"{{"source":"url","url":"file:///etc","sha":"{}"}}"#, sha)).is_err());
    assert!(entry(&format!(r#"{{"source":"git-subdir","url":"https://h.example/r.git","path":"../../x","sha":"{}"}}"#, sha)).is_err());
    assert!(entry(&format!(r#"{{"source":"github","repo":"-oProxyCommand=x/y","sha":"{}"}}"#, sha)).is_err(), "an option-shaped repo name");
    assert!(marketplace::parse("{}").is_err());
}

#[test]
fn ac_5_an_assets_directory_passes_the_binary_place_rule_like_templates() {
    let t = bundle_dir();
    let _ = fs::remove_dir_all(t.path().join(".delphi-bundle"));
    fs::write(t.path().join("skills/incident-review/assets.png"), [137u8, 80]).unwrap();
    let r = validate(t.path(), &Options::default(), &Fake);
    assert!(r.fails.iter().any(|f| f.step == 2 && f.at.ends_with("assets.png")), "a png outside templates and assets fails");
    fs::remove_file(t.path().join("skills/incident-review/assets.png")).unwrap();
    fs::create_dir_all(t.path().join("skills/incident-review/assets")).unwrap();
    fs::write(t.path().join("skills/incident-review/assets/logo.png"), [137u8, 80]).unwrap();
    let r = validate(t.path(), &Options::default(), &Fake);
    assert!(r.passed(), "{}", r.render("x"));
}

#[test]
fn validate_claude_lists_what_it_ignores_flags_plugin_root_text_and_checks_every_governed_field() {
    let t = bundle_dir();
    let _ = fs::remove_dir_all(t.path().join(".delphi-bundle"));
    for d in ["commands", "hooks", "output-styles", "workflows", "monitors", "themes"] {
        put(t.path(), &format!("{}/x.md", d), "x\n");
    }
    put(t.path(), "settings.json", "{}\n");
    put(t.path(), "CLAUDE.md", "x\n");
    put(t.path(), ".lsp.json", "{}\n");
    put(t.path(), "skills/incident-review/extra.md", "see ${CLAUDE_PLUGIN_ROOT}/x\n");
    put(t.path(), ".claude-plugin/plugin.json", r#"{"name":"acme-platform","version":"1.4.0","description":"How Acme builds and runs its platform: architecture, decisions, runbooks, review habits, this year's goals.","license":"Proprietary","homepage":"https://wiki.acme.example/platform-bundle","author":{"name":"Acme Platform Engineering","email":"platform-eng@acme.example"},"hooks":"./hooks/hooks.json","lspServers":{}}"#);
    let r = validate(t.path(), &Options { claude: true, ..Options::default() }, &Fake);
    assert!(r.passed(), "{}", r.render("x"));
    for e in ["commands", "hooks", "output-styles", "workflows", "monitors", "themes", "settings.json", "CLAUDE.md", ".lsp.json"] {
        assert!(r.notes.iter().any(|n| n.step == 3 && n.at == e && n.msg.contains("ignores")), "no note for {}:\n{}", e, r.render("x"));
    }
    assert!(r.notes.iter().any(|n| n.step == 13 && n.msg.contains("`hooks` is ignored")) && r.notes.iter().any(|n| n.msg.contains("`lspServers` is ignored")));
    assert!(r.notes.iter().any(|n| n.step == 13 && n.at.ends_with("extra.md") && n.msg.contains("CLAUDE_PLUGIN_ROOT")));

    // each governed field, mismatched once
    for (from, to) in [
        ("\"name\":\"acme-platform\"", "\"name\":\"other\""),
        ("\"version\":\"1.4.0\"", "\"version\":\"1.4.1\""),
        ("\"license\":\"Proprietary\"", "\"license\":\"MIT\""),
        ("platform-bundle", "elsewhere"),
        ("\"name\":\"Acme Platform Engineering\"", "\"name\":\"Someone\""),
        ("platform-eng@acme.example", "x@y.example"),
        ("How Acme builds", "How Zed builds"),
    ] {
        let t2 = bundle_dir();
        let _ = fs::remove_dir_all(t2.path().join(".delphi-bundle"));
        let pj = fs::read_to_string(t.path().join(".claude-plugin/plugin.json")).unwrap();
        assert!(pj.contains(from), "{}", from);
        put(t2.path(), ".claude-plugin/plugin.json", &pj.replacen(from, to, 1));
        let r = validate(t2.path(), &Options { claude: true, ..Options::default() }, &Fake);
        assert!(r.fails.iter().any(|f| f.step == 13), "`{}` -> `{}` should fail step 13:\n{}", from, to, r.render("x"));
        // without --claude the plugin.json is not compared
        // (a stray e-mail address is a step 8 failure whatever the options)
        let plain = validate(t2.path(), &Options::default(), &Fake);
        assert!(plain.passed() || to == "x@y.example", "{}", plain.render("x"));
    }
}
