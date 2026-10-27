//! The read-only Xenia surfaces (spec 011 §4.1, §4.3, §7.3; plan milestone X9b): what `delphi xenia status`,
//! `delphi doctor`, `delphi needs` and the console show about the stores an install has joined.
//!
//! Rules every function here follows:
//!
//! * **A surface never changes state.** Nothing is created, fetched, signed or written, with the one exception of
//!   [`note_sync`], which is called by the sync itself (not by a surface) to remember how the last sync ended.
//!   Stores are opened with `Store::open_existing`, which creates nothing.
//! * **Offline, no model.** Only local files and the local clone are read. The clock is passed in.
//! * **Store text is hostile display text.** Titles, handles, tags, summaries and file names go through
//!   [`shown`] (the ingress sanitizer, one line, truncated) before they enter any struct here. Lists are capped
//!   ([`Capped`]) with the true total kept, so a store with ten thousand quarantined files costs a few lines.
//! * **Damage is data, not a panic.** A damaged or loosened state file becomes a [`Problem`] with a fixed slug;
//!   the rest of the report is still produced from what could be read.
//! * **No key material.** The report holds DIDs of nobody and keys of nobody: heads, counts, slugs and ids only.

use crate::xenia_index::{derive, Derived};
use crate::xenia_ingress::one_line;
use crate::xenia_scan::{scan_text, ScanContext};
use crate::xenia_store::{io, Store};
use crate::xenia_sync::{epoch_files, rerr, VerifiedSync, VerifyConfig};
use delphi_xenia::freshness::FreshnessState;
use delphi_xenia::sticky::is_commit_id;
use delphi_xenia::tier::Tier;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

/// How many orgs and stores a scan of `$DELPHI_HOME/xenia/` looks at.
pub const MAX_ORGS: usize = 16;
pub const MAX_STORES_PER_ORG: usize = 16;
/// Entries listed per list; the rest is "and N more".
pub const LIST_CAP: usize = 25;
/// Cards returned by [`pending_for`].
pub const CARD_CAP: usize = 50;
/// Text caps for one displayed field.
pub const TITLE_CAP: usize = 120;
pub const SUMMARY_CAP: usize = 400;
pub const TAG_CAP: usize = 40;
pub const TAGS_CAP: usize = 12;
/// A statement is "near expiry" for an admin once this share of its life is used (spec 011 §4.1: warn at 50%).
pub const WARN_LIFE_PCT: u8 = 50;
/// A store unreachable for this many days is a doctor warning (offline is allowed; it is only a warning).
pub const UNREACHABLE_WARN_DAYS: i64 = 7;

const LAST_SYNC: &str = "last-sync.json";
const FETCHED_REF: &str = "refs/xenia/fetched";

// ---------------------------------------------------------------------------------------------------------
// Display safety

/// Store text made safe to show: sanitized by the ingress sanitizer (controls, ANSI remnants, bidi and invisible
/// characters, fake prompt tags), one line, at most `max` characters. The result is data: no caller may render it as
/// markup, and the console draws it as text.
pub fn shown(s: &str, max: usize) -> String {
    let line = one_line(s);
    if line.chars().count() <= max {
        return line;
    }
    let mut cut: String = line.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// A path from a store, shown only when it is plain ASCII of the characters a store path legitimately has.
fn safe_path(p: &str) -> String {
    if !p.is_empty() && p.len() <= 120 && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/')) {
        p.to_string()
    } else {
        "(unprintable path)".to_string()
    }
}

/// A fixed slug from the library: lowercase letters, digits and dashes only, else a placeholder.
fn slug(s: &str) -> String {
    if !s.is_empty() && s.len() <= 48 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        s.to_string()
    } else {
        "unknown".to_string()
    }
}

// ---------------------------------------------------------------------------------------------------------
// The report

/// A list with its true size: at most [`LIST_CAP`] entries are kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Capped<T> {
    pub total: usize,
    pub shown: Vec<T>,
}

impl<T> Default for Capped<T> {
    fn default() -> Self {
        Capped { total: 0, shown: Vec::new() }
    }
}

impl<T> Capped<T> {
    pub fn collect_capped(items: impl IntoIterator<Item = T>, cap: usize) -> Capped<T> {
        let mut c = Capped { total: 0, shown: Vec::new() };
        for i in items {
            c.total += 1;
            if c.shown.len() < cap {
                c.shown.push(i);
            }
        }
        c
    }

    pub fn more(&self) -> usize {
        self.total.saturating_sub(self.shown.len())
    }
}

/// Something that stops part of the report from being produced or trusted. `file` and `slug` are fixed words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// `store`, `sticky`, `pins`, `last-sync`, `identity`, `roster`, `records`.
    pub file: &'static str,
    /// `missing`, `damaged`, `loosened-permissions`, `not-a-file`, `unreadable`, `not-joined`, or a library reason.
    pub slug: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Tiers {
    pub l2: usize,
    pub l1: usize,
    pub l0: usize,
    /// Records hidden by a tombstone or withdrawal (they read as L0 and are in none of the other counts).
    pub hidden: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FreshnessView {
    /// `fresh` or `stale`.
    pub state: &'static str,
    /// For stale: the fixed reason (`expired`, `missing`, `nothing-accepted`, ...). Empty when fresh.
    pub reason: String,
    pub issued_at: String,
    pub expires_at: String,
    pub age_secs: i64,
    pub life_used_pct: u8,
}

impl FreshnessView {
    fn of(f: &FreshnessState) -> FreshnessView {
        match f {
            FreshnessState::Fresh { issued_at, expires_at, age_secs, life_used_pct } => {
                FreshnessView { state: "fresh", reason: String::new(), issued_at: issued_at.clone(), expires_at: expires_at.clone(), age_secs: *age_secs, life_used_pct: *life_used_pct }
            }
            FreshnessState::Stale { reason } => FreshnessView { state: "stale", reason: slug(reason), issued_at: String::new(), expires_at: String::new(), age_secs: 0, life_used_pct: 100 },
        }
    }

    pub fn warns(&self) -> bool {
        self.state == "stale" || self.life_used_pct >= WARN_LIFE_PCT
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Quarantined {
    pub path: String,
    pub reason: String,
}

/// How the last sync ended, as `sync_verified` recorded it (all fields are fixed slugs and numbers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncNote {
    pub v: u32,
    /// Unix seconds of the sync.
    pub at: i64,
    /// `accepted`, `up-to-date`, `tampering`, `rejected`, `non-fast-forward`, `refused`, `unavailable`.
    pub outcome: String,
    /// The fixed reason for anything but `accepted`/`up-to-date`/`unavailable`.
    pub reason: String,
    /// Findings of a `tampering` outcome.
    pub findings: usize,
    /// Unix seconds when the host last answered at all (any outcome but `unavailable`).
    pub last_reachable: Option<i64>,
}

const OUTCOMES: [&str; 7] = ["accepted", "up-to-date", "tampering", "rejected", "non-fast-forward", "refused", "unavailable"];

impl SyncNote {
    fn valid(&self) -> bool {
        self.v == 1 && OUTCOMES.contains(&self.outcome.as_str()) && (self.reason.is_empty() || slug(&self.reason) == self.reason) && self.findings <= 1_000_000
    }

    /// Does this outcome ask the operator to look (tampering, a refused or rejected head)?
    pub fn alarming(&self) -> bool {
        matches!(self.outcome.as_str(), "tampering" | "rejected" | "non-fast-forward" | "refused")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RewritePending {
    /// The fetched commit waiting for the operator (hex).
    pub fetched: String,
    /// What to run. The command is built by the X9a terminal flow; the surface only names it.
    pub command: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoreStatus {
    pub org: String,
    pub store: String,
    pub problems: Vec<Problem>,
    pub accepted_head: Option<String>,
    pub roster_epoch: Option<u64>,
    /// Active admins, counted as people (spec 011 §3.2 `n_admin`).
    pub admins: Option<usize>,
    pub members: Option<usize>,
    pub freshness: Option<FreshnessView>,
    pub tiers: Tiers,
    /// Records in the `contested` state (hidden, flagged to humans and admins).
    pub contested: Capped<String>,
    pub quarantined: Capped<Quarantined>,
    /// Quarantined files per fixed reason slug (all of them, not only the ones listed).
    pub quarantine_by_reason: BTreeMap<String, usize>,
    pub last_sync: Option<SyncNote>,
    /// Whole days since the host last answered, when the last sync could not reach it.
    pub unreachable_days: Option<i64>,
    pub rewrite_pending: Option<RewritePending>,
    /// Whether this install's key is an active admin / a steward in the head epoch (for `needs` and the console).
    pub me: Option<MyStanding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MyStanding {
    pub admin: bool,
    pub steward: bool,
}

// ---------------------------------------------------------------------------------------------------------
// Discovery

/// The `(org, store)` pairs under `$DELPHI_HOME/xenia/` that have a clone. Names that are not slugs are ignored
/// (a hostile directory name never reaches a path or the screen); symlinks are not followed.
pub fn discover(home: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let root = home.join("xenia");
    let dirs = |p: &Path| -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(p)
            .map(|rd| rd.flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).filter_map(|e| e.file_name().into_string().ok()).filter(|n| delphi_xenia::is_slug(n, 32)).collect())
            .unwrap_or_default();
        v.sort();
        v
    };
    for org in dirs(&root).into_iter().take(MAX_ORGS) {
        for store in dirs(&root.join(&org)).into_iter().take(MAX_STORES_PER_ORG) {
            if root.join(&org).join(&store).join("clone").is_dir() {
                out.push((org.clone(), store));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------
// File health

/// `None` when the file is fine or absent-and-optional; else the fixed slug.
fn file_problem(p: &Path, required: bool) -> Option<&'static str> {
    match fs::symlink_metadata(p) {
        Ok(m) if !m.file_type().is_file() => Some("not-a-file"),
        Ok(m) if m.permissions().mode() & 0o077 != 0 => Some("loosened-permissions"),
        Ok(_) => None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => required.then_some("missing"),
        Err(_) => Some("unreadable"),
    }
}

fn error_slug(e: &delphi_xenia::Error) -> String {
    slug(e.reason())
}

fn read_note(store: &Store) -> Result<Option<SyncNote>, &'static str> {
    let p = store.paths.state.join(LAST_SYNC);
    if let Some(bad) = file_problem(&p, false) {
        return Err(bad);
    }
    let bytes = match fs::read(&p) {
        Ok(b) if b.len() <= 4096 => b,
        Ok(_) => return Err("damaged"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("unreadable"),
    };
    match serde_json::from_slice::<SyncNote>(&bytes) {
        Ok(n) if n.valid() => Ok(Some(n)),
        _ => Err("damaged"),
    }
}

/// Remember how a sync ended (called by `Store::sync_verified`, best effort; a failure to write loses only the
/// surface's memory, never a sync result). Only fixed slugs and numbers are stored: a hostile host's text, such as
/// git's stderr behind `Unavailable`, is dropped.
pub fn note_sync(store: &Store, out: &VerifiedSync, now: i64) {
    let (outcome, reason, findings) = match out {
        VerifiedSync::Accepted { .. } => ("accepted", String::new(), 0),
        VerifiedSync::UpToDate { .. } => ("up-to-date", String::new(), 0),
        VerifiedSync::Tampering { findings, .. } => ("tampering", findings.first().map(|f| slug(f.reason)).unwrap_or_default(), findings.len()),
        VerifiedSync::Rejected { reason, .. } => ("rejected", slug(reason), 0),
        VerifiedSync::NonFastForward(n) => ("non-fast-forward", match n.kind {
            crate::xenia_store::Divergence::Diverged => "diverged".to_string(),
            crate::xenia_store::Divergence::Behind => "behind".to_string(),
        }, 0),
        VerifiedSync::Refused(r) => ("refused", slug(r.reason()), 0),
        VerifiedSync::Unavailable(_) => ("unavailable", String::new(), 0),
    };
    let previous = read_note(store).ok().flatten().and_then(|n| n.last_reachable);
    let last_reachable = if outcome == "unavailable" { previous } else { Some(now) };
    let note = SyncNote { v: 1, at: now, outcome: outcome.to_string(), reason, findings, last_reachable };
    let _ = write_note(store, &note);
}

fn write_note(store: &Store, note: &SyncNote) -> Result<(), delphi_xenia::Error> {
    let dir = &store.paths.state;
    let tmp = dir.join(format!(".last-sync.{}.tmp", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(io)?;
    f.write_all(&serde_json::to_vec(note).unwrap_or_default()).and_then(|_| f.sync_all()).map_err(io)?;
    drop(f);
    fs::rename(&tmp, dir.join(LAST_SYNC)).map_err(io)
}

// ---------------------------------------------------------------------------------------------------------
// Status

fn quarantine_of(d: &Derived) -> (Capped<Quarantined>, BTreeMap<String, usize>) {
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    for (_, why) in &d.held_back {
        *by.entry(slug(why)).or_default() += 1;
    }
    let list = Capped::collect_capped(d.held_back.iter().map(|(p, why)| Quarantined { path: safe_path(p), reason: slug(why) }), LIST_CAP);
    (list, by)
}

fn tiers_of(d: &Derived) -> Tiers {
    let mut t = Tiers::default();
    for m in &d.memories {
        if m.hidden.is_some() {
            t.hidden += 1;
            continue;
        }
        match m.tier {
            Tier::L2 => t.l2 += 1,
            Tier::L1 => t.l1 += 1,
            Tier::L0 => t.l0 += 1,
        }
    }
    t
}

/// The report for one joined store. Never fails: whatever cannot be read becomes a [`Problem`].
/// `my_did` is this install's own DID (from its key file), used only to say whether it is an admin or a steward.
pub fn status_of(home: &Path, org: &str, store_name: &str, cfg: &VerifyConfig, my_did: Option<&str>) -> StoreStatus {
    let mut st = StoreStatus {
        org: shown(org, 32),
        store: shown(store_name, 32),
        problems: Vec::new(),
        accepted_head: None,
        roster_epoch: None,
        admins: None,
        members: None,
        freshness: None,
        tiers: Tiers::default(),
        contested: Capped::default(),
        quarantined: Capped::default(),
        quarantine_by_reason: BTreeMap::new(),
        last_sync: None,
        unreachable_days: None,
        rewrite_pending: None,
        me: None,
    };
    let store = match Store::open_existing(home, org, store_name) {
        Ok(s) => s,
        Err(e) => {
            st.problems.push(Problem { file: "store", slug: if matches!(e, delphi_xenia::Error::BadField(_)) { "not-joined".into() } else { error_slug(&e) } });
            return st;
        }
    };
    // File health first, from the file system, so a loosened mode is reported even when the content reads fine.
    let pins_path = store.paths.state.join("pins.toml");
    for (file, path, required) in [("sticky", &store.paths.sticky, false), ("pins", &pins_path, true)] {
        if let Some(bad) = file_problem(path, required) {
            st.problems.push(Problem { file, slug: bad.to_string() });
        }
    }
    match read_note(&store) {
        Ok(n) => st.last_sync = n,
        Err(bad) => st.problems.push(Problem { file: "last-sync", slug: bad.to_string() }),
    }
    let sticky = match store.load_sticky() {
        Ok(s) => Some(s),
        Err(e) => {
            if !st.problems.iter().any(|p| p.file == "sticky") {
                st.problems.push(Problem { file: "sticky", slug: if matches!(e, delphi_xenia::Error::Io(_)) { error_slug(&e) } else { "damaged".into() } });
            }
            None
        }
    };
    let pins = match store.load_pins() {
        Ok(p) => Some(p),
        Err(e) => {
            if !st.problems.iter().any(|p| p.file == "pins") {
                st.problems.push(Problem { file: "pins", slug: if matches!(e, delphi_xenia::Error::BadField(_)) { "damaged".into() } else { error_slug(&e) } });
            }
            None
        }
    };
    st.accepted_head = sticky.as_ref().and_then(|s| s.accepted_head.clone()).filter(|h| is_commit_id(h));

    // A rewritten head waiting for the operator: the staging ref differs from what was accepted.
    if let Ok(out) = store.git_ok(&["rev-parse", "--verify", "--quiet", &format!("{FETCHED_REF}^{{commit}}")], 256) {
        let fetched = String::from_utf8_lossy(&out).trim().to_string();
        let flagged = st.last_sync.as_ref().is_some_and(SyncNote::alarming);
        if is_commit_id(&fetched) && Some(&fetched) != st.accepted_head.as_ref() && flagged {
            st.rewrite_pending = Some(RewritePending { fetched, command: "delphi xenia sync --accept-rewritten-head" });
        }
    }
    if let (Some(n), Some(last)) = (&st.last_sync, st.last_sync.as_ref().and_then(|n| n.last_reachable)) {
        if n.outcome == "unavailable" {
            st.unreachable_days = Some(((cfg.now - last).max(0)) / 86_400);
        }
    }

    let (Some(head), Some(pins), Some(_)) = (st.accepted_head.clone(), pins, sticky) else {
        st.freshness = Some(FreshnessView::of(&FreshnessState::Stale { reason: "nothing-accepted" }));
        return st;
    };
    // Roster: epoch, admins, members, and what this key is.
    match store.roster_snapshot(&head).map_err(|e| error_slug(&e)).and_then(|snap| {
        let files = epoch_files(&snap);
        if files.is_empty() {
            return Err("no-roster".to_string());
        }
        pins.verify(&files).map_err(|e| slug(rerr(e).reason()))
    }) {
        Ok(roster) => {
            let e = &roster.head().epoch;
            st.roster_epoch = Some(roster.head_epoch());
            st.admins = Some(e.admin_people().len());
            st.members = Some(e.members.iter().filter(|m| m.is_active()).count());
            st.me = my_did.and_then(|d| roster.member(d)).filter(|m| m.is_active()).map(|m| MyStanding { admin: m.is_admin(), steward: m.steward_areas.is_steward() });
        }
        Err(slug_) => st.problems.push(Problem { file: "roster", slug: slug_ }),
    }
    match store.freshness_status(cfg) {
        Ok(f) => st.freshness = Some(FreshnessView::of(&f)),
        Err(e) => st.problems.push(Problem { file: "roster", slug: error_slug(&e) }),
    }
    match derive(&store, cfg) {
        Ok(d) => {
            st.tiers = tiers_of(&d);
            st.contested = Capped::collect_capped(d.memories.iter().filter(|m| m.hidden == Some("contested")).map(|m| shown(&m.id, 40)), LIST_CAP);
            let (q, by) = quarantine_of(&d);
            st.quarantined = q;
            st.quarantine_by_reason = by;
        }
        Err(e) => st.problems.push(Problem { file: "records", slug: error_slug(&e) }),
    }
    st
}

/// Status of every joined store plus this install's own standing. `home` is `$DELPHI_HOME`.
pub fn status_all(home: &Path, cfg: &VerifyConfig) -> Vec<StoreStatus> {
    let me = my_did(home);
    discover(home).into_iter().map(|(o, s)| status_of(home, &o, &s, cfg, me.as_deref())).collect()
}

/// This install's DID if its key file is readable. Never creates the key.
pub fn my_did(home: &Path) -> Option<String> {
    delphi_xenia::identity::Identity::load(&home.join("xenia").join("identity.key")).ok().map(|i| i.did())
}

/// The identity key file's health for doctor: `None` when absent (not an error: many installs never join).
pub fn identity_problem(home: &Path) -> Option<&'static str> {
    file_problem(&home.join("xenia").join("identity.key"), false)
}

// ---------------------------------------------------------------------------------------------------------
// The steward's attestation card

/// One flag the publish-gate scan raises on a record's text. Class and field only: never the text it matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanFlag {
    pub class: &'static str,
    pub field: String,
}

/// Exactly what a human must see before attesting a record (spec 011 §7.3). Every string was shown-sanitized.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PendingCard {
    pub org: String,
    pub store: String,
    pub id: String,
    pub rev: u64,
    pub title: String,
    pub author_handle: String,
    pub tags: Vec<String>,
    pub summary: String,
    pub scan_flags: Vec<ScanFlag>,
    /// The record is also disputed by a member.
    pub disputed: bool,
    /// It already has endorsements that cannot count because the freshness statement lapsed.
    pub held_by_staleness: bool,
    /// The terminal command that opens the approver for this record. No approval happens in the console.
    pub command: String,
}

/// Records awaiting attestation by the steward `my_did` in `store`: visible `L1` memories whose author is a
/// different person, whose tags this steward's areas cover, and that this steward has not endorsed at this revision.
/// Read-only; the answer says nothing about whether the steward *should* endorse. Empty for a non-steward.
pub fn pending_for(home: &Path, org: &str, store_name: &str, cfg: &VerifyConfig, my_did: &str) -> Capped<PendingCard> {
    let empty = Capped::default();
    let Ok(store) = Store::open_existing(home, org, store_name) else { return empty };
    let (Ok(pins), Ok(sticky)) = (store.load_pins(), store.load_sticky()) else { return empty };
    let Some(head) = sticky.accepted_head else { return empty };
    let Ok(snap) = store.roster_snapshot(&head) else { return empty };
    let files = epoch_files(&snap);
    let Ok(roster) = (if !files.is_empty() { pins.verify(&files) } else { Err(delphi_xenia::roster::RosterError::BadPins("no roster")) }) else { return empty };
    let Some(me) = roster.member(my_did).filter(|m| m.is_active() && m.steward_areas.is_steward()) else { return empty };
    let my_person = me.person.clone();
    let areas = me.steward_areas.clone();
    let Ok(d) = derive(&store, cfg) else { return empty };
    let ctx = ScanContext::default();
    let cards = d.memories.iter().filter(|m| m.hidden.is_none() && m.tier == Tier::L1).filter_map(|m| {
        if m.author_person == my_person || m.endorsed_by.iter().any(|e| *e == one_line(&my_person)) || !areas.covers(&m.tags) {
            return None;
        }
        let mut flags: Vec<ScanFlag> = Vec::new();
        let mut scan = |field: &str, text: &str| {
            for f in scan_text(field, text, &ctx).flags {
                if !flags.iter().any(|x| x.class == f.class && x.field == field) {
                    flags.push(ScanFlag { class: f.class, field: field.to_string() });
                }
            }
        };
        scan("title", &m.title);
        scan("summary", &m.summary);
        scan("body", &m.body);
        for p in &m.pointers {
            scan("pointer", &p.locator);
        }
        let handle = roster.member(&m.signer).map(|a| shown(&a.handle, 40)).unwrap_or_else(|| "(unknown)".to_string());
        Some(PendingCard {
            org: shown(org, 32),
            store: shown(store_name, 32),
            id: shown(&m.id, 40),
            rev: m.rev,
            title: shown(&m.title, TITLE_CAP),
            author_handle: handle,
            tags: m.tags.iter().take(TAGS_CAP).map(|t| shown(t, TAG_CAP)).collect(),
            summary: shown(&m.summary, SUMMARY_CAP),
            scan_flags: flags,
            disputed: m.disputed,
            held_by_staleness: !m.withheld_by_staleness.is_empty(),
            command: format!("delphi xenia endorse {} --org {} --store {}", shown(&m.id, 40), org, store_name),
        })
    });
    Capped::collect_capped(cards, CARD_CAP)
}

// ---------------------------------------------------------------------------------------------------------
// Things the operator has to do (for `delphi needs`)

/// One thing waiting on the operator. `what` is a fixed sentence built from slugs and counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    pub what: String,
}

/// Read-only detection over reports already built.
pub fn todos(all: &[StoreStatus], pending: &[usize]) -> Vec<Todo> {
    let mut out = Vec::new();
    for (i, s) in all.iter().enumerate() {
        let name = format!("{}/{}", s.org, s.store);
        let admin = s.me.as_ref().is_some_and(|m| m.admin);
        if let (true, Some(f)) = (admin, &s.freshness) {
            if f.warns() {
                out.push(Todo { what: if f.state == "stale" { format!("{name}: the freshness statement is stale ({}); an admin must run `delphi xenia admin refresh`", f.reason) } else { format!("{name}: the freshness statement has used {}% of its life; run `delphi xenia admin refresh`", f.life_used_pct) } });
            }
        }
        if let Some(r) = &s.rewrite_pending {
            out.push(Todo { what: format!("{name}: a rewritten head ({}) waits for your approval; run `{}`", &r.fetched[..12.min(r.fetched.len())], r.command) });
        }
        let n = pending.get(i).copied().unwrap_or(0);
        if n > 0 && s.me.as_ref().is_some_and(|m| m.steward) {
            out.push(Todo { what: format!("{name}: {n} record(s) await your endorsement") });
        }
    }
    out
}

#[cfg(test)]
#[path = "xenia_status_tests.rs"]
mod tests;
