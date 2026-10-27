//! `delphi bundle validate|manifest` (`specs/023`): check an org bundle directory, and write its file manifest.
//!
//! Both read a directory and need no workspace and no model. `validate` changes nothing. The checks that live in the
//! engine come to `delphi-bundle` through [`Real`], so that crate stays free of the engine.

use crate::{bundle_add, bundle_fetch, bundle_init, bundle_life, bundle_pack, frontmatter, home, seed, skills_cmd, xenia_cmd};
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

/// `Real` that also knows the names of one workspace (`specs/027` §6.2): its `[privacy] identifiers` and handles make
/// step 8 warn, so the author finds them before a stranger does.
pub(crate) struct Scoped {
    pub identifiers: Vec<String>,
    pub handles: Vec<String>,
}

impl Host for Scoped {
    fn provenance(&self, text: &str) -> Vec<String> {
        Real.provenance(text)
    }

    fn skill_errors(&self, fm: &Mapping, dir: &str) -> Vec<String> {
        Real.skill_errors(fm, dir)
    }

    fn chunk_keys(&self, org: &str, name: &str, texts: &std::collections::BTreeMap<String, String>) -> Option<std::collections::HashSet<[u8; 32]>> {
        Real.chunk_keys(org, name, texts)
    }

    fn role_scope(&self, role: &str) -> Option<RoleScope> {
        Real.role_scope(role)
    }

    fn identifiers(&self) -> Vec<String> {
        self.identifiers.clone()
    }

    fn roster_handles(&self) -> Vec<String> {
        self.handles.clone()
    }
}

/// The host for a command that reads a bundle directory: `Real`, with a workspace's names when one is named by
/// `--workspace` (or `DELPHI_WORKSPACE`). A name that is not a workspace is an error, never a silent plain scan.
fn host_for(args: &[String]) -> Result<Box<dyn Host>, String> {
    let Some(name) = flag(args, "--workspace").or_else(|| std::env::var("DELPHI_WORKSPACE").ok()) else { return Ok(Box::new(Real)) };
    let h = home::delphi_home(flag(args, "--home").as_deref());
    let ws = h.join("workspaces").join(&name);
    if !ws.join("config.toml").is_file() {
        return Err(format!("no workspace named `{}` in {}", name, h.display()));
    }
    let p = delphi_engine::distill::privacy(&ws);
    Ok(Box::new(Scoped { identifiers: p.identifiers, handles: p.handles }))
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// Flags that take a value, so the word after one is not a positional argument.
const VALUE_FLAGS: [&str; 25] = [
    "--against", "--order", "--workspace", "--home", "--skill-roles", "--to", "-o", "--as", "--org", "--name", "--report", "--agents", "--agent-role", "--out", "--from-marketplace", "--claude-plugin", "--agents-md", "--marketplace", "--repo", "--sha", "--ref", "--owner", "--subdir", "--marketplace-name", "--bundle",
];

fn positionals(args: &[String]) -> Vec<&String> {
    args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && a.as_str() != "-o" && (*i == 0 || !VALUE_FLAGS.contains(&args[*i - 1].as_str()))).map(|(_, a)| a).collect()
}

const USAGE: &str = "usage: delphi bundle init <dir> --org <org> --name <name>\n       delphi bundle validate <dir> [--claude] [--against <previous dir>] [--package] [--report <file>] [--workspace <name>]\n       delphi bundle distill [<path|missions/id>...] [--workspace <name>]\n       delphi bundle import <plugin dir> [-o <new dir>] [--as <name>] [--org <org>] [--agents <role,role>] [--agent-role <agent>=<role>]...\n       delphi bundle export <name> --claude-plugin <new dir> [--strip-delphi] | --agents-md [<file>] | (<name>... --marketplace <dir> --repo <owner/repo|https url> --sha <commit> [--ref R] [--owner O] [--subdir D])\n       delphi bundle manifest <dir> [--claude]\n       delphi bundle sign <dir> [--against <previous dir>] [--home <dir>]\n       delphi bundle package <dir> [-o <file.xbundle>] [--vectors] [--no-eval] [--workspace <name>] [--home <dir>]\n       delphi bundle prereg check <file> | digest <file or dir>\n       delphi bundle add <dir or file.xbundle> | --from-marketplace <repo> <name> [--order N] [--skill-roles <skill>=<roles>] [--replace-publisher] [--no-vectors] [--yes] [--dry-run]\n       delphi bundle report <name>   (counts from the event logs: citations, alignment findings, trims, vectors; never text)\n       delphi bundle list | remove <name> | update [name] | rollback <name> [--to <hash>] | gc | serve [--bundle <org/name>]...   (these take --workspace <name> --home <dir>)";

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
        (Some("report"), Some(name)) => crate::bundle_report::report(home, ws, name),
        (Some("serve"), _) => {
            let only: Vec<String> = args.iter().enumerate().filter(|(_, a)| *a == "--bundle").filter_map(|(i, _)| args.get(i + 1).cloned()).collect();
            delphi_engine::bundle_serve::run(home, ws, &only)
        }
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
    if positional.first().map(|s| s.as_str()) == Some("prereg") {
        return prereg(&positional[1..]);
    }
    let (Some(sub), Some(dir)) = (positional.first(), positional.get(1)) else { return usage() };
    let dir = dir.to_string();
    let dir = PathBuf::from(dir.as_str());
    if sub.as_str() == "init" {
        return init(&dir, args);
    }
    if !dir.is_dir() {
        eprintln!("delphi bundle: {} is not a directory", dir.display());
        return 2;
    }
    let claude = args.iter().any(|a| a == "--claude");
    let host = match host_for(args) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("delphi bundle: {}", e);
            return 2;
        }
    };
    match sub.as_str() {
        "validate" => validate(&dir, claude, args, &*host),
        "import" => crate::bundle_compat::import(&dir, args),
        "sign" => {
            let h = home::delphi_home(flag(args, "--home").as_deref());
            bundle_pack::sign(&dir, &xenia_cmd::key_path(&h), flag(args, "--against").map(PathBuf::from), &*host, &bundle_pack::tty_confirm)
        }
        "package" => {
            if args.iter().any(|a| a == "--vectors") {
                let h = home::delphi_home(flag(args, "--home").as_deref());
                let model = match delphi_engine::embed::open_default(Some(&h)) {
                    Ok(Some(m)) => m,
                    Ok(None) => {
                        eprintln!("delphi bundle: --vectors needs an embedding model and none is installed (install one with `delphi kb model pack`, or package without --vectors: adopters then embed locally)");
                        return 1;
                    }
                    Err(e) => {
                        eprintln!("delphi bundle: the embedding model could not be opened: {}", e);
                        return 1;
                    }
                };
                match bundle_pack::add_vectors(&dir, &*host, &*model, &mut |done, total| eprint!("\rembedding {}/{} chunks", done, total)) {
                    Ok(msg) => eprintln!("\n{}", msg),
                    Err(e) => {
                        eprintln!("\ndelphi bundle: {}", e);
                        return 1;
                    }
                }
            }
            if !args.iter().any(|a| a == "--no-eval") {
                let h = home::delphi_home(flag(args, "--home").as_deref());
                if let Some(lines) = bundle_pack::record_eval(&dir, &h) {
                    eprintln!("{}", lines);
                }
            }
            bundle_pack::package(&dir, flag(args, "-o").as_deref().map(Path::new), &*host)
        }
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

/// `delphi bundle prereg check <file>` prints the file's pin (`specs/027` §7.2) when it has all nine headings;
/// `delphi bundle prereg digest <file or dir>` prints a blake3 digest for the `## Task set` and `## Rubric` sections. The
/// bench runner (`dev/bench/bench.py`) calls both, so the hash it checks is the one this build computes.
fn prereg(rest: &[&String]) -> i32 {
    match (rest.first().map(|s| s.as_str()), rest.get(1)) {
        (Some("check"), Some(file)) => {
            let text = match std::fs::read_to_string(file.as_str()) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("delphi bundle: {}: {}", file, e);
                    return 2;
                }
            };
            let missing = delphi_bundle::prereg_missing(&text);
            if !missing.is_empty() {
                eprintln!("delphi bundle: {} lacks the required heading(s): {} (specs/027 §7.2)", file, missing.join(", "));
                return 1;
            }
            println!("{}", delphi_bundle::prereg_pin(&text));
            0
        }
        (Some("digest"), Some(path)) => match delphi_bundle::prereg_digest(Path::new(path.as_str())) {
            Ok(d) => {
                println!("{}", d);
                0
            }
            Err(e) => {
                eprintln!("delphi bundle: {}", e);
                2
            }
        },
        _ => {
            eprintln!("usage: delphi bundle prereg check <file> | digest <file or dir>");
            2
        }
    }
}

fn validate(dir: &Path, claude: bool, args: &[String], host: &dyn Host) -> i32 {
    let opts = Options { packaging: args.iter().any(|a| a == "--package"), claude, against: flag(args, "--against").map(PathBuf::from) };
    // The redaction report goes outside the bundle: a report inside it would be packaged with the bundle (`specs/027` §2.4).
    if let Some(path) = flag(args, "--report") {
        let path = PathBuf::from(path);
        match report_outside(dir, &path) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("delphi bundle: {}", e);
                return 2;
            }
        }
        if let Err(e) = std::fs::write(&path, delphi_bundle::redaction::render(dir, host)) {
            eprintln!("delphi bundle: {}: {}", path.display(), e);
            return 2;
        }
        eprintln!("wrote the redaction report to {}; work every finding, then a person approves it before `package`", path.display());
    }
    let report = delphi_bundle::validate(dir, &opts, host);
    print!("{}", report.render(&dir.display().to_string()));
    if report.passed() { 0 } else { 1 }
}

/// `path` must not be inside `dir` (it would ship), including through a symlinked parent.
fn report_outside(dir: &Path, path: &Path) -> Result<(), String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let (Ok(d), Ok(p)) = (dir.canonicalize(), parent.canonicalize()) else { return Err(format!("{}: the directory for the report does not exist", path.display())) };
    if p.starts_with(&d) {
        return Err(format!("{} is inside the bundle; a report there would be packaged with it. Write it beside the bundle", path.display()));
    }
    Ok(())
}

/// `delphi bundle init <dir> --org <org> --name <name>`.
fn init(dir: &Path, args: &[String]) -> i32 {
    let (Some(org), Some(name)) = (flag(args, "--org"), flag(args, "--name")) else {
        eprintln!("delphi bundle init: --org and --name are required\n{}", USAGE);
        return 2;
    };
    let h = home::delphi_home(flag(args, "--home").as_deref());
    let now = format!("{}Z", &delphi_core::time::now_rfc3339()[..19]);
    match bundle_init::init(&h, dir, &org, &name, &now) {
        Ok(files) => {
            println!("created {} ({} files); it validates as it stands", dir.display(), files.len());
            println!("{}", bundle_init::NEXT);
            0
        }
        Err(e) => {
            eprintln!("delphi bundle init: {}", e);
            2
        }
    }
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

    /// A model that puts each text at a point set by its bytes; enough for a cache to be built, written and read back.
    struct Words;
    impl delphi_engine::embed::Embedder for Words {
        fn model_id(&self) -> &str {
            "words-test"
        }
        fn dim(&self) -> usize {
            4
        }
        fn min_similarity(&self) -> f32 {
            0.0
        }
        fn embed_documents(&self, t: &[String]) -> Result<Vec<Vec<f32>>, String> {
            Ok(t.iter().map(|s| (0..4).map(|i| s.bytes().filter(|b| *b as usize % 4 == i).count() as f32 + 1.0).collect()).collect())
        }
        fn embed_query(&self, t: &str) -> Result<Vec<f32>, String> {
            Ok(self.embed_documents(&[t.to_string()])?.remove(0))
        }
    }

    #[test]
    fn package_vectors_writes_a_cache_that_passes_step_12_and_declares_the_section() {
        let t = tempfile::tempdir().unwrap();
        copy(&golden(), t.path());
        let toml = fs::read_to_string(t.path().join("bundle.toml")).unwrap();
        assert!(!toml.contains("vectors"), "the golden bundle ships none");
        let msg = bundle_pack::add_vectors(t.path(), &Real, &Words, &mut |_, _| {}).unwrap();
        assert!(msg.contains("words-test") && msg.contains("not Delphi's default embedder"), "{}", msg);
        assert!(fs::read_to_string(t.path().join("bundle.toml")).unwrap().contains("[sections]\nvectors = true\n"));
        let index: serde_json::Value = serde_json::from_str(&fs::read_to_string(t.path().join("vectors/index.json")).unwrap()).unwrap();
        let file = index["caches"][0]["file"].as_str().unwrap().to_string();
        assert!(t.path().join("vectors").join(&file).is_file());
        // the manifest is regenerated as `package` does, then the real checks (the real chunker) accept the keys
        delphi_bundle::write_manifest(t.path(), true, &Real).unwrap();
        let r = delphi_bundle::validate(t.path(), &Options { claude: true, packaging: true, ..Options::default() }, &Real);
        assert!(r.passed(), "{}", r.render("x"));
        assert!(r.notes.iter().all(|n| !n.msg.contains("cannot chunk")), "step 12 really checked the keys");
        // running it again replaces the same cache and keeps the index single
        bundle_pack::add_vectors(t.path(), &Real, &Words, &mut |_, _| {}).unwrap();
        let again: serde_json::Value = serde_json::from_str(&fs::read_to_string(t.path().join("vectors/index.json")).unwrap()).unwrap();
        assert_eq!(again["caches"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn prereg_check_prints_the_pin_and_refuses_a_file_without_all_nine_headings() {
        let t = tempfile::tempdir().unwrap();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let f = t.path().join("prereg.md");
        fs::write(&f, "## Hypotheses\nx\n").unwrap();
        assert_eq!(run(&args(&["prereg", "check", f.to_str().unwrap()])), 1);
        let full: String = delphi_bundle::PREREG_HEADINGS.iter().map(|h| format!("## {}\nx\n", h)).collect();
        fs::write(&f, &full).unwrap();
        assert_eq!(run(&args(&["prereg", "check", f.to_str().unwrap()])), 0);
        assert_eq!(run(&args(&["prereg", "check", "/nonexistent-prereg.md"])), 2);
        assert_eq!(run(&args(&["prereg", "digest", t.path().to_str().unwrap()])), 0);
        assert_eq!(run(&args(&["prereg"])), 2);
    }

    #[test]
    fn package_records_the_retrieval_numbers_without_changing_the_bundle_hash_and_never_blocks() {
        let home = tempfile::tempdir().unwrap();
        let t = tempfile::tempdir().unwrap();
        copy(&golden(), t.path());
        delphi_bundle::write_manifest(t.path(), true, &Real).unwrap();
        let hash_before = delphi_bundle::validate(t.path(), &Options { claude: true, packaging: true, ..Options::default() }, &Real).bundle_hash;
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let out = t.path().parent().unwrap().join("evalpack.xbundle");
        let code = run(&args(&["package", t.path().to_str().unwrap(), "--home", home.path().to_str().unwrap(), "-o", out.to_str().unwrap()]));
        assert_eq!(code, 0, "advisory: numbers below the thresholds would not block either");
        let rec = fs::read_to_string(t.path().join(".delphi-bundle/eval.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&rec).unwrap();
        assert!(v["recall_at_10"].is_number() && v["mrr"].is_number() && v["mode"] == "lexical" && v["embedder"].is_null(), "{}", rec);
        let hash_after = delphi_bundle::validate(t.path(), &Options { claude: true, packaging: true, ..Options::default() }, &Real).bundle_hash;
        assert_eq!(hash_before, hash_after, "eval.json is not listed in files.json");
        assert!(delphi_engine::bundle_eval::summary_line(&rec).unwrap().contains("publisher-reported"));
        let inspected = delphi_bundle::inspect(t.path(), &Options { claude: true, ..Options::default() }, &Real);
        assert_eq!(inspected.summary.eval_record.as_deref(), Some(rec.as_str()), "the add summary can show it");
        // --no-eval leaves no record
        fs::remove_file(t.path().join(".delphi-bundle/eval.json")).unwrap();
        assert_eq!(run(&args(&["package", t.path().to_str().unwrap(), "--no-eval", "--home", home.path().to_str().unwrap(), "-o", out.to_str().unwrap()])), 0);
        assert!(!t.path().join(".delphi-bundle/eval.json").exists());
    }

    #[test]
    fn package_vectors_refuses_without_a_model_and_a_bundle_that_is_not_well_formed() {
        let home = tempfile::tempdir().unwrap();
        let t = tempfile::tempdir().unwrap();
        copy(&golden(), t.path());
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // no model is installed under a fresh home (and none is embedded in a test build)
        let code = run(&args(&["package", t.path().to_str().unwrap(), "--vectors", "--home", home.path().to_str().unwrap(), "-o", t.path().parent().unwrap().join("x.xbundle").to_str().unwrap()]));
        assert_eq!(code, 1);
        assert!(!t.path().join("vectors").exists(), "nothing was written");
        fs::write(t.path().join("bundle.toml"), "not toml [").unwrap();
        assert!(bundle_pack::add_vectors(t.path(), &Real, &Words, &mut |_, _| {}).unwrap_err().contains("not well-formed"));
    }

    #[test]
    fn validate_report_writes_the_report_outside_the_bundle_and_refuses_a_path_inside_it() {
        let t = tempfile::tempdir().unwrap();
        copy(&golden(), t.path());
        let out = tempfile::tempdir().unwrap();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let d = t.path().to_str().unwrap();
        let report = out.path().join("redaction.md");
        assert_eq!(run(&args(&["validate", d, "--claude", "--report", report.to_str().unwrap()])), 0);
        let md = fs::read_to_string(&report).unwrap();
        assert!(md.starts_with("# Redaction report: acme/acme-platform 1.4.0") && md.contains("| – | none |"), "{}", md);
        // inside the bundle, directly or through a subdirectory, it would be packaged
        for inside in [t.path().join("report.md"), t.path().join("knowledge/report.md")] {
            assert_eq!(run(&args(&["validate", d, "--report", inside.to_str().unwrap()])), 2);
            assert!(!inside.exists());
        }
        // a symlinked parent that leads back into the bundle is the same place
        let link = out.path().join("back");
        std::os::unix::fs::symlink(t.path().join("knowledge"), &link).unwrap();
        assert_eq!(run(&args(&["validate", d, "--report", link.join("r.md").to_str().unwrap()])), 2);
        assert!(!t.path().join("knowledge/r.md").exists());
    }

    #[test]
    fn a_named_workspaces_identifiers_and_handles_become_warnings_and_an_unknown_workspace_is_an_error() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("config.toml"), "[privacy]\nidentifiers = [\"project-heron\"]\nhandles = [\"jessevondoom\"]\n").unwrap();
        let t = tempfile::tempdir().unwrap();
        copy(&golden(), t.path());
        let p = t.path().join("knowledge/glossary.md");
        fs::write(&p, fs::read_to_string(&p).unwrap() + "\nHeron is project-heron, kept by jessevondoom.\n").unwrap();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let a = args(&["validate", t.path().to_str().unwrap(), "--workspace", "w", "--home", home.path().to_str().unwrap()]);
        let host = host_for(&a).unwrap();
        let r = delphi_bundle::validate(t.path(), &Options::default(), &*host);
        assert!(r.warns.iter().any(|w| w.step == 8 && w.msg.contains("project-heron")) && r.warns.iter().any(|w| w.step == 8 && w.msg.contains("jessevondoom")), "{}", r.render("x"));
        let none = args(&["validate", t.path().to_str().unwrap(), "--workspace", "nope", "--home", home.path().to_str().unwrap()]);
        assert_eq!(run(&none), 2);
    }

    #[test]
    fn init_through_the_command_line_makes_a_bundle_that_validates_and_never_overwrites() {
        let t = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let dir = t.path().join("kit");
        let d = dir.to_str().unwrap();
        let h = home.path().to_str().unwrap();
        assert_eq!(run(&args(&["init", d, "--org", "acme", "--name", "kit", "--home", h])), 0);
        assert_eq!(run(&args(&["validate", d])), 0);
        assert_eq!(run(&args(&["init", d, "--org", "acme", "--name", "kit", "--home", h])), 2, "a second init refuses");
        assert_eq!(run(&args(&["init", d, "--home", h])), 2, "--org and --name are required");
    }
}
