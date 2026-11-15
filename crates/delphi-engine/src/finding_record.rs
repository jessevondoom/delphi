//! Findings: what an investigation concluded (`specs/116` §5). (Not `findings.rs`, which groups a red-team review's findings.)
//!
//! `record_finding` is the one way a finding is written. The investigator and the scout call it inside the daemon; a host session
//! acting in one of those roles calls the MCP tool of the same name, which reaches `host.record_finding`. Either way the caller sends
//! words and pointers and the daemon does the rest: it checks that every evidence pointer into a file resolves (one that does not is
//! refused by name and nothing is written), renders the file, and writes `missions/<id>/findings/<slug>.md` itself. No agent's file
//! tools can write that folder (`delphi_core::pathguard`), so a file there is always one this module made.
//!
//! What a finding is trusted as is never the caller's to say. Inside the daemon it is stored at the writing span's own tier, at most
//! `internal_generated`: a span that had read untrusted content writes a finding stored as untrusted, which the default search does
//! not return. A host's finding is host-supplied text and is always stored `external_untrusted`. Every finding carries the label
//! [`LABEL`]; recording one validates nothing.
//!
//! The index files a finding under zone [`ZONE`] (`kb.rs`), visible to every role, for the missions of the same project and for the
//! home layer. A finding that names `supersedes` retires the earlier one from default search: the earlier file stays, gains
//! `status: superseded` and a `superseded_by` link, and is otherwise untouched. A finding cannot retire one stored at a higher tier
//! than its own, so untrusted text cannot hide a trusted finding.

use crate::tools::{ActionKind, Classified, ToolCtx, ToolError, ToolExec, ToolResult, ToolSpec, ToolTier};
use delphi_adapter::claude::ToolDef;
use delphi_core::provenance::{OutputCheck, Tier};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The tool's name, inside the daemon and on the host server.
pub const TOOL: &str = "record_finding";
/// The index zone of findings.
pub const ZONE: &str = "findings";
/// What every finding is labelled, wherever it is shown.
pub const LABEL: &str = "agent finding, not validated";
/// The label of a finding stored below `internal_generated`: a span that had read untrusted content wrote it, or a host did.
pub const LABEL_UNTRUSTED: &str = "agent finding, not validated, from untrusted content";
/// The roles that record findings.
pub const ROLES: [&str; 2] = ["investigator", "scout"];
/// What an investigation may conclude.
pub const STATUSES: [&str; 4] = ["confirmed", "likely", "ruled_out", "open"];

const MAX_QUESTION: usize = 300;
const MAX_CONCLUSION: usize = 2000;
const MAX_HOW: usize = 1000;
const MAX_LINE: usize = 300;
const MAX_POINTER: usize = 300;
const MAX_EVIDENCE: usize = 20;
const MAX_RULED_OUT: usize = 10;
/// Most findings one mission holds: a loop cannot fill the folder.
pub const MAX_PER_MISSION: usize = 50;
/// Most characters of a finding a host's search shows: the question and the conclusion, not the whole file.
pub const HOST_PREVIEW: usize = 800;
/// Most bytes of a file read to check a pointer's lines.
const MAX_CHECK_BYTES: usize = 4 * 1024 * 1024;
const KEYS: [&str; 7] = ["question", "conclusion", "status", "evidence", "ruled_out", "how_verified", "supersedes"];

/// Whether a logical path is a finding's: exactly `missions/<id>/findings/<name>.md`. The index files such a path under [`ZONE`];
/// the path guard refuses every agent write to it.
pub fn is_finding_path(rel: &str) -> bool {
    let seg: Vec<&str> = rel.split('/').collect();
    seg.len() == 4 && seg[0] == "missions" && !seg[1].is_empty() && seg[2] == "findings" && seg[3].ends_with(".md")
}

/// What an evidence pointer points at (`specs/116` §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    /// A file, with or without lines: `path`, `path#L10` or `path#L10-L20`. Checked to resolve.
    File,
    /// A commit, by 7 to 40 hexadecimal characters.
    Commit,
    /// A command that was run, with the hash of its output.
    Command,
    /// A source Delphi holds, by its id.
    Source,
}

impl EvidenceKind {
    fn as_str(self) -> &'static str {
        match self {
            EvidenceKind::File => "file",
            EvidenceKind::Commit => "commit",
            EvidenceKind::Command => "command",
            EvidenceKind::Source => "source",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub kind: EvidenceKind,
    pub pointer: String,
    /// For a command: the hash of its output.
    pub output_hash: Option<String>,
    /// One line: what this shows.
    pub shows: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuledOut {
    pub hypothesis: String,
    /// The evidence against it, in one line.
    pub evidence: String,
}

/// The lines a file pointer names, from 1 and inclusive, when it names any.
pub type Lines = Option<(usize, usize)>;
/// How a file pointer is resolved: the path and its lines in, or why it does not resolve.
pub type Resolve<'a> = dyn Fn(&str, Lines) -> Result<(), String> + 'a;

/// A finding as the caller sent it, shape-checked. Nothing in it says how far it is trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub question: String,
    pub conclusion: String,
    pub status: String,
    pub evidence: Vec<Evidence>,
    pub ruled_out: Vec<RuledOut>,
    pub how_verified: String,
    /// An earlier finding's id (`<mission>/<slug>`), its path or its pointer.
    pub supersedes: Option<String>,
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Several lines of prose: control characters other than a line break go, and runs of blank lines close up.
fn prose(s: &str) -> String {
    let kept: String = s.chars().filter(|c| *c == '\n' || !c.is_control()).collect();
    let lines: Vec<&str> = kept.lines().map(str::trim_end).collect();
    let mut out: Vec<&str> = Vec::new();
    for l in lines {
        if !(l.trim().is_empty() && out.last().is_none_or(|p| p.trim().is_empty())) {
            out.push(l);
        }
    }
    out.join("\n").trim().to_string()
}

fn is_hex(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Finding {
    /// The finding in `args`, or why it is refused. `also` names keys the caller's envelope carries besides the finding's own (a
    /// host call's `session`, `tool`, `cwd`, `role` and `model`); any other key is refused, not ignored. `clean` is applied to every
    /// text the caller wrote (the daemon's secret redaction).
    pub fn parse(args: &Value, also: &[&str], clean: &dyn Fn(&str) -> String) -> Result<Finding, String> {
        let Some(obj) = args.as_object() else { return Err("a finding is an object".into()) };
        if let Some(k) = obj.keys().find(|k| !KEYS.contains(&k.as_str()) && !also.contains(&k.as_str())) {
            return Err(format!("`{k}` is not part of a finding: send {}; the daemon sets the author, the tier, the label and the path", KEYS.join(", ")));
        }
        let text = |v: &Value, name: &str, max: usize, lines: bool| -> Result<String, String> {
            let Some(s) = v.as_str() else { return Err(format!("`{name}` is text")) };
            let s = clean(&if lines { prose(s) } else { one_line(s) });
            if s.is_empty() || s.chars().count() > max {
                return Err(format!("`{name}` is 1 to {max} characters"));
            }
            Ok(s)
        };
        let question = text(&args["question"], "question", MAX_QUESTION, false)?;
        let conclusion = text(&args["conclusion"], "conclusion", MAX_CONCLUSION, true)?;
        let how_verified = text(&args["how_verified"], "how_verified", MAX_HOW, true)?;
        let status = args["status"].as_str().unwrap_or("").to_string();
        if !STATUSES.contains(&status.as_str()) {
            return Err(format!("`status` is one of {}", STATUSES.join(", ")));
        }
        let list = |name: &str, max: usize| -> Result<Vec<Value>, String> {
            match &args[name] {
                Value::Null => Ok(vec![]),
                Value::Array(a) if a.len() <= max => Ok(a.clone()),
                _ => Err(format!("`{name}` is a list of at most {max} items")),
            }
        };
        let mut evidence = Vec::new();
        for (i, e) in list("evidence", MAX_EVIDENCE)?.iter().enumerate() {
            let at = format!("evidence[{i}]");
            if e.as_object().is_none_or(|o| o.keys().any(|k| !["kind", "pointer", "output_hash", "shows"].contains(&k.as_str()))) {
                return Err(format!("`{at}` is an object of kind, pointer, shows and, for a command, output_hash"));
            }
            let kind = match e["kind"].as_str().unwrap_or("file") {
                "file" => EvidenceKind::File,
                "commit" => EvidenceKind::Commit,
                "command" => EvidenceKind::Command,
                "source" => EvidenceKind::Source,
                _ => return Err(format!("`{at}.kind` is file, commit, command or source")),
            };
            let pointer = text(&e["pointer"], &format!("{at}.pointer"), MAX_POINTER, false)?;
            let shows = text(&e["shows"], &format!("{at}.shows"), MAX_LINE, false)?;
            let output_hash = e["output_hash"].as_str().map(|h| h.trim().to_lowercase());
            match kind {
                EvidenceKind::Commit if !is_hex(&pointer, 7, 40) => return Err(format!("`{at}.pointer` is a commit: 7 to 40 hexadecimal characters")),
                EvidenceKind::Command if !output_hash.as_deref().is_some_and(|h| is_hex(h, 8, 128)) => return Err(format!("`{at}` is a command: give `output_hash`, the hash of its output (8 to 128 hexadecimal characters)")),
                EvidenceKind::Source if !pointer.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/')) => return Err(format!("`{at}.pointer` is a source id")),
                EvidenceKind::File => {
                    file_pointer(&pointer).map_err(|why| format!("`{at}.pointer` `{pointer}` is not a file pointer ({why}): write `path`, `path#L10` or `path#L10-L20`"))?;
                }
                _ => {}
            }
            evidence.push(Evidence { kind, pointer, output_hash: output_hash.filter(|_| kind == EvidenceKind::Command), shows });
        }
        let mut ruled_out = Vec::new();
        for (i, r) in list("ruled_out", MAX_RULED_OUT)?.iter().enumerate() {
            let at = format!("ruled_out[{i}]");
            if r.as_object().is_none_or(|o| o.keys().any(|k| !["hypothesis", "evidence"].contains(&k.as_str()))) {
                return Err(format!("`{at}` is an object of hypothesis and evidence"));
            }
            ruled_out.push(RuledOut { hypothesis: text(&r["hypothesis"], &format!("{at}.hypothesis"), MAX_LINE, false)?, evidence: text(&r["evidence"], &format!("{at}.evidence"), MAX_LINE, false)? });
        }
        if matches!(status.as_str(), "confirmed" | "likely") && evidence.is_empty() {
            return Err(format!("a `{status}` finding names its evidence: at least one pointer with what it shows. With none, record it as `open`"));
        }
        let supersedes = match &args["supersedes"] {
            Value::Null => None,
            v => Some(text(v, "supersedes", MAX_POINTER, false)?),
        };
        Ok(Finding { question, conclusion, status, evidence, ruled_out, how_verified, supersedes })
    }

    /// Check every evidence pointer into a file with `resolve` (the path, and the lines when the pointer names them). The first
    /// one that does not resolve refuses the whole finding, by name.
    pub fn check_files(&self, resolve: &Resolve) -> Result<(), String> {
        for e in self.evidence.iter().filter(|e| e.kind == EvidenceKind::File) {
            let (path, lines) = file_pointer(&e.pointer)?;
            resolve(&path, lines).map_err(|why| format!("refused: the evidence pointer `{}` does not resolve ({why}). Nothing was recorded; point at a file and lines that exist, or leave that evidence out", e.pointer))?;
        }
        Ok(())
    }
}

/// A file pointer's path and, when it names them, its lines (from 1, inclusive): `path`, `path#L10`, `path#L10-L20`.
pub fn file_pointer(pointer: &str) -> Result<(String, Option<(usize, usize)>), String> {
    let (path, frag) = match pointer.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (pointer, None),
    };
    if path.is_empty() || path.chars().any(|c| c.is_control()) {
        return Err("no path".into());
    }
    let Some(frag) = frag else { return Ok((path.to_string(), None)) };
    let num = |s: &str| s.strip_prefix('L').and_then(|n| n.parse::<usize>().ok()).filter(|n| *n >= 1);
    let lines = match frag.split_once('-') {
        Some((a, b)) => num(a).zip(num(b)),
        None => num(frag).map(|a| (a, a)),
    };
    match lines {
        Some((a, b)) if a <= b => Ok((path.to_string(), Some((a, b)))),
        _ => Err("the lines are not `#L<n>` or `#L<a>-L<b>`".into()),
    }
}

/// Whether `bytes` has the lines a pointer names.
pub fn lines_exist(bytes: &[u8], truncated: bool, lines: Option<(usize, usize)>) -> Result<(), String> {
    let Some((_, end)) = lines else { return Ok(()) };
    let count = bytes.split(|b| *b == b'\n').count() - usize::from(bytes.ends_with(b"\n") || bytes.is_empty());
    if end <= count {
        Ok(())
    } else if truncated {
        Err(format!("line {end} is past the first {} MiB of the file, which is all that is checked", MAX_CHECK_BYTES / (1024 * 1024)))
    } else {
        Err(format!("the file has {count} lines"))
    }
}

/// A host's file pointer, resolved against the registered project its session is in: a relative path with no `..`, inside that
/// root after links are followed. With no registered project there is nothing to check a file against, so it does not resolve.
pub fn resolve_in_root(root: Option<&Path>, path: &str, lines: Option<(usize, usize)>) -> Result<(), String> {
    let Some(root) = root else { return Err("this session's folder is not in a registered project, so a file cannot be checked".into()) };
    let rel = path.trim_start_matches("./");
    if Path::new(rel).is_absolute() || rel.split('/').any(|s| s == ".." || s.is_empty()) {
        return Err("write the path relative to the project's root, with no `..`".into());
    }
    let full = std::fs::canonicalize(root.join(rel)).map_err(|_| "no such file".to_string())?;
    if !full.starts_with(root) || !full.is_file() {
        return Err("not a file inside the project".into());
    }
    let (bytes, truncated) = read_capped(&full).map_err(|e| e.to_string())?;
    lines_exist(&bytes, truncated, lines)
}

fn read_capped(p: &Path) -> std::io::Result<(Vec<u8>, bool)> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(p)?.take(MAX_CHECK_BYTES as u64 + 1).read_to_end(&mut buf)?;
    let truncated = buf.len() > MAX_CHECK_BYTES;
    buf.truncate(MAX_CHECK_BYTES);
    Ok((buf, truncated))
}

/// What the daemon, not the caller, says about a finding.
pub struct Meta<'a> {
    /// The frontmatter `author`: the role id inside the daemon; a host's own label (`host:<tool>/<model>`) for a host.
    pub author: &'a str,
    /// The role the finding was recorded as.
    pub role: &'a str,
    pub mission: &'a str,
    pub now: &'a str,
    /// The tier it is stored at: the writing span's, at most `internal_generated`; `external_untrusted` for a host.
    pub tier: Tier,
}

/// A finding on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// `<mission>/<slug>`: what a later finding names in `supersedes`.
    pub id: String,
    /// The logical path, `missions/<mission>/findings/<slug>.md`.
    pub rel_path: String,
    pub text: String,
    /// The id of the finding this one retired, if it named one.
    pub superseded: Option<String>,
}

fn slug(question: &str) -> String {
    let mut s = String::new();
    for c in question.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
        if s.len() >= 60 {
            break;
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "finding".into()
    } else {
        s
    }
}

fn plain(s: &str) -> bool {
    !s.is_empty() && s.len() <= 120 && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// `(mission, slug)` of a finding named by its id (`<mission>/<slug>`), its path or its pointer.
fn named(reference: &str) -> Option<(String, String)> {
    let r = reference.split('#').next().unwrap_or("").trim().trim_start_matches("./");
    let r = r.strip_prefix("missions/").unwrap_or(r);
    let r = r.strip_suffix(".md").unwrap_or(r);
    let (mission, rest) = r.split_once('/')?;
    let slug = rest.strip_prefix("findings/").unwrap_or(rest);
    (plain(mission) && plain(slug)).then(|| (mission.to_string(), slug.to_string()))
}

fn tier_of(source_type: &str) -> Tier {
    [Tier::L5, Tier::L4, Tier::L3, Tier::L2, Tier::L1].into_iter().find(|t| t.name() == source_type).unwrap_or(Tier::L0)
}

fn split(text: &str) -> Option<(serde_yaml::Mapping, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    match serde_yaml::from_str(&rest[..end]).ok()? {
        serde_yaml::Value::Mapping(m) => Some((m, &rest[end + 5..])),
        _ => None,
    }
}

fn join(map: serde_yaml::Mapping, body: &str) -> Result<String, String> {
    Ok(format!("---\n{}---\n{}", serde_yaml::to_string(&serde_yaml::Value::Mapping(map)).map_err(|e| e.to_string())?, body))
}

/// The findings folder of a mission, made if missing; refused when it, or the mission's folder, is not a plain directory.
fn folder(ws_dir: &Path, mission: &str) -> Result<PathBuf, String> {
    if !plain(mission) {
        return Err("no such mission".into());
    }
    let mdir = delphi_core::sessions::dir(ws_dir, mission);
    let real = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink());
    if !real(&mdir) {
        return Err("no such mission".into());
    }
    let dir = mdir.join("findings");
    if std::fs::symlink_metadata(&dir).is_err() {
        std::fs::create_dir(&dir).map_err(|e| format!("the findings folder could not be made: {e}"))?;
    }
    if !real(&dir) {
        return Err("the findings folder is not a plain directory".into());
    }
    Ok(dir)
}

/// Replace `path` with `text` by a temporary file in the same folder and a rename.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if std::fs::symlink_metadata(path).is_ok_and(|m| !m.is_file()) {
        return Err("the finding's path is not a plain file".into());
    }
    let tmp = path.with_file_name(format!(".{}.tmp-{}-{}", path.file_name().and_then(|n| n.to_str()).unwrap_or("finding"), std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let done = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, path));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done.map_err(|e| format!("the finding could not be written: {e}"))
}

/// The project a mission's findings belong to, from the `scope` file the daemon stamped its folder with at creation: a registered
/// project's id (32 hexadecimal characters), or the empty string for the home layer (no file, the default project, anything else).
/// A finding is visible to the missions of its own project, and a home finding to every mission (`kb.rs`).
pub fn scope_of_dir(mission_dir: &Path) -> String {
    scope_word(std::fs::read_to_string(mission_dir.join("scope")).ok().as_deref())
}

/// [`scope_of_dir`] for a project id already in hand (a host session's resolved project).
pub fn scope_word(id: Option<&str>) -> String {
    id.map(str::trim).filter(|s| is_hex(s, 32, 32) && !s.bytes().any(|b| b.is_ascii_uppercase())).unwrap_or("").to_string()
}

fn scope_of(ws_dir: &Path, mission: &str) -> String {
    scope_of_dir(&delphi_core::sessions::dir(ws_dir, mission))
}

/// The finding as a file: frontmatter the daemon wrote, then the question, the conclusion, the evidence, what was ruled out and
/// how it was verified.
pub fn render(f: &Finding, meta: &Meta, id: &str, supersedes: Option<&str>) -> Result<String, String> {
    let mut map = serde_yaml::Mapping::new();
    let mut put = |k: &str, v: &str| {
        map.insert(serde_yaml::Value::String(k.into()), serde_yaml::Value::String(v.into()));
    };
    put("id", id);
    put("title", &f.question);
    put("author", meta.author);
    put("role", meta.role);
    put("timestamp", meta.now);
    put("status", "active");
    put("source_type", meta.tier.min(Tier::L3).name());
    put("kind", "finding");
    put("label", LABEL);
    put("finding_status", &f.status);
    put("confidence", if f.status == "confirmed" { "medium" } else { "low" });
    put("source_mission", meta.mission);
    if let Some(s) = supersedes {
        put("supersedes", s);
    }
    map.insert(serde_yaml::Value::String("tags".into()), serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("finding".into())]));
    let mut body = format!("# {}\n\nConclusion ({}): {}\n", f.question, f.status.replace('_', " "), f.conclusion);
    if !f.evidence.is_empty() {
        body.push_str("\n## Evidence\n\n");
        for e in &f.evidence {
            let what = match (e.kind, &e.output_hash) {
                (EvidenceKind::Command, Some(h)) => format!("command `{}` (output hash `{}`)", e.pointer.replace('`', "'"), h),
                (EvidenceKind::File, _) => format!("`{}`", e.pointer.replace('`', "'")),
                (k, _) => format!("{} `{}`", k.as_str(), e.pointer.replace('`', "'")),
            };
            body.push_str(&format!("- {what}: {}\n", e.shows));
        }
    }
    if !f.ruled_out.is_empty() {
        body.push_str("\n## Ruled out\n\n");
        for r in &f.ruled_out {
            body.push_str(&format!("- {}: {}\n", r.hypothesis, r.evidence));
        }
    }
    body.push_str(&format!("\n## How verified\n\n{}\n", f.how_verified));
    join(map, &body)
}

/// Write a checked finding into its mission's findings folder and, when it names one, retire the finding it supersedes. The
/// caller has already checked the evidence ([`Finding::check_files`]); this never overwrites a finding.
pub fn store(ws_dir: &Path, f: &Finding, meta: &Meta) -> Result<Stored, String> {
    let dir = folder(ws_dir, meta.mission)?;
    let held = std::fs::read_dir(&dir).map(|rd| rd.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".md")).count()).unwrap_or(0);
    if held >= MAX_PER_MISSION {
        return Err(format!("this mission already holds {MAX_PER_MISSION} findings; supersede one in a later mission, or say what is new in your report"));
    }
    // The earlier finding, checked before anything is written: it exists, is still current, belongs to the same project, and is
    // not stored at a higher tier than this one.
    let earlier = match &f.supersedes {
        None => None,
        Some(reference) => {
            let refused = |why: &str| format!("refused: `supersedes` names `{reference}`, {why}. Nothing was recorded");
            let (mission, slug) = named(reference).ok_or_else(|| refused("which is not a finding's id (`<mission>/<slug>`) or path"))?;
            let path = delphi_core::sessions::dir(ws_dir, &mission).join("findings").join(format!("{slug}.md"));
            let text = std::fs::symlink_metadata(&path).ok().filter(|m| m.is_file()).and_then(|_| std::fs::read_to_string(&path).ok()).ok_or_else(|| refused("and there is no such finding"))?;
            let (map, body) = split(&text).ok_or_else(|| refused("which is not a finding"))?;
            let get = |k: &str| map.get(serde_yaml::Value::String(k.into())).and_then(|v| v.as_str()).unwrap_or("").to_string();
            if get("kind") != "finding" {
                return Err(refused("which is not a finding"));
            }
            if get("status") != "active" {
                return Err(refused(&format!("which was already superseded by `{}`", get("superseded_by"))));
            }
            if scope_of(ws_dir, &mission) != scope_of(ws_dir, meta.mission) {
                return Err(refused("which belongs to another project"));
            }
            if tier_of(&get("source_type")) > meta.tier.min(Tier::L3) {
                return Err(refused("which is stored at a higher tier than this finding would be: a finding cannot retire one more trusted than itself"));
            }
            Some((format!("{mission}/{slug}"), path, map, body.to_string()))
        }
    };
    let base = slug(&f.question);
    let name = (1..=MAX_PER_MISSION + 1).map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") }).find(|s| std::fs::symlink_metadata(dir.join(format!("{s}.md"))).is_err()).ok_or("no free name for the finding")?;
    let id = format!("{}/{}", meta.mission, name);
    let text = render(f, meta, &id, earlier.as_ref().map(|e| e.0.as_str()))?;
    write_atomic(&dir.join(format!("{name}.md")), &text)?;
    let mut superseded = None;
    if let Some((old_id, path, mut map, body)) = earlier {
        map.insert(serde_yaml::Value::String("status".into()), serde_yaml::Value::String("superseded".into()));
        map.insert(serde_yaml::Value::String("superseded_by".into()), serde_yaml::Value::String(id.clone()));
        write_atomic(&path, &join(map, &body)?)?;
        superseded = Some(old_id);
    }
    Ok(Stored { rel_path: format!("missions/{}/findings/{}.md", meta.mission, name), id, text, superseded })
}

/// What the caller is told: where the finding is, what it is labelled, and what it retired. Never the text back.
pub fn receipt(s: &Stored, tier: Tier) -> Value {
    let mut v = json!({"recorded": true, "finding": s.id, "path": s.rel_path, "label": label_for(tier.min(Tier::L3).name()), "status": "active"});
    if let Some(old) = &s.superseded {
        v["superseded"] = json!(old);
    }
    if tier.min(Tier::L3) < Tier::L3 {
        v["note"] = json!("stored as untrusted: the default search does not return it");
    }
    v
}

/// The label of a finding stored at `source_type`.
pub fn label_for(source_type: &str) -> &'static str {
    if tier_of(source_type) >= Tier::L3 {
        LABEL
    } else {
        LABEL_UNTRUSTED
    }
}

/// The keys a host call carries besides the finding's own: who is calling (set by the MCP process) and the role it acts in.
pub const HOST_KEYS: [&str; 5] = ["session", "tool", "cwd", "role", "model"];

/// The host tool's input: the finding, and the role the host is acting in.
pub fn host_input_schema() -> Value {
    let mut s = input_schema();
    s["properties"]["role"] = json!({"type": "string", "enum": ROLES, "description": "The Delphi role you are acting in."});
    s["properties"]["model"] = json!({"type": "string", "maxLength": 40, "description": "Your model's name, as a label."});
    if let Some(required) = s["required"].as_array_mut() {
        required.push(json!("role"));
    }
    s
}

/// The host tool's description: the inside tool's words, and what differs for a host.
pub fn host_description() -> String {
    format!("{DESCRIPTION} As a host, say which `role` you are acting in (`investigator` or `scout`). Write a file pointer as a path relative to this project's root: it is checked there, and in a folder that is not a registered project a file pointer cannot be checked and is refused. What a host records is host-supplied text: it is stored as untrusted and labelled `{LABEL_UNTRUSTED}`; search_kb shows it with that label, and Delphi's own agents are not handed it. Changes no file in your project.")
}

/// The input schema, shared by the inside tool and the host tool.
pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "required": ["question", "conclusion", "status", "how_verified"],
        "additionalProperties": false,
        "properties": {
            "question": {"type": "string", "maxLength": MAX_QUESTION, "description": "What was investigated, as one question."},
            "conclusion": {"type": "string", "maxLength": MAX_CONCLUSION, "description": "What you concluded. For `open`, what is still unknown."},
            "status": {"type": "string", "enum": STATUSES},
            "evidence": {"type": "array", "maxItems": MAX_EVIDENCE, "items": {"type": "object", "required": ["pointer", "shows"], "additionalProperties": false, "properties": {
                "kind": {"type": "string", "enum": ["file", "commit", "command", "source"], "description": "file (the default): `path`, `path#L10` or `path#L10-L20`, checked to exist. commit: 7 to 40 hex characters. command: the command line, with `output_hash`. source: a source id."},
                "pointer": {"type": "string", "maxLength": MAX_POINTER},
                "output_hash": {"type": "string", "maxLength": 128, "description": "For a command: the hash of its output."},
                "shows": {"type": "string", "maxLength": MAX_LINE, "description": "One line: what this shows."}
            }}},
            "ruled_out": {"type": "array", "maxItems": MAX_RULED_OUT, "items": {"type": "object", "required": ["hypothesis", "evidence"], "additionalProperties": false, "properties": {
                "hypothesis": {"type": "string", "maxLength": MAX_LINE},
                "evidence": {"type": "string", "maxLength": MAX_LINE, "description": "One line: the evidence against it."}
            }}},
            "how_verified": {"type": "string", "maxLength": MAX_HOW, "description": "How the conclusion was checked, or that it was not."},
            "supersedes": {"type": "string", "maxLength": MAX_POINTER, "description": "The id (`<mission>/<slug>`) or path of an earlier finding this one replaces."}
        }
    })
}

/// What the tool is for, in the words both the inside agent and a host read.
pub const DESCRIPTION: &str = "Record what an investigation concluded, so a later investigation finds it instead of starting again. Investigator and scout only. Give the `question`, the `conclusion`, a `status` (`confirmed`, `likely`, `ruled_out` or `open`), the `evidence` (each a pointer with one line of what it shows: a file as `path#L10-L20`, a commit, a command with the hash of its output, or a source id), the hypotheses you `ruled_out` with the evidence against each, and `how_verified`. End every investigation with it, including when the answer was not found: record that as `open` with what was ruled out. Delphi checks that every file pointer exists and refuses the finding, naming the pointer, when one does not. It writes the file itself, labelled `agent finding, not validated`; you cannot set its author, tier or label, and recording a finding validates nothing. `supersedes` names an earlier finding this one replaces: the earlier one leaves the default search and links to this one.";

/// Recording writes one file in the mission's own folder, which only the daemon writes: it never asks.
fn classify(tool: &str, args: &Value) -> Result<Classified, ToolError> {
    if args.get("question").and_then(Value::as_str).is_none() {
        return Err(ToolError("missing string argument `question`".into()));
    }
    Ok(Classified { tool: tool.into(), tier: ToolTier::Green, kind: ActionKind::Read, summary: tool.into(), payload: json!({"question": args["question"]}), pattern: tool.into() })
}

/// The inside tool: the role comes from the daemon's tool context, the tier from the span, the path from the mission.
fn record_finding(ctx: &mut ToolCtx, args: &Value) -> ToolResult {
    if !ROLES.contains(&ctx.role) {
        return ToolResult::err("only the investigator and the scout record findings; use `note` to leave something for the operator");
    }
    let f = match Finding::parse(args, &[], &|s| s.to_string()) {
        Ok(f) => f,
        Err(e) => return ToolResult::err(e),
    };
    // A file pointer is read through the role's own guard, so it resolves exactly what the role could read; nothing read here
    // reaches the model.
    let guard = ctx.guard;
    let checked = f.check_files(&|path, lines| {
        let (bytes, truncated) = guard.read_file(path, MAX_CHECK_BYTES).map_err(|d| match d {
            delphi_core::pathguard::Denied::NotFound => "no such file".to_string(),
            other => crate::tools::denied(other).content,
        })?;
        lines_exist(&bytes, truncated, lines)
    });
    if let Err(e) = checked {
        return ToolResult::err(e);
    }
    let Some(ws_dir) = ctx.guard.zones.local.parent() else { return ToolResult::err("no workspace") };
    // Never above the span's ceiling: a span that read untrusted content records an untrusted finding.
    let tier = ctx.span.output_tier(OutputCheck::Unverified);
    let stored = match store(ws_dir, &f, &Meta { author: ctx.role, role: ctx.role, mission: ctx.mission_id, now: ctx.now_rfc3339, tier }) {
        Ok(s) => s,
        Err(e) => return ToolResult::err(e),
    };
    let hash = blake3::hash(stored.text.as_bytes()).to_hex().to_string();
    ctx.index.stamp(delphi_core::provenance::Zone::Local, &stored.rel_path, &hash, tier);
    ctx.stamps.push(crate::tools::Stamp { zone: delphi_core::provenance::Zone::Local, path: stored.rel_path.clone(), blake3: hash, tier });
    ctx.events.push(("finding.recorded".into(), json!({"finding": stored.id, "path": stored.rel_path, "status": f.status, "by": ctx.role, "tier": tier.as_str(), "superseded": stored.superseded})));
    ToolResult::ok(receipt(&stored, tier).to_string())
}

pub fn specs() -> Vec<ToolSpec> {
    vec![ToolSpec { def: ToolDef { name: TOOL.into(), description: DESCRIPTION.into(), input_schema: input_schema() }, requires_write_scope: false, classify, exec: ToolExec::Local(record_finding) }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(args: Value) -> Result<Finding, String> {
        Finding::parse(&args, &[], &|s| s.to_string())
    }

    fn sample() -> Value {
        json!({"question": "Why does the retry ladder stall at rung three?", "conclusion": "The backoff timer is reset on every 429, so rung three never elapses.", "status": "confirmed",
            "evidence": [{"pointer": "notes.md#L1-L2", "shows": "the reset on 429"}, {"kind": "commit", "pointer": "abc1234", "shows": "introduced the reset"}, {"kind": "command", "pointer": "cargo test retry", "output_hash": "deadbeefdeadbeef", "shows": "fails before, passes after"}],
            "ruled_out": [{"hypothesis": "clock skew", "evidence": "the same on a fixed clock"}], "how_verified": "Ran the test with the reset removed."})
    }

    fn ws() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path().join("ws");
        std::fs::create_dir_all(ws.join("local/sessions/m1")).unwrap();
        std::fs::create_dir_all(ws.join("local/sessions/m2")).unwrap();
        (t, ws)
    }

    fn meta<'a>(mission: &'a str, tier: Tier) -> Meta<'a> {
        Meta { author: "investigator", role: "investigator", mission, now: "2026-11-14T00:00:00Z", tier }
    }

    #[test]
    fn a_finding_is_shape_checked_and_nothing_else_is_accepted() {
        let f = finding(sample()).unwrap();
        assert_eq!((f.status.as_str(), f.evidence.len(), f.ruled_out.len()), ("confirmed", 3, 1));
        assert_eq!(f.evidence[2].output_hash.as_deref(), Some("deadbeefdeadbeef"));
        let with = |k: &str, v: Value| {
            let mut s = sample();
            s[k] = v;
            finding(s)
        };
        for (k, v) in [("status", json!("validated")), ("source_type", json!("user_ratified")), ("author", json!("operator")), ("label", json!("validated")), ("question", json!("")), ("evidence", json!([{"pointer": "a.md#L9-L2", "shows": "x"}])), ("evidence", json!([{"kind": "commit", "pointer": "not-a-sha", "shows": "x"}])), ("evidence", json!([{"kind": "command", "pointer": "ls", "shows": "x"}])), ("evidence", json!([{"pointer": "a.md", "shows": "x", "tier": "L5"}]))] {
            assert!(with(k, v.clone()).is_err(), "{k}: {v}");
        }
        assert!(with("evidence", json!([])).unwrap_err().contains("names its evidence"), "a confirmed finding has evidence");
        let mut open = sample();
        open["status"] = json!("open");
        open["evidence"] = json!([]);
        assert!(finding(open).is_ok(), "`open` may rest on what was ruled out alone");
        assert_eq!(file_pointer("a/b.rs#L3").unwrap(), ("a/b.rs".to_string(), Some((3, 3))));
        assert_eq!(file_pointer("a/b.rs").unwrap(), ("a/b.rs".to_string(), None));
        assert!(file_pointer("a/b.rs#L0").is_err() && file_pointer("a/b.rs#3-4").is_err());
    }

    #[test]
    fn a_pointer_to_a_missing_file_or_line_is_refused_by_name() {
        let f = finding(sample()).unwrap();
        let missing = f.check_files(&|_, _| Err("no such file".into())).unwrap_err();
        assert!(missing.contains("`notes.md#L1-L2`") && missing.contains("does not resolve") && missing.contains("Nothing was recorded"), "{missing}");
        assert!(f.check_files(&|path, lines| if path == "notes.md" && lines == Some((1, 2)) { Ok(()) } else { Err("x".into()) }).is_ok(), "only file pointers are resolved");
        assert!(lines_exist(b"one\ntwo\n", false, Some((1, 2))).is_ok());
        assert_eq!(lines_exist(b"one\ntwo\n", false, Some((2, 3))).unwrap_err(), "the file has 2 lines");
        assert!(lines_exist(b"one", false, Some((1, 1))).is_ok() && lines_exist(b"", false, Some((1, 1))).is_err());

        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "a\nb\nc\n").unwrap();
        assert!(resolve_in_root(Some(&root), "src/lib.rs", Some((1, 3))).is_ok());
        assert!(resolve_in_root(Some(&root), "./src/lib.rs", None).is_ok());
        assert_eq!(resolve_in_root(Some(&root), "src/lib.rs", Some((2, 9))).unwrap_err(), "the file has 3 lines");
        assert_eq!(resolve_in_root(Some(&root), "src/gone.rs", None).unwrap_err(), "no such file");
        for outside in ["../x", "/etc/hosts", "src/../../x"] {
            assert!(resolve_in_root(Some(&root), outside, None).is_err(), "{outside}");
        }
        assert!(resolve_in_root(None, "src/lib.rs", None).unwrap_err().contains("not in a registered project"));
    }

    #[test]
    fn a_finding_is_written_active_labelled_and_at_most_at_the_writers_tier() {
        let (_t, ws) = ws();
        let f = finding(sample()).unwrap();
        let s = store(&ws, &f, &meta("m1", Tier::L5)).unwrap();
        assert_eq!((s.id.as_str(), s.rel_path.as_str()), ("m1/why-does-the-retry-ladder-stall-at-rung-three", "missions/m1/findings/why-does-the-retry-ladder-stall-at-rung-three.md"));
        assert!(is_finding_path(&s.rel_path) && !is_finding_path("missions/m1/research/findings/x.md") && !is_finding_path("missions/m1/findings/x.txt"));
        let on_disk = std::fs::read_to_string(ws.join("local/sessions/m1/findings/why-does-the-retry-ladder-stall-at-rung-three.md")).unwrap();
        assert_eq!(on_disk, s.text);
        let (map, body) = split(&on_disk).unwrap();
        let get = |k: &str| map.get(serde_yaml::Value::String(k.into())).and_then(|v| v.as_str()).unwrap().to_string();
        assert_eq!((get("status"), get("source_type"), get("kind"), get("label"), get("finding_status"), get("author")), ("active".into(), "internal_generated".into(), "finding".into(), LABEL.to_string(), "confirmed".into(), "investigator".into()));
        assert!(body.starts_with("# Why does the retry ladder stall at rung three?\n\nConclusion (confirmed): The backoff timer"), "{body}");
        assert!(body.contains("- `notes.md#L1-L2`: the reset on 429") && body.contains("- commit `abc1234`:") && body.contains("output hash `deadbeefdeadbeef`") && body.contains("## Ruled out\n\n- clock skew: the same on a fixed clock") && body.contains("## How verified"), "{body}");
        // The same question again is a second file, never an overwrite.
        assert_eq!(store(&ws, &f, &meta("m1", Tier::L5)).unwrap().id, "m1/why-does-the-retry-ladder-stall-at-rung-three-2");
        // A span that read untrusted content records an untrusted finding, and is told so.
        let tainted = store(&ws, &f, &meta("m2", Tier::L0)).unwrap();
        assert!(tainted.text.contains("source_type: external_untrusted"));
        let r = receipt(&tainted, Tier::L0);
        assert_eq!(r["label"], LABEL_UNTRUSTED);
        assert_eq!((scope_word(Some("0123456789abcdef0123456789abcdef\n")), scope_word(Some("default")), scope_word(Some("../x")), scope_word(None)), ("0123456789abcdef0123456789abcdef".to_string(), String::new(), String::new(), String::new()));
        assert!(r["note"].as_str().unwrap().contains("does not return it") && r.get("conclusion").is_none());
        assert!(store(&ws, &f, &meta("nope", Tier::L5)).is_err() && store(&ws, &f, &meta("../m1", Tier::L5)).is_err());
    }

    #[test]
    fn superseding_retires_the_earlier_finding_and_links_it_forward() {
        let (_t, ws) = ws();
        let first = store(&ws, &finding(sample()).unwrap(), &meta("m1", Tier::L5)).unwrap();
        let mut again = sample();
        again["conclusion"] = json!("The reset is in the pool, not the ladder.");
        again["supersedes"] = json!(first.id);
        let second = store(&ws, &finding(again.clone()).unwrap(), &meta("m2", Tier::L5)).unwrap();
        assert_eq!(second.superseded.as_deref(), Some(first.id.as_str()));
        assert!(second.text.contains(&format!("supersedes: {}", first.id)));
        let old = std::fs::read_to_string(ws.join("local/sessions/m1/findings/why-does-the-retry-ladder-stall-at-rung-three.md")).unwrap();
        assert!(old.contains("status: superseded") && old.contains(&format!("superseded_by: {}", second.id)), "{old}");
        assert!(old.ends_with(first.text.split("\n---\n").nth(1).unwrap()), "the earlier body is untouched");
        // Already superseded, unknown, another project's, or more trusted than the new one: refused, and nothing is written.
        let count = || std::fs::read_dir(ws.join("local/sessions/m2/findings")).unwrap().count();
        let before = count();
        let twice = store(&ws, &finding(again.clone()).unwrap(), &meta("m2", Tier::L5)).unwrap_err();
        assert!(twice.contains("already superseded by") && twice.contains(&second.id), "{twice}");
        again["supersedes"] = json!("m1/no-such-finding");
        assert!(store(&ws, &finding(again.clone()).unwrap(), &meta("m2", Tier::L5)).unwrap_err().contains("no such finding"));
        again["supersedes"] = json!(second.rel_path);
        let lower = store(&ws, &finding(again.clone()).unwrap(), &meta("m2", Tier::L0)).unwrap_err();
        assert!(lower.contains("cannot retire one more trusted than itself"), "{lower}");
        std::fs::write(ws.join("local/sessions/m1/scope"), "0123456789abcdef0123456789abcdef").unwrap();
        assert!(store(&ws, &finding(again.clone()).unwrap(), &meta("m1", Tier::L5)).unwrap_err().contains("another project"));
        assert_eq!(count(), before, "a refused finding leaves no file");
        // By path, in the same project: accepted.
        std::fs::remove_file(ws.join("local/sessions/m1/scope")).unwrap();
        assert_eq!(store(&ws, &finding(again).unwrap(), &meta("m1", Tier::L5)).unwrap().superseded.as_deref(), Some(second.id.as_str()));
    }
}
