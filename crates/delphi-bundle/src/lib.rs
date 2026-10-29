//! The org bundle standard (`specs/023`): the directory layout, the `bundle.toml` manifest and `delphi bundle validate`.
//!
//! Everything here is a pure function of a directory tree. Nothing touches the network or a model, and nothing is
//! executed. The checks that live in the engine (provenance frontmatter, the skill standard, the built-in role scopes)
//! come in through [`Host`], so this crate does not depend on `delphi-engine` and can be fuzzed alone.
//!
//! [`validate`] runs the fifteen steps of `specs/023` §10 in order and fails closed: every finding names its step and
//! the file it is about. The same function runs again at `add` (nothing trusts a publisher's "it passed").

pub mod archive;
mod agents;
mod behavior;
pub mod changes;
pub mod claude_plugin;
pub mod export;
pub mod import;
mod files;
mod frontmatter;
mod ids;
pub mod load;
pub mod mcp_decl;
mod manifest;
pub mod marketplace;
pub mod provenance;
mod relax;
pub mod redaction;
pub mod scan;
mod skills;
pub mod sources;
mod prereg;
pub mod source_check;
mod summary;
mod tree;
mod vectors;

pub use files::{BUNDLE_HASH_PREFIX, build_files_json, bundle_hash};
pub use manifest::{plugin_json, prereg_missing, Manifest, PREREG_HEADINGS};
pub use prereg::{digest as prereg_digest, pin as prereg_pin, section as prereg_section};
pub use relax::{no_relaxation, no_relaxation_doc};
pub use summary::{inspect, Inspected, Summary};

/// The structured ids a bundle directory promises to keep stable (knowledge, skills, agents, goals, perspectives),
/// each prefixed by its kind. Two added bundles must not share one (`specs/024` §7.1).
pub fn ids_of(dir: &Path) -> std::collections::BTreeSet<String> {
    ids::collect(&Tree::open(dir))
}
pub use tree::Tree;

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// The roles a bundle may name (D-17), plus `librarian` for guidance once that role is built (`specs/023` §6.3).
pub const ROLES: [&str; 11] = ["orchestrator", "builder", "architect", "investigator", "scout", "strategist", "red-team", "writer", "integrator", "librarian", "validator"];

/// The bundle schema major this build reads (`specs/023` §9).
pub const SCHEMA: u32 = 1;

/// What a role's built-in manifest lets it do, for the `required_permissions` fit of a bundle skill (`specs/023` §5A).
#[derive(Debug, Default, Clone)]
pub struct RoleScope {
    pub read: Vec<String>,
    pub write: Vec<String>,
    pub execute: Vec<String>,
    /// The tools the role may use, for intersecting an agent's `tools` list (`specs/023` §5B).
    pub tools: Vec<String>,
}

/// The checks that need the engine. The command line wires the real ones; a test can fake any of them.
pub trait Host {
    /// Errors from the provenance frontmatter schema of `specs/001` §5 for one document's text.
    fn provenance(&self, text: &str) -> Vec<String>;
    /// Errors from the Agent Skills standard for one `SKILL.md`'s frontmatter, with Delphi's own caps (name equals `dir`).
    fn skill_errors(&self, fm: &serde_yaml::Mapping, dir: &str) -> Vec<String>;
    /// The built-in scope of a canonical role, or `None` when this host does not know it (the fit check then notes that).
    fn role_scope(&self, role: &str) -> Option<RoleScope>;
    /// The keys of every embedding input the install's chunker would make from these text files (`path` to text) of
    /// bundle `org/name`, or `None` when this build cannot chunk (the vector reachability check then says so and does
    /// not guess). The org and name are part of the key: the index files a bundle's chunks under its own path.
    fn chunk_keys(&self, _org: &str, _name: &str, _texts: &BTreeMap<String, String>) -> Option<HashSet<[u8; 32]>> {
        None
    }
    /// Identifiers of this workspace that a bundle should not carry (`specs/004` §5.4); empty outside a workspace.
    fn identifiers(&self) -> Vec<String> {
        Vec::new()
    }
    /// Work handles on the roster; a hit is a warning (`specs/023` §10 step 8).
    fn roster_handles(&self) -> Vec<String> {
        Vec::new()
    }
}

/// What to check and how strictly.
#[derive(Debug, Default, Clone)]
pub struct Options {
    /// `package` time: a `review_by` in the past is an error rather than a warning.
    pub packaging: bool,
    /// `--claude`: step 13, the `plugin.json` agreement check.
    pub claude: bool,
    /// `--against <dir>`: the previous version's tree, for the smallest honest version bump.
    pub against: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub step: u8,
    pub at: String,
    pub msg: String,
}

/// The verdict: failures stop the bundle, warnings do not, notes are facts worth reporting.
#[derive(Debug, Default, Clone)]
pub struct Report {
    pub fails: Vec<Finding>,
    pub warns: Vec<Finding>,
    pub notes: Vec<Finding>,
    /// `blake3:<hex>` of `.delphi-bundle/files.json` when it is present and the bundle's identity is known.
    pub bundle_hash: Option<String>,
    pub manifest: Option<Manifest>,
    /// The `did:key` whose signature over the bundle verified at step 6; `None` for an unsigned bundle (or a failed one).
    pub signer: Option<String>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.fails.is_empty()
    }

    pub(crate) fn fail(&mut self, step: u8, at: &str, msg: impl Into<String>) {
        self.fails.push(Finding { step, at: at.into(), msg: msg.into() });
    }

    pub(crate) fn warn(&mut self, step: u8, at: &str, msg: impl Into<String>) {
        self.warns.push(Finding { step, at: at.into(), msg: msg.into() });
    }

    pub(crate) fn note(&mut self, step: u8, at: &str, msg: impl Into<String>) {
        self.notes.push(Finding { step, at: at.into(), msg: msg.into() });
    }

    /// One line per finding under a header that says pass or fail.
    pub fn render(&self, target: &str) -> String {
        let mut out = format!("bundle {}: {}\n", target, if self.passed() { "pass" } else { "FAIL" });
        for (tag, items) in [("fail", &self.fails), ("warn", &self.warns), ("note", &self.notes)] {
            for f in items {
                let at = if f.at.is_empty() { String::new() } else { format!("{}: ", f.at) };
                out.push_str(&format!("[{}] step {}: {}{}\n", tag, f.step, at, f.msg));
            }
        }
        if let Some(h) = &self.bundle_hash {
            out.push_str(&format!("bundle hash {}\n", h));
        }
        out
    }
}

/// Validate the bundle at `dir` (`specs/023` §10).
pub fn validate(dir: &Path, opts: &Options, host: &dyn Host) -> Report {
    let mut r = Report::default();
    // A schema major this build does not know is refused alone: nothing else about the bundle can be trusted (AC-11).
    if let Some(msg) = manifest::unsupported_schema(dir) {
        r.fail(4, "bundle.toml", msg);
        return r;
    }
    let tree = tree::scan(dir, &mut r);
    let m = manifest::check(&tree, opts, &mut r);
    files::check(&tree, m.as_ref(), &mut r);
    files::check_signature(&tree, m.as_ref(), &mut r);
    frontmatter::check(&tree, m.as_ref(), host, &mut r);
    sources::check(&tree, m.as_ref(), &mut r);
    scan::check(&tree, m.as_ref(), host, &mut r);
    skills::check(&tree, m.as_ref(), host, &mut r);
    agents::check(&tree, m.as_ref(), host, &mut r);
    behavior::check(&tree, m.as_ref(), host, &mut r);
    relax::check_knowledge(&tree, &mut r);
    sources::check_instructions(&tree, &mut r);
    vectors::check(&tree, m.as_ref(), host, &mut r);
    mcp_decl::check(&tree, &mut r);
    claude_plugin::check(&tree, m.as_ref(), opts.claude, &mut r);
    manifest::check_evals(&tree, m.as_ref(), &mut r);
    if let Some(prev) = &opts.against {
        manifest::check_against(&tree, m.as_ref(), prev, host, &mut r);
    }
    r.manifest = m;
    r
}

/// `delphi bundle manifest`: write `.delphi-bundle/files.json` (and, with `claude`, the generated
/// `.claude-plugin/plugin.json`), and return the bundle hash. Refuses a bundle that fails steps 1 to 4, because a hash
/// of a malformed tree would only make it look finished. `plugin.json` is written first and carries no bundle hash,
/// since the hash covers `plugin.json` itself.
pub fn write_manifest(dir: &Path, claude: bool, host: &dyn Host) -> Result<String, String> {
    let early = validate(dir, &Options::default(), host);
    let blocking: Vec<String> = early.fails.iter().filter(|f| f.step <= 4).map(|f| format!("step {}: {}{}", f.step, if f.at.is_empty() { String::new() } else { format!("{}: ", f.at) }, f.msg)).collect();
    if !blocking.is_empty() {
        return Err(format!("the bundle is not well-formed yet:\n  {}", blocking.join("\n  ")));
    }
    let m = early.manifest.ok_or("no manifest")?;
    let write = |rel: &str, text: &str| -> Result<(), String> {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| format!("{}: {}", rel, e))?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {}", rel, e))
    };
    if claude {
        write(".claude-plugin/plugin.json", &manifest::plugin_json(&m, None))?;
    }
    let mut scratch = Report::default();
    let tree = tree::scan(dir, &mut scratch);
    let json = files::build_files_json(&tree, &m);
    write(".delphi-bundle/files.json", &json)?;
    Ok(files::bundle_hash(json.as_bytes()))
}

#[cfg(test)]
mod compat_tests;
#[cfg(test)]
mod tests;
