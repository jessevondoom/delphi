//! Artifact tools (`specs/014` §7): deliverables that are not Markdown text, such as a zip bundle now and
//! decks, pages, charts and spreadsheets later. They share one write path so every artifact gets the same
//! guarantees:
//!
//! - it lands in the mission's `artifacts/` folder, or under `@worktree/` when the path says so. Role manifests
//!   differ in what they may write, so when a role has no `artifacts/` folder the first mission folder it may
//!   write (`reports`, `research`, `specs`, ...) is used instead; no manifest is widened to make this work;
//! - the role's write scope, the provenance gate and a 4 MB size cap are checked *before* anyone is asked;
//! - a **sidecar** `<name>.provenance.md` records who wrote it, with which tool, from what, and at what trust
//!   tier (a binary cannot carry the front matter `write_file` embeds), and the file is stamped in the
//!   provenance index at the span's ceiling, so an artifact built from untrusted input stays untrusted.

use crate::tools::{denied, observe, str_arg, with_provenance_header, zone_of, ActionKind, Classified, Stamp, ToolCtx, ToolError, ToolExec, ToolResult, ToolSpec, ToolTier};
use delphi_adapter::claude::ToolDef;
use delphi_core::pathguard::{Access, Guard};
use delphi_core::provenance::{gate_write, OutputCheck, Tier};
use serde_json::{json, Value};

/// Output cap for an artifact; plain-text `write_file` stays at 256 KB.
pub const MAX_ARTIFACT_BYTES: usize = 4 << 20;
const MAX_MEMBERS: usize = 200;
/// Directories an agent could mistake for a place to put a deliverable.
const OTHER_ROOTS: [&str; 6] = ["specs", "context", "memory", "skills", "agents", "themes"];
/// Mission folders tried, in order, for a plain path; the first one the role may write wins.
const MISSION_FOLDERS: [&str; 7] = ["artifacts", "reports", "research", "specs", "comms", "councils", "threat-models"];

const TOOLS: [&str; 1] = ["write_archive"];

pub fn is_artifact_tool(name: &str) -> bool {
    TOOLS.contains(&name)
}

fn extensions(tool: &str) -> &'static [&'static str] {
    match tool {
        "write_archive" => &["zip"],
        _ => &[],
    }
}

/// The folder a plain path goes in: the first of [`MISSION_FOLDERS`] this role may write for the mission.
pub fn default_dir(guard: &Guard, mission_id: &str) -> Option<String> {
    MISSION_FOLDERS.iter().map(|d| format!("missions/{}/{}", mission_id, d)).find(|dir| guard.check_policy(&format!("{}/x.zip", dir), Access::Write).is_ok())
}

/// Where an artifact goes: the path as given if it names `missions/…` or `@worktree/…`, otherwise inside `base`
/// (the mission folder from [`default_dir`]). The tool's extension is added when missing and enforced when present.
pub fn resolve_path(base: Option<&str>, raw: &str, tool: &str) -> Result<String, String> {
    let p = raw.trim().trim_start_matches("./");
    if p.is_empty() {
        return Err("the path is empty".into());
    }
    if p.len() > 300 || p.contains('\\') || p.chars().any(|c| c.is_control()) || p.split('/').any(|s| s == ".." || s.is_empty()) {
        return Err(format!("`{}` is not a usable artifact path (no `..`, empty segments, backslashes or control characters)", raw.chars().take(80).collect::<String>()));
    }
    let lower = p.to_lowercase();
    let full = if lower.starts_with("missions/") || lower.starts_with("@worktree/") {
        p.to_string()
    } else if p.starts_with('@') || p.starts_with('/') || p.starts_with('~') || OTHER_ROOTS.contains(&lower.split('/').next().unwrap_or("")) {
        return Err("an artifact is written into the mission's folder (give a plain path like `report.zip`) or under `@worktree/`".into());
    } else {
        match base {
            Some(b) => format!("{}/{}", b, p),
            None => return Err("this role may not write into any folder of the mission, so a plain path has nowhere to go; use a path under `@worktree/` if its manifest allows".into()),
        }
    };
    if full.to_lowercase().ends_with(".provenance.md") {
        return Err("names ending in `.provenance.md` are reserved for provenance records".into());
    }
    let exts = extensions(tool);
    let file = full.rsplit('/').next().unwrap_or("");
    match file.rsplit_once('.') {
        Some((stem, e)) if !stem.is_empty() => {
            if !exts.contains(&e.to_lowercase().as_str()) {
                return Err(format!("{} writes .{} files; give the path a .{} name", tool, exts[0], exts[0]));
            }
            Ok(full)
        }
        _ => Ok(format!("{}.{}", full, exts[0])),
    }
}

/// The record that travels with an artifact. For `missions/…` it sits beside the file. For `@worktree/…` it is
/// kept in the mission folder, so a provenance file never lands in the project's repository; a role with no
/// mission folder to write gets none, and the stamp in the mission log is the record.
pub fn sidecar_path(base: Option<&str>, resolved: &str) -> Option<String> {
    if resolved.starts_with("missions/") {
        Some(format!("{}.provenance.md", resolved))
    } else {
        base.map(|b| format!("{}/provenance/{}.provenance.md", b, resolved.trim_start_matches('@').replace('/', "__")))
    }
}

fn members_of(args: &Value) -> Result<Vec<String>, ToolError> {
    let arr = args.get("members").and_then(|m| m.as_array()).ok_or_else(|| ToolError("missing array argument `members`".into()))?;
    if arr.is_empty() || arr.len() > MAX_MEMBERS {
        return Err(ToolError(format!("`members` must list between 1 and {} files", MAX_MEMBERS)));
    }
    arr.iter()
        .map(|m| m.as_str().filter(|s| !s.trim().is_empty() && s.len() <= 300).map(|s| s.trim().to_string()).ok_or_else(|| ToolError("every member must be a non-empty path string".into())))
        .collect()
}

/// What the call would do, from the arguments alone (`specs/014` §7.2): Green in the mission, Amber in a worktree.
fn classify(tool: &str, args: &Value) -> Result<Classified, ToolError> {
    let path = str_arg(args, "path")?;
    let members = if tool == "write_archive" { Some(members_of(args)?) } else { None };
    let resolved = resolve_path(Some("missions/<mission>/artifacts"), path, tool).map_err(ToolError)?;
    let (tier, kind) = if resolved.starts_with("@worktree/") { (ToolTier::Amber, ActionKind::WorktreeEdit) } else { (ToolTier::Green, ActionKind::MissionArtifact) };
    let dir = resolved.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let args_hash = blake3::hash(serde_json::to_string(args).unwrap_or_default().as_bytes()).to_hex().to_string();
    let summary = match &members {
        Some(m) => format!("write {} (a zip of {} file{})", path, m.len(), if m.len() == 1 { "" } else { "s" }),
        None => format!("write {}", path),
    };
    Ok(Classified { tool: tool.into(), tier, kind, summary, payload: json!({"tool": tool, "path": path, "members": members, "args_blake3": args_hash}), pattern: format!("{} {}/*", tool, dir) })
}

/// Where a call will write: the artifact and, if there is one, its provenance record.
pub struct Target {
    pub path: String,
    pub sidecar: Option<String>,
}

/// Everything that can refuse an artifact without touching the filesystem, run before the operator is asked:
/// the path, the role's write scope (for the file and its provenance record), the provenance gate, and read
/// access to each input.
pub fn precheck(guard: &Guard, ceiling: Tier, mission_id: &str, tool: &str, args: &Value) -> Result<Target, ToolResult> {
    let path = str_arg(args, "path").map_err(|e| ToolResult::err(e.0))?;
    let base = default_dir(guard, mission_id);
    let resolved = resolve_path(base.as_deref(), path, tool).map_err(ToolResult::err)?;
    let sidecar = sidecar_path(base.as_deref(), &resolved);
    for p in std::iter::once(&resolved).chain(sidecar.iter()) {
        guard.check_policy(p, Access::Write).map_err(denied)?;
        gate_write(p, ceiling).map_err(|g| ToolResult::err(g.to_string()))?;
    }
    if tool == "write_archive" {
        for m in members_of(args).map_err(|e| ToolResult::err(e.0))? {
            guard.check_policy(&m, Access::Read).map_err(denied)?;
        }
    }
    Ok(Target { path: resolved, sidecar })
}

/// Write `bytes` as the artifact, with its provenance record, and stamp both.
pub fn write_artifact(ctx: &mut ToolCtx, target: &Target, bytes: &[u8], tool: &str, inputs: &[String]) -> ToolResult {
    let resolved = target.path.as_str();
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return ToolResult::err(format!("the artifact is {} bytes; the limit is {} (4 MB)", bytes.len(), MAX_ARTIFACT_BYTES));
    }
    let created = match ctx.guard.write_atomic(resolved, bytes) {
        Ok(c) => c,
        Err(d) => return denied(d),
    };
    let tier = ctx.span.output_tier(OutputCheck::Unverified);
    let hash = blake3::hash(bytes).to_hex().to_string();
    let (zone, rel) = zone_of(resolved);
    ctx.index.stamp(zone, &rel, &hash, tier);
    ctx.stamps.push(Stamp { zone, path: rel, blake3: hash.clone(), tier });
    let head = format!("{} {} ({} bytes, blake3 {}…, tier {} {})", if created { "created" } else { "replaced" }, resolved, bytes.len(), &hash[..12], tier, tier.name());
    let Some(side) = &target.sidecar else {
        return ToolResult::ok(format!("{}; no provenance record file (this role cannot write the mission folder): the stamp is in the mission log", head));
    };

    let name = resolved.rsplit('/').next().unwrap_or(resolved);
    let mut body = format!(
        "---\nartifact: {name}\nartifact_path: \"{resolved}\"\nartifact_blake3: {hash}\nartifact_bytes: {n}\nartifact_tier: \"{tier} {tname}\"\nproduced_by: {tool}\ntags: [artifact, provenance]\n---\n# Provenance of {name}\n\nWritten by role `{role}` in mission `{mission}` with `{tool}` at {now}.\n\nTrust: {tier} {tname}. The tier is the lowest-trust input that entered the span that wrote this file; it is computed, not declared, and an agent cannot raise it.\n",
        name = name,
        resolved = resolved,
        hash = hash,
        n = bytes.len(),
        tier = tier,
        tname = tier.name(),
        tool = tool,
        role = ctx.role,
        mission = ctx.mission_id,
        now = ctx.now_rfc3339,
    );
    if !inputs.is_empty() {
        body.push_str("\nInputs:\n");
        for i in inputs {
            body.push_str(&format!("- {}\n", i));
        }
    }
    let sidecar = match with_provenance_header(&body, ctx.role, ctx.mission_id, ctx.now_rfc3339, side) {
        Ok(s) => s,
        Err(e) => return ToolResult::err(format!("the artifact was written but its provenance record could not be: {}", e)),
    };
    if let Err(d) = ctx.guard.write_atomic(side, sidecar.as_bytes()) {
        return denied(d);
    }
    let shash = blake3::hash(sidecar.as_bytes()).to_hex().to_string();
    let (szone, srel) = zone_of(side);
    ctx.index.stamp(szone, &srel, &shash, tier);
    ctx.stamps.push(Stamp { zone: szone, path: srel, blake3: shash, tier });
    ToolResult::ok(format!("{}; provenance record: {}", head, side))
}

/// A member's name inside the archive: its logical path without the leading `@`.
fn member_name(logical: &str) -> String {
    delphi_core::pathguard::canonical(logical).unwrap_or_else(|_| logical.to_string()).trim_start_matches('@').to_string()
}

fn write_archive(ctx: &mut ToolCtx, args: &Value) -> ToolResult {
    let target = match precheck(ctx.guard, ctx.span.ceiling(), ctx.mission_id, "write_archive", args) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let members = match members_of(args) {
        Ok(m) => m,
        Err(e) => return ToolResult::err(e.0),
    };
    let mut zip = delphi_docs::ZipWriter::new();
    let (mut total, mut inputs) = (0usize, vec![]);
    for m in &members {
        let (bytes, truncated) = match ctx.guard.read_file(m, MAX_ARTIFACT_BYTES - total + 1) {
            Ok(r) => r,
            Err(d) => return denied(d),
        };
        total += bytes.len();
        if truncated || total > MAX_ARTIFACT_BYTES {
            return ToolResult::err(format!("the files add up to more than {} bytes (4 MB); archive fewer or smaller files", MAX_ARTIFACT_BYTES));
        }
        // What goes into the archive is read by this span: its trust counts against the result.
        observe(ctx, m, &bytes);
        if let Err(e) = zip.add(&member_name(m), &bytes) {
            return ToolResult::err(e.0);
        }
        inputs.push(format!("{} ({} bytes, blake3 {})", m, bytes.len(), blake3::hash(&bytes).to_hex()));
    }
    write_artifact(ctx, &target, &zip.finish(), "write_archive", &inputs)
}

pub fn specs() -> Vec<ToolSpec> {
    vec![ToolSpec {
        def: ToolDef {
            name: "write_archive".into(),
            description: "Bundle files you can already read into a .zip. `path` is where to put it: a plain name like `evidence.zip` goes in the mission's folder (artifacts/, or the first mission folder your role may write); `@worktree/…` writes into the build's worktree. `members` lists the files, by logical path (for example `@project/app/src/lib.rs`, `missions/<id>/notes.md`). At most 200 files and 4 MB in all. Each member is stored under its path without the leading `@`. A provenance record is written beside the archive, and the archive is as trusted as the least trusted file in it.".into(),
            input_schema: json!({"type": "object", "required": ["path", "members"], "properties": {"path": {"type": "string"}, "members": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": MAX_MEMBERS}}}),
        },
        requires_write_scope: true,
        classify: |tool, args| classify(tool, args),
        exec: ToolExec::Local(write_archive),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::AssumeClean;
    use crate::tools::Registry;
    use delphi_core::pathguard::Zones;
    use delphi_core::provenance::{Index, OriginRules, Span};
    use std::collections::HashSet;
    use std::fs;

    struct Fx {
        _t: tempfile::TempDir,
        root: std::path::PathBuf,
        guard: Guard,
        span: Span,
        index: Index,
        rules: OriginRules,
    }

    fn fx(write_globs: &[&str]) -> Fx {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        for d in ["local/missions/m1", "team/specs", "ratified", "base", "proj/src", "proj/vendor/lib", "wt/out"] {
            fs::create_dir_all(r.join(d)).unwrap();
        }
        fs::write(r.join("proj/src/lib.rs"), "fn a() {}\n").unwrap();
        fs::write(r.join("proj/src/other.rs"), "fn b() {}\n").unwrap();
        fs::write(r.join("proj/vendor/lib/x.js"), "vendored code\n").unwrap();
        fs::write(r.join("local/missions/m1/notes.md"), "---\nid: n\n---\nnotes\n").unwrap();
        let guard = Guard {
            zones: Zones { local: r.join("local"), team: r.join("team"), base: r.join("base"), ratified: r.join("ratified"), worktree: Some(r.join("wt")), projects: vec![("app".into(), r.join("proj"))], payload: None },
            role: "architect".into(),
            read_globs: vec!["*".into()],
            write_globs: write_globs.iter().map(|s| s.to_string()).collect(),
            protected: HashSet::new(),
        };
        Fx { root: r.to_path_buf(), _t: t, guard, span: Span::root("sp1"), index: Index::new(), rules: OriginRules::with_defaults() }
    }

    fn run(f: &mut Fx, args: Value) -> (ToolResult, Vec<Stamp>) {
        let mut ctx = ToolCtx { guard: &f.guard, span: &mut f.span, index: &mut f.index, rules: &f.rules, probe: &AssumeClean, role: "architect", mission_id: "m1", now_rfc3339: "2026-10-25T12:00:00.000Z", stamps: vec![] };
        let r = Registry::builtin().run(&mut ctx, "write_archive", &args);
        (r, std::mem::take(&mut ctx.stamps))
    }

    fn doc_names(bytes: &[u8]) -> String {
        delphi_docs::extract(bytes, "x.zip", &delphi_docs::Request::default(), &delphi_docs::Limits::default()).unwrap().text
    }

    #[test]
    fn paths_resolve_to_the_mission_artifacts_directory_and_everything_else_is_refused() {
        let r = |p: &str| resolve_path(Some("missions/m1/artifacts"), p, "write_archive");
        assert_eq!(r("bundle").unwrap(), "missions/m1/artifacts/bundle.zip");
        assert_eq!(r("./out/bundle.ZIP").unwrap(), "missions/m1/artifacts/out/bundle.ZIP");
        assert_eq!(r("missions/m2/artifacts/x.zip").unwrap(), "missions/m2/artifacts/x.zip");
        assert_eq!(r("@worktree/docs/x.zip").unwrap(), "@worktree/docs/x.zip");
        for bad in ["", "../x.zip", "a/../b.zip", "a//b.zip", "a\\b.zip", "/abs.zip", "~/x.zip", "@project/app/x.zip", "specs/x.zip", "skills/x.zip", "agents/x.zip", "bundle.txt", "x.provenance.md", "missions/m1/x.provenance.md"] {
            assert!(r(bad).is_err(), "{:?} was accepted: {:?}", bad, r(bad));
        }
        let base = Some("missions/m1/artifacts");
        assert_eq!(sidecar_path(base, "missions/m1/artifacts/x.zip").as_deref(), Some("missions/m1/artifacts/x.zip.provenance.md"));
        assert_eq!(sidecar_path(base, "@worktree/docs/x.zip").as_deref(), Some("missions/m1/artifacts/provenance/worktree__docs__x.zip.provenance.md"));
        assert_eq!(sidecar_path(None, "@worktree/docs/x.zip"), None);
        assert!(resolve_path(None, "plain.zip", "write_archive").unwrap_err().contains("nowhere to go"));
        assert_eq!(resolve_path(None, "@worktree/x.zip", "write_archive").unwrap(), "@worktree/x.zip");
    }

    #[test]
    fn classification_is_green_in_the_mission_and_amber_in_a_worktree_and_binds_the_arguments() {
        let reg = Registry::builtin();
        let c = reg.classify("write_archive", &json!({"path": "evidence.zip", "members": ["@project/app/src/lib.rs"]})).unwrap();
        assert_eq!((c.tier, c.kind), (ToolTier::Green, ActionKind::MissionArtifact));
        assert!(c.summary.contains("a zip of 1 file") && c.payload["members"][0] == "@project/app/src/lib.rs" && c.payload["args_blake3"].as_str().unwrap().len() == 64, "{:?}", c);
        let w = reg.classify("write_archive", &json!({"path": "@worktree/out/x.zip", "members": ["a", "b"]})).unwrap();
        assert_eq!((w.tier, w.kind, w.pattern.as_str()), (ToolTier::Amber, ActionKind::WorktreeEdit, "write_archive @worktree/out/*"));
        let other = reg.classify("write_archive", &json!({"path": "evidence.zip", "members": ["different"]})).unwrap();
        assert_ne!(c.payload["args_blake3"], other.payload["args_blake3"]);
        for bad in [json!({"path": "specs/x.zip", "members": ["a"]}), json!({"path": "x.zip", "members": []}), json!({"path": "x.zip"}), json!({"path": "x.zip", "members": [""]}), json!({"members": ["a"]}), json!({"path": "x.zip", "members": vec!["a"; 201]})] {
            assert!(reg.classify("write_archive", &bad).is_err(), "{} was accepted", bad);
        }
    }

    #[test]
    fn an_archive_is_written_with_a_provenance_record_and_both_are_stamped() {
        let mut f = fx(&["missions/**"]);
        let (r, stamps) = run(&mut f, json!({"path": "bundle", "members": ["@project/app/src/lib.rs", "@project/app/src/other.rs", "missions/m1/notes.md"]}));
        assert!(!r.is_error, "{}", r.content);
        assert!(r.content.starts_with("created missions/m1/artifacts/bundle.zip (") && r.content.contains("tier L3") && r.content.contains("provenance record: missions/m1/artifacts/bundle.zip.provenance.md"), "{}", r.content);
        let zip = fs::read(f.root.join("local/missions/m1/artifacts/bundle.zip")).unwrap();
        let listing = doc_names(&zip);
        assert!(listing.contains("project/app/src/lib.rs") && listing.contains("project/app/src/other.rs") && listing.contains("missions/m1/notes.md") && !listing.contains("@project"), "{}", listing);
        let side = fs::read_to_string(f.root.join("local/missions/m1/artifacts/bundle.zip.provenance.md")).unwrap();
        let hash = blake3::hash(&zip).to_hex().to_string();
        assert!(side.contains(&format!("artifact_blake3: {}", hash)) && side.contains(&format!("artifact_bytes: {}", zip.len())) && side.contains("author: architect") && side.contains("source_type: internal_generated") && side.contains("source_mission: m1"), "{}", side);
        assert!(side.contains("- @project/app/src/lib.rs (10 bytes, blake3 ") && side.contains("# Provenance of bundle.zip"), "{}", side);
        assert_eq!(stamps.len(), 2);
        assert_eq!(stamps[0].blake3, hash);
        assert!(stamps.iter().all(|s| s.tier == Tier::L3) && stamps[1].path.ends_with("bundle.zip.provenance.md"), "{:?}", stamps);
        // writing it again says so
        let (again, _) = run(&mut f, json!({"path": "bundle", "members": ["@project/app/src/lib.rs"]}));
        assert!(again.content.starts_with("replaced "), "{}", again.content);
    }

    #[test]
    fn an_archive_of_untrusted_files_is_untrusted() {
        let mut f = fx(&["missions/**"]);
        let (r, stamps) = run(&mut f, json!({"path": "mixed.zip", "members": ["@project/app/src/lib.rs", "@project/app/vendor/lib/x.js"]}));
        assert!(!r.is_error && r.content.contains("tier L0 external_untrusted"), "{}", r.content);
        assert_eq!(f.span.ceiling(), Tier::L0);
        assert!(stamps.iter().all(|s| s.tier == Tier::L0), "{:?}", stamps);
        let side = fs::read_to_string(f.root.join("local/missions/m1/artifacts/mixed.zip.provenance.md")).unwrap();
        assert!(side.contains("artifact_tier: L0 external_untrusted") && side.contains("Trust: L0 external_untrusted."), "{}", side);
    }

    #[test]
    fn what_cannot_succeed_is_refused_before_anything_is_written() {
        // no write scope at all
        let mut f = fx(&[]);
        let (r, stamps) = run(&mut f, json!({"path": "x.zip", "members": ["@project/app/src/lib.rs"]}));
        assert!(r.is_error && r.content.contains("nowhere to go") && stamps.is_empty(), "{}", r.content);
        assert!(!f.root.join("local/missions/m1/artifacts").exists());
        // scope for the archive but not for its provenance record
        let mut f = fx(&["missions/m1/artifacts/*.zip"]);
        let (r, _) = run(&mut f, json!({"path": "x.zip", "members": ["@project/app/src/lib.rs"]}));
        assert!(r.is_error, "{}", r.content);
        assert!(!f.root.join("local/missions/m1/artifacts/x.zip").exists(), "no archive without its record");
        // a member outside the read scope, or a missing one, or the same file twice
        let mut f = fx(&["missions/**"]);
        f.guard.read_globs = vec!["@project/**".into()];
        let (r, _) = run(&mut f, json!({"path": "x.zip", "members": ["missions/m1/notes.md"]}));
        assert!(r.is_error && r.content.contains("outside your role's access scope"), "{}", r.content);
        let (r, _) = run(&mut f, json!({"path": "x.zip", "members": ["@project/app/src/nope.rs"]}));
        assert!(r.is_error && r.content.contains("not found"), "{}", r.content);
        let (r, _) = run(&mut f, json!({"path": "x.zip", "members": ["@project/app/src/lib.rs", "@project/app/src/lib.rs"]}));
        assert!(r.is_error && r.content.contains("twice"), "{}", r.content);
        assert!(!f.root.join("local/missions/m1/artifacts/x.zip").exists());
    }

    #[test]
    fn the_size_cap_applies_to_what_goes_in() {
        let mut f = fx(&["missions/**"]);
        fs::write(f.root.join("proj/src/big1.bin"), vec![7u8; 3 << 20]).unwrap();
        fs::write(f.root.join("proj/src/big2.bin"), vec![8u8; 3 << 20]).unwrap();
        let (r, _) = run(&mut f, json!({"path": "big.zip", "members": ["@project/app/src/big1.bin", "@project/app/src/big2.bin"]}));
        assert!(r.is_error && r.content.contains("more than 4194304 bytes"), "{}", r.content);
        let (one, _) = run(&mut f, json!({"path": "big.zip", "members": ["@project/app/src/big1.bin"]}));
        assert!(!one.is_error, "{}", one.content);
        // the cap on the output itself
        let mut ctx = ToolCtx { guard: &f.guard, span: &mut f.span, index: &mut f.index, rules: &f.rules, probe: &AssumeClean, role: "architect", mission_id: "m1", now_rfc3339: "t", stamps: vec![] };
        let target = Target { path: "missions/m1/artifacts/huge.zip".into(), sidecar: None };
        let r = write_artifact(&mut ctx, &target, &vec![0u8; MAX_ARTIFACT_BYTES + 1], "write_archive", &[]);
        assert!(r.is_error && r.content.contains("limit is 4194304"), "{}", r.content);
    }

    #[test]
    fn a_worktree_artifact_keeps_its_provenance_record_with_the_mission_not_in_the_repository() {
        let mut f = fx(&["missions/**", "@worktree/**"]);
        let (r, stamps) = run(&mut f, json!({"path": "@worktree/out/bundle.zip", "members": ["@project/app/src/lib.rs"]}));
        assert!(!r.is_error, "{}", r.content);
        assert!(f.root.join("wt/out/bundle.zip").exists());
        assert!(!f.root.join("wt/out/bundle.zip.provenance.md").exists(), "nothing but the deliverable lands in the worktree");
        assert!(f.root.join("local/missions/m1/artifacts/provenance/worktree__out__bundle.zip.provenance.md").exists());
        assert_eq!(stamps.len(), 2);
    }

    #[test]
    fn a_plain_path_goes_where_this_roles_manifest_allows_and_nowhere_else() {
        // the architect's scope: research and specs subfolders of a mission, not artifacts/
        let mut f = fx(&["missions/**/research/**", "missions/**/specs/**"]);
        assert_eq!(default_dir(&f.guard, "m1").as_deref(), Some("missions/m1/research"));
        let (r, stamps) = run(&mut f, json!({"path": "evidence.zip", "members": ["@project/app/src/lib.rs"]}));
        assert!(!r.is_error && r.content.contains("created missions/m1/research/evidence.zip") && r.content.contains("evidence.zip.provenance.md"), "{}", r.content);
        assert_eq!(stamps.len(), 2);
        // a role with an artifacts folder uses it, ahead of the others
        let f2 = fx(&["missions/**"]);
        assert_eq!(default_dir(&f2.guard, "m1").as_deref(), Some("missions/m1/artifacts"));
        // the builder: only the worktree. A plain path has nowhere to go; a worktree path works, with no record file.
        let mut b = fx(&["@worktree/**"]);
        assert_eq!(default_dir(&b.guard, "m1"), None);
        let (r, _) = run(&mut b, json!({"path": "plain.zip", "members": ["@project/app/src/lib.rs"]}));
        assert!(r.is_error && r.content.contains("nowhere to go"), "{}", r.content);
        let (r, stamps) = run(&mut b, json!({"path": "@worktree/out/bundle.zip", "members": ["@project/app/src/lib.rs"]}));
        assert!(!r.is_error && r.content.contains("no provenance record file") && r.content.contains("the stamp is in the mission log"), "{}", r.content);
        assert!(b.root.join("wt/out/bundle.zip").exists() && !b.root.join("wt/out/bundle.zip.provenance.md").exists());
        assert_eq!(stamps.len(), 1, "the archive itself is still stamped");
        // and a role with no write scope at all is refused whatever the path
        let mut none = fx(&[]);
        let (r, _) = run(&mut none, json!({"path": "@worktree/x.zip", "members": ["@project/app/src/lib.rs"]}));
        assert!(r.is_error, "{}", r.content);
    }

    #[tokio::test]
    async fn through_the_engine_a_mission_artifact_needs_no_card_and_its_stamps_are_logged() {
        use crate::engine::Status;
        use crate::model::Step;
        use crate::testkit::{events_of, ratify_spec, setup};
        let f = setup(vec![Step::tool("a1", "write_archive", json!({"path": "evidence.zip", "members": ["specs/011-retry.md"]})), Step::text("Bundled it.")], 5.0, |_| {});
        ratify_spec(&f);
        let out = f.engine.run_task(&f.m, "architect", "Bundle the retry spec as evidence").await.unwrap();
        assert_eq!(out.status, Status::Done);
        assert!(events_of(&f, "approval.requested").is_empty() && f.engine.broker.pending().is_empty(), "a mission artifact is Green: nobody is asked");
        let zip = f.root.join(format!("workspaces/w/local/missions/{}/research/evidence.zip", f.m.id));
        assert!(zip.exists(), "the architect's manifest allows research/, so a plain path lands there");
        assert!(delphi_docs::extract(&std::fs::read(&zip).unwrap(), "evidence.zip", &delphi_docs::Request::default(), &delphi_docs::Limits::default()).unwrap().text.contains("specs/011-retry.md"));
        let stamped: Vec<String> = events_of(&f, "provenance.stamp").iter().map(|e| e.payload["path"].as_str().unwrap_or("").to_string()).collect();
        assert!(stamped.iter().any(|p| p.ends_with("research/evidence.zip")) && stamped.iter().any(|p| p.ends_with("research/evidence.zip.provenance.md")), "{:?}", stamped);
        let told = f.model.requests()[1].body["messages"].to_string();
        assert!(told.contains("created missions/") && told.contains("evidence.zip"), "{}", told);
    }

    #[tokio::test]
    async fn through_the_engine_a_refusal_comes_back_before_any_card_and_writes_nothing() {
        use crate::model::Step;
        use crate::testkit::{events_of, ratify_spec, setup};
        // the architect may not write into another zone, and `specs/…` is not an artifact folder
        let f = setup(vec![Step::tool("a1", "write_archive", json!({"path": "specs/x.zip", "members": ["specs/011-retry.md"]})), Step::text("Understood.")], 5.0, |_| {});
        ratify_spec(&f);
        f.engine.run_task(&f.m, "architect", "Try a bad path").await.unwrap();
        assert!(events_of(&f, "approval.requested").is_empty() && events_of(&f, "provenance.stamp").is_empty());
        let told = f.model.requests()[1].body["messages"].to_string();
        assert!(told.contains("an artifact is written into the mission"), "{}", told);
    }

    #[test]
    fn the_tool_is_offered_only_to_roles_with_a_write_scope_and_its_output_reads_back_through_read_doc() {
        assert!(!Registry::builtin().defs(false).iter().any(|d| d.name == "write_archive"));
        assert!(Registry::builtin().defs(true).iter().any(|d| d.name == "write_archive"));
        let mut f = fx(&["missions/**"]);
        let (r, _) = run(&mut f, json!({"path": "evidence.zip", "members": ["@project/app/src/lib.rs"]}));
        assert!(!r.is_error, "{}", r.content);
        let mut ctx = ToolCtx { guard: &f.guard, span: &mut f.span, index: &mut f.index, rules: &f.rules, probe: &AssumeClean, role: "architect", mission_id: "m1", now_rfc3339: "t", stamps: vec![] };
        let back = Registry::builtin().run(&mut ctx, "read_doc", &json!({"path": "missions/m1/artifacts/evidence.zip", "member": "project/app/src/lib.rs"}));
        assert!(!back.is_error && back.content.contains("fn a() {}"), "{}", back.content);
    }
}
