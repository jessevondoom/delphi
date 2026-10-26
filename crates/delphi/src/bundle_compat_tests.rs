//! Tests of `bundle_compat.rs`: the command line over an added bundle, with the real host checks.

use super::*;
use crate::bundle_add::{self, AddOpts};
use std::fs;

fn golden() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dev/bundles/acme-platform")
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() { copy(&e.path(), &to.join(e.file_name())) } else { fs::copy(e.path(), to.join(e.file_name())).unwrap(); }
    }
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// A home with workspace `w` that has the golden bundle added.
fn added() -> (tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let ws = home.path().join("workspaces/w");
    fs::create_dir_all(ws.join("local")).unwrap();
    let src = tempfile::tempdir().unwrap();
    copy(&golden(), src.path());
    let o = AddOpts { skill_roles: Default::default(), order: None, dry_run: false, yes: false, replace_publisher: false, no_vectors: false, embed_in_background: false };
    assert_eq!(bundle_add::add(home.path(), &ws, src.path(), &o, &Real), 0);
    (home, ws)
}

#[test]
fn export_claude_plugin_writes_a_plugin_that_validates_and_refuses_a_cache_that_changed_since_add() {
    let (home, ws) = added();
    let out = tempfile::tempdir().unwrap();
    let plugin = out.path().join("plugin");
    assert_eq!(export(home.path(), &ws, &args(&["acme-platform"]), &args(&["--claude-plugin", plugin.to_str().unwrap()])), 0);
    assert!(delphi_bundle::validate(&plugin, &Options { claude: true, ..Options::default() }, &Real).passed());
    let stripped = out.path().join("stripped");
    assert_eq!(export(home.path(), &ws, &args(&["acme/acme-platform"]), &args(&["--claude-plugin", stripped.to_str().unwrap(), "--strip-delphi"])), 0);
    assert!(!stripped.join("bundle.toml").exists() && stripped.join("skills/acme-platform-knowledge/SKILL.md").is_file());
    // an output that is not new, or that sits in the cache, is refused
    assert_ne!(export(home.path(), &ws, &args(&["acme-platform"]), &args(&["--claude-plugin", plugin.to_str().unwrap()])), 0);
    let e = bundles::read(&ws).unwrap().bundle.remove(0);
    let cache = bundles::cache_dir(home.path(), &e.org, &e.name, &e.hash);
    assert_ne!(export(home.path(), &ws, &args(&["acme-platform"]), &args(&["--claude-plugin", cache.join("x").to_str().unwrap()])), 0);
    // a cached file edited after add is not exported
    fs::write(cache.join("knowledge/glossary.md"), "tampered\n").unwrap();
    let again = out.path().join("again");
    assert_ne!(export(home.path(), &ws, &args(&["acme-platform"]), &args(&["--claude-plugin", again.to_str().unwrap()])), 0);
    assert!(!again.exists());
    assert_ne!(export(home.path(), &ws, &args(&["no-such"]), &args(&["--claude-plugin", again.to_str().unwrap()])), 0);
}

#[test]
fn ac_7_agents_md_defaults_to_a_file_under_the_delphi_home_never_in_a_project() {
    let (home, ws) = added();
    assert_eq!(export(home.path(), &ws, &args(&["acme-platform"]), &args(&["--agents-md"])), 0);
    let default = home.path().join("exports/acme/acme-platform/AGENTS.delphi.md");
    let text = fs::read_to_string(&default).unwrap();
    assert!(text.contains("Cut p99 checkout latency") && !text.contains("hooks"));
    // an explicit directory gets the same file name; an explicit file is used as given
    let d = tempfile::tempdir().unwrap();
    assert_eq!(export(home.path(), &ws, &args(&["acme-platform"]), &args(&["--agents-md", d.path().to_str().unwrap()])), 0);
    assert!(d.path().join("AGENTS.delphi.md").is_file());
    let f = d.path().join("frag.md");
    assert_eq!(export(home.path(), &ws, &args(&["acme-platform"]), &args(&["--agents-md", f.to_str().unwrap()])), 0);
    assert!(f.is_file());
    // a symlink in the way is not written through
    let link = d.path().join("link.md");
    std::os::unix::fs::symlink("/etc/hosts", &link).unwrap();
    assert_ne!(export(home.path(), &ws, &args(&["acme-platform"]), &args(&["--agents-md", link.to_str().unwrap()])), 0);
}

#[test]
fn ac_8_export_marketplace_pins_every_bundle_and_refuses_an_unpinned_one() {
    let (home, ws) = added();
    let out = tempfile::tempdir().unwrap();
    let dir = out.path().to_str().unwrap();
    let sha = "0123456789abcdef0123456789abcdef01234567";
    assert_ne!(export(home.path(), &ws, &args(&["acme-platform"]), &args(&["--marketplace", dir, "--repo", "acme/bundles"])), 0, "no sha");
    assert_ne!(export(home.path(), &ws, &args(&["acme-platform"]), &args(&["--marketplace", dir, "--repo", "acme/bundles", "--sha", "main"])), 0, "a branch name");
    assert_eq!(export(home.path(), &ws, &args(&["acme-platform"]), &args(&["--marketplace", dir, "--repo", "acme/bundles", "--sha", sha, "--ref", "main"])), 0);
    let text = fs::read_to_string(out.path().join(".claude-plugin/marketplace.json")).unwrap();
    let entries = delphi_bundle::marketplace::parse(&text).unwrap();
    let e = bundles::read(&ws).unwrap().bundle.remove(0);
    assert_eq!((entries[0].name.as_str(), entries[0].bundle_hash.as_deref()), ("acme-platform", Some(e.hash.as_str())));
    assert_eq!(entries[0].source.as_ref().unwrap().sha, sha);
}

#[test]
fn import_on_the_command_line_writes_a_bundle_that_adds_and_exits_non_zero_when_it_does_not_validate() {
    let plugin = tempfile::tempdir().unwrap();
    fs::create_dir_all(plugin.path().join("skills/triage")).unwrap();
    fs::write(plugin.path().join("skills/triage/SKILL.md"), "---\nname: triage\ndescription: Sort reports by severity.\n---\nSort them.\n").unwrap();
    fs::create_dir_all(plugin.path().join("hooks")).unwrap();
    fs::write(plugin.path().join("hooks/hooks.json"), "{}\n").unwrap();
    let out = tempfile::tempdir().unwrap();
    let o = out.path().join("b");
    assert_eq!(import(plugin.path(), &args(&["import", "x", "-o", o.to_str().unwrap(), "--as", "triage-kit", "--org", "zed", "--agents", "scout"])), 0);
    assert!(!o.join("hooks").exists());
    // the imported directory is addable as it stands
    let home = tempfile::tempdir().unwrap();
    let ws = home.path().join("workspaces/w");
    fs::create_dir_all(ws.join("local")).unwrap();
    let opts = AddOpts { skill_roles: Default::default(), order: None, dry_run: false, yes: false, replace_publisher: false, no_vectors: false, embed_in_background: false };
    assert_eq!(bundle_add::add(home.path(), &ws, &o, &opts, &Real), 0);
    let lock = bundles::read(&ws).unwrap();
    assert_eq!(lock.bundle[0].id(), "zed/triage-kit");
    // a skill whose description is over the cap is written but reported, and the command fails
    fs::write(plugin.path().join("skills/triage/SKILL.md"), format!("---\nname: triage\ndescription: {}\n---\nSort them.\n", "long ".repeat(60))).unwrap();
    let o2 = out.path().join("b2");
    assert_eq!(import(plugin.path(), &args(&["import", "x", "-o", o2.to_str().unwrap()])), 1);
    assert!(o2.join("bundle.toml").is_file(), "the directory is left for the operator to fix");
    // a bad flag and a refused plugin
    assert_eq!(import(plugin.path(), &args(&["import", "x", "--agent-role", "oops"])), 2);
    fs::create_dir_all(plugin.path().join("bin")).unwrap();
    assert_eq!(import(plugin.path(), &args(&["import", "x", "-o", out.path().join("b3").to_str().unwrap()])), 1);
    assert!(!out.path().join("b3").exists());
}
