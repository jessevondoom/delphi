//! `delphi bundle import|export` (`specs/025` §7): a Claude Code plugin in, a plugin, an `AGENTS.md` fragment or a
//! marketplace out. Import reads a directory and writes a new one; it never adds. Export reads a bundle that `add` already
//! verified and cached, and checks it is still that bundle before it writes anything.

use crate::bundle_cmd::{flag_values, Real};
use delphi_bundle::import::{self, ImportOpts};
use delphi_bundle::marketplace::{self, Listing, Published};
use delphi_bundle::Options;
use delphi_core::bundles::{self, Entry};
use std::path::{Path, PathBuf};

fn flag(args: &[String], name: &str) -> Option<String> {
    flag_values(args, name).into_iter().next()
}

fn usage(msg: &str) -> i32 {
    eprintln!("delphi bundle: {}", msg);
    2
}

/// `delphi bundle import <plugin dir> [-o <new dir>] [--as <name>] [--org <org>] [--agents <role,role>] [--agent-role <agent>=<role>]...`
pub fn import(dir: &Path, args: &[String]) -> i32 {
    let mut agent_roles = std::collections::BTreeMap::new();
    for v in flag_values(args, "--agent-role") {
        let Some((a, r)) = v.split_once('=') else { return usage("--agent-role wants <agent>=<role>") };
        agent_roles.insert(a.to_string(), r.to_string());
    }
    let opts = ImportOpts {
        name: flag(args, "--as"),
        org: flag(args, "--org"),
        skill_roles: flag(args, "--agents").map(|a| a.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default(),
        agent_roles,
    };
    let out = flag(args, "-o").or_else(|| flag(args, "--out")).map(PathBuf::from).unwrap_or_else(|| {
        let base = dir.canonicalize().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "plugin".into());
        PathBuf::from(format!("{}-bundle", base))
    });
    match import::import(dir, &out, &opts, &Real) {
        Err(why) => {
            for w in &why {
                eprintln!("delphi bundle import: {}", w);
            }
            eprintln!("nothing was imported");
            1
        }
        Ok(done) => {
            println!("imported {} as {}/{} into {}\n", dir.display(), done.org, done.name, out.display());
            print!("{}", done.render_table());
            println!();
            print!("{}", done.report.render(&out.display().to_string()));
            if done.report.passed() {
                println!("\nnext: read {}/README.md and bundle.toml, then `delphi bundle add {}`", out.display(), out.display());
                0
            } else {
                println!("\nthe directory is written but does not validate yet; fix what is listed, run `delphi bundle manifest {} --claude`, then validate again", out.display());
                1
            }
        }
    }
}

/// The active bundle `name` and its cached tree, checked to be the bytes `add` verified.
fn cached(home: &Path, ws: &Path, name: &str) -> Result<(Entry, PathBuf), String> {
    let lock = bundles::read(ws)?;
    let mut hits = lock.bundle.iter().filter(|e| e.is_active() && (e.name == name || e.id() == name));
    let entry = hits.next().ok_or_else(|| format!("no bundle `{}` is added to this workspace (`delphi bundle list`)", name))?.clone();
    if hits.next().is_some() {
        return Err(format!("several added bundles are called `{}`; say org/name", name));
    }
    let dir = bundles::cache_dir(home, &entry.org, &entry.name, &entry.hash);
    let i = delphi_bundle::inspect(&dir, &Options::default(), &Real);
    if !i.report.passed() || i.hash.as_deref() != Some(entry.hash.as_str()) {
        return Err(format!("the cached copy of {} no longer matches what was added; nothing was exported (re-add it)", entry.id()));
    }
    Ok((entry, dir))
}

/// True when `child` is `parent` or below it, after resolving what exists (an output path that does not exist yet is
/// compared by its nearest existing ancestor).
fn inside(child: &Path, parent: &Path) -> bool {
    let resolve = |p: &Path| -> PathBuf {
        let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(p) };
        let mut tail = Vec::new();
        let mut cur = abs.as_path();
        loop {
            if let Ok(c) = cur.canonicalize() {
                return tail.iter().rev().fold(c, |acc, t| acc.join(t));
            }
            match (cur.parent(), cur.file_name()) {
                (Some(par), Some(n)) => {
                    tail.push(n.to_os_string());
                    cur = par;
                }
                _ => return abs,
            }
        }
    };
    resolve(child).starts_with(resolve(parent))
}

/// `delphi bundle export <name> --claude-plugin <dir> [--strip-delphi] | --agents-md [<file>] | <name>... --marketplace <dir> …`
pub fn export(home: &Path, ws: &Path, names: &[String], args: &[String]) -> i32 {
    let flag_set = |f: &str| args.iter().any(|a| a == f);
    let modes = ["--claude-plugin", "--agents-md", "--marketplace"].iter().filter(|m| flag_set(m)).count();
    if modes != 1 {
        return usage("export wants exactly one of --claude-plugin <dir>, --agents-md [<file>] or --marketplace <dir>");
    }
    if flag_set("--marketplace") {
        return export_marketplace(home, ws, names, args);
    }
    if names.len() != 1 {
        return usage("this export takes one bundle name");
    }
    let (entry, dir) = match cached(home, ws, &names[0]) {
        Ok(v) => v,
        Err(e) => return crate::bundle_add::fail(e),
    };
    if flag_set("--claude-plugin") {
        let Some(out) = flag(args, "--claude-plugin") else { return usage("--claude-plugin wants an output directory") };
        let out = PathBuf::from(out);
        if inside(&out, &dir) || inside(&dir, &out) {
            return crate::bundle_add::fail("the output directory and the bundle cache must not contain one another");
        }
        let strip = flag_set("--strip-delphi");
        return match delphi_bundle::export::claude_plugin(&dir, &out, strip, &Real) {
            Ok(notes) => {
                println!("wrote {} as a Claude Code plugin{} in {}", entry.id(), if strip { " (Delphi-only content removed)" } else { "" }, out.display());
                for n in notes {
                    println!("  note: {}", n);
                }
                println!("  try it: claude --plugin-dir {}", out.display());
                0
            }
            Err(e) => crate::bundle_add::fail(e),
        };
    }
    // --agents-md, optionally with a path; the default is under the Delphi home, never in a project.
    let given = flag(args, "--agents-md").filter(|p| !p.starts_with("--"));
    let target = match given {
        Some(p) => {
            let p = PathBuf::from(p);
            if p.is_dir() { p.join("AGENTS.delphi.md") } else { p }
        }
        None => home.join("exports").join(&entry.org).join(&entry.name).join("AGENTS.delphi.md"),
    };
    if inside(&target, &dir) {
        return crate::bundle_add::fail("the output must not be inside the bundle cache");
    }
    let (text, notes) = match delphi_bundle::export::agents_md(&dir, &Real) {
        Ok(v) => v,
        Err(e) => return crate::bundle_add::fail(e),
    };
    if std::fs::symlink_metadata(&target).is_ok_and(|m| !m.is_file()) {
        return crate::bundle_add::fail(format!("{} exists and is not a regular file", target.display()));
    }
    if let Err(e) = target.parent().map(std::fs::create_dir_all).transpose().and_then(|_| std::fs::write(&target, text)) {
        return crate::bundle_add::fail(format!("{}: {}", target.display(), e));
    }
    println!("wrote {}", target.display());
    for n in notes {
        println!("  note: {}", n);
    }
    println!("  Delphi never commits this file; add it to a project yourself if you want it there.");
    0
}

#[cfg(test)]
#[path = "bundle_compat_tests.rs"]
mod tests;

fn export_marketplace(home: &Path, ws: &Path, names: &[String], args: &[String]) -> i32 {
    if names.is_empty() {
        return usage("--marketplace wants the names of the bundles to list");
    }
    let (Some(out), Some(repo), Some(sha)) = (flag(args, "--marketplace"), flag(args, "--repo"), flag(args, "--sha")) else {
        return usage("--marketplace wants <dir>, --repo <owner/repo|https url> and --sha <commit> (an entry without a pinned commit is refused by readers)");
    };
    let mut found = Vec::new();
    for n in names {
        match cached(home, ws, n) {
            Ok(v) => found.push(v),
            Err(e) => return crate::bundle_add::fail(e),
        }
    }
    let mut listings = Vec::new();
    let mut metas = Vec::new();
    for (entry, dir) in &found {
        let r = delphi_bundle::validate(dir, &Options::default(), &Real);
        let Some(m) = r.manifest else { return crate::bundle_add::fail(format!("{} has no readable bundle.toml", entry.id())) };
        metas.push((entry.hash.clone(), m));
    }
    for (hash, m) in &metas {
        let mut tags = vec![m.org.clone()];
        tags.extend(m.keywords.iter().cloned());
        listings.push(Listing { name: &m.name, version: &m.version, description: &m.description, tags, bundle_hash: hash });
    }
    let owner = flag(args, "--owner").unwrap_or_else(|| metas[0].1.publisher_name.clone());
    let mname = flag(args, "--marketplace-name").unwrap_or_else(|| format!("{}-bundles", metas[0].1.org));
    let (git_ref, subdir) = (flag(args, "--ref"), flag(args, "--subdir"));
    let published = Published { marketplace_name: &mname, owner: &owner, repo: &repo, git_ref: git_ref.as_deref(), sha: &sha, subdir: subdir.as_deref() };
    let json = match marketplace::write(&published, &listings) {
        Ok(j) => j,
        Err(e) => return crate::bundle_add::fail(e),
    };
    let file = Path::new(&out).join(".claude-plugin/marketplace.json");
    if std::fs::symlink_metadata(&file).is_ok_and(|m| !m.is_file()) {
        return crate::bundle_add::fail(format!("{} exists and is not a regular file", file.display()));
    }
    if let Err(e) = std::fs::create_dir_all(file.parent().unwrap()).and_then(|_| std::fs::write(&file, json)) {
        return crate::bundle_add::fail(format!("{}: {}", file.display(), e));
    }
    println!("wrote {} ({} bundle(s), each pinned to commit {})", file.display(), names.len(), &sha[..12.min(sha.len())]);
    println!("  publish the bundles' trees at that commit; `delphi bundle add --from-marketplace <repo> <name>` fetches exactly it.");
    0
}
