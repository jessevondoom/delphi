//! `delphi bundle validate|manifest` (`specs/023`): check an org bundle directory, and write its file manifest.
//!
//! Both read a directory and need no workspace and no model. `validate` changes nothing. The checks that live in the
//! engine come to `delphi-bundle` through [`Real`], so that crate stays free of the engine.

use crate::{frontmatter, seed, skills_cmd};
use delphi_bundle::{Host, Options, RoleScope};
use serde_yaml::{Mapping, Value};
use std::path::{Path, PathBuf};

struct Real;

fn strings(fm: &Mapping, scope: &str, key: &str) -> Vec<String> {
    fm.get(Value::String(scope.into()))
        .and_then(|s| s.as_mapping())
        .and_then(|s| s.get(Value::String(key.into())))
        .and_then(|v| v.as_sequence())
        .map(|s| s.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

impl Host for Real {
    fn provenance(&self, text: &str) -> Vec<String> {
        delphi_engine::checks::provenance(text)
    }

    fn skill_errors(&self, fm: &Mapping, dir: &str) -> Vec<String> {
        skills_cmd::standard_errors(fm, dir)
    }

    fn role_scope(&self, role: &str) -> Option<RoleScope> {
        let fm = frontmatter::parse(seed::builtin_agent(role)?).ok()?.frontmatter?;
        let (read, write, execute) = (strings(&fm, "default_access_scope", "read"), strings(&fm, "default_access_scope", "write"), strings(&fm, "default_access_scope", "execute"));
        // A role's tools follow from its scope: every role reads, a role with a write glob writes, and a role that may run
        // the scoped shell has Bash. The names are Claude Code's, which is what an agent file's `tools` list uses.
        let mut tools: Vec<String> = ["Read", "Grep", "Glob"].map(String::from).to_vec();
        if !write.is_empty() {
            tools.extend(["Write", "Edit"].map(String::from));
        }
        if execute.iter().any(|e| e == "shell-worktree-scoped") {
            tools.push("Bash".into());
        }
        Some(RoleScope { read, write, execute, tools })
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

pub fn run(args: &[String]) -> i32 {
    // `args` is what follows `delphi bundle`, as for `delphi skills`.
    let rest: Vec<&String> = args.iter().collect();
    let positional: Vec<&String> = rest.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || rest[*i - 1].as_str() != "--against")).map(|(_, a)| *a).collect();
    let usage = || {
        eprintln!("usage: delphi bundle validate <dir> [--claude] [--against <previous dir>] [--package]\n       delphi bundle manifest <dir> [--claude]");
        2
    };
    let (Some(sub), Some(dir)) = (positional.first(), positional.get(1)) else { return usage() };
    let dir = dir.to_string();
    let dir = PathBuf::from(dir.as_str());
    if !dir.is_dir() {
        eprintln!("delphi bundle: {} is not a directory", dir.display());
        return 2;
    }
    let claude = args.iter().any(|a| a == "--claude");
    match sub.as_str() {
        "validate" => validate(&dir, claude, args),
        "manifest" => match delphi_bundle::write_manifest(&dir, claude, &Real) {
            Ok(hash) => {
                println!("wrote .delphi-bundle/files.json{}\nbundle hash {}", if claude { " and .claude-plugin/plugin.json" } else { "" }, hash);
                0
            }
            Err(e) => {
                eprintln!("delphi bundle manifest: {}", e);
                1
            }
        },
        _ => usage(),
    }
}

fn validate(dir: &Path, claude: bool, args: &[String]) -> i32 {
    let opts = Options { packaging: args.iter().any(|a| a == "--package"), claude, against: flag(args, "--against").map(PathBuf::from) };
    let report = delphi_bundle::validate(dir, &opts, &Real);
    print!("{}", report.render(&dir.display().to_string()));
    if report.passed() { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn the_golden_bundle_passes_with_the_real_checks_and_its_committed_hash_is_current() {
        let r = delphi_bundle::validate(&golden(), &Options { claude: true, ..Options::default() }, &Real);
        assert!(r.passed(), "{}", r.render("dev/bundles/acme-platform"));
        // Editing the fixture and forgetting `delphi bundle manifest` fails step 5 above; here the hash must be known.
        assert!(r.bundle_hash.as_deref().is_some_and(|h| h.starts_with("blake3:")), "files.json is committed and current");
        assert!(r.warns.is_empty(), "{}", r.render("x"));
    }

    #[test]
    fn the_real_provenance_check_and_the_real_role_scopes_apply() {
        let t = tempfile::tempdir().unwrap();
        copy(&golden(), t.path());
        let _ = fs::remove_dir_all(t.path().join(".delphi-bundle"));
        // a knowledge file whose frontmatter breaks the real schema (author is not a role, timestamp not ISO)
        let p = t.path().join("knowledge/glossary.md");
        let text = fs::read_to_string(&p).unwrap().replacen("author: operator", "author: wizard", 1).replacen("2026-09-12T10:00:00Z", "yesterday", 1);
        fs::write(&p, text).unwrap();
        // a skill that asks for the shell on a role that has none
        let s = t.path().join("skills/migration-plan/SKILL.md");
        fs::write(&s, fs::read_to_string(&s).unwrap().replacen("[\"read:all\"]", "[\"read:all\",\"shell:scoped_scripts\"]", 1)).unwrap();
        let r = delphi_bundle::validate(t.path(), &Options::default(), &Real);
        let all: Vec<String> = r.fails.iter().map(|f| format!("{} {}", f.at, f.msg)).collect();
        assert!(all.iter().any(|f| f.contains("knowledge/glossary.md") && f.contains("author `wizard`")), "{:#?}", all);
        assert!(all.iter().any(|f| f.contains("knowledge/glossary.md") && f.contains("ISO-8601")), "{:#?}", all);
        assert!(all.iter().any(|f| f.contains("skills/migration-plan") && f.contains("architect role's scope")), "{:#?}", all);
    }

    #[test]
    fn built_in_roles_have_scopes_and_tools_an_agent_list_is_intersected_with() {
        let b = Real.role_scope("builder").unwrap();
        assert!(b.tools.contains(&"Bash".to_string()) && b.tools.contains(&"Write".to_string()) && b.write.contains(&"@worktree/**".to_string()));
        let rt = Real.role_scope("red-team").unwrap();
        assert!(!rt.tools.contains(&"Bash".to_string()), "red-team has no shell");
        assert!(Real.role_scope("librarian").is_none(), "an unbuilt role is unknown, not guessed");
    }

    #[test]
    fn the_command_line_validates_writes_the_manifest_and_refuses_a_bad_directory() {
        let t = tempfile::tempdir().unwrap();
        copy(&golden(), t.path());
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let d = t.path().to_str().unwrap();
        assert_eq!(run(&args(&["validate", d, "--claude"])), 0);
        assert_eq!(run(&args(&["manifest", d, "--claude"])), 0);
        assert_eq!(run(&args(&["validate", d, "--claude", "--package"])), 0);
        fs::write(t.path().join("README.md"), "changed\n").unwrap();
        assert_eq!(run(&args(&["validate", d])), 1, "a changed file no longer matches files.json");
        assert_eq!(run(&args(&["validate"])), 2);
        assert_eq!(run(&args(&["frobnicate", d])), 2);
        assert_eq!(run(&args(&["validate", "/nonexistent-bundle-dir"])), 2);
    }
}
