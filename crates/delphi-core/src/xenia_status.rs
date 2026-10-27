//! What an operator sees of a Xenia store (spec 011 §4.1, §5.3, §7.3; plan milestone X9): `delphi xenia status`,
//! the `delphi doctor` checks, the `delphi needs xenia` evidence and the Narrator's events all read this module and
//! nothing else, so they cannot disagree.
//!
//! * [`list_stores`] finds the stores this install joined by their pins, never by anything a store says.
//! * The **journal** (`state/journal.jsonl`, `0600`, beside the sticky file and outside the clone) records the outcome
//!   of each sync and each publish, because `delphi xenia sync` runs in the CLI and the daemon is not in that loop.
//!   It is how tampering found by a sync stays visible in `status` afterwards (the sticky state deliberately keeps
//!   nothing about a refused sync), and how the engine tells a mission what happened without a model turn. Entries
//!   carry fixed slugs, counts and store paths only: never record text.
//! * [`status`] derives the accepted state again (no network) and reports quarantine, `contested`, freshness age,
//!   tampering and the warnings that `doctor` repeats.
//!
//! Nothing here reads a clock: the caller passes `now`.

use crate::xenia_index::{self, Derived};
use crate::xenia_store::Store;
use crate::xenia_sync::{VerifiedSync, VerifyConfig};
use delphi_xenia::freshness::FreshnessState;
use delphi_xenia::is_slug;
use delphi_xenia::pins::Pins;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

/// A joined store, as the pins name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRef {
    pub org: String,
    pub store: String,
    pub url: String,
}

impl StoreRef {
    pub fn id(&self) -> String {
        format!("{}/{}", self.org, self.store)
    }
}

/// Every store with a readable `pins.toml` under `$DELPHI_HOME/xenia/<org>/<store>/state/`, sorted. A directory whose
/// pins do not parse is left out here (and found by `status` when asked for by name).
pub fn list_stores(home: &Path) -> Vec<StoreRef> {
    let mut out = Vec::new();
    let Ok(orgs) = std::fs::read_dir(home.join("xenia")) else { return out };
    for o in orgs.flatten().filter(|e| e.path().is_dir()) {
        let org = o.file_name().to_string_lossy().into_owned();
        if !is_slug(&org, 32) {
            continue;
        }
        let Ok(stores) = std::fs::read_dir(o.path()) else { continue };
        for s in stores.flatten().filter(|e| e.path().is_dir()) {
            let store = s.file_name().to_string_lossy().into_owned();
            if !is_slug(&store, 32) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(s.path().join("state").join("pins.toml")) else { continue };
            if let Ok(p) = Pins::parse(&text) {
                if p.org() == org && p.store() == store {
                    out.push(StoreRef { org: org.clone(), store, url: p.url().to_string() });
                }
            }
        }
    }
    out.sort_by_key(|r| r.id());
    out
}

/// Pick the store a command is about: `org/store`, or the only one joined.
pub fn pick_store(home: &Path, selector: Option<&str>) -> Result<StoreRef, String> {
    let all = list_stores(home);
    match selector {
        Some(sel) => all.into_iter().find(|r| r.id() == sel).ok_or_else(|| format!("no joined store `{sel}` (`delphi xenia status` lists them)")),
        None => match all.len() {
            0 => Err("no store is joined; `delphi xenia join <url> --org <org> --store <store> --admin <fingerprint> --head <epoch>:<hash>`".into()),
            1 => Ok(all.into_iter().next().unwrap()),
            _ => Err(format!("several stores are joined ({}); name one with --store <org>/<store>", all.iter().map(StoreRef::id).collect::<Vec<_>>().join(", "))),
        },
    }
}

/// Open a joined store. A local-path or `file://` URL in the pins was admitted by the operator at join, so it is
/// admitted again here; `https` and `ssh` are the only transports otherwise.
pub fn open(home: &Path, r: &StoreRef) -> delphi_xenia::Result<Store> {
    Store::open(home, &r.org, &r.store, &r.url, !(r.url.starts_with("https://") || r.url.starts_with("ssh://")))
}

// ---------------------------------------------------------------------------------------------------------
// The journal

const MAX_ENTRIES: usize = 200;
const MAX_FINDINGS: usize = 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// 1, 2, 3, ... per store. The engine remembers the last one it told a mission about.
    pub seq: u64,
    /// Unix seconds.
    pub at: i64,
    /// `sync` or `publish`.
    pub kind: String,
    /// A fixed slug: `accepted`, `up-to-date`, `tampering`, `rejected`, `non-fast-forward`, `refused`, `unavailable`;
    /// for a publish `published`, `already-there` or the error's reason.
    pub outcome: String,
    #[serde(default)]
    pub head: Option<String>,
    /// Counts, slugs and paths. Never record text.
    #[serde(default)]
    pub detail: Value,
}

fn journal_path(store: &Store) -> std::path::PathBuf {
    store.paths.state.join("journal.jsonl")
}

pub fn read_journal(store: &Store) -> Vec<Entry> {
    std::fs::read_to_string(journal_path(store)).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn append(store: &Store, kind: &str, outcome: &str, head: Option<String>, detail: Value, now: i64) -> std::io::Result<()> {
    let lock = store.paths.root.join(".journal.lock");
    crate::lock::with_lock(&lock, || {
        let mut entries = read_journal(store);
        let seq = entries.last().map(|e| e.seq).unwrap_or(0) + 1;
        entries.push(Entry { seq, at: now, kind: kind.into(), outcome: outcome.into(), head, detail });
        if entries.len() > MAX_ENTRIES {
            entries.drain(..entries.len() - MAX_ENTRIES);
        }
        let mut text = String::new();
        for e in &entries {
            text.push_str(&serde_json::to_string(e).unwrap_or_default());
            text.push('\n');
        }
        delphi_xenia::pins::write_atomic(&journal_path(store), text.as_bytes()).map_err(|e| std::io::Error::other(e.to_string()))
    })?
}

/// Record an outcome a command produced (the admin operations write `admin` entries). Slugs, commits and counts only.
pub fn record(store: &Store, kind: &str, outcome: &str, head: Option<String>, detail: Value, now: i64) -> std::io::Result<()> {
    append(store, kind, outcome, head, detail, now)
}

fn freshness_fields(f: &FreshnessState) -> Value {
    match f {
        FreshnessState::Fresh { life_used_pct, age_secs, .. } => json!({"freshness": "fresh", "life_used_pct": life_used_pct, "age_secs": age_secs}),
        FreshnessState::Stale { reason } => json!({"freshness": reason}),
    }
}

/// The journal's view of a sync result. Pure.
pub fn sync_entry(outcome: &VerifiedSync) -> (&'static str, Option<String>, Value) {
    match outcome {
        VerifiedSync::UpToDate { head, freshness } => ("up-to-date", Some(head.clone()), freshness_fields(freshness)),
        VerifiedSync::Accepted { head, freshness, quarantined, added, superseded, unchanged, .. } => {
            let mut d = freshness_fields(freshness);
            d["added"] = json!(added);
            d["superseded"] = json!(superseded);
            d["unchanged"] = json!(unchanged);
            d["quarantined"] = json!(quarantined.len());
            ("accepted", Some(head.clone()), d)
        }
        VerifiedSync::Tampering { head, findings } => {
            let list: Vec<Value> = findings.iter().take(MAX_FINDINGS).map(|f| json!([f.path, f.reason])).collect();
            ("tampering", Some(head.clone()), json!({"count": findings.len(), "findings": list}))
        }
        VerifiedSync::Rejected { head, reason } => ("rejected", Some(head.clone()), json!({"reason": reason})),
        VerifiedSync::NonFastForward(n) => (
            "non-fast-forward",
            None,
            json!({"reason": match n.kind {
                crate::xenia_store::Divergence::Diverged => "diverged",
                crate::xenia_store::Divergence::Behind => "behind",
            }}),
        ),
        VerifiedSync::Refused(r) => ("refused", None, json!({"reason": r.reason()})),
        VerifiedSync::Unavailable(_) => ("unavailable", None, json!({})),
    }
}

/// Sync, and remember what happened. A journal that cannot be written does not hide the sync's result.
pub fn sync_and_record(store: &Store, cfg: &VerifyConfig) -> delphi_xenia::Result<VerifiedSync> {
    let outcome = store.sync_verified(cfg)?;
    let (slug, head, detail) = sync_entry(&outcome);
    let _ = append(store, "sync", slug, head, detail, cfg.now);
    Ok(outcome)
}

/// Remember a publish (or endorsement, withdrawal ...). `action` is the command word; `reason` the fixed slug of an error.
pub fn record_publish(store: &Store, action: &str, result: &Result<crate::xenia_publish::Published, crate::xenia_publish::PublishError>, now: i64) {
    let _ = match result {
        Ok(p) => append(store, "publish", if p.already_there { "already-there" } else { "published" }, Some(p.commit.clone()), json!({"action": action, "files": p.paths.len(), "attempts": p.attempts}), now),
        Err(e) => append(store, "publish", e.reason(), None, json!({"action": action}), now),
    };
}

// ---------------------------------------------------------------------------------------------------------
// Status

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterView {
    pub epoch: u64,
    /// People holding an active admin key: `n_admin` of spec 011 §3.2.
    pub admins: usize,
    pub stewards: usize,
    pub members: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FreshnessView {
    /// `fresh`, or the fixed reason endorsements do not count (`expired`, `missing`, ...).
    pub state: String,
    pub age_secs: Option<i64>,
    pub life_used_pct: Option<u8>,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Warn,
    Fail,
}

/// Something the operator should look at, with a fixed id (`doctor` and the events key off it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub id: &'static str,
    pub severity: Severity,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub store: StoreRef,
    /// `None` until a sync has accepted a head.
    pub head: Option<String>,
    pub roster: Option<RosterView>,
    pub freshness: Option<FreshnessView>,
    /// In force by tier.
    pub l2: usize,
    pub l1: usize,
    /// Visible to nobody (author-hidden, tombstoned, contested, held): `(id, slug)`.
    pub hidden: Vec<(String, &'static str)>,
    /// `contested` ids: stewards who cover the record disagree; hidden and flagged to humans and admins.
    pub contested: Vec<String>,
    /// Records (any kind) not in force, by path, with a fixed slug.
    pub quarantine: Vec<(String, &'static str)>,
    pub last_sync: Option<Entry>,
    /// What the last sync found when it refused a head: `(path, reason)`.
    pub tampering: Vec<(String, String)>,
    pub warnings: Vec<Warning>,
}

fn days(secs: i64) -> String {
    match secs {
        s if s < 3600 => format!("{} min", s.max(0) / 60),
        s if s < 48 * 3600 => format!("{} h", s / 3600),
        s => format!("{} days", s / 86400),
    }
}

/// Fewer admins than this and the 2-of-3 roster quorum cannot lose one person (spec 011 §3.2).
pub const MIN_ADMINS: usize = 3;
/// `status` and `doctor` warn when this much of a statement's life is used (spec 011 §4.1).
pub const AGING_PCT: u8 = 50;

fn roster_view(store: &Store) -> Option<RosterView> {
    let r = store.accepted_roster().ok().flatten()?;
    let head = r.head();
    let active: Vec<_> = head.epoch.members.iter().filter(|m| m.is_active()).collect();
    let people: std::collections::BTreeSet<&str> = active.iter().map(|m| m.person.as_str()).collect();
    let stewards: std::collections::BTreeSet<&str> = active.iter().filter(|m| m.steward_areas.is_steward()).map(|m| m.person.as_str()).collect();
    Some(RosterView { epoch: r.head_epoch(), admins: head.epoch.admin_people().len(), stewards: stewards.len(), members: people.len() })
}

fn freshness_view(d: &Derived) -> FreshnessView {
    match &d.freshness {
        FreshnessState::Fresh { issued_at, expires_at, age_secs, life_used_pct } => FreshnessView { state: "fresh".into(), age_secs: Some(*age_secs), life_used_pct: Some(*life_used_pct), issued_at: Some(issued_at.clone()), expires_at: Some(expires_at.clone()) },
        FreshnessState::Stale { reason } => FreshnessView { state: (*reason).into(), age_secs: None, life_used_pct: None, issued_at: None, expires_at: None },
    }
}

/// Read `store` as it stands (no network): the accepted head, judged at `cfg.now`, plus the journal.
pub fn status(store: &Store, r: &StoreRef, cfg: &VerifyConfig) -> delphi_xenia::Result<Status> {
    let journal = read_journal(store);
    let last_sync = journal.iter().rev().find(|e| e.kind == "sync").cloned();
    let tampering: Vec<(String, String)> = match &last_sync {
        Some(e) if e.outcome == "tampering" => e.detail["findings"].as_array().map(|a| a.iter().filter_map(|p| Some((p[0].as_str()?.to_string(), p[1].as_str()?.to_string()))).collect()).unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut st = Status { store: r.clone(), head: None, roster: None, freshness: None, l2: 0, l1: 0, hidden: Vec::new(), contested: Vec::new(), quarantine: Vec::new(), last_sync, tampering, warnings: Vec::new() };
    if store.accepted_head()?.is_none() {
        st.warnings.push(Warning { id: "not-synced", severity: Severity::Warn, text: "nothing has been accepted yet: run `delphi xenia sync`".into() });
    } else {
        let d = xenia_index::derive(store, cfg)?;
        st.head = Some(d.head.clone());
        st.roster = roster_view(store);
        st.freshness = Some(freshness_view(&d));
        for m in &d.memories {
            match (m.hidden, m.tier) {
                (Some(h), _) => {
                    st.hidden.push((m.id.clone(), h));
                    if h == "contested" {
                        st.contested.push(m.id.clone());
                    }
                }
                (None, delphi_xenia::tier::Tier::L2) => st.l2 += 1,
                (None, _) => st.l1 += 1,
            }
        }
        st.quarantine = d.held_back.clone();
    }
    st.warnings.extend(warnings(&st));
    st.warnings.extend(file_warnings(store));
    Ok(st)
}

/// Install state that other users can read or write (`pins.toml` is checked nowhere else; the sticky file is refused on
/// load; the journal decides what `status` says about tampering). Pure file-system reads.
fn file_warnings(store: &Store) -> Vec<Warning> {
    use std::os::unix::fs::PermissionsExt;
    let mut w = Vec::new();
    for (name, path) in [("pins.toml", store.paths.state.join("pins.toml")), ("journal.jsonl", journal_path(store))] {
        if let Ok(m) = std::fs::symlink_metadata(&path) {
            if !m.file_type().is_file() || m.permissions().mode() & 0o077 != 0 {
                w.push(Warning { id: "state-permissions", severity: Severity::Fail, text: format!("{name} is not a private regular file (expected mode 0600, no symlink): `chmod 600` it, or join the store again") });
            }
        }
    }
    w
}

/// Store-controlled text for a terminal: sanitized, one line, short.
fn shown(s: &str) -> String {
    let t = crate::xenia_ingress::one_line(s);
    if t.chars().count() > 120 { format!("{}…", t.chars().take(119).collect::<String>()) } else { t }
}

/// The warnings of spec 011 §4.1 (statement past half its life, stale), §3.2 (fewer than three admins), §4.2/§5.3
/// (tampering, `contested`). Pure.
pub fn warnings(st: &Status) -> Vec<Warning> {
    let mut w = Vec::new();
    if let Some(r) = &st.roster {
        if r.admins < MIN_ADMINS {
            w.push(Warning {
                id: "few-admins",
                severity: Severity::Warn,
                text: format!("only {} admin(s): the 2-of-3 roster quorum needs at least {MIN_ADMINS} people holding an admin key, or losing one stops every roster change and freshness refresh", r.admins),
            });
        }
    }
    if let Some(f) = &st.freshness {
        if f.state != "fresh" {
            w.push(Warning { id: "stale", severity: Severity::Warn, text: format!("the freshness statement is not current ({}): no endorsement counts, so every record reads at most L1 until an admin runs `delphi xenia admin refresh`", f.state) });
        } else if f.life_used_pct.is_some_and(|p| p >= AGING_PCT) {
            w.push(Warning {
                id: "statement-aging",
                severity: Severity::Warn,
                text: format!("the freshness statement is {} old and {}% of its life is used; an admin should refresh it before it lapses", days(f.age_secs.unwrap_or(0)), f.life_used_pct.unwrap_or(0)),
            });
        }
    }
    if !st.tampering.is_empty() {
        w.push(Warning {
            id: "tampering",
            severity: Severity::Fail,
            text: format!("the last sync refused the store's head: {} path(s) were deleted, reverted or taken over. Nothing was applied; what you had stays in force", st.tampering.len()),
        });
    } else if let Some(e) = &st.last_sync {
        if matches!(e.outcome.as_str(), "rejected" | "non-fast-forward" | "refused") {
            w.push(Warning { id: "sync-refused", severity: Severity::Fail, text: format!("the last sync was refused ({}: {}); the store's head was not accepted", e.outcome, e.detail["reason"].as_str().unwrap_or("-")) });
        }
    }
    if !st.contested.is_empty() {
        w.push(Warning { id: "contested", severity: Severity::Warn, text: format!("{} record(s) are contested (covering stewards disagree) and hidden until they agree: {}", st.contested.len(), st.contested.join(", ")) });
    }
    w
}

pub fn render(st: &Status, now: i64) -> String {
    let mut o = format!("Xenia store {}  ({})\n", st.store.id(), st.store.url);
    match &st.head {
        None => o.push_str("  accepted head: none yet\n"),
        Some(h) => o.push_str(&format!("  accepted head: {}\n", &h[..h.len().min(12)])),
    }
    if let Some(r) = &st.roster {
        o.push_str(&format!("  roster:        epoch {}, {} admin(s), {} steward(s), {} member(s)\n", r.epoch, r.admins, r.stewards, r.members));
    }
    if let Some(f) = &st.freshness {
        if f.state == "fresh" {
            o.push_str(&format!("  freshness:     fresh, issued {} ago, {}% of its life used (expires {})\n", days(f.age_secs.unwrap_or(0)), f.life_used_pct.unwrap_or(0), f.expires_at.as_deref().unwrap_or("?")));
        } else {
            o.push_str(&format!("  freshness:     STALE ({}): no endorsement counts\n", f.state));
        }
    }
    if st.head.is_some() {
        o.push_str(&format!("  records:       {} at L2 (endorsed), {} at L1, {} hidden, {} contested, {} quarantined\n", st.l2, st.l1, st.hidden.len(), st.contested.len(), st.quarantine.len()));
    }
    match &st.last_sync {
        Some(e) => o.push_str(&format!("  last sync:     {} ({} ago)\n", e.outcome, days(now - e.at))),
        None => o.push_str("  last sync:     never run from this install\n"),
    }
    if !st.tampering.is_empty() {
        o.push_str("\nTampering found by the last sync (nothing was applied):\n");
        for (p, why) in st.tampering.iter().take(50) {
            o.push_str(&format!("  {}: {}\n", shown(p), shown(why)));
        }
    }
    if !st.contested.is_empty() {
        o.push_str("\nContested (hidden until the stewards who cover them agree):\n");
        for id in st.contested.iter().take(50) {
            o.push_str(&format!("  {}\n", shown(id)));
        }
    }
    let others: Vec<_> = st.hidden.iter().filter(|(_, h)| *h != "contested").collect();
    if !others.is_empty() {
        o.push_str("\nHidden records:\n");
        for (id, why) in others.into_iter().take(50) {
            o.push_str(&format!("  {}: {}\n", shown(id), why));
        }
    }
    if !st.quarantine.is_empty() {
        o.push_str("\nQuarantined (not in force, never loaded):\n");
        for (p, why) in st.quarantine.iter().take(50) {
            o.push_str(&format!("  {}: {}\n", shown(p), why));
        }
        if st.quarantine.len() > 50 {
            o.push_str(&format!("  ... and {} more\n", st.quarantine.len() - 50));
        }
    }
    if !st.warnings.is_empty() {
        o.push_str("\nWarnings:\n");
        for w in &st.warnings {
            o.push_str(&format!("  {} {}\n", if w.severity == Severity::Fail { "FAIL" } else { "warn" }, w.text));
            if let Some(t) = crate::xenia_help::what_to_do(w.id) {
                o.push_str(&format!("       {t}\n"));
            }
        }
    }
    // What each reason word above means, in the words `delphi xenia explain` uses (fixed text, never the store's).
    let mut words: Vec<&str> = st.hidden.iter().map(|(_, h)| *h).chain(st.quarantine.iter().map(|(_, h)| *h)).chain(st.tampering.iter().map(|(_, r)| r.as_str())).collect();
    words.sort_unstable();
    words.dedup();
    let known: Vec<_> = words.iter().filter_map(|w| crate::xenia_help::lookup(w)).take(12).collect();
    if !known.is_empty() {
        o.push_str("\nWhat the words above mean (`delphi xenia explain <word>` for more):\n");
        for x in known {
            o.push_str(&format!("  {}: {} {}\n", x.slug, x.meaning, x.next));
        }
    }
    o
}

/// Lists in the JSON are cut here; the `*_total` beside each says how many there are.
pub const JSON_CAP: usize = 50;

/// The same facts as JSON, for the console (`xenia.status`) and `--json`.
pub fn to_json(st: &Status) -> Value {
    json!({
        "store": st.store.id(), "url": st.store.url, "head": st.head,
        "roster": st.roster.as_ref().map(|r| json!({"epoch": r.epoch, "admins": r.admins, "stewards": r.stewards, "members": r.members})),
        "freshness": st.freshness.as_ref().map(|f| json!({"state": f.state, "age_secs": f.age_secs, "life_used_pct": f.life_used_pct, "expires_at": f.expires_at})),
        "l2": st.l2, "l1": st.l1,
        "hidden": st.hidden.iter().take(JSON_CAP).map(|(i, h)| json!([shown(i), h])).collect::<Vec<_>>(), "hidden_total": st.hidden.len(),
        "contested": st.contested.iter().take(JSON_CAP).map(|i| shown(i)).collect::<Vec<_>>(), "contested_total": st.contested.len(),
        "quarantine": st.quarantine.iter().take(JSON_CAP).map(|(p, h)| json!([shown(p), h])).collect::<Vec<_>>(), "quarantine_total": st.quarantine.len(),
        "tampering": st.tampering.iter().take(JSON_CAP).map(|(p, h)| json!([shown(p), shown(h)])).collect::<Vec<_>>(), "tampering_total": st.tampering.len(),
        "last_sync": st.last_sync.as_ref().map(|e| json!({"outcome": e.outcome, "at": e.at})),
        "warnings": st.warnings.iter().map(|w| json!({"id": w.id, "fail": w.severity == Severity::Fail, "text": w.text, "help": crate::xenia_help::lookup(w.id).map(crate::xenia_help::entry_json)})).collect::<Vec<_>>(),
    })
}

/// Status of every joined store, for `doctor`, `needs` and the console. A store that cannot be read is reported as
/// a failure of its own rather than skipped.
pub fn all(home: &Path, cfg: &VerifyConfig) -> Vec<(StoreRef, Result<Status, String>)> {
    list_stores(home)
        .into_iter()
        .map(|r| {
            let s = open(home, &r).and_then(|store| status(&store, &r, cfg)).map_err(|e| e.to_string());
            (r, s)
        })
        .chain(damaged_stores(home).into_iter().map(|r| (r, Err("pins.toml is damaged or belongs to another store; join the store again".to_string()))))
        .collect()
}

/// Slug-named store directories whose `pins.toml` exists but is not this store's valid pins: [`list_stores`] leaves them
/// out, so without this `doctor` would say nothing about a store whose pins were damaged.
pub fn damaged_stores(home: &Path) -> Vec<StoreRef> {
    let good: Vec<String> = list_stores(home).iter().map(|r| r.id()).collect();
    let mut out = Vec::new();
    let Ok(orgs) = std::fs::read_dir(home.join("xenia")) else { return out };
    for o in orgs.flatten().filter(|e| e.path().is_dir()) {
        let org = o.file_name().to_string_lossy().into_owned();
        let Ok(stores) = std::fs::read_dir(o.path()) else { continue };
        for s in stores.flatten().filter(|e| e.path().is_dir()) {
            let r = StoreRef { org: org.clone(), store: s.file_name().to_string_lossy().into_owned(), url: String::new() };
            if is_slug(&r.org, 32) && is_slug(&r.store, 32) && s.path().join("state").join("pins.toml").exists() && !good.contains(&r.id()) {
                out.push(r);
            }
        }
    }
    out.sort_by_key(|r| r.id());
    out
}

#[cfg(test)]
#[path = "xenia_status_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "xenia_status_hardening_tests.rs"]
mod hardening_tests;
