//! Project-to-home promotion (`specs/058` §7, slice PS5).
//!
//! A promotion is a proposal kind of the `/retro` flow: a `learn.proposed` event with `op: "promote"` in a mission's log
//! (the rule proposals of [`crate::learn::live_proposals`] ignore it, so `/retro` is unchanged). It carries a concise
//! summary, the originating project, the original project records with their hashes, the home destination and a hash of
//! the home records it was made against. It is data: nothing here loads it, and no code path in this module accepts a
//! proposal by itself. The operator's own command calls [`accept`].
//!
//! [`preflight`] is the deterministic, model-free eligibility check (kind and status, private flag, secrets,
//! provenance, summary size). It runs when a proposal is made and again inside [`accept`], under the same lock as the
//! compare-and-swap against the proposal's version and the home records (amendment 058-A1), so a record that became
//! private, retired or secret-bearing after the proposal is refused. Acceptance writes the summary as a home memory entry
//! under `local/promoted/` that keeps the records' provenance and the lowest of their source tiers (capped below
//! `user_ratified`): the operator ratifies the summary as home memory, not any fact in it. A duplicate or a possible
//! contradiction in home memory is shown, never merged and never superseded. Rejecting records the outcome and the
//! operator's reason, not the proposal's text. [`withdraw`] marks an entry `retired` with an audit reference and deletes
//! nothing.

use crate::frontmatter::{self, SOURCE_TYPES};
use crate::scope::{self, ProjectId};
use delphi_core::event::EventEnvelope;
use delphi_core::lock;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Characters a promotion summary may have.
pub const MIN_SUMMARY: usize = 12;
pub const MAX_SUMMARY: usize = 600;
/// Most project records one promotion names.
pub const MAX_RECORDS: usize = 8;
/// Longest operator reason kept for a rejection or a withdrawal.
pub const MAX_REASON: usize = 200;
/// The kinds (`specs/050` ML-1) a record may have to be promoted: an episode is about one event, not reusable.
pub const ELIGIBLE_KINDS: [&str; 2] = ["fact", "procedure"];
/// The project-layer directories a promotable record lives in.
const RECORD_DIRS: [&str; 3] = ["memory/agents/", "knowledge/", "staged/"];
const MAX_RECORD_BYTES: u64 = 64 * 1024;
/// The home destination of an accepted summary: `<ws>/local/promoted/<mission>-<id>.md`, indexed as home memory.
pub const DESTINATION: &str = "home:memory/promoted";
/// Source tiers above this are never given to a promoted summary: acceptance ratifies a summary, it validates no fact.
const CEILING: &str = "internal_generated";

/// Why a record cannot be promoted, by a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub detail: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

fn refuse<T>(code: &'static str, detail: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal { code, detail: detail.into() })
}

/// An eligible original record as the proposal pins it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Original {
    /// The path inside the project's layer, such as `staged/m1/n1.md`.
    pub rel: String,
    pub blake3: String,
    pub source_type: String,
}

fn flag(m: &serde_yaml::Mapping, key: &str) -> bool {
    match m.get(serde_yaml::Value::String(key.into())) {
        Some(serde_yaml::Value::Bool(b)) => *b,
        Some(serde_yaml::Value::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn record_path(ws_dir: &Path, project: &ProjectId, rel: &str) -> Result<PathBuf, Refusal> {
    let bad = rel.is_empty() || rel.contains('\\') || !rel.ends_with(".md") || !RECORD_DIRS.iter().any(|d| rel.starts_with(d)) || rel.split('/').any(|s| s.is_empty() || s.starts_with('.'));
    if bad {
        return refuse("record_missing", format!("`{}` is not a memory record of a project layer", rel.chars().take(80).collect::<String>()));
    }
    let layer = scope::layer_dir(ws_dir, project).or_else(|e| refuse("scope_unavailable", e))?;
    let mut path = layer.clone();
    for seg in rel.split('/') {
        path.push(seg);
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return refuse("record_missing", format!("`{}` goes through a symbolic link", rel));
        }
    }
    Ok(path)
}

/// Whether the summary may be proposed or accepted: its size and any secret or local detail it carries.
fn check_summary(ws_dir: &Path, summary: &str) -> Result<(), Refusal> {
    let n = summary.trim().chars().count();
    if !(MIN_SUMMARY..=MAX_SUMMARY).contains(&n) {
        return refuse("summary_size", format!("the summary is {} characters; it must be {} to {}", n, MIN_SUMMARY, MAX_SUMMARY));
    }
    if let Some(h) = delphi_bundle::scan::scan_text(summary, &crate::distill::scan_context(ws_dir)).into_iter().find(|h| h.fail) {
        return refuse("summary_not_redacted", format!("the summary carries {}", h.found));
    }
    Ok(())
}

fn read_record(ws_dir: &Path, project: &ProjectId, rel: &str) -> Result<Original, Refusal> {
    let path = record_path(ws_dir, project, rel)?;
    let meta = std::fs::symlink_metadata(&path).ok().filter(|m| m.is_file() && m.len() <= MAX_RECORD_BYTES);
    let Some(text) = meta.and_then(|_| std::fs::read_to_string(&path).ok()) else { return refuse("record_missing", format!("no readable record `{}` in project {}", rel, project.as_str())) };
    let Some(fm) = frontmatter::parse(&text).ok().and_then(|d| d.frontmatter) else { return refuse("missing_provenance", format!("`{}` has no readable frontmatter", rel)) };
    let source_type = frontmatter::get_str(&fm, "source_type").filter(|s| SOURCE_TYPES.contains(s));
    let author = frontmatter::get_str(&fm, "author").is_some_and(|a| !a.trim().is_empty());
    let Some(source_type) = source_type.filter(|_| author) else { return refuse("missing_provenance", format!("`{}` names no valid `source_type` and `author`", rel)) };
    if !frontmatter::get_str(&fm, "kind").is_some_and(|k| ELIGIBLE_KINDS.contains(&k)) {
        return refuse("kind_not_eligible", format!("`{}` is not a {} record", rel, ELIGIBLE_KINDS.join(" or ")));
    }
    let status = frontmatter::get_str(&fm, "status").unwrap_or("active");
    if status != "active" {
        return refuse("status_not_eligible", format!("`{}` is `{}`, not active", rel, status));
    }
    if flag(&fm, "private") || frontmatter::get_str(&fm, "visibility") == Some("private") {
        return refuse("record_private", format!("`{}` is marked private", rel));
    }
    let ctx = crate::distill::scan_context(ws_dir);
    if let Some(h) = delphi_bundle::scan::scan_text(&text, &ctx).into_iter().find(|h| h.fail && matches!(h.class, "secret" | "token" | "private-key")) {
        return refuse("contains_secret", format!("`{}` carries {} (line {})", rel, h.found, h.line));
    }
    Ok(Original { rel: rel.to_string(), blake3: lock::hash(text.as_bytes()), source_type: source_type.to_string() })
}

/// The deterministic eligibility check (`specs/058` §7.1, 058-A1): every record in `rels` must be an eligible, readable
/// memory record of `project`'s layer and the summary must be within bounds and free of secrets. No model, no write.
pub fn preflight(ws_dir: &Path, project: &ProjectId, rels: &[String], summary: &str) -> Result<Vec<Original>, Refusal> {
    if crate::distill::privacy(ws_dir).isolated {
        return refuse("workspace_isolated", "this workspace is isolated: nothing is promoted out of it");
    }
    if !scope::is_registered(ws_dir, project) {
        return refuse("scope_unavailable", format!("project {} is not registered in this workspace", project.as_str()));
    }
    if rels.is_empty() || rels.len() > MAX_RECORDS {
        return refuse("records_count", format!("name one to {} project records", MAX_RECORDS));
    }
    let originals = rels.iter().map(|r| read_record(ws_dir, project, r)).collect::<Result<Vec<_>, _>>()?;
    check_summary(ws_dir, summary)?;
    Ok(originals)
}

/// What home memory already holds that resembles a summary. Shown to the operator; never merged, never superseded.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub path: String,
    /// `duplicate`, `conflict` (close in words, but one side negates) or `related`.
    pub class: &'static str,
    pub overlap: f64,
}

impl Match {
    pub fn to_json(&self) -> Value {
        json!({"path": self.path, "class": self.class, "overlap": (self.overlap * 100.0).round() / 100.0})
    }
}

const NEGATIONS: [&str; 8] = ["not", "never", "no", "don't", "dont", "cannot", "avoid", "without"];

fn negated(s: &str) -> bool {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '\'').any(|w| NEGATIONS.contains(&w))
}

/// Home memory entries and notes near `summary`, best first (at most five). Reads home-layer files only.
pub fn home_matches(ws_dir: &Path, summary: &str) -> Vec<Match> {
    let a = crate::retro::words(summary);
    let mut out: Vec<Match> = Vec::new();
    for f in crate::kb::sources(ws_dir).into_iter().filter(|f| matches!(f.zone, "memory" | "knowledge" | "context" | "staged")) {
        let Ok(text) = std::fs::read_to_string(&f.path) else { continue };
        let body = frontmatter::parse(&text).ok().map(|d| (d.frontmatter, d.body));
        let Some((fm, body)) = body else { continue };
        if fm.as_ref().and_then(|m| frontmatter::get_str(m, "status")).is_some_and(|s| s != "active") {
            continue;
        }
        let best = body
            .lines()
            .filter(|l| l.split_whitespace().count() >= 3)
            .map(|l| {
                let b = crate::retro::words(l);
                let (i, u) = (a.intersection(&b).count() as f64, a.union(&b).count() as f64);
                (if u == 0.0 { 0.0 } else { i / u }, l.to_string())
            })
            .max_by(|x, y| x.0.total_cmp(&y.0));
        let Some((overlap, line)) = best.filter(|(o, _)| *o >= crate::retro::SUPERSESSION_AT) else { continue };
        let class = if negated(summary) != negated(&line) {
            "conflict"
        } else if overlap >= crate::retro::DUPLICATE_AT {
            "duplicate"
        } else {
            "related"
        };
        out.push(Match { path: f.rel_path, class, overlap });
    }
    out.sort_by(|x, y| y.overlap.total_cmp(&x.overlap).then_with(|| x.path.cmp(&y.path)));
    out.truncate(5);
    out
}

fn store(ws_dir: &Path) -> PathBuf {
    ws_dir.join("local/promoted")
}

/// A hash of the home records a promotion targets: the promoted entries and their bytes, in name order.
pub fn home_version(ws_dir: &Path) -> String {
    let mut names: Vec<(String, String)> = std::fs::read_dir(store(ws_dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), lock::hash(&std::fs::read(e.path()).ok()?))))
        .collect();
    names.sort();
    lock::hash(serde_json::to_string(&names).unwrap_or_default().as_bytes())
}

/// A promotion proposal that has not been resolved, read back from a mission's log.
#[derive(Debug, Clone, PartialEq)]
pub struct Promotion {
    pub mission: String,
    pub id: String,
    pub project: ProjectId,
    pub records: Vec<Original>,
    pub summary: String,
    pub uncertainty: String,
    pub destination: String,
    /// The home records' version when the proposal was made; acceptance needs it unchanged.
    pub home_version: String,
    pub matches: Vec<Match>,
    pub timestamp: String,
}

impl Promotion {
    /// `<mission>/<id>`, the same reference `/retro` uses.
    pub fn reference(&self) -> String {
        format!("{}/{}", self.mission, self.id)
    }

    /// What the operator reviews and acceptance is bound to: a hash of everything the proposal says and the home
    /// version it was made against.
    pub fn version(&self) -> String {
        let records: Vec<_> = self.records.iter().map(|r| json!([r.rel, r.blake3])).collect();
        lock::hash(json!([self.project.as_str(), records, self.summary, self.destination, self.home_version]).to_string().as_bytes())
    }

    pub fn to_json(&self) -> Value {
        json!({"kind": "promotion", "ref": self.reference(), "mission": self.mission, "id": self.id, "project": self.project.as_str(), "destination": self.destination,
               "summary": self.summary, "uncertainty": self.uncertainty, "records": self.records.iter().map(|r| json!({"rel": r.rel, "blake3": r.blake3, "source_type": r.source_type})).collect::<Vec<_>>(),
               "home_version": self.home_version, "version": self.version(), "matches": self.matches.iter().map(Match::to_json).collect::<Vec<_>>(), "at": self.timestamp})
    }
}

fn plain(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn one_line(s: &str, max: usize) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// The unresolved promotions of one mission: every `learn.proposed` with `op: promote` minus every
/// `learn.proposal_resolved` for its id. A malformed one is ignored, whatever wrote it.
pub fn live_promotions(mission: &str, events: &[EventEnvelope]) -> Vec<Promotion> {
    let mut out: Vec<Promotion> = Vec::new();
    for e in events {
        match e.kind.as_str() {
            "learn.proposed" if e.payload["op"] == "promote" => {
                let s = |k: &str| e.payload[k].as_str().unwrap_or("").to_string();
                let Ok(project) = ProjectId::parse(&s("project")) else { continue };
                let records: Vec<Original> = e.payload["records"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| Some(Original { rel: r["rel"].as_str()?.to_string(), blake3: r["blake3"].as_str()?.to_string(), source_type: r["source_type"].as_str()?.to_string() }))
                    .collect();
                if records.is_empty() || records.len() > MAX_RECORDS || s("summary").is_empty() || s("destination") != DESTINATION || !plain(mission) {
                    continue;
                }
                let matches = e.payload["matches"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| Some(Match { path: m["path"].as_str()?.to_string(), class: ["duplicate", "conflict", "related"].into_iter().find(|c| m["class"] == *c)?, overlap: m["overlap"].as_f64()? }))
                    .collect();
                out.push(Promotion { mission: mission.to_string(), id: format!("p{}", e.seq), project, records, summary: s("summary"), uncertainty: s("uncertainty"), destination: s("destination"), home_version: s("home_version"), matches, timestamp: e.timestamp.clone() });
            }
            "learn.proposal_resolved" => {
                if let Some(id) = e.payload["id"].as_str() {
                    out.retain(|p| p.id != id);
                }
            }
            _ => {}
        }
    }
    out
}

/// Every unresolved promotion in the workspace, newest first. Reads the logs and nothing else.
pub fn collect(ws_dir: &Path) -> Vec<Promotion> {
    let mut all = Vec::new();
    for entry in std::fs::read_dir(ws_dir.join("local/missions")).into_iter().flatten().flatten() {
        let Some(id) = entry.file_name().to_str().map(String::from) else { continue };
        if let Ok(events) = delphi_core::log::replay(&entry.path()) {
            all.extend(live_promotions(&id, &events));
        }
    }
    all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.reference().cmp(&a.reference())));
    all
}

/// The proposal event for `rels` of `project`, after [`preflight`]. The caller appends it to `mission`'s log; it
/// changes no memory. Matches in home memory are recorded in it for the operator to see.
pub fn propose(ws_dir: &Path, mission: &str, project: &ProjectId, rels: &[String], summary: &str, uncertainty: &str) -> Result<(EventEnvelope, Vec<Match>), Refusal> {
    if !plain(mission) {
        return refuse("mission_invalid", "not a mission id");
    }
    let summary = summary.trim();
    let originals = preflight(ws_dir, project, rels, summary)?;
    let matches = home_matches(ws_dir, summary);
    let payload = json!({
        "op": "promote", "scope": "home", "project": project.as_str(), "destination": DESTINATION, "summary": summary, "uncertainty": one_line(uncertainty, 300),
        "records": originals.iter().map(|r| json!({"rel": r.rel, "blake3": r.blake3, "source_type": r.source_type})).collect::<Vec<_>>(),
        "home_version": home_version(ws_dir), "matches": matches.iter().map(Match::to_json).collect::<Vec<_>>(), "tier": "L3",
    });
    Ok((EventEnvelope::new(mission, "learn.proposed", payload), matches))
}

/// What accepting wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct Accepted {
    pub entry: String,
    pub file: String,
    pub blake3: String,
    pub source_type: String,
    /// Home entries near the summary at the moment of acceptance; shown, never merged.
    pub matches: Vec<Match>,
}

fn rank(source_type: &str) -> usize {
    SOURCE_TYPES.iter().position(|s| *s == source_type).unwrap_or(0)
}

fn entry_text(p: &Promotion, source_type: &str, by: &str, now: &str) -> String {
    let records: Vec<String> = p.records.iter().map(|r| format!("  - {}:{}", p.project.label(), r.rel)).collect();
    format!(
        "---\nid: promoted-{id}\nsource_type: {st}\nauthor: delphi-core\nconfidence: low\nstatus: active\nkind: fact\ncreated: {now}\nlast_confirmed: {now}\npromoted_from: {label}\npromoted_records:\n{records}\npromoted_by: {by}\npromoted_at: {now}\nproposal: {reference}\n---\n# Promoted from {label}\n\nA summary the operator accepted as home memory. It validates no fact in it and raises no source: its tier is {st}.\n\n{summary}\n",
        id = entry_id(p),
        st = source_type,
        now = now,
        label = p.project.label(),
        records = records.join("\n"),
        by = one_line(by, 80),
        reference = p.reference(),
        summary = p.summary,
    )
}

fn entry_id(p: &Promotion) -> String {
    format!("{}-{}", p.mission, p.id)
}

fn lock_path(ws_dir: &Path) -> PathBuf {
    ws_dir.join("locks/promote.lock")
}

/// Accept `p` as home memory, the operator's own act (`by` names them). Under one lock: the version the operator
/// reviewed must still be the proposal's, the preflight runs again on the records as they are now (a record that became
/// private, retired or secret-bearing is refused), the records must be the ones the proposal pinned and the home records
/// unchanged since; only then is the entry written, create-new, so two accepts cannot both land.
pub fn accept(ws_dir: &Path, p: &Promotion, expected_version: &str, by: &str, now: &str) -> Result<Accepted, Refusal> {
    if !plain(&entry_id(p)) {
        return refuse("mission_invalid", "not a mission id");
    }
    lock::with_lock(&lock_path(ws_dir), || -> Result<Accepted, Refusal> {
        if p.version() != expected_version {
            return refuse("stale_proposal", "the proposal is not the version you reviewed; review it again");
        }
        let rels: Vec<String> = p.records.iter().map(|r| r.rel.clone()).collect();
        let now_records = preflight(ws_dir, &p.project, &rels, &p.summary)?;
        if now_records != p.records {
            return refuse("stale_proposal", "a project record changed after the proposal was made; propose it again");
        }
        if home_version(ws_dir) != p.home_version {
            return refuse("stale_home", "home memory changed after the proposal was made; review it again");
        }
        let lowest = p.records.iter().map(|r| r.source_type.as_str()).min_by_key(|s| rank(s)).unwrap_or("external_untrusted");
        let source_type = if rank(lowest) > rank(CEILING) { CEILING } else { lowest };
        let path = store(ws_dir).join(format!("{}.md", entry_id(p)));
        if path.exists() {
            return refuse("already_promoted", format!("{} is already a home entry", entry_id(p)));
        }
        let text = entry_text(p, source_type, by, now);
        lock::write_atomic(&path, text.as_bytes()).map_err(|e| Refusal { code: "io", detail: e.to_string() })?;
        Ok(Accepted { entry: entry_id(p), file: format!("local/promoted/{}.md", entry_id(p)), blake3: lock::hash(text.as_bytes()), source_type: source_type.to_string(), matches: home_matches(ws_dir, &p.summary) })
    })
    .map_err(|e| Refusal { code: "io", detail: e.to_string() })?
}

/// The operator's reason, masked of secrets and local detail and cut to one short line.
pub fn clean_reason(ws_dir: &Path, reason: &str) -> String {
    one_line(&crate::distill::mask(ws_dir, reason), MAX_REASON)
}

/// The event that closes a proposal: the outcome, who decided, and the operator's reason. It never carries the
/// proposal's summary or records, so nothing of the proposal is copied into another store.
pub fn rejection_event(ws_dir: &Path, p: &Promotion, by: &str, reason: &str) -> EventEnvelope {
    EventEnvelope::new(&p.mission, "learn.proposal_resolved", json!({"id": p.id, "resolution": "rejected", "detail": {"by": one_line(by, 80), "reason": clean_reason(ws_dir, reason)}}))
}

/// The event that closes an accepted proposal, naming the entry written.
pub fn acceptance_event(p: &Promotion, done: &Accepted, by: &str) -> EventEnvelope {
    EventEnvelope::new(&p.mission, "learn.proposal_resolved", json!({"id": p.id, "resolution": "accepted", "detail": {"by": one_line(by, 80), "entry": done.entry, "file": done.file, "blake3": done.blake3, "source_type": done.source_type}}))
}

/// A promoted entry that was retired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withdrawn {
    pub entry: String,
    /// `<mission>/<id>` of the proposal it came from.
    pub proposal: String,
    pub mission: String,
    pub proposal_id: String,
    /// The reference the log and the retired entry share.
    pub audit: String,
}

/// Retire the promoted entry `entry`: its status becomes `retired`, with who, when, why and an audit reference; the
/// body stays, the file stays, and nothing else is touched (the project's records and every source are untouched).
pub fn withdraw(ws_dir: &Path, entry: &str, by: &str, reason: &str, now: &str) -> Result<Withdrawn, Refusal> {
    if !plain(entry) {
        return refuse("entry_invalid", "not a promoted entry id");
    }
    let path = store(ws_dir).join(format!("{}.md", entry));
    lock::with_lock(&lock_path(ws_dir), || -> Result<Withdrawn, Refusal> {
        let Ok(text) = std::fs::read_to_string(&path) else { return refuse("entry_missing", format!("no promoted entry `{}`", entry)) };
        let doc = frontmatter::parse(&text).ok().and_then(|d| d.frontmatter);
        let Some(fm) = doc.filter(|m| frontmatter::has(m, "promoted_from")) else { return refuse("entry_invalid", "not a promoted entry") };
        if frontmatter::get_str(&fm, "status") != Some("active") {
            return refuse("not_active", format!("`{}` is already retired", entry));
        }
        let proposal = frontmatter::get_str(&fm, "proposal").unwrap_or("").to_string();
        let Some((mission, proposal_id)) = proposal.split_once('/').filter(|(m, i)| plain(m) && plain(i)) else { return refuse("entry_invalid", "the entry names no proposal") };
        let audit = format!("wd-{}", &lock::hash(format!("{}{}{}", text, now, by).as_bytes())[..12]);
        let close = text[4..].find("\n---\n").map(|i| i + 4).ok_or(Refusal { code: "entry_invalid", detail: "unclosed frontmatter".into() })?;
        let head = text[..close].replacen("\nstatus: active", "\nstatus: retired", 1);
        let note = format!("\nretired_at: {}\nretired_by: {}\nwithdrawal_audit: {}\nwithdrawal_reason: {:?}\nprevious_blake3: {}", now, one_line(by, 80), audit, clean_reason(ws_dir, reason), lock::hash(text.as_bytes()));
        let new = format!("{}{}{}", head, note, &text[close..]);
        lock::write_atomic(&path, new.as_bytes()).map_err(|e| Refusal { code: "io", detail: e.to_string() })?;
        Ok(Withdrawn { entry: entry.to_string(), proposal: proposal.clone(), mission: mission.to_string(), proposal_id: proposal_id.to_string(), audit })
    })
    .map_err(|e| Refusal { code: "io", detail: e.to_string() })?
}

/// The event that records a withdrawal in the log of the mission whose proposal was promoted.
pub fn withdrawal_event(w: &Withdrawn, by: &str) -> EventEnvelope {
    EventEnvelope::new(&w.mission, "learn.proposal_resolved", json!({"id": w.proposal_id, "resolution": "withdrawn", "detail": {"by": one_line(by, 80), "entry": w.entry, "audit": w.audit}}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::make_workspace;
    use std::fs;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const NOTE: &str = "staged/m1/n1.md";
    const SUMMARY: &str = "The release build must pin the toolchain version in CI images.";

    // Assembled so that `delphi lint` does not flag this file's own fixture.
    fn secret() -> String {
        format!("sk-ant-{}", "api03-abcdefghijklmnopqrstuvwx")
    }

    fn record(extra: &str, kind: &str, status: &str, body: &str) -> String {
        format!("---\nsource_type: internal_generated\nauthor: daemon\nconfidence: low\nstatus: {}\nkind: {}\n{}---\n# Staged note n1\n\n{}\n", status, kind, extra, body)
    }

    struct Fx {
        _d: tempfile::TempDir,
        ws: PathBuf,
    }

    impl Fx {
        fn layer(&self, id: &str, rel: &str) -> PathBuf {
            self.ws.join("local/projects").join(id).join(rel)
        }

        fn put(&self, id: &str, rel: &str, text: &str) {
            let p = self.layer(id, rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        }

        fn project(&self) -> ProjectId {
            ProjectId::parse(A).unwrap()
        }

        fn propose(&self, summary: &str) -> Result<Promotion, Refusal> {
            let (e, _) = propose(&self.ws, "m1", &self.project(), &[NOTE.to_string()], summary, "")?;
            let mut e = e;
            e.seq = 7;
            Ok(live_promotions("m1", &[e]).remove(0))
        }
    }

    fn fx() -> Fx {
        let d = tempfile::tempdir().unwrap();
        let ws = make_workspace(d.path()).dir;
        let projects: serde_json::Map<String, Value> = [A, B].iter().map(|id| (id.to_string(), json!({"project_id": id, "root": format!("/r/{}", id), "display_name": id, "created_unix": 1}))).collect();
        fs::write(ws.join("local/projects.json"), json!({"schema": 1, "projects": projects}).to_string()).unwrap();
        let f = Fx { _d: d, ws };
        f.put(A, NOTE, &record("", "fact", "active", "Pin the toolchain version in CI images, the build is flaky otherwise."));
        f
    }

    fn code<T: std::fmt::Debug>(r: Result<T, Refusal>) -> &'static str {
        r.unwrap_err().code
    }

    #[test]
    fn preflight_refuses_each_ineligible_record_with_a_stable_code() {
        let f = fx();
        let p = f.project();
        let go = |rel: &str, s: &str| preflight(&f.ws, &p, &[rel.to_string()], s);
        assert_eq!(go(NOTE, SUMMARY).unwrap()[0].source_type, "internal_generated");
        f.put(A, "staged/m1/priv.md", &record("private: true\n", "fact", "active", "x y z"));
        f.put(A, "staged/m1/vis.md", &record("visibility: private\n", "fact", "active", "x y z"));
        f.put(A, "staged/m1/sec.md", &record("", "fact", "active", &format!("the key is {}", secret())));
        f.put(A, "staged/m1/epi.md", &record("", "episode", "active", "x y z"));
        f.put(A, "staged/m1/old.md", &record("", "fact", "superseded", "x y z"));
        f.put(A, "staged/m1/prov.md", "---\nkind: fact\nstatus: active\n---\nno author or source type\n");
        f.put(A, "staged/m1/plain.md", "no frontmatter at all\n");
        assert_eq!(code(go("staged/m1/priv.md", SUMMARY)), "record_private");
        assert_eq!(code(go("staged/m1/vis.md", SUMMARY)), "record_private");
        assert_eq!(code(go("staged/m1/sec.md", SUMMARY)), "contains_secret");
        assert_eq!(code(go("staged/m1/epi.md", SUMMARY)), "kind_not_eligible");
        assert_eq!(code(go("staged/m1/old.md", SUMMARY)), "status_not_eligible");
        assert_eq!(code(go("staged/m1/prov.md", SUMMARY)), "missing_provenance");
        assert_eq!(code(go("staged/m1/plain.md", SUMMARY)), "missing_provenance");
        assert_eq!(code(go("staged/m1/none.md", SUMMARY)), "record_missing");
        assert_eq!(code(go("../a.md", SUMMARY)), "record_missing");
        assert_eq!(code(go("context/spec.md", SUMMARY)), "record_missing", "only memory directories of the layer");
        assert_eq!(code(go(NOTE, "too short")), "summary_size");
        assert_eq!(code(go(NOTE, &"x".repeat(MAX_SUMMARY + 1))), "summary_size");
        assert_eq!(code(go(NOTE, &format!("the key is {} ok", secret()))), "summary_not_redacted");
        assert_eq!(code(preflight(&f.ws, &ProjectId::parse(&"c".repeat(32)).unwrap(), &[NOTE.into()], SUMMARY)), "scope_unavailable");
        assert_eq!(code(preflight(&f.ws, &p, &[], SUMMARY)), "records_count");
        assert_eq!(code(preflight(&f.ws, &ProjectId::parse(B).unwrap(), &[NOTE.into()], SUMMARY)), "record_missing", "another project's layer does not hold it");
    }

    #[test]
    fn a_proposal_is_data_in_the_retro_flow_and_leaves_rules_and_home_untouched() {
        let f = fx();
        let (e, _) = propose(&f.ws, "m1", &f.project(), &[NOTE.to_string()], SUMMARY, "one build only").unwrap();
        assert_eq!((e.kind.as_str(), e.payload["op"].as_str(), e.payload["destination"].as_str()), ("learn.proposed", Some("promote"), Some(DESTINATION)));
        assert!(crate::learn::live_proposals("m1", std::slice::from_ref(&e)).is_empty(), "the rule queue of /retro does not list it");
        let p = f.propose(SUMMARY).unwrap();
        assert_eq!((p.project.as_str(), p.records[0].rel.as_str(), p.summary.as_str()), (A, NOTE, SUMMARY));
        assert_eq!(p.home_version, home_version(&f.ws));
        assert!(!f.ws.join("local/promoted").exists(), "proposing writes no memory");
        let refused = propose(&f.ws, "m1", &f.project(), &[NOTE.to_string()], "short", "").unwrap_err();
        assert_eq!(refused.code, "summary_size");
    }

    #[test]
    fn a_duplicate_or_conflict_in_home_memory_is_shown_and_never_merged() {
        let f = fx();
        let home = f.ws.join("local/memory/agents/builder/notes.md");
        fs::create_dir_all(home.parent().unwrap()).unwrap();
        let dup = "---\nsource_type: internal_generated\nauthor: builder\nstatus: active\n---\nThe release build must pin the toolchain version in CI images.\n";
        fs::write(&home, dup).unwrap();
        let p = f.propose(SUMMARY).unwrap();
        assert_eq!((p.matches[0].class, p.matches[0].path.as_str()), ("duplicate", "memory/agents/builder/notes.md"));
        let neg = f.propose("The release build must never pin the toolchain version in CI images.").unwrap();
        assert_eq!(neg.matches[0].class, "conflict");
        let done = accept(&f.ws, &p, &p.version(), "console-1", "2026-10-30T00:00:00Z").unwrap();
        assert_eq!(done.matches.iter().map(|m| m.class).collect::<Vec<_>>().first(), Some(&"duplicate"), "shown again at acceptance");
        assert_eq!(fs::read_to_string(&home).unwrap(), dup, "the home record is neither merged nor superseded");
    }

    #[test]
    fn accepting_writes_a_home_entry_with_provenance_that_stays_tainted_and_unvalidated() {
        let f = fx();
        f.put(A, "staged/m1/n2.md", &record("", "procedure", "active", "second"));
        let (e, _) = propose(&f.ws, "m1", &f.project(), &[NOTE.into(), "staged/m1/n2.md".into()], SUMMARY, "").unwrap();
        let mut e = e;
        e.seq = 3;
        let p = live_promotions("m1", &[e]).remove(0);
        let done = accept(&f.ws, &p, &p.version(), "console-9", "2026-10-30T12:00:00Z").unwrap();
        assert_eq!((done.entry.as_str(), done.source_type.as_str()), ("m1-p3", "internal_generated"));
        let text = fs::read_to_string(f.ws.join("local/promoted/m1-p3.md")).unwrap();
        for want in [&format!("promoted_from: project:{}", A), &format!("- project:{}:{}", A, NOTE), "promoted_by: console-9", "promoted_at: 2026-10-30T12:00:00Z", "proposal: m1/p3", "status: active", "confidence: low", SUMMARY] {
            assert!(text.contains(want), "{} in {}", want, text);
        }
        assert!(!text.contains("user_ratified") && !text.contains("validated\n"), "acceptance raises nothing: {}", text);
        let kb = crate::kb::sources(&f.ws);
        assert!(kb.iter().any(|s| s.rel_path == "memory/promoted/m1-p3.md" || s.path.ends_with("promoted/m1-p3.md")), "indexed as home memory");
        // A record below the ceiling keeps its lower tier.
        f.put(A, "staged/m1/n3.md", &record("", "fact", "active", "x y z").replace("internal_generated", "external_untrusted"));
        let (e, _) = propose(&f.ws, "m2", &f.project(), &["staged/m1/n3.md".into()], SUMMARY, "").unwrap();
        let mut e = e;
        e.seq = 1;
        let q = live_promotions("m2", &[e]).remove(0);
        assert_eq!(accept(&f.ws, &q, &q.version(), "c", "t").unwrap().source_type, "external_untrusted");
        // A ratified-tier record is capped: the summary is not validated by being accepted.
        f.put(A, "staged/m1/n4.md", &record("", "fact", "active", "x y z").replace("internal_generated", "user_ratified"));
        let (e, _) = propose(&f.ws, "m3", &f.project(), &["staged/m1/n4.md".into()], SUMMARY, "").unwrap();
        let mut e = e;
        e.seq = 1;
        let r = live_promotions("m3", &[e]).remove(0);
        assert_eq!(accept(&f.ws, &r, &r.version(), "c", "t").unwrap().source_type, "internal_generated");
    }

    #[test]
    fn a_stale_proposal_is_refused_and_must_be_reviewed_again() {
        let f = fx();
        let p = f.propose(SUMMARY).unwrap();
        assert_eq!(code(accept(&f.ws, &p, "not-the-reviewed-version", "c", "t")), "stale_proposal");
        // The record changed (still eligible): the proposal pinned its hash.
        f.put(A, NOTE, &record("", "fact", "active", "A different body that is still fine."));
        assert_eq!(code(accept(&f.ws, &p, &p.version(), "c", "t")), "stale_proposal");
        assert!(!f.ws.join("local/promoted").exists());
        // Home memory changed after the proposal.
        let g = fx();
        let q = g.propose(SUMMARY).unwrap();
        fs::create_dir_all(g.ws.join("local/promoted")).unwrap();
        fs::write(g.ws.join("local/promoted/other.md"), "x").unwrap();
        assert_eq!(code(accept(&g.ws, &q, &q.version(), "c", "t")), "stale_home");
    }

    #[test]
    fn a_record_that_turned_private_revoked_or_secret_after_the_proposal_is_refused_at_accept() {
        for (name, extra, status, body, want) in [
            ("private", "private: true\n", "active", "ok body".to_string(), "record_private"),
            ("revoked", "", "superseded", "ok body".to_string(), "status_not_eligible"),
            ("secret", "", "active", format!("key {}", secret()), "contains_secret"),
        ] {
            let f = fx();
            let p = f.propose(SUMMARY).unwrap();
            f.put(A, NOTE, &record(extra, "fact", "active", &body).replace("status: active", &format!("status: {}", status)));
            assert_eq!(code(accept(&f.ws, &p, &p.version(), "c", "t")), want, "{}", name);
            assert!(!f.ws.join("local/promoted").exists(), "{}: nothing was written", name);
        }
    }

    #[test]
    fn two_concurrent_accepts_of_one_proposal_land_once() {
        let f = fx();
        let p = f.propose(SUMMARY).unwrap();
        let results: Vec<Result<Accepted, Refusal>> = std::thread::scope(|s| {
            let (fr, pr) = (&f, &p);
            let hs: Vec<_> = (0..6).map(|i| s.spawn(move || accept(&fr.ws, pr, &pr.version(), &format!("c{}", i), "t"))).collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1, "{:?}", results);
        assert!(results.iter().filter_map(|r| r.as_ref().err()).all(|e| matches!(e.code, "stale_home" | "already_promoted")));
        assert_eq!(fs::read_dir(f.ws.join("local/promoted")).unwrap().count(), 1);
    }

    #[test]
    fn rejecting_records_the_outcome_and_the_reason_and_copies_no_proposal_text() {
        let f = fx();
        let p = f.propose(SUMMARY).unwrap();
        let e = rejection_event(&f.ws, &p, "console-1", &format!("too narrow   for home {}", secret()));
        assert_eq!(e.kind, "learn.proposal_resolved");
        let body = e.payload.to_string();
        assert!(body.contains("rejected") && body.contains("too narrow for home") && !body.contains(&secret()), "{}", body);
        assert!(!body.contains("toolchain") && !body.contains(NOTE) && !body.contains(A), "no summary, no record, no project: {}", body);
        assert!(!f.ws.join("local/promoted").exists(), "nothing is written to home");
        assert!(live_promotions("m1", &[EventEnvelope::new("m1", "learn.proposed", json!({"op": "promote"})), e]).is_empty());
    }

    #[test]
    fn withdrawing_retires_the_entry_with_an_audit_reference_and_deletes_nothing() {
        let f = fx();
        let p = f.propose(SUMMARY).unwrap();
        let done = accept(&f.ws, &p, &p.version(), "console-1", "2026-10-30T12:00:00Z").unwrap();
        let before = fs::read_to_string(f.ws.join(&done.file)).unwrap();
        let w = withdraw(&f.ws, &done.entry, "console-2", "no longer true", "2026-11-01T00:00:00Z").unwrap();
        let after = fs::read_to_string(f.ws.join(&done.file)).unwrap();
        assert!(after.contains("status: retired") && after.contains(&format!("withdrawal_audit: {}", w.audit)) && after.contains("retired_by: console-2") && after.contains("previous_blake3:"), "{}", after);
        assert!(after.contains(SUMMARY) && after.contains("promoted_from:"), "the entry stays");
        assert_ne!(before, after);
        assert_eq!((w.mission.as_str(), w.proposal_id.as_str()), ("m1", "p7"));
        assert_eq!(withdrawal_event(&w, "console-2").payload["detail"]["audit"], w.audit.as_str());
        assert_eq!(code(withdraw(&f.ws, &done.entry, "c", "r", "t")), "not_active");
        assert_eq!(code(withdraw(&f.ws, "nope", "c", "r", "t")), "entry_missing");
        assert_eq!(code(withdraw(&f.ws, "../x", "c", "r", "t")), "entry_invalid");
        assert!(f.layer(A, NOTE).is_file(), "the project's original is untouched");
        let retired_trusted = crate::frontmatter::parse(&after).unwrap().frontmatter.unwrap();
        assert_eq!(frontmatter::get_str(&retired_trusted, "status"), Some("retired"));
    }

    #[test]
    fn an_entry_that_is_not_one_the_daemon_promoted_cannot_be_withdrawn() {
        let f = fx();
        fs::create_dir_all(f.ws.join("local/promoted")).unwrap();
        fs::write(f.ws.join("local/promoted/hand.md"), "---\nstatus: active\n---\nbody\n").unwrap();
        assert_eq!(code(withdraw(&f.ws, "hand", "c", "r", "t")), "entry_invalid");
    }
}
