//! The blind audit, the PROMOTE report and the retraction report (`specs/040` §15.2, §15.3, §15.4, V2c).
//!
//! Everything here is written by the daemon into its own validation state (`<workspace>/local/validation/`, which no
//! agent can read or write) and read back by the operator's commands. The journal holds ids, counts and verdict
//! classes, never claim text: the claim lives in the mission record (§15.3), and a card fetches it from there.
//!
//! * A **card** is made for every would-ratify (advisory) or ratified fact until volume passes 50 a week, then for a
//!   sample stratified by source class and origin; non-ratified verdicts are sampled at random, at most 5 a week (§15.3 item 2).
//! * The operator answers a card **before** seeing the sources and the verdict: [`answer`] records the answer first,
//!   and [`reveal`] refuses a card nobody answered. `unsure` counts for nothing.
//! * A fact the operator calls false lowers the latch to `advisory` and quarantines its batch ([`answer`]); an
//!   unanswered card older than 7 days quarantines its fact ([`sweep_stale`]). Neither can be undone from here.
//! * [`report`] reads only the journal, so no verdict text a span could influence can move it.

use crate::fact_mode::{lower_latch, read_latch, Effective, Latch};
use crate::fact_store::{fact_id, FactStore};
use delphi_adapter::config::ValidatorMode;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const STALE_AUDIT_SECS: i64 = 7 * 86_400;
pub const WEEK_SECS: i64 = 7 * 86_400;
/// Every positive verdict gets a card until the week's volume passes this.
pub const FULL_AUDIT_PER_WEEK: usize = 50;
pub const MAX_NEGATIVE_CARDS_PER_WEEK: usize = 5;
/// Past [`FULL_AUDIT_PER_WEEK`], each source-class-and-origin stratum is audited at least this share of the time.
pub const SAMPLE_PERCENT: usize = 20;
pub const PROMOTE_MIN_AUDITS: usize = 60;
pub const PROMOTE_MIN_MISSIONS: usize = 10;
pub const PROMOTE_MIN_LABELLED: usize = 40;
/// No origin may supply more than this share of the audited facts (§15.4).
pub const MAX_ORIGIN_SHARE: f64 = 0.20;
/// §20.1: the validator's share of a mission's tokens is at most 15%, which is `V <= 0.176 * O`.
pub const SHARE_FACTOR: f64 = 0.176;
/// §20.1 asks for a floor measured after V1; until then this provisional one keeps a small mission from being flagged.
pub const FLOOR_TOKENS: u64 = 50_000;

const JOURNAL: &str = "journal.jsonl";
const BATCH: &str = "batch.json";

pub struct Journal {
    dir: PathBuf,
}

impl Journal {
    pub fn new(state: &Path) -> Journal {
        Journal { dir: state.to_path_buf() }
    }

    /// Append one `validator.event` record (`kind` is verdict, card, audit, check, retract, latch or fact.read).
    pub fn append(&self, kind: &str, body: Value, at: i64) {
        let mut rec = json!({"type": "validator.event", "kind": kind, "at": at});
        if let (Some(r), Some(b)) = (rec.as_object_mut(), body.as_object()) {
            for (k, v) in b {
                r.insert(k.clone(), v.clone());
            }
        }
        if std::fs::create_dir_all(&self.dir).is_ok() {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join(JOURNAL)) {
                let _ = writeln!(f, "{}", rec);
            }
        }
    }

    pub fn all(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.join(JOURNAL)).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }
}

/// The mode period new records belong to. It changes only when the operator raises the latch.
pub fn current_batch(state: &Path) -> String {
    std::fs::read(state.join(BATCH)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v["id"].as_str().map(String::from)).unwrap_or_else(|| "b1".into())
}

pub fn start_new_batch(state: &Path) -> String {
    let n: u32 = current_batch(state).trim_start_matches('b').parse().unwrap_or(1) + 1;
    let id = format!("b{}", n);
    let _ = std::fs::create_dir_all(state);
    let _ = std::fs::write(state.join(BATCH), json!({"id": id}).to_string());
    id
}

/// What the daemon saw in one finished check, with no claim text in it.
pub struct Observed {
    pub fact_id: String,
    pub mission: String,
    pub verdict: String,
    pub would_ratify: bool,
    pub ratified: bool,
    pub origins: Vec<String>,
    pub class: String,
    pub tokens: u64,
}

fn positive(v: &Value) -> bool {
    v["would_ratify"].as_bool().unwrap_or(false) || v["ratified"].as_bool().unwrap_or(false)
}

/// Record a finished check and decide whether it needs a card. `roll` is the daemon's own random draw (it picks the
/// negative sample; the positive one is stratified and needs none).
pub fn observe(state: &Path, o: &Observed, roll: u32, now: i64) -> bool {
    let j = Journal::new(state);
    let batch = current_batch(state);
    j.append(
        "verdict",
        json!({"fact_id": o.fact_id, "mission": o.mission, "verdict": o.verdict, "would_ratify": o.would_ratify, "ratified": o.ratified, "origins": o.origins, "class": o.class, "tokens": o.tokens, "batch": batch}),
        now,
    );
    let events = j.all();
    if events.iter().any(|e| e["kind"] == "card" && e["fact_id"] == o.fact_id.as_str()) {
        return false;
    }
    let week: Vec<&Value> = events.iter().filter(|e| e["kind"] == "card" && e["at"].as_i64().unwrap_or(0) > now - WEEK_SECS).collect();
    let is_positive = o.would_ratify || o.ratified;
    let wanted = if is_positive {
        let positives = week.iter().filter(|c| positive(c)).count();
        positives < FULL_AUDIT_PER_WEEK || stratum_wants_card(&events, o, now)
    } else {
        matches!(o.verdict.as_str(), "refuted" | "unsupported" | "already_known") && week.iter().filter(|c| !positive(c)).count() < MAX_NEGATIVE_CARDS_PER_WEEK && roll % 4 == 0
    };
    if wanted {
        j.append(
            "card",
            json!({"card": o.fact_id, "fact_id": o.fact_id, "mission": o.mission, "batch": batch, "verdict": o.verdict, "would_ratify": o.would_ratify, "ratified": o.ratified, "sampled": !is_positive, "origins": o.origins, "class": o.class}),
            now,
        );
    }
    wanted
}

/// Past the weekly volume the sample is stratified by source class and origin (§15.3 item 2), not drawn by a coin: a
/// stratum with no card this week always gets one, and every stratum is kept at [`SAMPLE_PERCENT`] of its own
/// positive verdicts, so a flood from one origin cannot thin the audit of another.
fn stratum_wants_card(events: &[Value], o: &Observed, now: i64) -> bool {
    let in_week = |e: &Value| e["at"].as_i64().unwrap_or(0) > now - WEEK_SECS;
    let origin = o.origins.first().map(String::as_str).unwrap_or("");
    let in_stratum = |e: &Value| e["class"] == o.class.as_str() && e["origins"].get(0).and_then(|x| x.as_str()).unwrap_or("") == origin;
    let verdicts: BTreeSet<&str> = events.iter().filter(|e| e["kind"] == "verdict" && in_week(e) && positive(e) && in_stratum(e)).filter_map(|e| e["fact_id"].as_str()).collect();
    let cards = events.iter().filter(|e| e["kind"] == "card" && in_week(e) && positive(e) && in_stratum(e)).count();
    cards == 0 || cards * 100 < SAMPLE_PERCENT * verdicts.len()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    True,
    False,
    Unsure,
}

impl Answer {
    pub fn parse(s: &str) -> Option<Answer> {
        match s.trim().to_lowercase().as_str() {
            "true" | "t" => Some(Answer::True),
            "false" | "f" => Some(Answer::False),
            "unsure" | "u" => Some(Answer::Unsure),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Answer::True => "true",
            Answer::False => "false",
            Answer::Unsure => "unsure",
        }
    }
}

/// A card nobody has answered, with its age.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    pub card: String,
    pub mission: String,
    pub positive: bool,
    pub at: i64,
}

pub fn open_cards(state: &Path) -> Vec<Open> {
    let events = Journal::new(state).all();
    let answered: BTreeSet<String> = events.iter().filter(|e| e["kind"] == "audit").filter_map(|e| e["card"].as_str().map(String::from)).collect();
    events
        .iter()
        .filter(|e| e["kind"] == "card")
        .filter_map(|c| Some(Open { card: c["card"].as_str()?.into(), mission: c["mission"].as_str()?.into(), positive: positive(c), at: c["at"].as_i64()? }))
        .filter(|c| !answered.contains(&c.card))
        .collect()
}

/// `(count, oldest age in seconds)` of the unanswered cards, for `delphi needs`.
pub fn backlog(state: &Path, now: i64) -> (usize, i64) {
    let open = open_cards(state);
    (open.len(), open.iter().map(|c| now - c.at).max().unwrap_or(0))
}

fn mission_verdict(ws: &Path, mission: &str, fact: &str) -> Option<Value> {
    if mission.contains(['/', '\\']) || mission.contains("..") {
        return None;
    }
    let events = delphi_core::log::replay(&ws.join("local/missions").join(mission)).ok()?;
    events.into_iter().rev().find(|e| e.kind == "validator.verdict" && e.payload["claim"].as_str().is_some_and(|c| fact_id(c) == fact)).map(|e| e.payload)
}

/// The first half of a card: the claim and nothing else. It is read from the mission record, where the claim lives.
pub fn show_claim(ws: &Path, state: &Path, card: &str) -> Result<String, String> {
    let c = open_cards(state).into_iter().find(|c| c.card == card).ok_or("no such card, or it was already answered")?;
    let v = mission_verdict(ws, &c.mission, card).ok_or("the claim is not in the mission record any more")?;
    Ok(v["claim"].as_str().unwrap_or("").to_string())
}

/// What the operator's answer did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Confirmed,
    /// A would-ratify or ratified fact the operator called false: the latch tripped.
    Rejected { quarantined: Vec<String> },
    Unsure,
    /// A `refuted` verdict the operator called false: a confirmed catch (§15.3 item 4).
    Catch,
    /// A `refuted` or `unsupported` verdict on a claim the operator calls true.
    FalseRefutation,
    Noted,
}

/// What a card shows after the answer. Only [`answer`] hands this out.
#[derive(Debug, Clone, PartialEq)]
pub struct Reveal {
    pub outcome: Outcome,
    pub verdict: String,
    pub sources: Vec<Value>,
    pub reasons: Vec<String>,
}

/// Record the operator's answer, then act on it, then show what the validator said. The order is the point: the
/// answer is on disk before the sources and the verdict are read.
pub fn answer(ws: &Path, state: &Path, store: &FactStore, card: &str, a: Answer, now: i64) -> Result<Reveal, String> {
    let c = open_cards(state).into_iter().find(|c| c.card == card).ok_or("no such card, or it was already answered")?;
    let events = Journal::new(state).all();
    let meta = events.iter().find(|e| e["kind"] == "card" && e["card"] == card).cloned().unwrap_or_default();
    let verdict = meta["verdict"].as_str().unwrap_or("").to_string();
    let batch = meta["batch"].as_str().unwrap_or("b1").to_string();
    let outcome = match (c.positive, a, verdict.as_str()) {
        (_, Answer::Unsure, _) => Outcome::Unsure,
        (true, Answer::True, _) => Outcome::Confirmed,
        (true, Answer::False, _) => Outcome::Rejected { quarantined: vec![] },
        (false, Answer::False, "refuted") => Outcome::Catch,
        (false, Answer::True, "refuted" | "unsupported") => Outcome::FalseRefutation,
        _ => Outcome::Noted,
    };
    let j = Journal::new(state);
    j.append("audit", json!({"card": card, "fact_id": card, "mission": c.mission, "answer": a.as_str(), "positive": c.positive, "verdict": verdict, "batch": batch, "origins": meta["origins"], "outcome": outcome_name(&outcome)}), now);
    let outcome = match outcome {
        Outcome::Rejected { .. } => {
            let reason = "an audited fact was rejected by the operator";
            let _ = lower_latch(state, ValidatorMode::Advisory, reason);
            let _ = std::fs::write(state.join("promote.json"), json!({"promote": false, "why": reason, "at": now}).to_string());
            let mut q = store.quarantine_batch(&batch, reason);
            if let Ok(w) = store.quarantine(card, reason) {
                q.push(w.id);
            }
            q.sort();
            q.dedup();
            j.append("latch", json!({"act": "trip", "to": "advisory", "reason": reason, "card": card, "batch": batch, "quarantined": q.len()}), now);
            Outcome::Rejected { quarantined: q }
        }
        o => o,
    };
    let v = mission_verdict(ws, &c.mission, card).unwrap_or_default();
    Ok(Reveal {
        outcome,
        verdict,
        sources: v["sources"].as_array().cloned().unwrap_or_default(),
        reasons: v["reasons"].as_array().map(|r| r.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
    })
}

fn outcome_name(o: &Outcome) -> &'static str {
    match o {
        Outcome::Confirmed => "confirmed",
        Outcome::Rejected { .. } => "rejected",
        Outcome::Unsure => "unsure",
        Outcome::Catch => "catch",
        Outcome::FalseRefutation => "false_refutation",
        Outcome::Noted => "noted",
    }
}

/// An unanswered would-ratify card older than 7 days quarantines its fact, so an operator who stops auditing cannot
/// leave unchecked facts in circulation (§15.2 item 2). Returns the ids quarantined now.
pub fn sweep_stale(state: &Path, store: &FactStore, now: i64) -> Vec<String> {
    let j = Journal::new(state);
    let done: BTreeSet<String> = j.all().iter().filter(|e| e["kind"] == "check" && e["why"] == "stale_audit").filter_map(|e| e["card"].as_str().map(String::from)).collect();
    let mut out = vec![];
    for c in open_cards(state).into_iter().filter(|c| c.positive && now - c.at > STALE_AUDIT_SECS && !done.contains(&c.card)) {
        let _ = store.quarantine(&c.card, "its audit card is older than 7 days");
        j.append("check", json!({"why": "stale_audit", "card": c.card, "mission": c.mission}), now);
        out.push(c.card);
    }
    out
}

/// The `fact.read` event for a read of facts through the knowledge-base tool (§15.2): which span read which fact.
pub fn fact_read_events(span: &str, role: &str, rel_paths: &[String]) -> Vec<(String, Value)> {
    rel_paths
        .iter()
        .filter_map(|p| p.strip_prefix("context/facts/")?.strip_suffix(".md"))
        .map(|id| ("validator.event".to_string(), json!({"kind": "fact.read", "fact": id, "span": span, "role": role})))
        .collect()
}

/// Operator act: take one fact out of quarantine (`delphi validation unquarantine <fact-id> --reason`). It needs a
/// reason and is journaled as the operator's (L5). It cannot be a way round a false audit: a fact the operator called
/// false stays out for good, and while the latch stands tripped (no raise since the trip) no fact comes back at all.
pub fn unquarantine(state: &Path, store: &FactStore, fact: &str, reason: &str, now: i64) -> Result<String, String> {
    if reason.trim().chars().count() < 8 {
        return Err("a reason of at least a few words is required".into());
    }
    let events = Journal::new(state).all();
    if events.iter().any(|e| e["kind"] == "audit" && e["fact_id"] == fact && e["answer"] == "false" && e["positive"] == true) {
        return Err("you called this fact false in an audit; it stays quarantined (retract it instead)".into());
    }
    if events.iter().rfind(|e| e["kind"] == "latch").is_some_and(|e| e["act"] == "trip") {
        return Err("the latch is tripped by a false audit; raise it first (`delphi validation latch raise`), which needs a reason".into());
    }
    let w = store.unquarantine(fact, reason.trim()).map_err(|e| format!("{e:?}"))?;
    Journal::new(state).append("check", json!({"why": "unquarantine", "fact_id": fact, "reason": reason.trim(), "by": "operator", "level": "L5"}), now);
    Ok(w.id)
}

/// Operator act: raise the latch again. Needs a reason; to `ratify` it needs PROMOTE or an explicit `force`. A new
/// batch begins, and the act is logged as the operator's (L5).
pub fn raise_latch(state: &Path, to: ValidatorMode, reason: &str, force: bool, now: i64) -> Result<String, String> {
    raise_latch_by(state, to, reason, force, now, "operator")
}

/// What the operator is shown before a console raise, and the word they must type (`specs/040` §25 V2d-3).
pub struct LatchCard {
    pub text: String,
    pub expect: String,
}

fn check_raise(state: &Path, to: ValidatorMode, force: bool) -> Result<ValidatorMode, String> {
    let from = read_latch(state).ceiling;
    if to <= from {
        return Err(format!("the latch is already at {}; raising goes up, and lowering is automatic", from.as_str()));
    }
    if to == ValidatorMode::Ratify && !force && !promote_is_yes(state) {
        return Err("PROMOTE is not yes (`delphi validation report`); --force is an operator act and is logged".into());
    }
    Ok(from)
}

/// The approval card for a latch raise or `--force`: refused when the raise itself would be, otherwise the text and the
/// exact word to type. Nothing is written; only [`raise_latch_by`] with the typed word does that.
pub fn latch_card(state: &Path, to: ValidatorMode, force: bool) -> Result<LatchCard, String> {
    if to == ValidatorMode::Off {
        return Err("`to` is advisory or ratify".into());
    }
    let from = check_raise(state, to, force)?;
    let expect = if force { format!("force {}", to.as_str()) } else { to.as_str().to_string() };
    let forced = if force { " PROMOTE is not yes: forcing is logged as forced and is yours alone." } else { "" };
    let text = format!("Raise the validator's latch from {} to {}.{forced} A new audit batch starts, so older facts stay with the old one. Nothing a source, a span or the daemon does can raise it. Type `{expect}` to confirm.", from.as_str(), to.as_str());
    Ok(LatchCard { text, expect })
}

/// The raise itself, journaled with who confirmed it (`operator` at the terminal, `console` after the typed word).
pub fn raise_latch_by(state: &Path, to: ValidatorMode, reason: &str, force: bool, now: i64, by: &str) -> Result<String, String> {
    if reason.trim().chars().count() < 8 {
        return Err("a reason of at least a few words is required".into());
    }
    let from = check_raise(state, to, force)?;
    let f = json!({"ceiling": to.as_str(), "reason": format!("raised by the operator: {}", reason.trim().chars().take(160).collect::<String>())});
    std::fs::create_dir_all(state).map_err(|e| e.to_string())?;
    std::fs::write(state.join("latch.json"), serde_json::to_vec_pretty(&f).unwrap_or_default()).map_err(|e| e.to_string())?;
    let batch = start_new_batch(state);
    Journal::new(state).append("latch", json!({"act": "raise", "from": from.as_str(), "to": to.as_str(), "reason": reason.trim(), "forced": force, "by": by, "level": "L5", "batch": batch}), now);
    Ok(batch)
}

fn promote_is_yes(state: &Path) -> bool {
    std::fs::read(state.join("promote.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).is_some_and(|v| v["promote"].as_bool() == Some(true))
}

/// What the operator's labelled-set score (V1b) left behind: counts only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labelled {
    pub claims: usize,
    pub false_ratified: usize,
    pub poisoned_passes: usize,
}

pub struct Inputs {
    pub labelled: Option<Labelled>,
    /// Tokens of every role but the validator, per mission, from the ledger (`O` in §20.1).
    pub other_tokens: BTreeMap<String, u64>,
    pub now: i64,
}

pub struct ReportOut {
    pub text: String,
    pub promote: bool,
    pub why: String,
}

fn age(secs: i64) -> String {
    if secs >= 86_400 {
        format!("{} d", secs / 86_400)
    } else {
        format!("{} h", secs / 3_600)
    }
}

/// `delphi validation report` (§15.1, §15.3, §15.4): the effective mode and why, the audit numbers, and PROMOTE.
pub fn report(state: &Path, eff: &Effective, latch: &Latch, inputs: &Inputs) -> ReportOut {
    let events = Journal::new(state).all();
    let audits: Vec<&Value> = events.iter().filter(|e| e["kind"] == "audit").collect();
    let pos: Vec<&&Value> = audits.iter().filter(|a| a["positive"] == true).collect();
    let confirmed: Vec<&&&Value> = pos.iter().filter(|a| a["answer"] == "true").collect();
    let errors = pos.iter().filter(|a| a["answer"] == "false").count();
    let unsure = audits.iter().filter(|a| a["answer"] == "unsure").count();
    let missions: BTreeSet<&str> = confirmed.iter().filter_map(|a| a["mission"].as_str()).collect();
    let mut origin_counts: BTreeMap<String, usize> = BTreeMap::new();
    for a in &confirmed {
        if let Some(o) = a["origins"].get(0).and_then(|o| o.as_str()) {
            *origin_counts.entry(o.to_string()).or_default() += 1;
        }
    }
    let top = origin_counts.iter().max_by_key(|(_, n)| **n).map(|(o, n)| (o.clone(), *n as f64 / confirmed.len().max(1) as f64));
    let catches = audits.iter().filter(|a| a["outcome"] == "catch").count();
    let false_refutations = audits.iter().filter(|a| a["outcome"] == "false_refutation").count();
    let (open, oldest) = backlog(state, inputs.now);

    let mut tokens: BTreeMap<String, u64> = BTreeMap::new();
    let verdicts: Vec<&Value> = events.iter().filter(|e| e["kind"] == "verdict").collect();
    for v in &verdicts {
        *tokens.entry(v["mission"].as_str().unwrap_or("").to_string()).or_default() += v["tokens"].as_u64().unwrap_or(0);
    }
    let over: Vec<&String> = tokens.iter().filter(|(m, v)| **v > FLOOR_TOKENS && (**v as f64) > SHARE_FACTOR * inputs.other_tokens.get(*m).copied().unwrap_or(0) as f64).map(|(m, _)| m).collect();
    let total_tokens: u64 = tokens.values().sum();

    let mut why: Vec<String> = vec![];
    match &inputs.labelled {
        None => why.push("the labelled set has not been scored (`delphi validation score --results <file>`)".into()),
        Some(l) => {
            if l.claims < PROMOTE_MIN_LABELLED {
                why.push(format!("the labelled set has {} claims; at least {PROMOTE_MIN_LABELLED} are needed", l.claims));
            }
            if l.false_ratified > 0 || l.poisoned_passes > 0 {
                why.push(format!("the labelled set shows {} false-ratified and {} poisoned passes; both must be 0", l.false_ratified, l.poisoned_passes));
            }
        }
    }
    if confirmed.len() < PROMOTE_MIN_AUDITS {
        why.push(format!("{} would-ratify claims audited as true; at least {PROMOTE_MIN_AUDITS} are needed", confirmed.len()));
    }
    if errors > 0 {
        why.push(format!("{errors} audited fact(s) were called false; 0 errors are allowed"));
    }
    if missions.len() < PROMOTE_MIN_MISSIONS {
        why.push(format!("audited claims come from {} mission(s); at least {PROMOTE_MIN_MISSIONS} are needed", missions.len()));
    }
    if let Some((o, share)) = &top {
        if *share > MAX_ORIGIN_SHARE {
            why.push(format!("origin {o} supplies {:.0}% of the audited facts; the limit is {:.0}%", share * 100.0, MAX_ORIGIN_SHARE * 100.0));
        }
    }
    if open > 0 {
        why.push(format!("the audit backlog is {open}; it must be 0"));
    }
    if !over.is_empty() {
        why.push(format!("{} mission(s) used more than their token cap with no operator override", over.len()));
    }
    let promote = why.is_empty();
    let why_text = if promote { "every PROMOTE condition holds".to_string() } else { why.join("; ") };

    let mut t = String::from("delphi validation report (reads only the validator's own audit journal)\n\n");
    t += &format!("mode in effect: {}\n  config value: {}\n  latch: {} ({})\n  {}\n", eff.mode.as_str(), eff.config.as_str(), latch.ceiling.as_str(), latch.reason, eff.reason);
    t += "  advisory uses the network: it fetches and searches, then writes nothing to context/facts/\n\n";
    t += &format!("claims checked: {}\n", verdicts.len());
    t += &format!("cards: {} answered, {} open (oldest {})\n", audits.len(), open, age(oldest));
    t += &format!("would-ratify audited true: {} from {} mission(s); called false: {}; unsure (counts for nothing): {}\n", confirmed.len(), missions.len(), errors, unsure);
    if let Some((o, share)) = &top {
        t += &format!("largest origin: {o} at {:.0}% of audited facts\n", share * 100.0);
    }
    t += &format!("confirmed catches (a refuted claim the operator calls false): {catches}; false refutations: {false_refutations}\n");
    if !pos.is_empty() && errors == 0 {
        t += &format!("rule of three: 0 errors in {} audits bounds the error rate only to under {:.1}% (3/{}); the audit continues at 100% until volume passes {FULL_AUDIT_PER_WEEK} a week\n", pos.len(), 300.0 / pos.len() as f64, pos.len());
    } else if pos.is_empty() {
        t += "rule of three: nothing audited yet\n";
    }
    t += &format!("validator tokens: {total_tokens} over {} claim(s){}", verdicts.len(), if verdicts.is_empty() { String::new() } else { format!(" ({} per claim)", total_tokens / verdicts.len() as u64) });
    t += &format!("{}\n", if catches > 0 { format!("; {} per confirmed catch", total_tokens / catches as u64) } else { String::new() });
    t += "token share: the 15% mission cap is checked here against the ledger, not enforced while a mission runs\n";
    t += &format!("\nPROMOTE: {}\n  {}\n", if promote { "yes" } else { "no" }, why_text);
    ReportOut { text: t, promote, why: why_text }
}

/// Write the PROMOTE record `fact_mode::effective` reads. Only the report calls this, from the journal alone.
pub fn write_promote(state: &Path, out: &ReportOut, now: i64) -> std::io::Result<()> {
    std::fs::create_dir_all(state)?;
    std::fs::write(state.join("promote.json"), json!({"promote": out.promote, "why": out.why, "at": now}).to_string())
}

/// The readers of the retracted facts, from `fact.read` events in the mission records, for the operator's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reader {
    pub fact: String,
    pub mission: String,
    pub span: String,
}

pub struct Retraction {
    pub retracted: Vec<(String, String)>,
    pub readers: Vec<Reader>,
}

/// `delphi validation retract --all` (§15.2 item 3). No cascade: it lists, it does not mark anything stale.
pub fn retract_all(ws: &Path, state: &Path, store: &FactStore, now: i64) -> Retraction {
    let retracted = store.retract_all();
    let ids: BTreeSet<&str> = retracted.iter().map(|(i, _)| i.as_str()).collect();
    let mut readers: Vec<Reader> = vec![];
    if let Ok(rd) = std::fs::read_dir(ws.join("local/missions")) {
        let mut dirs: Vec<_> = rd.flatten().map(|d| d.path()).collect();
        dirs.sort();
        for d in dirs {
            for e in delphi_core::log::replay(&d).unwrap_or_default() {
                if e.kind == "validator.event" && e.payload["kind"] == "fact.read" && e.payload["fact"].as_str().is_some_and(|f| ids.contains(f)) {
                    readers.push(Reader { fact: e.payload["fact"].as_str().unwrap_or("").into(), mission: e.mission_id.clone(), span: e.payload["span"].as_str().unwrap_or("").into() });
                }
            }
        }
    }
    let batches: BTreeSet<&str> = retracted.iter().map(|(_, b)| b.as_str()).collect();
    Journal::new(state).append("retract", json!({"count": retracted.len(), "batches": batches, "readers": readers.len()}), now);
    Retraction { retracted, readers }
}

/// The mission name the journal uses for background validation, which belongs to no mission (§20.2).
pub const BACKGROUND: &str = "background";
/// §20.2: background validation is at most a tenth of the install's tokens over the trailing week.
pub const BACKGROUND_SHARE: f64 = 0.10;
/// What one operator raise adds to a mission's cap (§25 V2d-2): the floor, once.
pub const RAISE_TOKENS: u64 = FLOOR_TOKENS;

/// Why a claim could not be checked now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cap {
    Mission { spent: u64, cap: u64 },
    Background { spent: u64, cap: u64 },
}

impl Cap {
    /// The one line the operator sees (§20.1): what happened and what raises it. Never carries claim text.
    pub fn notice(&self, mission: &str) -> String {
        match self {
            Cap::Mission { spent, cap } => format!("validation budget reached for {mission} ({spent} of {cap} tokens): claims stay not validated and are queued; `delphi validation more {mission} --reason <why>` raises it once"),
            Cap::Background { spent, cap } => format!("background validation paused ({spent} of {cap} tokens this week); it resumes as the week rolls on"),
        }
    }
}

fn spent_in(events: &[Value], mission: &str, since: i64) -> u64 {
    events.iter().filter(|e| (e["kind"] == "verdict" || e["kind"] == "span") && e["mission"] == mission && e["at"].as_i64().unwrap_or(0) >= since).map(|e| e["tokens"].as_u64().unwrap_or(0)).sum()
}

/// §20.1, live: may this mission spend `reserve` more validator tokens? `other` is every other role's tokens in the
/// mission (the ledger). The cap is `max(floor, 0.176 * O)` plus one floor per operator raise.
pub fn mission_allows(state: &Path, mission: &str, other: u64, reserve: u64) -> Result<(), Cap> {
    let events = Journal::new(state).all();
    let spent = spent_in(&events, mission, 0);
    let raises = events.iter().filter(|e| e["kind"] == "share_raise" && e["mission"] == mission).count() as u64;
    let cap = FLOOR_TOKENS.max((SHARE_FACTOR * other as f64) as u64) + raises * RAISE_TOKENS;
    if spent + reserve <= cap {
        Ok(())
    } else {
        Err(Cap::Mission { spent, cap })
    }
}

/// §20.2, live: may background validation spend `reserve` more? `install_week` is the install's tokens over the
/// trailing seven days, the validator's own included.
pub fn background_allows(state: &Path, install_week: u64, now: i64, reserve: u64) -> Result<(), Cap> {
    let spent = spent_in(&Journal::new(state).all(), BACKGROUND, now - WEEK_SECS);
    let cap = (BACKGROUND_SHARE * install_week as f64) as u64;
    if spent + reserve <= cap {
        Ok(())
    } else {
        Err(Cap::Background { spent, cap })
    }
}

/// D-8: the claim stays brought in, labelled not validated (budget reached), and waits for review.
pub fn queue_claim(state: &Path, mission: &str, fact: &str, now: i64) {
    Journal::new(state).append("queued", json!({"fact_id": fact, "mission": mission, "label": "not validated (budget reached)"}), now);
}

/// The review queue, most-referenced first (§18.3): every request for the same claim is one reference.
pub fn queue(state: &Path) -> Vec<(String, usize)> {
    let mut refs: BTreeMap<String, usize> = BTreeMap::new();
    for e in Journal::new(state).all().iter().filter(|e| e["kind"] == "queued") {
        if let Some(f) = e["fact_id"].as_str() {
            *refs.entry(f.to_string()).or_default() += 1;
        }
    }
    let mut v: Vec<(String, usize)> = refs.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// Operator act (`delphi validation more <mission> --reason`): raise one mission's cap once, journaled as the
/// operator's. Only this path writes `share_raise`; nothing in a span, a source or the daemon's own loop calls it.
pub fn raise_share(state: &Path, mission: &str, reason: &str, now: i64) -> Result<String, String> {
    if reason.trim().chars().count() < 8 {
        return Err("a raise needs a reason of at least 8 characters".into());
    }
    if mission.is_empty() || mission == BACKGROUND {
        return Err("name a mission; background validation has its own weekly cap".into());
    }
    Journal::new(state).append("share_raise", json!({"mission": mission, "reason": reason, "by": "operator", "tokens": RAISE_TOKENS}), now);
    Ok(format!("raised the validation cap of {mission} by {RAISE_TOKENS} tokens"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact_mode::effective;

    fn obs(id: &str, mission: &str, verdict: &str, origin: &str) -> Observed {
        let positive = verdict == "would_ratify";
        Observed { fact_id: id.into(), mission: mission.into(), verdict: verdict.into(), would_ratify: positive, ratified: false, origins: vec![origin.into()], class: "primary".into(), tokens: 100 }
    }

    fn state() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("local/validation");
        (d, s)
    }

    /// A mission record holding the verdict event, so a card can show its claim.
    fn mission_with_claim(ws: &Path, mission: &str, claim: &str) {
        let mut log = delphi_core::log::EventLog::open(&ws.join("local/missions").join(mission), Default::default()).unwrap();
        log.append(delphi_core::event::EventEnvelope::new(mission, "validator.verdict", json!({"claim": claim, "verdict": "would_ratify", "sources": [{"url": "https://example.org/a", "origin": "example.org"}], "reasons": ["two independent origins"]})))
            .unwrap();
    }

    #[test]
    fn every_would_ratify_verdict_gets_a_card_and_negative_ones_are_sampled_at_most_five_a_week() {
        let (_d, s) = state();
        for i in 0..8 {
            assert!(observe(&s, &obs(&format!("fact-{i}"), "m1", "would_ratify", "a.org"), 1, 1000 + i));
        }
        let mut negative = 0;
        for i in 0..20 {
            if observe(&s, &obs(&format!("neg-{i}"), "m1", "refuted", "a.org"), 0, 2000 + i) {
                negative += 1;
            }
        }
        assert_eq!(negative, MAX_NEGATIVE_CARDS_PER_WEEK);
        assert!(!observe(&s, &obs("fact-0", "m1", "would_ratify", "a.org"), 1, 3000), "one card per claim");
        assert!(!observe(&s, &obs("x", "m1", "trivial", "a.org"), 0, 3000), "a verdict outside the sampled set gets none");
        let j = Journal::new(&s).all();
        assert!(j.iter().all(|e| e.get("claim").is_none()), "the journal holds ids, never claim text");
    }

    #[test]
    fn past_the_weekly_volume_the_sample_is_stratified_by_class_and_origin_not_a_coin() {
        let (_d, s) = state();
        for i in 0..FULL_AUDIT_PER_WEEK {
            assert!(observe(&s, &obs(&format!("f{i}"), "m1", "would_ratify", "a.org"), 1, 100 + i as i64));
        }
        // The stratum a.org was audited in full; whatever the roll says, the answer is the same.
        let mut carded = 0;
        for i in 0..100 {
            if observe(&s, &obs(&format!("x{i}"), "m1", "would_ratify", "a.org"), (i % 2) as u32, 500 + i as i64) {
                carded += 1;
            }
        }
        assert_eq!(carded, 0, "a stratum already above its sampling share is not audited further");
        // A stratum with no card this week is audited at once, by origin and by class.
        assert!(observe(&s, &obs("b1", "m1", "would_ratify", "b.org"), 1, 700));
        let mut other = obs("c1", "m1", "would_ratify", "a.org");
        other.class = "reputable_secondary".into();
        assert!(observe(&s, &other, 1, 701));
        // Within a new stratum the rate holds at about one in five, and does not depend on the roll.
        let cards: usize = (0..50).filter(|i| observe(&s, &obs(&format!("b-more{i}"), "m1", "would_ratify", "b.org"), 1, 800 + *i as i64)).count();
        assert!((8..=12).contains(&cards), "{cards} of 50 in one stratum");
    }

    #[test]
    fn the_claim_is_shown_first_the_answer_is_recorded_before_the_reveal_and_unsure_counts_for_nothing() {
        let (d, s) = state();
        let ws = d.path();
        mission_with_claim(ws, "m1", "The Eiffel Tower is in Paris.");
        let id = fact_id("The Eiffel Tower is in Paris.");
        observe(&s, &obs(&id, "m1", "would_ratify", "example.org"), 1, 1000);
        assert_eq!(show_claim(ws, &s, &id).unwrap(), "The Eiffel Tower is in Paris.");
        assert!(Journal::new(&s).all().iter().all(|e| e["kind"] != "audit"), "looking at the claim records nothing");
        let store = FactStore::new(ws);
        let r = answer(ws, &s, &store, &id, Answer::Unsure, 1100).unwrap();
        assert_eq!(r.outcome, Outcome::Unsure);
        assert_eq!(r.sources.len(), 1, "the sources come only with the answer");
        assert!(answer(ws, &s, &store, &id, Answer::True, 1200).is_err(), "a card is answered once");
        let out = report(&s, &effective(ValidatorMode::Advisory, &s), &read_latch(&s), &Inputs { labelled: None, other_tokens: BTreeMap::new(), now: 1300 });
        assert!(out.text.contains("would-ratify audited true: 0") && out.text.contains("unsure (counts for nothing): 1"), "{}", out.text);
    }

    #[test]
    fn rejecting_an_audited_fact_lowers_the_mode_quarantines_its_batch_and_survives_a_restart() {
        let (d, s) = state();
        let ws = d.path();
        mission_with_claim(ws, "m1", "Claim one.");
        let id = fact_id("Claim one.");
        observe(&s, &obs(&id, "m1", "would_ratify", "example.org"), 1, 1000);
        std::fs::create_dir_all(&s).unwrap();
        std::fs::write(s.join("promote.json"), r#"{"promote":true,"why":"x"}"#).unwrap();
        assert_eq!(effective(ValidatorMode::Ratify, &s).mode, ValidatorMode::Ratify);
        let store = FactStore::new(ws).with_batch("b1");
        let r = answer(ws, &s, &store, &id, Answer::False, 1100).unwrap();
        assert!(matches!(r.outcome, Outcome::Rejected { .. }));
        let e = effective(ValidatorMode::Ratify, &s);
        assert_eq!(e.mode, ValidatorMode::Advisory, "{}", e.reason);
        assert!(e.reason.contains("audited fact was rejected"));
        assert_eq!(effective(ValidatorMode::Ratify, &s).mode, ValidatorMode::Advisory, "a fresh read of the state, as after a restart");
        assert!(Journal::new(&s).all().iter().any(|e| e["kind"] == "latch" && e["act"] == "trip"));
    }

    #[test]
    fn unquarantine_needs_a_reason_is_journaled_and_cannot_get_round_a_false_audit_or_the_latch() {
        let (d, s) = state();
        let ws = d.path();
        mission_with_claim(ws, "m1", "Claim one.");
        let id = fact_id("Claim one.");
        let store = FactStore::new(ws).with_batch("b1");
        let r = crate::fact_store::tests::report(crate::fact_run::Verdict::Ratifiable, &["https://www.britannica.com/one", "https://www.allmusic.com/one"], crate::fact_check::ClaimKind::Fact);
        let w = store.record(&r, "2026-10-28T00:00:00Z", "m1", "q").unwrap();
        store.quarantine(&w.id, "stale card").unwrap();
        assert!(unquarantine(&s, &store, &w.id, "ok", 10).is_err(), "a reason is required");
        unquarantine(&s, &store, &w.id, "checked the sources again by hand", 11).unwrap();
        let j = Journal::new(&s).all();
        assert!(j.iter().any(|e| e["why"] == "unquarantine" && e["by"] == "operator"));
        // A false audit trips the latch and quarantines the batch; neither the latch nor the answer can be walked round.
        observe(&s, &obs(&w.id, "m1", "would_ratify", "example.org"), 1, 1000);
        answer(ws, &s, &store, &w.id, Answer::False, 1100).unwrap();
        let e = unquarantine(&s, &store, &w.id, "I changed my mind about it", 1200).unwrap_err();
        assert!(e.contains("called this fact false"), "{e}");
        raise_latch(&s, ValidatorMode::Ratify, "reviewed the rejected fact and the batch", true, 1300).unwrap();
        assert!(unquarantine(&s, &store, &w.id, "I changed my mind about it", 1400).is_err(), "still false after the raise");
        let _ = id;
    }

    #[test]
    fn an_unanswered_card_older_than_seven_days_quarantines_its_fact() {
        let (d, s) = state();
        let ws = d.path();
        observe(&s, &obs("fact-aaaaaaaaaaaa", "m1", "would_ratify", "example.org"), 1, 1000);
        let store = FactStore::new(ws);
        assert!(sweep_stale(&s, &store, 1000 + STALE_AUDIT_SECS).is_empty(), "exactly seven days is not older");
        assert_eq!(sweep_stale(&s, &store, 1001 + STALE_AUDIT_SECS), vec!["fact-aaaaaaaaaaaa".to_string()]);
        assert!(sweep_stale(&s, &store, 2000 + STALE_AUDIT_SECS).is_empty(), "once");
        assert_eq!(backlog(&s, 1000 + 3 * 86_400), (1, 3 * 86_400));
    }

    #[test]
    fn only_the_operator_raises_the_latch_with_a_reason_and_ratify_needs_promote_or_a_logged_force() {
        let (_d, s) = state();
        lower_latch(&s, ValidatorMode::Off, "an audited fact was rejected").unwrap();
        assert!(raise_latch(&s, ValidatorMode::Advisory, "short", false, 1).is_err(), "a reason is required");
        assert!(raise_latch(&s, ValidatorMode::Ratify, "after a long review of the audit", false, 1).is_err(), "no PROMOTE");
        assert_eq!(raise_latch(&s, ValidatorMode::Advisory, "reviewed the rejected fact and the batch", false, 2).unwrap(), "b2");
        assert_eq!(read_latch(&s).ceiling, ValidatorMode::Advisory);
        assert!(raise_latch(&s, ValidatorMode::Advisory, "again for no good reason", false, 3).is_err(), "not a raise");
        raise_latch(&s, ValidatorMode::Ratify, "forced for a pilot with the operator watching", true, 4).unwrap();
        let j = Journal::new(&s).all();
        let forced = j.iter().find(|e| e["kind"] == "latch" && e["forced"] == true).unwrap();
        assert_eq!((forced["by"].as_str(), forced["level"].as_str()), (Some("operator"), Some("L5")));
        assert_eq!(effective(ValidatorMode::Ratify, &s).mode, ValidatorMode::Advisory, "forcing the latch is not PROMOTE: ratify still needs the record");
    }

    fn audited(s: &Path, ws: &Path, n: usize, missions: usize, origins: usize) {
        let store = FactStore::new(ws);
        for i in 0..n {
            let id = format!("fact-{:012}", i);
            let m = format!("m{}", i % missions);
            observe(s, &obs(&id, &m, "would_ratify", &format!("origin{}.org", i % origins)), 1, 100 + i as i64 * 700_000);
            answer(ws, s, &store, &id, Answer::True, 200 + i as i64 * 700_000).unwrap();
        }
    }

    fn run(s: &Path, labelled: Option<Labelled>) -> ReportOut {
        report(s, &effective(ValidatorMode::Ratify, s), &read_latch(s), &Inputs { labelled, other_tokens: BTreeMap::new(), now: 10_000 })
    }

    fn clean() -> Option<Labelled> {
        Some(Labelled { claims: 41, false_ratified: 0, poisoned_passes: 0 })
    }

    #[test]
    fn promote_needs_all_of_the_pre_registered_conditions_and_names_the_ones_missing() {
        let (d, s) = state();
        let ws = d.path();
        audited(&s, ws, 60, 10, 6);
        let ok = run(&s, clean());
        assert!(ok.promote, "{}", ok.text);
        assert!(ok.text.contains("PROMOTE: yes") && ok.text.contains("5.0%"), "{}", ok.text);
        write_promote(&s, &ok, 10_000).unwrap();
        assert_eq!(effective(ValidatorMode::Ratify, &s).mode, ValidatorMode::Ratify);

        assert!(!run(&s, None).promote, "unscored");
        let bad = run(&s, Some(Labelled { claims: 41, false_ratified: 1, poisoned_passes: 0 }));
        assert!(!bad.promote && bad.why.contains("1 false-ratified"));
        assert!(!run(&s, Some(Labelled { claims: 39, false_ratified: 0, poisoned_passes: 0 })).promote, "fewer than 40 labelled claims");

        let (d2, s2) = state();
        audited(&s2, d2.path(), 59, 10, 6);
        assert!(run(&s2, clean()).why.contains("59 would-ratify"), "one short of 60");
        let (d3, s3) = state();
        audited(&s3, d3.path(), 60, 9, 6);
        assert!(run(&s3, clean()).why.contains("9 mission(s)"), "nine missions");
        let (d4, s4) = state();
        audited(&s4, d4.path(), 60, 10, 2);
        assert!(run(&s4, clean()).why.contains("supplies 50%"), "one origin over 20%");
        let (_d5, s5) = state();
        audited(&s5, _d5.path(), 60, 10, 6);
        observe(&s5, &obs("fact-unaudited", "m1", "would_ratify", "z.org"), 1, 90_000_000);
        assert!(run(&s5, clean()).why.contains("backlog is 1"), "an unanswered card");
    }

    #[test]
    fn a_false_answer_is_an_error_that_blocks_promote_and_unsure_is_not_a_confirmed_audit() {
        let (d, s) = state();
        let ws = d.path();
        audited(&s, ws, 60, 10, 6);
        observe(&s, &obs("fact-badbadbadbad", "m1", "would_ratify", "q.org"), 1, 90_000_000);
        answer(ws, &s, &FactStore::new(ws), "fact-badbadbadbad", Answer::False, 90_000_100).unwrap();
        let r = run(&s, clean());
        assert!(!r.promote && r.why.contains("1 audited fact(s) were called false"), "{}", r.text);
        assert!(std::fs::read_to_string(s.join("promote.json")).unwrap().contains("false"), "the trip withdraws any PROMOTE record");
    }

    #[test]
    fn a_mission_over_its_token_share_blocks_promote() {
        let (d, s) = state();
        audited(&s, d.path(), 60, 10, 6);
        let j = Journal::new(&s);
        j.append("verdict", json!({"fact_id": "x", "mission": "m0", "verdict": "refuted", "tokens": 200_000}), 5);
        let mut inputs = Inputs { labelled: clean(), other_tokens: BTreeMap::from([("m0".to_string(), 100_000u64)]), now: 10_000 };
        let r = report(&s, &effective(ValidatorMode::Ratify, &s), &read_latch(&s), &inputs);
        assert!(!r.promote && r.why.contains("token cap"), "{}", r.why);
        inputs.other_tokens.insert("m0".into(), 5_000_000);
        assert!(report(&s, &effective(ValidatorMode::Ratify, &s), &read_latch(&s), &inputs).promote);
    }

    #[test]
    fn a_span_cannot_move_the_report_because_it_reads_only_the_journal() {
        let (d, s) = state();
        mission_with_claim(d.path(), "m1", "A claim.");
        let mut log = delphi_core::log::EventLog::open(&d.path().join("local/missions/m1"), Default::default()).unwrap();
        log.append(delphi_core::event::EventEnvelope::new("m1", "validator.event", json!({"kind": "audit", "answer": "true", "positive": true, "mission": "m9"}))).unwrap();
        assert!(run(&s, clean()).text.contains("would-ratify audited true: 0"), "an event in a mission record is not an audit");
    }

    #[test]
    fn retract_all_supersedes_with_the_batch_keeps_the_files_and_lists_who_read_them() {
        let (d, s) = state();
        let ws = d.path();
        let facts = ws.join("context/facts");
        std::fs::create_dir_all(&facts).unwrap();
        let rec = |id: &str, status: &str, batch: &str| format!("---\nid: {id}\nstatus: {status}\nbatch: {batch}\nkind: fact\n---\nbody\n");
        std::fs::write(facts.join("fact-a.md"), rec("fact-a", "active", "b1")).unwrap();
        std::fs::write(facts.join("fact-b.md"), rec("fact-b", "contested", "b2")).unwrap();
        std::fs::write(facts.join("disp-c.md"), rec("disp-c", "disproved", "b1")).unwrap();
        let mut log = delphi_core::log::EventLog::open(&ws.join("local/missions/m7"), Default::default()).unwrap();
        for ev in fact_read_events("sp_1", "builder", &["context/facts/fact-a.md".into(), "context/other.md".into()]) {
            log.append(delphi_core::event::EventEnvelope::new("m7", &ev.0, ev.1)).unwrap();
        }
        let r = retract_all(ws, &s, &FactStore::new(ws), 5);
        assert_eq!(r.retracted, vec![("fact-a".to_string(), "b1".to_string()), ("fact-b".to_string(), "b2".to_string())]);
        assert_eq!(r.readers, vec![Reader { fact: "fact-a".into(), mission: "m7".into(), span: "sp_1".into() }]);
        let a = std::fs::read_to_string(facts.join("fact-a.md")).unwrap();
        assert!(a.contains("status: superseded") && a.contains("superseded_by: retraction:b1") && a.contains("body"));
        assert!(std::fs::read_to_string(facts.join("disp-c.md")).unwrap().contains("status: disproved"), "a refutation record stays");
        assert!(Journal::new(&s).all().iter().any(|e| e["kind"] == "retract" && e["count"] == 2));
    }

    #[test]
    fn the_mission_share_the_queue_order_and_the_raise_are_live_and_operator_only() {
        let dir = tempfile::tempdir().unwrap();
        let s = dir.path().to_path_buf();
        // Floor: a small mission may spend up to the floor whatever O is.
        assert!(mission_allows(&s, "m1", 0, 12_000).is_ok());
        Journal::new(&s).append("verdict", json!({"fact_id": "a", "mission": "m1", "tokens": 45_000}), 1);
        Journal::new(&s).append("span", json!({"mission": "m1", "tokens": 10_000}), 1);
        assert_eq!(mission_allows(&s, "m1", 0, 12_000), Err(Cap::Mission { spent: 55_000, cap: FLOOR_TOKENS }));
        // 0.176 * O lifts the cap once O is large; another mission is unaffected.
        assert!(mission_allows(&s, "m1", 1_000_000, 12_000).is_ok());
        assert!(mission_allows(&s, "m2", 0, 12_000).is_ok());
        // Only an operator raise with a reason, once, adds the floor.
        assert!(raise_share(&s, "m1", "short", 2).is_err() && raise_share(&s, BACKGROUND, "a good enough reason", 2).is_err());
        raise_share(&s, "m1", "the rest matters to me", 2).unwrap();
        assert!(mission_allows(&s, "m1", 0, 12_000).is_ok());
        assert!(Journal::new(&s).all().iter().any(|e| e["kind"] == "share_raise" && e["by"] == "operator"), "logged");
        // Queue order: the most referenced claim first.
        for (f, n) in [("fact-aaa", 1), ("fact-bbb", 3), ("fact-ccc", 2)] {
            for _ in 0..n {
                queue_claim(&s, "m1", f, 3);
            }
        }
        assert_eq!(queue(&s).iter().map(|(f, n)| (f.as_str(), *n)).collect::<Vec<_>>(), vec![("fact-bbb", 3), ("fact-ccc", 2), ("fact-aaa", 1)]);
    }

    #[test]
    fn the_background_share_is_a_tenth_of_the_trailing_week() {
        let dir = tempfile::tempdir().unwrap();
        let s = dir.path().to_path_buf();
        let now = 10_000_000;
        Journal::new(&s).append("verdict", json!({"fact_id": "old", "mission": BACKGROUND, "tokens": 90_000}), now - 8 * 86_400);
        Journal::new(&s).append("verdict", json!({"fact_id": "new", "mission": BACKGROUND, "tokens": 90_000}), now - 86_400);
        assert!(background_allows(&s, 1_000_000, now, 10_000).is_ok(), "90k of a 100k cap, the old spend rolled out");
        assert_eq!(background_allows(&s, 1_000_000, now, 12_000), Err(Cap::Background { spent: 90_000, cap: 100_000 }));
    }
}
