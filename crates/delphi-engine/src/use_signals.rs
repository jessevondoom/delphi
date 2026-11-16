//! Use feeds back (`specs/116` §7): the read-count report of `specs/080`, disputes (`flag_wrong`), what the daemon handed to spans
//! that then ended well, and the small ranking terms built on them.
//!
//! Everything here is a label or a share of a score. Nothing deletes, edits or promotes an item, and nothing loads into a prompt:
//!
//! * A **dispute** is one line in `local/memory/disputes.jsonl`, which only the daemon writes (`delphi_core::pathguard` denies
//!   `local/` to every agent). A disputed item keeps its place in search and gains the label `disputed by <who>`. It leaves
//!   *automatic* retrieval (the passages a span starts with) when the operator disputed it, or when [`DROP_AT`] disputes count; an
//!   explicit search still returns it, labelled. A dispute counts only when an agent inside the daemon made it from a span that had
//!   read nothing untrusted, and only once for each mission and role: a host's dispute, and one made after reading web text, is
//!   shown and never counted, so no page can tell an agent to bury a good item. The operator clears an item's disputes
//!   (`delphi kb disputes clear`); nothing else does.
//! * **Ended well** is one line per task in `local/memory/ended-well.jsonl`: the items the daemon itself handed to the task's spans as
//!   start passages, for a task that completed with no failed gate, from spans that ended `done`. An agent's own `kb_search` earns
//!   nothing here (it earns a provisional note a use, `specs/116` §2 item 6, which is a different count), and a provisional note
//!   earns no boost, so no agent can raise an item's rank by searching for it. One count a task.
//! * The **ranking terms** (`[retrieval] use_boost`, `dispute_penalty`, `confidence_weight`) are each off at zero, the default, and
//!   at most half of a result's score; each saturates at three. Read counts never feed ranking (`specs/080` §3).
//! * The **read-count report** counts `fact.read` (an agent's `kb_search`) and `kb.host_read` (a host's `search_kb`, fact ids and a
//!   hit count only, never the query) in the mission logs. It changes nothing.

use crate::kb::{Kb, Ranking};
use delphi_core::event::EventEnvelope;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The disputes ledger, under the workspace.
pub const DISPUTES: &str = "local/memory/disputes.jsonl";
/// The ended-well ledger, under the workspace.
pub const ENDED_WELL: &str = "local/memory/ended-well.jsonl";
/// Counted disputes that take an item out of automatic retrieval until the operator reviews it.
pub const DROP_AT: usize = 3;
/// The most a reason may say; the rest is cut.
pub const MAX_WHY: usize = 300;
/// The event a dispute writes in its mission's log: `{target, by, counted}`; the reason stays in the ledger.
pub const EVENT: &str = "kb.disputed";
/// The event a host's search writes: `{facts, hits}` (`specs/080` W1.2), never the query.
pub const HOST_READ: &str = "kb.host_read";
/// The `kb_meta` key the index reads disputes from, and the one it reads ended-well counts from.
pub const META_DISPUTES: &str = "disputes";
pub const META_ENDED_WELL: &str = "ended_well";
/// The tool's name, inside and for hosts.
pub const TOOL: &str = "flag_wrong";

/// What the inside agent and a host are told the tool does.
pub const DESCRIPTION: &str = "Say that a passage, claim, finding or note the knowledge base gave you is wrong, with `pointer` (as a search result gave it, such as `context/x.md#L3-L9`) and `why` (one or two sentences: what is wrong and how you know). Nothing is deleted or edited: the item keeps its place in search and is labelled `disputed by <role>`, and the operator reviews it. Once three agents' disputes count (one per mission and role, from a task that read nothing untrusted) it is no longer handed to agents automatically until the operator has looked; a dispute made after reading web or host text is shown and never counted. Use it for something you found to be wrong, never to rank one result above another.";

/// The host tool's description: the inside tool's words, and what differs for a host.
pub fn host_description() -> String {
    format!("{DESCRIPTION} As a host, your dispute is recorded and shown with the item as `disputed by host`, and the operator sees it; it never counts toward taking the item out of automatic retrieval, because Delphi cannot see what your session read. One dispute per session and item. Changes no file in your project.")
}

/// One dispute, as the ledger holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Dispute {
    /// The logical path of the item in the index.
    pub target: String,
    /// What the label says: a role id, `host` or `operator`.
    pub by: String,
    /// One dispute per `who` and target: `<mission>/<role>` inside, `host:<session>` for a host, `operator`.
    pub who: String,
    /// Whether it counts toward [`DROP_AT`].
    pub counted: bool,
    pub at: String,
    pub why: String,
}

/// The disputes of one item since the operator last cleared it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Disputed {
    /// Who disputed it, each once, in order.
    pub by: Vec<String>,
    /// How many count.
    pub counted: usize,
    /// The operator disputed it.
    pub operator: bool,
}

impl Disputed {
    /// Out of automatic retrieval until the operator reviews it.
    pub fn dropped(&self) -> bool {
        self.operator || self.counted >= DROP_AT
    }

    /// The share of the dispute penalty that applies: all of it once the item is dropped.
    pub fn weight(&self) -> f64 {
        if self.dropped() { 1.0 } else { self.counted.min(DROP_AT) as f64 / DROP_AT as f64 }
    }

    pub fn label(&self) -> String {
        format!("disputed by {}", self.by.join(", "))
    }
}

fn path(ws: &Path, rel: &str) -> PathBuf {
    ws.join(rel)
}

fn lines(p: &Path) -> Vec<Value> {
    std::fs::read_to_string(p).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn append(p: &Path, v: &Value) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = p.parent() {
        if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!("{} is a symlink", dir.display()));
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if std::fs::symlink_metadata(p).is_ok_and(|m| !m.is_file()) {
        return Err(format!("{} is not a plain file", p.display()));
    }
    std::fs::OpenOptions::new().create(true).append(true).open(p).and_then(|mut f| writeln!(f, "{v}")).map_err(|e| format!("{}: {e}", p.display()))
}

fn lock<T>(ws: &Path, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    delphi_core::lock::with_lock(&ws.join("locks/disputes.lock"), f).map_err(|e| format!("the disputes could not be locked: {e}"))?
}

fn clean(s: &str, max: usize) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// The disputes of every item, since each was last cleared. A line that does not read is skipped.
pub fn state(ws: &Path) -> BTreeMap<String, Disputed> {
    let mut out: BTreeMap<String, Disputed> = BTreeMap::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for v in lines(&path(ws, DISPUTES)) {
        if let Some(t) = v["cleared"].as_str() {
            out.remove(t);
            seen.retain(|(target, _)| target != t);
            continue;
        }
        let (Some(t), Some(by), Some(who)) = (v["target"].as_str(), v["by"].as_str(), v["who"].as_str()) else { continue };
        if !seen.insert((t.to_string(), who.to_string())) {
            continue;
        }
        let d = out.entry(t.to_string()).or_default();
        if !d.by.iter().any(|b| b == by) {
            d.by.push(by.to_string());
        }
        if who == "operator" {
            d.operator = true;
        } else if v["counted"] == true {
            d.counted += 1;
        }
    }
    out
}

/// Every dispute line since its item was last cleared, oldest first: what `delphi kb disputes` shows.
pub fn list(ws: &Path) -> Vec<Dispute> {
    let mut out: Vec<Dispute> = Vec::new();
    for v in lines(&path(ws, DISPUTES)) {
        if let Some(t) = v["cleared"].as_str() {
            out.retain(|d| d.target != t);
            continue;
        }
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        if s("target").is_empty() {
            continue;
        }
        out.push(Dispute { target: s("target"), by: s("by"), who: s("who"), counted: v["counted"] == true, at: s("at"), why: s("why") });
    }
    out
}

/// The item a pointer names, as the index files it: a search result's pointer (`path#L1-L9`, `@bundle/...`), or its path. `Err` when
/// it is not an item of the index.
pub fn resolve(db: &Path, pointer: &str) -> Result<String, String> {
    let p = pointer.trim();
    let p = p.split('#').next().unwrap_or("").trim().trim_start_matches("./");
    let rel = match p.strip_prefix("@bundle/") {
        Some(rest) => format!("bundles/{rest}"),
        None => p.to_string(),
    };
    if rel.is_empty() || rel.len() > 400 || rel.split('/').any(|s| s == "..") || rel.starts_with('/') {
        return Err("`pointer` is a search result's pointer or path, such as `context/x.md#L3-L9`".into());
    }
    let kb = Kb::open_readonly(db).map_err(|_| "the knowledge base has no index yet".to_string())?;
    if kb.has_document(&rel) {
        Ok(rel)
    } else {
        Err(format!("`{rel}` is not an item of the knowledge base; flag a pointer a search gave you"))
    }
}

/// Write `disputes` and `ended_well` into the index's `kb_meta`, where every search reads them. A failure leaves the last copy;
/// the next refresh writes them again.
pub fn publish(ws: &Path, kb: &Kb) -> Result<(), String> {
    let (d, e) = meta_values(ws);
    kb.set_meta(META_DISPUTES, &d).and_then(|_| kb.set_meta(META_ENDED_WELL, &e)).map_err(|e| e.to_string())
}

/// [`publish`] through the index file, for a caller that does not hold the index open.
pub fn publish_to(ws: &Path, db: &Path) -> Result<(), String> {
    if !db.is_file() {
        return Ok(());
    }
    let (d, e) = meta_values(ws);
    crate::kb::write_meta(db, &[(META_DISPUTES, &d), (META_ENDED_WELL, &e)]).map_err(|e| e.to_string())
}

fn meta_values(ws: &Path) -> (String, String) {
    let d: Vec<String> = state(ws).into_iter().map(|(t, d)| format!("{}\t{}\t{}\t{}", t, d.by.join(","), d.counted, u8::from(d.operator))).collect();
    let e: Vec<String> = ended_well(ws).into_iter().map(|(t, n)| format!("{t}\t{n}")).collect();
    (d.join("\n"), e.join("\n"))
}

/// Parse what [`publish`] wrote.
pub fn parse_disputes(meta: &str) -> BTreeMap<String, Disputed> {
    meta.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (t, by, n, op) = (f.next()?, f.next()?, f.next()?.parse().ok()?, f.next()? == "1");
            Some((t.to_string(), Disputed { by: by.split(',').filter(|b| !b.is_empty()).map(String::from).collect(), counted: n, operator: op }))
        })
        .collect()
}

pub fn parse_ended_well(meta: &str) -> BTreeMap<String, usize> {
    meta.lines().filter_map(|l| l.split_once('\t')).filter_map(|(t, n)| Some((t.to_string(), n.parse().ok()?))).collect()
}

/// Who flags, and whether it counts.
pub enum Flagger<'a> {
    /// An agent inside the daemon: its role, its mission, and whether its span had read untrusted content.
    Agent { role: &'a str, mission: &'a str, tainted: bool },
    /// A host session, by its mission (`host-<session>`).
    Host { mission: &'a str },
    Operator,
}

impl Flagger<'_> {
    fn by(&self) -> String {
        match self {
            Flagger::Agent { role, .. } => role.to_string(),
            Flagger::Host { .. } => "host".into(),
            Flagger::Operator => "operator".into(),
        }
    }

    fn who(&self) -> String {
        match self {
            Flagger::Agent { role, mission, .. } => format!("{mission}/{role}"),
            Flagger::Host { mission } => format!("host:{mission}"),
            Flagger::Operator => "operator".into(),
        }
    }

    fn counted(&self) -> bool {
        matches!(self, Flagger::Agent { tainted: false, .. })
    }
}

/// Record one dispute against the item `pointer` names, and publish the new state to the index. `Err` for an item the index does not
/// hold, an empty reason, or a second dispute of the same item by the same mission and role (or host session) since it was cleared.
pub fn flag(ws: &Path, db: &Path, pointer: &str, why: &str, who: Flagger, now: &str) -> Result<Value, String> {
    let target = resolve(db, pointer)?;
    let why = clean(why, MAX_WHY);
    if why.chars().count() < 3 {
        return Err("say `why`: what is wrong and how you know".into());
    }
    let d = Dispute { target: target.clone(), by: who.by(), who: who.who(), counted: who.counted(), at: now.to_string(), why };
    let state = lock(ws, || {
        if list(ws).iter().any(|o| o.target == d.target && o.who == d.who) {
            return Err(format!("`{}` is already disputed by this {}; the operator reviews it", d.target, if matches!(who, Flagger::Host { .. }) { "session" } else { "mission and role" }));
        }
        append(&path(ws, DISPUTES), &json!({"target": d.target, "by": d.by, "who": d.who, "counted": d.counted, "at": d.at, "why": d.why}))?;
        Ok(state(ws).remove(&d.target).unwrap_or_default())
    })?;
    if let Err(e) = publish_to(ws, db) {
        delphi_core::elog::warn("engine", "disputes_publish", &e, json!({"target": target}));
    }
    Ok(json!({"flagged": target, "label": state.label(), "counted": d.counted, "dropped_from_automatic": state.dropped(), "note": if d.counted { "recorded; nothing was deleted or edited, and the operator reviews it" } else { "recorded and shown with the item; it does not count toward taking it out of automatic retrieval (made by a host, or after reading untrusted content)" }}))
}

/// The operator reviewed an item: its disputes are cleared, and it is handed out automatically again.
pub fn clear(ws: &Path, db: &Path, pointer: &str, now: &str) -> Result<usize, String> {
    let target = match resolve(db, pointer) {
        Ok(t) => t,
        // An item that left the index may still carry disputes; the ledger's own name for it is enough.
        Err(_) => pointer.split('#').next().unwrap_or("").trim().to_string(),
    };
    let n = lock(ws, || {
        let n = list(ws).iter().filter(|d| d.target == target).count();
        if n == 0 {
            return Err(format!("`{target}` has no dispute"));
        }
        append(&path(ws, DISPUTES), &json!({"cleared": target, "at": now, "by": "operator"}))?;
        Ok(n)
    })?;
    publish_to(ws, db)?;
    Ok(n)
}

/// The logical paths of what the daemon handed a finished task's spans as start passages (`retrieval.done` `given`), when the task
/// completed with no failed gate, from the spans that ended `done`. Provisional notes are left out: their uses are their own count.
pub fn earned(events: &[EventEnvelope], task: &str) -> BTreeSet<String> {
    let of_task: Vec<&EventEnvelope> = events.iter().filter(|e| e.task_id.as_deref() == Some(task)).collect();
    let completed = of_task.iter().any(|e| e.kind == "task.completed" && e.payload["status"] != "escalated");
    let failed = of_task.iter().any(|e| e.kind == "task.failed" || e.kind == "task.cancelled" || (e.kind == "gates.evaluated" && e.payload["gates"].as_array().is_some_and(|g| g.iter().any(|g| g["state"] != "Green"))));
    if !completed || failed {
        return BTreeSet::new();
    }
    let done: BTreeSet<&str> = of_task.iter().filter(|e| e.kind == "dag.node.state" && e.payload["state"] == "done").filter_map(|e| e.payload["node_id"].as_str()).collect();
    of_task
        .iter()
        .filter(|e| e.kind == "retrieval.done" && e.span_id.as_deref().is_some_and(|s| done.contains(s)))
        .flat_map(|e| e.payload["given"].as_array().into_iter().flatten())
        .filter_map(|v| v.as_str())
        .filter(|r| crate::provisional_notes::label(r, "").is_none())
        .map(String::from)
        .collect()
}

/// Record what a finished task earned, once for the task.
pub fn record_ended_well(ws: &Path, mission: &str, task: &str, paths: &BTreeSet<String>, now: &str) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    lock(ws, || {
        if lines(&path(ws, ENDED_WELL)).iter().any(|v| v["mission"] == mission && v["task"] == task) {
            return Ok(());
        }
        append(&path(ws, ENDED_WELL), &json!({"at": now, "mission": mission, "task": task, "given": paths}))
    })
}

/// How many tasks that ended well each item was handed to.
pub fn ended_well(ws: &Path) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for v in lines(&path(ws, ENDED_WELL)) {
        let key = (v["mission"].as_str().unwrap_or("").to_string(), v["task"].as_str().unwrap_or("").to_string());
        if key.1.is_empty() || !seen.insert(key) {
            continue;
        }
        for p in v["given"].as_array().into_iter().flatten().filter_map(|p| p.as_str()) {
            *out.entry(p.to_string()).or_default() += 1;
        }
    }
    out
}

/// The ranking terms as the operator set them in `config.toml`, read at each search; all off when the file cannot be read as written.
pub fn ranking(ws: &Path) -> Ranking {
    let c = std::fs::read_to_string(ws.join("config.toml")).ok().and_then(|t| delphi_adapter::config::RetrievalConfig::read(&t)).unwrap_or_default();
    Ranking { use_boost: c.use_boost, dispute_penalty: c.dispute_penalty, confidence_weight: c.confidence_weight }
}

/// The reads of one fact: by agents inside the daemon (`fact.read`), and by hosts (`kb.host_read`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reads {
    pub agent: usize,
    pub host: usize,
}

/// Reads per fact over every mission log of the workspace, at or after `since` (an RFC 3339 time or a date) when given (`specs/080`
/// §2). It reads the logs and writes nothing.
pub fn read_counts(ws: &Path, since: Option<&str>) -> BTreeMap<String, Reads> {
    let mut out: BTreeMap<String, Reads> = BTreeMap::new();
    for (_, dir) in delphi_core::sessions::dirs(ws) {
        let Ok(events) = delphi_core::log::replay(&dir) else { continue };
        for e in events.iter().filter(|e| since.is_none_or(|s| e.timestamp.as_str() >= s)) {
            if e.kind == "validator.event" && e.payload["kind"] == "fact.read" {
                if let Some(f) = e.payload["fact"].as_str() {
                    out.entry(f.to_string()).or_default().agent += 1;
                }
            } else if e.kind == HOST_READ {
                for f in e.payload["facts"].as_array().into_iter().flatten().filter_map(|f| f.as_str()) {
                    out.entry(f.to_string()).or_default().host += 1;
                }
            }
        }
    }
    out
}

/// The fact ids among the logical paths of a search's results.
pub fn fact_ids<'a>(rel_paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in rel_paths.into_iter().filter_map(|p| p.strip_prefix("context/facts/")?.strip_suffix(".md")) {
        if !out.iter().any(|o| o == id) {
            out.push(id.to_string());
        }
    }
    out
}

/// The report `delphi kb reads` prints: the most-read facts first, with agent and host reads as two columns. The zero-read list is
/// withheld until facts can carry the operator's `invariant` tag (`specs/080` W1.3), so that no security or constraint fact is ever
/// listed as unread.
pub fn render_reads(counts: &BTreeMap<String, Reads>, top: usize, since: Option<&str>) -> String {
    let mut rows: Vec<(&String, &Reads)> = counts.iter().collect();
    rows.sort_by(|a, b| (b.1.agent + b.1.host).cmp(&(a.1.agent + a.1.host)).then_with(|| a.0.cmp(b.0)));
    let mut out = format!("reads per fact{}: agent reads (an agent's kb_search) and host reads (a host's search_kb). A direct file read is not counted.\n", since.map(|s| format!(" since {s}")).unwrap_or_default());
    if rows.is_empty() {
        out.push_str("no fact has been read yet\n");
        return out;
    }
    out.push_str(&format!("{:>6}  {:>6}  fact\n", "agent", "host"));
    for (f, r) in rows.iter().take(top) {
        out.push_str(&format!("{:>6}  {:>6}  {}\n", r.agent, r.host, f));
    }
    if rows.len() > top {
        out.push_str(&format!("({} more; --top <n> shows more)\n", rows.len() - top));
    }
    out.push_str("Counts are evidence for a decision you make; nothing is pruned or demoted from them, and they never change ranking. The list of facts never read is withheld until facts can carry an `invariant` tag (specs/080 W1.3).\n");
    out
}

/// The tool's input, for the inside agent and for a host (which adds its session fields).
pub fn input_schema() -> Value {
    json!({"type": "object", "required": ["pointer", "why"], "additionalProperties": false, "properties": {
        "pointer": {"type": "string", "maxLength": 400, "description": "The pointer or path a search result gave, such as `context/x.md#L3-L9`."},
        "why": {"type": "string", "maxLength": MAX_WHY, "description": "What is wrong and how you know, in a sentence or two."}
    }})
}

/// A dispute writes one line of the daemon's own ledger and changes no item: it never asks.
fn classify(tool: &str, args: &Value) -> Result<crate::tools::Classified, crate::tools::ToolError> {
    if args.get("pointer").and_then(Value::as_str).is_none() {
        return Err(crate::tools::ToolError("missing string argument `pointer`".into()));
    }
    Ok(crate::tools::Classified { tool: tool.into(), tier: crate::tools::ToolTier::Green, kind: crate::tools::ActionKind::Read, summary: tool.into(), payload: json!({"pointer": args["pointer"]}), pattern: tool.into() })
}

/// The inside tool: the role and mission are the daemon's, and whether the dispute counts is the span's own taint, never the agent's say.
fn flag_wrong(ctx: &mut crate::tools::ToolCtx, args: &Value) -> crate::tools::ToolResult {
    use crate::tools::ToolResult;
    let (Some(pointer), why) = (args["pointer"].as_str(), args["why"].as_str().unwrap_or("")) else { return ToolResult::err("missing string argument `pointer`") };
    let Some(ws) = ctx.guard.zones.local.parent() else { return ToolResult::err("no workspace") };
    let tainted = ctx.span.output_tier(delphi_core::provenance::OutputCheck::Unverified).is_external();
    let db = ctx.guard.zones.local.join("state.db");
    match flag(ws, &db, pointer, why, Flagger::Agent { role: ctx.role, mission: ctx.mission_id, tainted }, ctx.now_rfc3339) {
        Ok(r) => {
            ctx.events.push((EVENT.into(), json!({"target": r["flagged"], "by": ctx.role, "counted": r["counted"]})));
            ToolResult::ok(r.to_string())
        }
        Err(e) => ToolResult::err(e),
    }
}

/// `flag_wrong` (`specs/116` §7 item 2), offered to every role.
pub fn specs() -> Vec<crate::tools::ToolSpec> {
    vec![crate::tools::ToolSpec { def: delphi_adapter::claude::ToolDef { name: TOOL.into(), description: DESCRIPTION.into(), input_schema: input_schema() }, requires_write_scope: false, classify, exec: crate::tools::ToolExec::Local(flag_wrong) }]
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-11-15T10:00:00Z";

    fn ws() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path().join("ws");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let db = ws.join("local/state.db");
        let mut kb = Kb::open(&db).unwrap();
        for (rel, zone) in [("context/runbook.md", "context"), ("context/facts/f1.md", "context"), ("missions/m1/findings/why.md", "findings")] {
            kb.upsert_from(rel, zone, "internal_generated", "# Kestrel\n\nThe kestrel hovers over the verge.\n", "t").unwrap();
        }
        (t, ws, db)
    }

    fn agent<'a>(role: &'a str, mission: &'a str) -> Flagger<'a> {
        Flagger::Agent { role, mission, tainted: false }
    }

    /// `specs/116` §7 item 2: a dispute labels and never deletes; three that count, or the operator's one, take an item out of
    /// automatic retrieval; nothing else does.
    #[test]
    fn three_counted_disputes_or_the_operators_one_drop_an_item_and_nothing_is_deleted() {
        let (_t, ws, db) = ws();
        let r = flag(&ws, &db, "context/runbook.md#L3-L3", "the verge was mowed in 2025, the kestrel left", agent("builder", "m1"), NOW).unwrap();
        assert_eq!((r["flagged"].as_str(), r["label"].as_str(), r["counted"].as_bool(), r["dropped_from_automatic"].as_bool()), (Some("context/runbook.md"), Some("disputed by builder"), Some(true), Some(false)));
        // The same mission and role cannot count twice, however it words it.
        assert!(flag(&ws, &db, "context/runbook.md", "still wrong, really", agent("builder", "m1"), NOW).unwrap_err().contains("already disputed"));
        // A host's dispute and a tainted span's are shown and never counted, however many.
        for s in ["host-a", "host-b", "host-c"] {
            assert_eq!(flag(&ws, &db, "context/runbook.md", "a web page said so", Flagger::Host { mission: s }, NOW).unwrap()["counted"], false);
        }
        for m in ["m7", "m8", "m9"] {
            assert_eq!(flag(&ws, &db, "context/runbook.md", "a web page said so", Flagger::Agent { role: "scout", mission: m, tainted: true }, NOW).unwrap()["counted"], false);
        }
        let s = &state(&ws)["context/runbook.md"];
        assert_eq!((s.counted, s.dropped(), s.label().as_str()), (1, false, "disputed by builder, host, scout"));
        flag(&ws, &db, "context/runbook.md", "the survey of 2026 has no kestrel", agent("investigator", "m2"), NOW).unwrap();
        let third = flag(&ws, &db, "context/runbook.md", "no kestrel since spring", agent("builder", "m3"), NOW).unwrap();
        assert_eq!(third["dropped_from_automatic"], true, "three counted disputes");
        // The operator's one dispute drops an item at once; clearing it is the operator's review.
        assert_eq!(flag(&ws, &db, "missions/m1/findings/why.md", "the finding is wrong", Flagger::Operator, NOW).unwrap()["dropped_from_automatic"], true);
        assert_eq!(clear(&ws, &db, "context/runbook.md", NOW).unwrap(), 9, "every dispute line of the item");
        assert!(!state(&ws).contains_key("context/runbook.md"));
        assert!(flag(&ws, &db, "context/runbook.md", "it is wrong again", agent("builder", "m1"), NOW).is_ok(), "after a review the same agent may dispute anew");
        // The index holds what the searches read.
        let kb = Kb::open_readonly(&db).unwrap();
        let d = parse_disputes(&kb.meta(META_DISPUTES).unwrap_or_default());
        assert!(d["missions/m1/findings/why.md"].dropped() && !d["context/runbook.md"].dropped());
        // Only items of the index can be flagged, and no path leaves it.
        for bad in ["context/nope.md", "../etc/passwd", "/etc/passwd", ""] {
            assert!(flag(&ws, &db, bad, "wrong", agent("builder", "m4"), NOW).is_err(), "{bad}");
        }
        assert!(flag(&ws, &db, "context/facts/f1.md", "", agent("builder", "m4"), NOW).is_err(), "a reason is required");
    }

    fn ev(kind: &str, task: &str, span: Option<&str>, payload: Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m1", kind, payload);
        e.task_id = Some(task.into());
        e.span_id = span.map(String::from);
        e
    }

    /// Only what the daemon handed a span at its start counts, once a task, for a task that ended well; an agent's own search and a
    /// provisional note earn nothing.
    #[test]
    fn ended_well_counts_only_start_passages_of_spans_that_ended_done_once_a_task() {
        let (_t, ws, _db) = ws();
        let note = "provisional/builder/2026-11-15-m1-n1.md";
        let log = vec![
            ev("retrieval.done", "t1", Some("sp1"), json!({"given": ["context/runbook.md", note], "provisional": [note]})),
            ev(crate::provisional_notes::RETRIEVED, "t1", Some("sp1"), json!({"notes": [note]})),
            ev("validator.event", "t1", Some("sp1"), json!({"kind": "fact.read", "fact": "f1", "span": "sp1", "role": "builder"})),
            ev("retrieval.done", "t1", Some("sp2"), json!({"given": ["missions/m1/findings/why.md"]})),
            ev("dag.node.state", "t1", Some("sp1"), json!({"node_id": "sp1", "state": "done"})),
            ev("dag.node.state", "t1", Some("sp2"), json!({"node_id": "sp2", "state": "escalated"})),
            ev("task.completed", "t1", None, json!({"status": "done"})),
        ];
        let got = earned(&log, "t1");
        assert_eq!(got, ["context/runbook.md".to_string()].into());
        for _ in 0..3 {
            record_ended_well(&ws, "m1", "t1", &got, NOW).unwrap();
        }
        record_ended_well(&ws, "m1", "t2", &got, NOW).unwrap();
        assert_eq!(ended_well(&ws)["context/runbook.md"], 2, "one count a task, however often it is recorded");
        let mut failed = log.clone();
        failed.push(ev("gates.evaluated", "t1", None, json!({"gates": [{"state": "Red"}]})));
        assert!(earned(&failed, "t1").is_empty());
    }

    /// `specs/080`: agent and host reads are two columns; the query is never in the event, and the report changes nothing.
    #[test]
    fn the_read_report_counts_agent_and_host_reads_apart_and_never_the_query() {
        let (_t, ws, _db) = ws();
        let dir = ws.join("local/sessions/m1");
        std::fs::create_dir_all(&dir).unwrap();
        let mut log = delphi_core::log::EventLog::open(&dir, delphi_core::log::LogOptions::default()).unwrap();
        for f in ["f1", "f1", "f2"] {
            log.append(EventEnvelope::new("m1", "validator.event", json!({"kind": "fact.read", "fact": f, "span": "s", "role": "builder"}))).unwrap();
        }
        log.append(EventEnvelope::new("m1", HOST_READ, json!({"facts": fact_ids(["context/facts/f2.md", "context/facts/f2.md", "context/x.md"]), "hits": 3}))).unwrap();
        drop(log);
        let c = read_counts(&ws, None);
        assert_eq!((c["f1"], c["f2"]), (Reads { agent: 2, host: 0 }, Reads { agent: 1, host: 1 }));
        let text = render_reads(&c, 10, None);
        assert!(text.contains("     2       0  f1") && text.contains("     1       1  f2") && text.contains("withheld"), "{text}");
        assert!(read_counts(&ws, Some("2999-01-01")).is_empty());
    }

    /// `specs/116` §7 item 3: each term is off at zero, moves a result by a bounded share, and reads only what the daemon wrote. A file
    /// that claims `confidence: high` outside the fact store, and a provisional note handed out often, gain nothing.
    #[test]
    fn the_ranking_terms_are_off_at_zero_bounded_and_read_only_what_the_daemon_wrote() {
        use crate::kb::{Legs, SearchOpts};
        let t = tempfile::tempdir().unwrap();
        let ws = t.path().join("ws");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let db = ws.join("local/state.db");
        let mut kb = Kb::open(&db).unwrap();
        let doc = |conf: &str| format!("---\nsource_type: internal_generated\nconfidence: {conf}\n---\n# Plover\n\nThe plover nests on shingle.\n");
        // Four documents that match a query equally.
        for (rel, conf) in [("context/a.md", "low"), ("context/b.md", "high"), ("context/facts/c.md", "high"), ("context/d.md", "low")] {
            kb.upsert_from(rel, "context", "external_untrusted", &doc(conf), "t").unwrap();
        }
        let order = |kb: &Kb, r: Ranking| -> Vec<String> { kb.search("plover shingle", &SearchOpts { legs: Legs::LexicalOnly, ranking: r, limit: 10, ..Default::default() }).unwrap().into_iter().map(|h| h.rel_path).collect() };
        let plain = order(&kb, Ranking::default());
        assert_eq!(plain.len(), 4);
        // Confidence: only the fact store's word counts; `context/b.md` says `high` of itself and stays where it was.
        let conf = order(&kb, Ranking { confidence_weight: 0.5, ..Default::default() });
        assert_eq!(conf[0], "context/facts/c.md");
        let rest = |v: &[String]| v.iter().filter(|p| *p != "context/facts/c.md").cloned().collect::<Vec<_>>();
        assert_eq!(rest(&conf), rest(&plain), "the others keep their order: a file's own `confidence: high` counts for nothing");
        // Ended well and disputes, as the daemon published them.
        let last = plain[3].clone();
        let first = plain[0].clone();
        record_ended_well(&ws, "m1", "t1", &[last.clone()].into(), NOW).unwrap();
        flag(&ws, &db, &first, "the plover left the shingle", agent("builder", "m1"), NOW).unwrap();
        publish(&ws, &kb).unwrap();
        assert_eq!(order(&kb, Ranking::default()), plain, "every term off: the order is unchanged");
        let boosted = order(&kb, Ranking { use_boost: 0.5, ..Default::default() });
        assert_eq!(boosted[0], last, "the item spans that ended well were handed rises");
        let penalised = order(&kb, Ranking { dispute_penalty: 0.5, ..Default::default() });
        assert_eq!(penalised[3], first, "the disputed item sinks, and is still returned");
        // Bounded: the factor stays within [0.5, 1.5] for each term, whatever the counts.
        let r = Ranking { use_boost: 9.0, dispute_penalty: 9.0, confidence_weight: 9.0 };
        assert_eq!((r.factor(100, 0.0, "low"), r.factor(0, 1.0, "low"), r.factor(0, 0.0, "high")), (1.5, 0.5, 1.5));
        assert_eq!(Ranking::default().factor(100, 1.0, "high"), 1.0);
        // A provisional note handed out in many tasks earns no boost.
        let note = "provisional/builder/2026-11-15-m1-n1.md";
        let ev = |task: &str| {
            let mut e = EventEnvelope::new("m1", "retrieval.done", json!({"given": [note]}));
            e.task_id = Some(task.into());
            e.span_id = Some("sp".into());
            e
        };
        let mut log = vec![ev("t9")];
        let mut done = EventEnvelope::new("m1", "dag.node.state", json!({"node_id": "sp", "state": "done"}));
        done.task_id = Some("t9".into());
        log.push(done);
        let mut fin = EventEnvelope::new("m1", "task.completed", json!({"status": "done"}));
        fin.task_id = Some("t9".into());
        log.push(fin);
        assert!(earned(&log, "t9").is_empty());
    }
}
