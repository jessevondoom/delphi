//! Accept all vetted (`specs/100` §6): the operator's one act on the claims that read `vetted`.
//!
//! * **The operator's act only.** [`plan`] reads; [`accept`] writes, and only the `delphi validation accept-all` command calls it, which needs a
//!   terminal and is refused inside an agent seat. Nothing here runs by itself, on a timer, or from a tool.
//! * **What it accepts** is exactly what the validator's own dry run already passed (`would_ratify`), with the counts the ladder asks for. The
//!   acceptance itself is the library's existing operator path ([`Library::validate_claim_in_batch`], which is `validate_claim` by the operator
//!   plus the `batch-accepted` mark and the audit batch); there is no second way to validate.
//! * **What it skips, and says so:** a claim in a protected class (`specs/040` §9.2, by the validator's own check), a contested claim, and a claim whose
//!   sources changed since the check (the support is recomputed first, so a changed count drops the dry run and the claim falls back a rung).
//! * **Disabled while the vetted tier is rolled back** (`specs/100` §7.3) or switched off, and it says why.
//! * **The glance is not the bound.** [`sample`] picks three claims to show with their quotes; that bounds nothing, and the text says so. The blind
//!   audit (`delphi validation tier-audit --tier batch`) is the bound.
//! * Nothing here reads a mark, a batch or the lane as a criterion for promotion, trust, tool access or approval: they are labels for tracing.

use crate::fact_audit::{current_batch, Journal};
use crate::library::claims::{status_with, tags_of};
use crate::library::Library;
use crate::tier_audit;
use delphi_core::event::EventEnvelope;
use delphi_core::knowledge_status::{tier_for, tier_params_configured, Status};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One claim an accept-all would accept.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub text: String,
    /// What the rows are grouped by: the claim's project when it has one, else its scope. Claims carry no topic field.
    pub group: String,
    pub origins: u64,
    pub trusted_sources: u64,
    /// The hosts of the sources the claim rests on (a file the operator added is said to be theirs).
    pub domains: Vec<String>,
    /// When the validator's dry run was recorded.
    pub checked_at: String,
    /// The first anchored quote, for the glance.
    pub quote: String,
}

/// A claim that reads, or would read, `vetted` and is left out, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skip {
    pub id: String,
    pub text: String,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    /// Why accept-all cannot be used now (the tier is rolled back or switched off); no row is offered then.
    pub disabled: Option<String>,
    pub rows: Vec<Row>,
    pub skipped: Vec<Skip>,
}

fn host_of(place: &str) -> String {
    place.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or("").to_string()
}

fn short(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(n).collect()
}

/// Why accept-all is off, if it is.
pub fn disabled_reason(state: &Path) -> Option<String> {
    if tier_audit::latched(state).contains("vetted") {
        return Some("the vetted tier is rolled back, so accept-all is disabled; `delphi validation tier-report` shows why, and `delphi validation tier restore vetted --reason \"<text>\"` is yours alone".into());
    }
    if !tier_params_configured().vetted_on {
        return Some("the vetted tier is switched off in [research.tiers], so no claim reads vetted".into());
    }
    None
}

/// The claims an accept-all would accept now, and the ones it leaves out.
///
/// `recheck` recomputes each claim's support first (the library's own derivation, which writes only the daemon's tags and can only lower what a count
/// shows), so a claim whose sources changed since the validator checked it drops out and says so. The operator's accept runs with it on; a plain
/// listing runs with it off and says that the check happens when you accept.
pub fn plan(lib: &Library, state: &Path, recheck: bool) -> Plan {
    if let Some(why) = disabled_reason(state) {
        return Plan { disabled: Some(why), ..Plan::default() };
    }
    let p = tier_params_configured();
    let mut out = Plan::default();
    for c0 in lib.claims() {
        let Some(id) = c0["id"].as_str().map(String::from) else { continue };
        if c0["kind"] == "attributed" || !matches!(status_with(&c0, &p), Status::Vetted | Status::Contested) {
            continue;
        }
        // A claim is in the picture when its tags reach vetted. A contested one still has those tags and is reported, not offered.
        if tier_for(&tags_of(&c0), &p) != Status::Vetted {
            continue;
        }
        let text = c0["text"].as_str().unwrap_or("").to_string();
        let skip = |why: &str| Skip { id: id.clone(), text: short(&text, 200), why: why.to_string() };
        if c0["contested_by"].as_array().is_some_and(|m| !m.is_empty()) || status_with(&c0, &p) == Status::Contested {
            out.skipped.push(skip("contested: resolve the contest first"));
            continue;
        }
        if let Some(why) = crate::fact_check::protected_class(&text) {
            out.skipped.push(skip(&format!("protected class ({why}): it goes to you as a card with its evidence, never in a batch")));
            continue;
        }
        let c = if recheck { lib.refresh_support(&id).unwrap_or_else(|| c0.clone()) } else { c0.clone() };
        if status_with(&c, &p) != Status::Vetted {
            out.skipped.push(skip("its sources changed since the validator checked it, so it needs a fresh check and falls back a rung"));
            continue;
        }
        let domains: BTreeSet<String> = lib.claim_sources(&id).iter().map(|s| host_of(&s.1)).filter(|h| !h.is_empty()).collect();
        out.rows.push(Row {
            id,
            text: short(&text, 300),
            group: c["project"].as_str().filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| c["scope"].as_str().unwrap_or("personal").to_string()),
            origins: c["support"]["origins"].as_u64().unwrap_or(0),
            trusted_sources: c["support"]["trusted_sources"].as_u64().unwrap_or(0),
            domains: domains.into_iter().collect(),
            checked_at: c["support"]["would_ratify_at"].as_str().unwrap_or("").to_string(),
            quote: short(c["anchors"][0]["quote"].as_str().unwrap_or(""), 300),
        });
    }
    out.rows.sort_by(|a, b| (a.group.as_str(), a.id.as_str()).cmp(&(b.group.as_str(), b.id.as_str())));
    out.skipped.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Rows grouped by `group`, in order.
pub fn grouped(rows: &[Row]) -> BTreeMap<&str, Vec<&Row>> {
    let mut m: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for r in rows {
        m.entry(r.group.as_str()).or_default().push(r);
    }
    m
}

/// Up to three rows to show with their quotes before the batch (`specs/100` §6). A fixed shuffle by the audit batch and the id, so a re-run in the same
/// batch shows the same three. **A convenience, not a bound.**
pub fn sample<'a>(rows: &'a [Row], batch: &str) -> Vec<&'a Row> {
    let mut keyed: Vec<(String, &Row)> = rows.iter().map(|r| (blake3::hash(format!("{batch}\u{0}{}", r.id).as_bytes()).to_hex().to_string(), r)).collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.into_iter().take(3).map(|(_, r)| r).collect()
}

/// The words printed with the glance.
pub const GLANCE_NOTE: &str = "These three are a glance and nothing more. Looking at three claims bounds nothing about the rest. The bound comes from the blind audit: `delphi validation tier-audit --tier batch`, and a failed audit rolls the tier back.";

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub audit_batch: String,
    pub accepted: Vec<String>,
    /// Chosen claims the library refused at the last moment, with its words.
    pub refused: Vec<(String, String)>,
    pub skipped: Vec<Skip>,
}

/// The operator's act. Re-plans with the recheck on, accepts the planned claims (or only `only`, when the operator ticked some) through the library's
/// operator path, marks each `batch-accepted` with the current audit batch, and journals the act. An id in `only` that is not in the plan is reported
/// as refused, never accepted. Errors, writing nothing, while accept-all is disabled.
pub fn accept(lib: &Library, state: &Path, only: Option<&[String]>, now: i64) -> Result<Outcome, String> {
    let p = plan(lib, state, true);
    if let Some(why) = p.disabled {
        return Err(why);
    }
    let batch = current_batch(state);
    let mut out = Outcome { audit_batch: batch.clone(), accepted: Vec::new(), refused: Vec::new(), skipped: p.skipped };
    let planned: BTreeSet<&str> = p.rows.iter().map(|r| r.id.as_str()).collect();
    if let Some(ids) = only {
        for id in ids.iter().filter(|i| !planned.contains(i.as_str())) {
            out.refused.push((id.clone(), "not in the list of vetted claims that accept-all can take now".into()));
        }
    }
    for r in &p.rows {
        if only.is_some_and(|ids| !ids.contains(&r.id)) {
            continue;
        }
        match lib.validate_claim_in_batch(&r.id, &batch) {
            Ok(_) => out.accepted.push(r.id.clone()),
            Err(e) => out.refused.push((r.id.clone(), e)),
        }
    }
    Journal::new(state).append(
        "accept_all",
        json!({"audit_batch": batch, "accepted": out.accepted, "refused": out.refused.len(), "skipped": out.skipped.len(), "partial": only.is_some(), "by": "operator"}),
        now,
    );
    Ok(out)
}

/// The time of the last good audit of the batch-accepted claims: the latest answer in any audit batch whose `batch` sample met the floor with no
/// error (`specs/100` §7.3). `None` when there has been none.
pub fn last_good_audit(state: &Path) -> Option<i64> {
    let events = Journal::new(state).all();
    let mut per: BTreeMap<String, (usize, usize, i64)> = BTreeMap::new();
    for e in events.iter().filter(|e| e["kind"] == "tier_audit" && e["tier"] == "batch") {
        let Some(b) = e["batch"].as_str() else { continue };
        let at = e["at"].as_i64().unwrap_or(0);
        let s = per.entry(b.to_string()).or_insert((0, 0, 0));
        match e["answer"].as_str() {
            Some("true") => s.0 += 1,
            Some("false") => {
                s.0 += 1;
                s.1 += 1;
            }
            _ => {}
        }
        s.2 = s.2.max(at);
    }
    per.values().filter(|(n, errors, _)| *n >= tier_audit::SAMPLE_FLOOR && *errors == 0).map(|(_, _, at)| *at).max()
}

/// After a roll-back: flag the batch-accepted claims accepted since the last good audit `re-audit due` (`specs/100` §7.3). They stay `validated` and
/// searchable; nothing is retired. Idempotent. Journals the count. Returns how many were flagged now.
pub fn flag_reaudit(lib: &Library, state: &Path, now: i64) -> usize {
    let since = last_good_audit(state);
    let due: Vec<String> = lib
        .batch_accepted()
        .into_iter()
        .filter(|(_, _, at, _)| since.is_none_or(|t| delphi_core::xenia_index::unix_of(at).unwrap_or(0) > t))
        .map(|(id, ..)| id)
        .collect();
    let n = lib.mark_reaudit_due(&due);
    if n > 0 {
        Journal::new(state).append("reaudit_due", json!({"count": n, "since_good_audit": since, "audit_batch": current_batch(state)}), now);
    }
    n
}

/// How many batch-accepted claims are flagged `re-audit due` now.
pub fn reaudit_due_count(lib: &Library) -> usize {
    lib.batch_accepted().iter().filter(|(_, _, _, due)| *due).count()
}

/// One or two lines for `delphi needs`. Empty when nothing waits.
pub fn needs_line(lib: &Library, state: &Path) -> String {
    let mut s = String::new();
    let p = plan(lib, state, false);
    if p.disabled.is_none() && !p.rows.is_empty() {
        s.push_str(&format!(
            "Accept all vetted: {} claim(s) in {} group(s) wait for you; `delphi validation accept-all` lists them and accepts only when you type so at a terminal ({} left out and why)\n",
            p.rows.len(),
            grouped(&p.rows).len(),
            p.skipped.len()
        ));
    }
    let due = reaudit_due_count(lib);
    if due > 0 {
        s.push_str(&format!("Re-audit due: {due} batch-accepted claim(s) were accepted since the last good audit; `delphi validation tier-audit --tier batch` samples them\n"));
    }
    s
}

/// The id of the decision card for an audit batch. A new batch (a `tier restore`) is a new card.
pub fn card_id(batch: &str) -> String {
    format!("accept-all-{}", short(batch, 24))
}

/// What the card asks the operator to judge, hashed: the batch, the claims on offer and the claims left out. An answer to a card that has changed
/// since it was shown (a claim rejected, a source changed, a tier rolled back) is refused, as for every digest card.
pub fn card_hash(plan: &Plan, batch: &str) -> String {
    let mut s = format!("{batch}\n");
    for r in &plan.rows {
        s.push_str(&r.id);
        s.push('\n');
    }
    s.push('|');
    for k in &plan.skipped {
        s.push_str(&k.id);
        s.push('\n');
    }
    blake3::hash(s.as_bytes()).to_hex().to_string()
}

/// The choice id of a per-claim rejection: `reject:<claim id>`.
pub const REJECT_PREFIX: &str = "reject:";
/// How many claims a card offers a rejection for; the rest are rejected with `delphi validation accept-all --reject <id>` at a terminal.
const REJECT_CHOICES: usize = 8;

/// The decision card the digest would carry for the batch (`specs/052` §3.3, kind `learn`): counts by group, what is left out and why, and the
/// command that does it. Pure data. `None` when nothing waits or accept-all is disabled. **Accepting is never a choice on the card**: it is the
/// operator's act at a terminal, so the card points at the command. Its choices are to leave the batch for now (`skip`) and, per claim, to
/// reject one as wrong (`reject:<id>`, which marks that claim `disproved`; nothing is deleted). Both are the operator's, via the console.
pub fn card(plan: &Plan, now: &str, batch: &str) -> Option<Value> {
    if plan.disabled.is_some() || plan.rows.is_empty() {
        return None;
    }
    let groups: Vec<String> = grouped(&plan.rows).iter().map(|(g, v)| format!("{g}: {} claim(s)", v.len())).collect();
    let mut changes = groups;
    for s in &plan.skipped {
        changes.push(format!("left out: {} ({})", short(&s.text, 80), short(&s.why, 80)));
    }
    changes.push(GLANCE_NOTE.to_string());
    let mut choices = vec![json!({"id": "skip", "label": "Leave them for now", "effect": "digest", "reversible": true})];
    for r in plan.rows.iter().take(REJECT_CHOICES) {
        choices.push(json!({"id": format!("{REJECT_PREFIX}{}", r.id), "label": format!("Reject as wrong (disproved): {}", short(&r.text, 60)), "effect": "digest", "reversible": false}));
    }
    Some(json!({
        "id": card_id(batch), "source": "digest", "source_id": "accept_all_vetted", "label": "DIGEST", "payload_blake3": card_hash(plan, batch),
        "kind": "learn", "tier": "open", "title": format!("{} vetted claim(s) wait for your accept", plan.rows.len()),
        "requester": "daemon", "task": "", "raised_at": now, "timeout_secs": Value::Null,
        "brief": {
            "summary": "These claims passed the validator's dry run and have 5 or more independent origins with 2 or more trusted sources. They are not validated until you accept. Nothing waits on this.",
            "changes": changes, "state": "plain",
            "recommendation": {"choice_id": "skip", "why": "accepting is yours, at a terminal: `delphi validation accept-all`"},
        },
        "choices": choices,
        "tickable": false, "tickable_reason": "accepting is done with the command, not from this card",
    }))
}

/// Has the operator already left this card (this batch, these claims) for now? The card is not offered again until it changes.
pub fn dismissed(events: &[EventEnvelope], id: &str, hash: &str) -> bool {
    events.iter().any(|e| e.kind == "accept_all.card_dismissed" && e.payload["id"] == id && e.payload["payload_blake3"] == hash)
}

/// The operator marks one claim of the batch wrong: it reads `disproved` through the library's existing operator path (`Library::reject_claim`),
/// leaves the batch, and nothing is deleted. Only a claim that accept-all would take now can be rejected here, so this is not a way to disprove an
/// arbitrary claim. Journaled. `by` is the operator's console or terminal; the callers are the console action and `--reject`, both refused inside
/// an agent seat.
pub fn reject_one(lib: &Library, state: &Path, claim: &str, by: &str, now: i64) -> Result<(), String> {
    let p = plan(lib, state, false);
    if let Some(why) = p.disabled {
        return Err(why);
    }
    if !p.rows.iter().any(|r| r.id == claim) {
        return Err("that claim is not in the list of vetted claims that accept-all can take now".into());
    }
    lib.reject_claim(claim)?;
    Journal::new(state).append("accept_all_reject", json!({"claim": claim, "audit_batch": current_batch(state), "by": by}), now);
    Ok(())
}

/// The operator's answer to the card: which card, the hash it was shown with, and the choice.
#[derive(Debug, Clone, Copy)]
pub struct Answer<'a> {
    pub card_id: &'a str,
    pub payload_blake3: &'a str,
    pub choice: &'a str,
}

/// What the operator's choice on the card does, as the events to append. `Err`, writing nothing, while accept-all is disabled, when the card has
/// changed since it was shown (`hash`), for a choice the card does not offer, and for a claim outside the batch. `skip` leaves the card; `reject:<id>`
/// disproves one claim ([`reject_one`]). There is deliberately no choice that accepts.
pub fn act(mission_id: &str, lib: &Library, state: &Path, answer: &Answer, by: &str, now: i64) -> Result<Vec<EventEnvelope>, String> {
    let (id, hash, choice) = (answer.card_id, answer.payload_blake3, answer.choice);
    let p = plan(lib, state, false);
    if let Some(why) = &p.disabled {
        return Err(why.clone());
    }
    let batch = current_batch(state);
    if p.rows.is_empty() || id != card_id(&batch) || hash != card_hash(&p, &batch) {
        return Err("the accept-all card changed since you looked at it; open it again".into());
    }
    if choice == "skip" {
        return Ok(vec![EventEnvelope::new(mission_id, "accept_all.card_dismissed", json!({"id": id, "payload_blake3": hash}))]);
    }
    let Some(claim) = choice.strip_prefix(REJECT_PREFIX) else {
        return Err(format!("`{choice}` is not an action of this card (skip, or {REJECT_PREFIX}<claim id>); accepting is done with `delphi validation accept-all` at a terminal"));
    };
    reject_one(lib, state, claim, by, now)?;
    Ok(vec![EventEnvelope::new(mission_id, "accept_all.claim_rejected", json!({"claim": claim, "by": by, "audit_batch": batch}))])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::library::claims::{AnchorSpec, ClaimOpts, BATCH_MARK};
    use crate::library::{AddOpts, Budgets};

    pub(crate) struct Fix {
        pub(crate) _d: tempfile::TempDir,
        pub(crate) lib: Library,
        pub(crate) state: std::path::PathBuf,
    }

    fn fixture() -> Fix {
        let d = tempfile::tempdir().unwrap();
        let (home, ws) = (d.path().join("home"), d.path().join("ws"));
        std::fs::create_dir_all(&ws).unwrap();
        let lib = Library::in_home(&home);
        let state = crate::fact_mode::state_dir(&ws);
        Fix { _d: d, lib, state }
    }

    /// The same fixture over a home and workspace that already exist (a service test's), with its own scratch directory for the source pages.
    pub(crate) fn fixture_in(home: &std::path::Path, ws_dir: &std::path::Path) -> Fix {
        Fix { _d: tempfile::tempdir().unwrap(), lib: Library::in_home(home), state: crate::fact_mode::state_dir(ws_dir) }
    }

    /// A claim on the vetted rung the real way: five sources as the fetcher leaves them (each its own origin, a reputable class, an address), the
    /// claim anchored in all five, its support computed by the library, and the validator's dry run recorded. Returns the claim id and its source ids.
    pub(crate) fn vetted_with_sources(f: &Fix, name: &str, text: &str) -> (String, Vec<String>) {
        let topics = ["harbours", "fishing", "navigation", "weather", "lighthouses"];
        let quote = text.trim_end_matches('.').to_string();
        let ids: Vec<String> = topics
            .iter()
            .enumerate()
            .map(|(i, topic)| {
                let host = format!("{name}-{i}.example");
                let page = f._d.path().join(format!("{host}.md"));
                std::fs::write(&page, format!("# {topic}\n\nA separate write-up about {topic} for {name}, with its own wording and its own examples. Here {text} {topic} is all this page covers, in {host}.\n")).unwrap();
                let s = f.lib.add_file(&page, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap();
                let mut m = f.lib.read_meta(&s.id).unwrap();
                m["canonical_url"] = json!(format!("https://{host}/page"));
                m["origin"] = json!(host);
                m["content_hash"] = json!(format!("sha256:{host}"));
                m["source_class"] = json!("reputable_secondary");
                m["source_unfamiliar"] = json!(false);
                std::fs::write(f.lib.source_dir(&s.id).join("meta.json"), serde_json::to_vec(&m).unwrap()).unwrap();
                s.id
            })
            .collect();
        let specs: Vec<AnchorSpec> = ids.iter().map(|s| AnchorSpec { source_id: s.clone(), locator: Some("§1".into()), quote: quote.clone(), ..Default::default() }).collect();
        let id = f.lib.add_claim(text, &ClaimOpts::default(), &specs).unwrap()["id"].as_str().unwrap().to_string();
        let c = f.lib.refresh_support(&id).unwrap();
        assert_eq!((c["support"]["origins"].as_u64(), c["support"]["trusted_sources"].as_u64()), (Some(5), Some(5)), "the fixture really has five trusted origins: {c}");
        f.lib.record_would_ratify(&crate::fact_store::fact_id(text));
        (id, ids)
    }

    pub(crate) fn vetted(f: &Fix, name: &str, text: &str) -> String {
        vetted_with_sources(f, name, text).0
    }

    #[test]
    fn a_vetted_claim_is_listed_with_its_counts_and_a_protected_a_contested_and_a_changed_source_claim_are_left_out_with_a_reason() {
        let f = fixture();
        let ok = vetted(&f, "harbour", "The harbour closes at dusk.");
        let prot = vetted(&f, "advisory", "CVE-2026-0001 is a vulnerability in the harbour crane.");
        let contested = vetted(&f, "tides", "The tide rises twice a day.");
        let mut c = f.lib.claim(&contested).unwrap();
        c["contested_by"] = json!([{"reason": "contradicted", "source": "x"}]);
        std::fs::write(f.lib.claims_dir().join(format!("{contested}.json")), serde_json::to_vec(&c).unwrap()).unwrap();
        let (changed, changed_sources) = vetted_with_sources(&f, "lights", "The lighthouse lamp is lit by hand.");
        // one source went away after the dry run (written to its record without telling the claim): the counts the check described are no longer
        // the claim's counts, and the recheck at accept time finds it
        let mut m = f.lib.read_meta(&changed_sources[0]).unwrap();
        m["retired"] = json!("now");
        std::fs::write(f.lib.source_dir(&changed_sources[0]).join("meta.json"), serde_json::to_vec(&m).unwrap()).unwrap();

        let p = plan(&f.lib, &f.state, true);
        let ids: Vec<&str> = p.rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec![ok.as_str()], "only the clean vetted claim is offered: {p:?}");
        let why = |id: &str| p.skipped.iter().find(|s| s.id == id).map(|s| s.why.clone()).unwrap_or_default();
        assert!(why(&prot).contains("protected class"), "{p:?}");
        assert!(why(&contested).contains("contested"), "{p:?}");
        assert!(why(&changed).contains("sources changed"), "{p:?}");
        let r = &p.rows[0];
        assert_eq!((r.origins, r.trusted_sources), (5, 5));
        assert!(r.domains.iter().any(|d| d == "harbour-0.example") && r.domains.len() == 5, "{r:?}");
        assert!(!r.quote.is_empty() && !r.checked_at.is_empty(), "{r:?}");
    }

    #[test]
    fn accept_marks_each_claim_batch_accepted_with_the_audit_batch_through_the_operators_validation_and_skips_the_rest() {
        let f = fixture();
        let a = vetted(&f, "a", "The harbour closes at dusk.");
        let b = vetted(&f, "b", "The lamp is lit by hand.");
        let prot = vetted(&f, "p", "CVE-2026-0002 is a vulnerability in the crane.");
        let out = accept(&f.lib, &f.state, None, 100).unwrap();
        assert_eq!(out.accepted.len(), 2, "{out:?}");
        for id in [&a, &b] {
            let c = f.lib.claim(id).unwrap();
            assert_eq!(c["validation"]["by"], "operator");
            assert_eq!(c["validation"]["batch"]["mark"], BATCH_MARK);
            assert_eq!(c["validation"]["batch"]["audit_batch"].as_str(), Some(out.audit_batch.as_str()));
            assert_eq!(crate::library::claims::status(&c), Status::Validated);
        }
        let pc = f.lib.claim(&prot).unwrap();
        assert!(pc["validation"].is_null(), "the protected claim was not accepted");
        assert_eq!(f.lib.tier_members("batch").len(), 2, "the audit's batch tier is now the marked claims");
        assert!(Journal::new(&f.state).all().iter().any(|e| e["kind"] == "accept_all" && e["by"] == "operator" && e["partial"] == false));
        // one at a time is not marked
        let one = vetted(&f, "c", "The quay is open on Sundays.");
        let c = f.lib.validate_claim(&one, crate::library::claims::Validator::Operator).unwrap();
        assert!(c["validation"]["batch"].is_null());
    }

    #[test]
    fn accept_some_takes_only_the_ticked_ids_and_refuses_an_id_that_is_not_in_the_list() {
        let f = fixture();
        let a = vetted(&f, "a", "The harbour closes at dusk.");
        let b = vetted(&f, "b", "The lamp is lit by hand.");
        let out = accept(&f.lib, &f.state, Some(&[a.clone(), "nope".into()]), 100).unwrap();
        assert_eq!(out.accepted, vec![a.clone()]);
        assert!(out.refused.iter().any(|(id, _)| id == "nope"));
        assert!(f.lib.claim(&b).unwrap()["validation"].is_null());
        assert!(Journal::new(&f.state).all().iter().any(|e| e["kind"] == "accept_all" && e["partial"] == true));
    }

    #[test]
    fn accept_all_is_disabled_while_the_vetted_tier_is_rolled_back_writes_nothing_and_says_why() {
        let f = fixture();
        let a = vetted(&f, "a", "The harbour closes at dusk.");
        tier_audit::rollback(&f.state, "vetted", "operator", "an audit found a false claim", 50).unwrap();
        let p = plan(&f.lib, &f.state, true);
        assert!(p.rows.is_empty() && p.disabled.as_deref().is_some_and(|w| w.contains("rolled back") && w.contains("tier restore")), "{p:?}");
        let before = std::fs::read(f.lib.claims_dir().join(format!("{a}.json"))).unwrap();
        let err = accept(&f.lib, &f.state, None, 100).unwrap_err();
        assert!(err.contains("rolled back"), "{err}");
        assert_eq!(std::fs::read(f.lib.claims_dir().join(format!("{a}.json"))).unwrap(), before, "nothing was written");
        assert!(!Journal::new(&f.state).all().iter().any(|e| e["kind"] == "accept_all"));
        assert!(card(&p, "t", "b").is_none());
        // restoring it brings accept-all back
        tier_audit::restore(&f.state, "vetted", "audited again and fine", 60).unwrap();
        assert_eq!(plan(&f.lib, &f.state, true).rows.len(), 1);
    }

    #[test]
    fn a_rollback_flags_the_batch_accepted_claims_since_the_last_good_audit_re_audit_due_and_deletes_nothing() {
        let f = fixture();
        let a = vetted(&f, "a", "The harbour closes at dusk.");
        accept(&f.lib, &f.state, None, 100).unwrap();
        let before = f.lib.claim(&a).unwrap();
        tier_audit::rollback(&f.state, "vetted", "operator", "the operator lost trust in it", 200).unwrap();
        // the claim's acceptance time is "now" in rfc3339; with no good audit, every batch-accepted claim is due
        assert_eq!(flag_reaudit(&f.lib, &f.state, 300), 1);
        assert_eq!(flag_reaudit(&f.lib, &f.state, 301), 0, "idempotent");
        let after = f.lib.claim(&a).unwrap();
        assert_eq!(after["validation"]["batch"]["reaudit_due"], true);
        assert_eq!(crate::library::claims::status(&after), Status::Validated, "still validated: the operator accepted it");
        assert_eq!(after["text"], before["text"], "not retired, not edited");
        assert_eq!(reaudit_due_count(&f.lib), 1);
        assert!(needs_line(&f.lib, &f.state).contains("Re-audit due: 1"));
        // an audit answer of true clears the flag for that claim
        f.lib.clear_reaudit_due(&a);
        assert_eq!(reaudit_due_count(&f.lib), 0);
    }

    #[test]
    fn a_good_audit_covers_what_was_accepted_before_it() {
        let f = fixture();
        // 30 true answers in one audit batch: a good audit at time 1000
        let j = Journal::new(&f.state);
        let batch = current_batch(&f.state);
        for i in 0..30 {
            j.append("tier_audit", json!({"card": format!("c{i}"), "tier": "batch", "answer": "true", "batch": batch}), 1000);
        }
        assert_eq!(last_good_audit(&f.state), Some(1000));
        // 29 answers do not make a good audit; neither does one error
        let g = fixture();
        let batch = current_batch(&g.state);
        let j = Journal::new(&g.state);
        for i in 0..29 {
            j.append("tier_audit", json!({"card": format!("c{i}"), "tier": "batch", "answer": "true", "batch": batch}), 1000);
        }
        assert_eq!(last_good_audit(&g.state), None);
        j.append("tier_audit", json!({"card": "c30", "tier": "batch", "answer": "false", "batch": batch}), 1001);
        assert_eq!(last_good_audit(&g.state), None);
    }

    #[test]
    fn the_glance_is_three_rows_in_a_fixed_order_and_says_it_bounds_nothing_and_the_card_only_points_at_the_command() {
        let f = fixture();
        for (i, t) in ["The harbour closes at dusk.", "The lamp is lit by hand.", "The quay is open on Sundays.", "The bell rings at noon."].iter().enumerate() {
            vetted(&f, &format!("n{i}"), t);
        }
        let p = plan(&f.lib, &f.state, false);
        assert_eq!(p.rows.len(), 4);
        let s1: Vec<&str> = sample(&p.rows, "b1").iter().map(|r| r.id.as_str()).collect();
        let s2: Vec<&str> = sample(&p.rows, "b1").iter().map(|r| r.id.as_str()).collect();
        assert_eq!((s1.len(), s1.clone()), (3, s2), "the same three in the same batch");
        assert!(GLANCE_NOTE.contains("bounds nothing") && GLANCE_NOTE.contains("tier-audit --tier batch"));
        let c = card(&p, "2026-11-11T00:00:00Z", "b1").unwrap();
        assert_eq!((c["kind"].as_str(), c["source_id"].as_str(), c["tickable"].as_bool()), (Some("learn"), Some("accept_all_vetted"), Some(false)));
        let choices = c["choices"].as_array().unwrap();
        let ids: Vec<&str> = choices.iter().filter_map(|x| x["id"].as_str()).collect();
        assert_eq!(ids[0], "skip");
        assert!(ids[1..].iter().all(|i| i.starts_with(REJECT_PREFIX)) && ids.len() == 5, "skip, then one rejection per claim: {c}");
        assert!(!ids.iter().any(|i| i.contains("accept")), "the card offers no choice that accepts: {c}");
        assert!(needs_line(&f.lib, &f.state).contains("Accept all vetted: 4 claim(s)"));
    }

    fn card_of(f: &Fix) -> (String, String, Plan) {
        let p = plan(&f.lib, &f.state, false);
        let batch = current_batch(&f.state);
        (card_id(&batch), card_hash(&p, &batch), p)
    }

    #[test]
    fn leaving_the_card_hides_it_until_the_batch_changes() {
        let f = fixture();
        vetted(&f, "a", "The harbour closes at dusk.");
        let (id, hash, _) = card_of(&f);
        let evs = act("m1", &f.lib, &f.state, &Answer { card_id: &id, payload_blake3: &hash, choice: "skip" }, "console-1", 10).unwrap();
        assert_eq!(evs.len(), 1);
        assert!(dismissed(&evs, &id, &hash), "the dismissal names this card and this hash");
        assert!(!dismissed(&evs, &id, "some-other-hash"));
        // a second vetted claim changes what the card asks, so the old dismissal no longer hides it
        vetted(&f, "b", "The lamp is lit by hand.");
        let (_, hash2, _) = card_of(&f);
        assert_ne!(hash, hash2);
        assert!(!dismissed(&evs, &id, &hash2));
    }

    #[test]
    fn rejecting_a_claim_disproves_that_one_claim_deletes_nothing_and_touches_no_other() {
        let f = fixture();
        let a = vetted(&f, "a", "The harbour closes at dusk.");
        let b = vetted(&f, "b", "The lamp is lit by hand.");
        let (id, hash, _) = card_of(&f);
        let choice = format!("{REJECT_PREFIX}{a}");
        let evs = act("m1", &f.lib, &f.state, &Answer { card_id: &id, payload_blake3: &hash, choice: &choice }, "console-1", 10).unwrap();
        assert_eq!((evs.len(), evs[0].kind.as_str()), (1, "accept_all.claim_rejected"));
        let ca = f.lib.claim(&a).unwrap();
        assert_eq!(ca["lifecycle"], "disproved");
        assert_eq!(ca["disproved_by"]["by"], "operator");
        assert!(ca["anchors"].as_array().is_some_and(|x| !x.is_empty()), "nothing was deleted");
        assert!(f.lib.claim(&b).unwrap()["lifecycle"].is_null(), "the other claim is untouched");
        assert!(Journal::new(&f.state).all().iter().any(|e| e["kind"] == "accept_all_reject" && e["claim"] == a.as_str() && e["by"] == "console-1"));
        // the rejected claim has left the batch, and the card has changed with it
        let (_, hash2, p2) = card_of(&f);
        assert_eq!(p2.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec![b.as_str()]);
        assert_ne!(hash, hash2);
    }

    #[test]
    fn the_card_refuses_a_stale_answer_a_claim_outside_the_batch_a_made_up_choice_and_an_accept() {
        let f = fixture();
        let a = vetted(&f, "a", "The harbour closes at dusk.");
        let (id, hash, _) = card_of(&f);
        let run = |card: &str, h: &str, choice: &str| act("m1", &f.lib, &f.state, &Answer { card_id: card, payload_blake3: h, choice }, "console-1", 10);
        assert!(run(&id, "stale", "skip").unwrap_err().contains("changed"));
        assert!(run("accept-all-other", &hash, "skip").unwrap_err().contains("changed"));
        assert!(run(&id, &hash, &format!("{REJECT_PREFIX}no-such-claim")).unwrap_err().contains("not in the list"));
        assert!(run(&id, &hash, "accept").unwrap_err().contains("not an action of this card"));
        assert!(run(&id, &hash, "accept all").unwrap_err().contains("at a terminal"));
        assert!(f.lib.claim(&a).unwrap()["lifecycle"].is_null() && f.lib.claim(&a).unwrap()["validated_by"].is_null(), "nothing was written");
    }

    #[test]
    fn the_card_and_the_reject_are_refused_while_the_tier_is_rolled_back() {
        let f = fixture();
        let a = vetted(&f, "a", "The harbour closes at dusk.");
        let (id, hash, _) = card_of(&f);
        tier_audit::rollback(&f.state, "vetted", "operator", "an audit found a false claim", 50).unwrap();
        let choice = format!("{REJECT_PREFIX}{a}");
        let err = act("m1", &f.lib, &f.state, &Answer { card_id: &id, payload_blake3: &hash, choice: &choice }, "console-1", 60).unwrap_err();
        assert!(err.contains("rolled back"), "{err}");
        assert!(reject_one(&f.lib, &f.state, &a, "operator", 60).unwrap_err().contains("rolled back"));
        assert!(f.lib.claim(&a).unwrap()["lifecycle"].is_null(), "nothing was disproved");
    }
}
