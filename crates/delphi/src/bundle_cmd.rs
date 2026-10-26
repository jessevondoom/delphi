//! `delphi bundle validate|manifest` (`specs/023`): check an org bundle directory, and write its file manifest.
//!
//! Both read a directory and need no workspace and no model. `validate` changes nothing. The checks that live in the
//! engine come to `delphi-bundle` through [`Real`], so that crate stays free of the engine.

use crate::{bundle_add, bundle_fetch, bundle_life, bundle_pack, frontmatter, home, seed, skills_cmd, xenia_cmd};
use delphi_bundle::{Host, Options, RoleScope};
use serde_yaml::{Mapping, Value};
use std::path::{Path, PathBuf};

pub(crate) struct Real;

pub(crate) fn flag_values(args: &[String], name: &str) -> Vec<String> {
    args.iter().enumerate().filter(|(_, a)| *a == name).filter_map(|(i, _)| args.get(i + 1).cloned()).collect()
}

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

    fn chunk_keys(&self, org: &str, name: &str, texts: &std::collections::BTreeMap<String, String>) -> Option<std::collections::HashSet<[u8; 32]>> {
        Some(delphi_engine::bundle_vectors::chunk_keys(org, name, texts))
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

/// Flags that take a value, so the word after one is not a positional argument.
const VALUE_FLAGS: [&str; 22] = [
    "--against", "--order", "--workspace", "--home", "--skill-roles", "--to", "-o", "--as", "--org", "--agents", "--agent-role", "--out", "--from-marketplace", "--claude-plugin", "--agents-md", "--marketplace", "--repo", "--sha", "--ref", "--owner", "--subdir", "--marketplace-name",
];

fn positionals(args: &[String]) -> Vec<&String> {
    args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && a.as_str() != "-o" && (*i == 0 || !VALUE_FLAGS.contains(&args[*i - 1].as_str()))).map(|(_, a)| a).collect()
}

const USAGE: &str = "usage: delphi bundle validate <dir> [--claude] [--against <previous dir>] [--package]\n       delphi bundle import <plugin dir> [-o <new dir>] [--as <name>] [--org <org>] [--agents <role,role>] [--agent-role <agent>=<role>]...\n       delphi bundle export <name> --claude-plugin <new dir> [--strip-delphi] | --agents-md [<file>] | (<name>... --marketplace <dir> --repo <owner/repo|https url> --sha <commit> [--ref R] [--owner O] [--subdir D])\n       delphi bundle manifest <dir> [--claude]\n       delphi bundle sign <dir> [--against <previous dir>] [--home <dir>]\n       delphi bundle package <dir> [-o <file.xbundle>]\n       delphi bundle add <dir or file.xbundle> | --from-marketplace <repo> <name> [--order N] [--skill-roles <skill>=<roles>] [--replace-publisher] [--no-vectors] [--yes] [--dry-run]\n       delphi bundle list | remove <name> | update [name] | rollback <name> [--to <hash>] | gc   (these take --workspace <name> --home <dir>)";

/// `add`, `list` and `remove`: they change one workspace, which the caller has resolved.
pub fn run_in(home: &Path, ws: &Path, args: &[String]) -> i32 {
    let positional = positionals(args);
    let flag_set = |f: &str| args.iter().any(|a| a == f);
    match (positional.first().map(|s| s.as_str()), positional.get(1)) {
        (Some("add"), Some(name)) if flag(args, "--from-marketplace").is_some() => {
            let repo = flag(args, "--from-marketplace").unwrap();
            let opts = bundle_add::AddOpts { skill_roles: Default::default(), order: flag(args, "--order").and_then(|o| o.parse().ok()), dry_run: flag_set("--dry-run"), yes: flag_set("--yes"), replace_publisher: flag_set("--replace-publisher"), no_vectors: flag_set("--no-vectors"), embed_in_background: true };
            bundle_fetch::add_from_marketplace(home, ws, &repo, name, &opts, &bundle_fetch::real_fetcher(home), &Real)
        }
        (Some("export"), Some(_)) => crate::bundle_compat::export(home, ws, &positional[1..].iter().map(|s| s.to_string()).collect::<Vec<_>>(), args),
        (Some("add"), Some(dir)) => {
            let order = match flag(args, "--order").map(|o| o.parse::<i64>()) {
                None => None,
                Some(Ok(n)) => Some(n),
                Some(Err(_)) => {
                    eprintln!("delphi bundle: --order wants a whole number");
                    return 2;
                }
            };
            let mut skill_roles = std::collections::BTreeMap::new();
            for (i, _) in args.iter().enumerate().filter(|(_, a)| *a == "--skill-roles") {
                let Some((skill, roles)) = args.get(i + 1).and_then(|v| v.split_once('=')) else {
                    eprintln!("delphi bundle: --skill-roles wants <skill>=<role,role>");
                    return 2;
                };
                skill_roles.insert(skill.to_string(), roles.split(',').map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect());
            }
            bundle_add::add(home, ws, Path::new(dir.as_str()), &bundle_add::AddOpts { skill_roles, order, dry_run: flag_set("--dry-run"), yes: flag_set("--yes"), replace_publisher: flag_set("--replace-publisher"), no_vectors: flag_set("--no-vectors"), embed_in_background: true }, &Real)
        }
        (Some("update"), name) => bundle_life::update(home, ws, name.map(|s| s.as_str()), true, &Real),
        (Some("rollback"), Some(name)) => {
            let to = flag(args, "--to");
            if to.as_deref().is_some_and(|t| bundle_life::hash_prefix(t).is_none()) {
                eprintln!("delphi bundle: --to wants a bundle hash: at least 8 hex digits");
                return 2;
            }
            bundle_life::rollback(home, ws, name, to.as_deref(), true, &Real)
        }
        (Some("gc"), _) => bundle_life::gc(home, ws),
        (Some("list"), _) => bundle_add::list(home, ws),
        (Some("remove"), Some(name)) => bundle_add::remove(ws, name),
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}

pub fn run(args: &[String]) -> i32 {
    // `args` is what follows `delphi bundle`, as for `delphi skills`.
    let positional = positionals(args);
    let usage = || {
        eprintln!("{}", USAGE);
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
        "import" => crate::bundle_compat::import(&dir, args),
        "sign" => {
            let h = home::delphi_home(flag(args, "--home").as_deref());
            bundle_pack::sign(&dir, &xenia_cmd::key_path(&h), flag(args, "--against").map(PathBuf::from), &Real, &bundle_pack::tty_confirm)
        }
        "package" => bundle_pack::package(&dir, flag(args, "-o").as_deref().map(Path::new), &Real),
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

    /// The golden bundle with a vector cache of its real chunk keys (all of them, or half), plus `foreign` keys that no text makes.
    fn with_cache(own_all: bool, foreign: usize) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        copy(&golden(), t.path());
        let texts = delphi_bundle::Tree::open(t.path()).texts("knowledge/", ".md");
        let mut keys: Vec<[u8; 32]> = delphi_engine::bundle_vectors::chunk_keys("acme", "acme-platform", &texts).into_iter().collect();
        keys.sort();
        if !own_all {
            keys.truncate(keys.len() / 2);
        }
        keys.extend((0..foreign).map(|i| *blake3::hash(&[i as u8, 7]).as_bytes()));
        let entries: Vec<_> = keys.iter().map(|k| (*k, vec![vec![0.5f32, 0.5]])).collect();
        let file = "m-2-1-2.dlphvec";
        fs::create_dir_all(t.path().join("vectors")).unwrap();
        fs::write(t.path().join("vectors").join(file), delphi_engine::embed::vectors::VecCache::serialize("m", 2, "1", &entries)).unwrap();
        let index = format!("{{\"vectors\":1,\"caches\":[{{\"file\":\"{}\",\"model_id\":\"m\",\"dim\":2,\"embedding_format\":\"1\",\"chunker_version\":\"2\",\"chunks\":{},\"windows\":{},\"dtype\":\"f32\"}}]}}\n", file, entries.len(), entries.len());
        fs::write(t.path().join("vectors/index.json"), index).unwrap();
        let toml = fs::read_to_string(t.path().join("bundle.toml")).unwrap();
        fs::write(t.path().join("bundle.toml"), toml.replacen("knowledge = true", "knowledge = true\nvectors = true", 1)).unwrap();
        delphi_bundle::write_manifest(t.path(), true, &Real).unwrap();
        t
    }

    #[test]
    fn the_real_chunker_lets_step_12_check_that_a_caches_keys_come_from_the_bundles_own_text() {
        let own = with_cache(true, 0);
        let r = delphi_bundle::validate(own.path(), &Options { claude: true, ..Options::default() }, &Real);
        assert!(r.passed() && r.warns.is_empty(), "{}", r.render("x"));
        assert!(r.notes.iter().all(|n| !n.msg.contains("cannot chunk")), "the check really ran");
        // keys no chunk of the text makes: more than 1% fails, as built from other text
        let foreign = with_cache(true, 5);
        let r = delphi_bundle::validate(foreign.path(), &Options { claude: true, ..Options::default() }, &Real);
        assert!(r.fails.iter().any(|f| f.step == 12 && f.msg.contains("built from other text")), "{}", r.render("x"));
        // half the chunks and none foreign is a cache that covers less, which is fine
        let half = with_cache(false, 0);
        assert!(delphi_bundle::validate(half.path(), &Options { claude: true, ..Options::default() }, &Real).passed());
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
