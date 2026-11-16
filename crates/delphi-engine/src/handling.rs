//! Handling traits (`specs/121`, slice S1): how a piece of data is to be handled.
//!
//! An item carries a **set** of three traits: `personal` (shown to the operator as *private*), `internal` and `confidential`. The
//! code name `personal` differs from the display name on purpose, so that no existing check on the word `private` reads this trait
//! (`specs/121` §2 item 2). The set is the daemon's:
//!
//! * It is kept in a record under a path no agent can read or write: `<workspace>/local/handling/` for the workspace's items and
//!   `<home>/library/handling/` for the library's sources and claims ([`Handling`]). An item's frontmatter (or, for a JSON record, its
//!   `handling` field) is a **view** of the record and is never read back as authority.
//! * It is assigned by the arrival path the operator configured (`[handling]`, [`assign`]), by a marking in the content **only on a
//!   configured path** ([`marking`], [`Handling::arrive`]), by the operator ([`Handling::operator_set`]), by an agent or a host
//!   **adding** a trait ([`Handling::add`]), and by derivation from the items a record used ([`Handling::derive`]).
//! * Nothing here removes a trait except [`Handling::operator_clear`] and the reconciliation that follows a clear
//!   (`handling_reconcile`). A test enumerates the callers of the clear.
//!
//! **What this file must never do** (`specs/121` §2 item 4, §13): decide a `source_type`, count toward promotion, grant or remove a
//! tool, or set, read or clear the private-read mark of `specs/103`. It imports none of those modules, and a test in this file
//! (`no_trust_decision_reads_handling`) fails when a file that decides one of them names this module.
//!
//! Slice S1 changes nothing an agent reads: every item is as readable as it was. Output labelling, the placeholder and the ask are
//! S2; the confidential reader grant is S3; exits are S4.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use delphi_adapter::config::HandlingConfig;
use serde_json::{json, Value};

/// One handling trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Trait {
    Personal,
    Internal,
    Confidential,
}

/// A set of traits. Empty: the item is unlabelled.
pub type Set = BTreeSet<Trait>;

impl Trait {
    pub const ALL: [Trait; 3] = [Trait::Personal, Trait::Internal, Trait::Confidential];

    /// The name in code, files and config.
    pub fn as_str(self) -> &'static str {
        match self {
            Trait::Personal => "personal",
            Trait::Internal => "internal",
            Trait::Confidential => "confidential",
        }
    }

    /// The operator's word (`specs/121` §2 item 2): `personal` is shown as private.
    pub fn shown(self) -> &'static str {
        match self {
            Trait::Personal => "private",
            Trait::Internal => "internal",
            Trait::Confidential => "confidential",
        }
    }

    /// A trait by its code name. The display word `private` is accepted too, for the operator's commands; it is never a library
    /// flag here.
    pub fn parse(s: &str) -> Option<Trait> {
        match s.trim().to_ascii_lowercase().as_str() {
            "personal" | "private" => Some(Trait::Personal),
            "internal" => Some(Trait::Internal),
            "confidential" => Some(Trait::Confidential),
            _ => None,
        }
    }
}

/// The code names of a set, in a fixed order.
pub fn names(set: &Set) -> Vec<&'static str> {
    set.iter().map(|t| t.as_str()).collect()
}

/// The set as the operator reads it: `private, internal`. Empty for an unlabelled item.
pub fn shown(set: &Set) -> String {
    set.iter().map(|t| t.shown()).collect::<Vec<_>>().join(", ")
}

/// `confidential` implies `internal` (`specs/121` §2 item 3).
fn closed(mut set: Set) -> Set {
    if set.contains(&Trait::Confidential) {
        set.insert(Trait::Internal);
    }
    set
}

/// Who set a trait (`specs/121` §3 item 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum By {
    /// The arrival path. `configured`: a `[[handling.path]]` row of the operator's matched; false, a built-in default did.
    Path { configured: bool },
    /// A marking in the content, on a configured path.
    Marking,
    Operator,
    /// An agent added it to an item it wrote or reported.
    Agent(String),
    /// A host declared it (`record_research`).
    Host,
    /// Carried from the items a record used (`specs/121` §4).
    Derived,
}

impl By {
    pub fn as_string(&self) -> String {
        match self {
            By::Path { .. } => "path".into(),
            By::Marking => "marking".into(),
            By::Operator => "operator".into(),
            By::Agent(role) => format!("agent:{role}"),
            By::Host => "host".into(),
            By::Derived => "derived".into(),
        }
    }

    /// Does the operator's clear of a trait hold against a later assignment of this kind? It holds against everything the daemon
    /// computes again on its own (a path rule, a marking, a derivation); a later act of the operator, an agent or a host is new.
    fn held_back_by_a_clear(&self) -> bool {
        matches!(self, By::Path { .. } | By::Marking | By::Derived)
    }
}

/// An item's id: a workspace-relative path (`local/summaries/x.md`, `local/missions/<m>/findings/<f>.md`), or, for the library,
/// `library/sources/<id>` and `library/claims/<id>`. An identifier, never text.
pub fn item_ok(item: &str) -> bool {
    (1..=300).contains(&item.len()) && !item.starts_with('/') && !item.split('/').any(|p| p.is_empty() || p == "." || p == "..") && item.chars().all(|c| !c.is_control() && c != '\\')
}

/// The id of a library source.
pub fn source_item(id: &str) -> String {
    format!("library/sources/{id}")
}

/// The id of a library claim.
pub fn claim_item(id: &str) -> String {
    format!("library/claims/{id}")
}

/// The item a path of the knowledge index names (`Hit::rel_path` with its zone), or a pointer an agent was handed. A library file
/// (`library/sources/<id>/source.md`, `library/claims/<id>.json`, `@library/claim/<id>`) is its source or claim; a file of the
/// operator's zone is `local/<path>`; a team file is `team/<path>`. Anything else names no item this slice labels.
pub fn item_of(zone: &str, rel_path: &str) -> Option<String> {
    let p = rel_path.trim_start_matches("./");
    let p = p.split('#').next().unwrap_or(p);
    for (prefix, plural) in [("@library/claim/", "claims"), ("@library/source/", "sources"), ("library/claims/", "claims"), ("library/sources/", "sources"), ("research/claims/", "claims")] {
        if let Some(rest) = p.strip_prefix(prefix) {
            let id = rest.split('/').next().unwrap_or("").trim_end_matches(".json").trim_end_matches(".md");
            return (!id.is_empty()).then(|| format!("library/{plural}/{id}")).filter(|i| item_ok(i));
        }
    }
    let item = match zone {
        // a finding is indexed as `missions/<m>/findings/<f>.md` in its own zone
        "local" | "findings" => normal(&format!("local/{p}")),
        "team" => format!("team/{p}"),
        // the home layer's provisional notes (`local/memory/provisional/<role>/<note>.md`)
        "provisional" if p.starts_with("provisional/") => format!("local/memory/{p}"),
        _ => return None,
    };
    item_ok(&item).then_some(item)
}

/// One id for a mission's file whichever of the two session folders holds it (`delphi_core::sessions`): `local/sessions/...`.
fn normal(item: &str) -> String {
    match item.strip_prefix("local/missions/") {
        Some(rest) => format!("local/sessions/{rest}"),
        None => item.to_string(),
    }
}

/// One writer at a time within the process: the stores are small read-modify-write files.
static LOCK: Mutex<()> = Mutex::new(());

/// The record file of one store.
#[derive(Debug, Clone)]
struct Store {
    dir: PathBuf,
}

/// What a store's file holds.
enum Held {
    Items(BTreeMap<String, Value>),
    /// The file exists and cannot be read. Nothing is written over it.
    Unreadable,
}

impl Store {
    fn file(&self) -> PathBuf {
        self.dir.join("records.json")
    }

    fn read(&self) -> Held {
        match std::fs::read_to_string(self.file()) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Held::Items(BTreeMap::new()),
            Err(_) => Held::Unreadable,
            Ok(t) => match serde_json::from_str::<Value>(&t) {
                Ok(v) if v["items"].is_object() => Held::Items(v["items"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()),
                _ => Held::Unreadable,
            },
        }
    }

    fn items(&self) -> Result<BTreeMap<String, Value>, String> {
        match self.read() {
            Held::Items(m) => Ok(m),
            Held::Unreadable => Err(format!("the handling record {} cannot be read; nothing is written over it (move it aside and run `delphi handling reconcile`)", self.file().display())),
        }
    }

    fn write(&self, items: &BTreeMap<String, Value>) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700));
        }
        let tmp = self.dir.join("records.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&json!({"v": 1, "items": items})).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.file()).map_err(|e| e.to_string())
    }

    fn log(&self, event: &str, data: Value) {
        use std::io::Write;
        let line = json!({"ts": delphi_core::time::now_rfc3339(), "event": event, "data": data});
        if std::fs::create_dir_all(&self.dir).is_ok() {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("log.jsonl")) {
                let _ = writeln!(f, "{}", line);
            }
        }
    }
}

/// The log event of the operator's clear (`specs/121` §3 item 3).
pub const CLEARED_EVENT: &str = "handling.cleared";
/// The log event of the operator's set.
pub const SET_EVENT: &str = "handling.set";
/// The log event written when a view (frontmatter, a JSON field) had dropped a trait and it was put back (`specs/121` §3 item 6).
pub const RESTORED_EVENT: &str = "handling.view_restored";

/// The handling records of a workspace and of the install's library.
#[derive(Debug, Clone)]
pub struct Handling {
    ws: Option<Store>,
    lib: Option<Store>,
}

/// What [`Handling::derive`] found for a record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Derived {
    /// The record's whole set after the write.
    pub set: Set,
    /// The labelled items the span was given and the record did not use, with their traits.
    pub may_rest_on: Vec<(String, Set)>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn set_of_record(rec: &Value) -> Set {
    rec["traits"].as_array().into_iter().flatten().filter_map(|t| t["trait"].as_str().and_then(Trait::parse)).collect()
}

/// The set a kept record holds.
pub fn set_of(rec: &Value) -> Set {
    set_of_record(rec)
}

/// The content hash a record keeps for an item's text (what an operator's clear is tied to).
pub fn content_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex()[..32].to_string()
}

fn set_from_names(v: &Value) -> Set {
    v.as_array().into_iter().flatten().filter_map(|t| t.as_str().and_then(Trait::parse)).collect()
}

fn cleared_holds(rec: &Value, t: Trait, hash: Option<&str>) -> bool {
    rec["cleared"].as_array().into_iter().flatten().any(|c| c["trait"] == t.as_str() && (c["hash"].is_null() || hash.is_none() || c["hash"].as_str() == hash))
}

impl Handling {
    /// The records of the workspace at `ws_dir` and of the library under `home`.
    pub fn of(ws_dir: &Path, home: &Path) -> Handling {
        Handling { ws: Some(Store { dir: dir(ws_dir) }), lib: Some(Store { dir: library_dir(home) }) }
    }

    /// The library's records only: for code that holds a library and no workspace. A workspace item reads as unlabelled.
    pub fn library_only(home: &Path) -> Handling {
        Handling { ws: None, lib: Some(Store { dir: library_dir(home) }) }
    }

    /// Records kept in two given folders: the reconciliation's dry run works on a copy.
    pub(crate) fn at(ws_store: PathBuf, lib_store: PathBuf) -> Handling {
        Handling { ws: Some(Store { dir: ws_store }), lib: Some(Store { dir: lib_store }) }
    }

    /// The workspace's records only: for code that holds a workspace and no home. A library item reads as unlabelled.
    pub fn workspace_only(ws_dir: &Path) -> Handling {
        Handling { ws: Some(Store { dir: dir(ws_dir) }), lib: None }
    }

    /// The same, from the library's own folder (`<home>/library`).
    pub fn in_library(library_root: &Path) -> Handling {
        Handling { ws: None, lib: Some(Store { dir: library_root.join("handling") }) }
    }

    fn store(&self, item: &str) -> Option<&Store> {
        if item.starts_with("library/") { self.lib.as_ref() } else { self.ws.as_ref() }
    }

    /// The item's record as it is kept, or `None`.
    pub fn record(&self, item: &str) -> Option<Value> {
        let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        self.record_unlocked(item)
    }

    fn record_unlocked(&self, item: &str) -> Option<Value> {
        match self.store(item)?.read() {
            Held::Items(m) => m.get(item).cloned(),
            Held::Unreadable => None,
        }
    }

    /// The item's traits. An item with no record, or in a store that cannot be read, is unlabelled: nothing is withheld while it
    /// waits (`specs/121` §3 item 5).
    pub fn set(&self, item: &str) -> Set {
        self.record(item).map(|r| set_of_record(&r)).unwrap_or_default()
    }

    /// Is the item `internal` **by a configured arrival path or by the operator** (`specs/121` §7 item 1)? Only then is it
    /// authoritative for the organisation's own specifics. A marking, an agent's or a host's word, a built-in default and a
    /// derivation give the label and not the weight (§7 item 4).
    pub fn has_weight(&self, item: &str) -> bool {
        self.record(item).is_some_and(|r| r["traits"].as_array().into_iter().flatten().any(|t| t["trait"] == "internal" && (t["by"] == "operator" || (t["by"] == "path" && t["configured"] == true))))
    }

    /// Every record of both stores, by item. A store that cannot be read gives none.
    pub fn all(&self) -> BTreeMap<String, Value> {
        let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = BTreeMap::new();
        for s in [&self.ws, &self.lib].into_iter().flatten() {
            if let Held::Items(m) = s.read() {
                out.extend(m);
            }
        }
        out
    }

    /// The stores whose file exists and cannot be read.
    pub fn unreadable(&self) -> Vec<PathBuf> {
        [&self.ws, &self.lib].into_iter().flatten().filter(|s| matches!(s.read(), Held::Unreadable)).map(Store::file).collect()
    }

    fn change(&self, item: &str, f: impl FnOnce(&mut Value) -> Result<bool, String>) -> Result<bool, String> {
        if !item_ok(item) {
            return Err(format!("`{}` is not an item id", item.chars().take(80).collect::<String>()));
        }
        let store = self.store(item).ok_or("no store for this item")?;
        let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut items = store.items()?;
        let mut rec = items.get(item).cloned().unwrap_or_else(|| json!({"item": item, "traits": []}));
        if !f(&mut rec)? {
            return Ok(false);
        }
        let empty = rec["traits"].as_array().is_none_or(|a| a.is_empty()) && rec["cleared"].as_array().is_none_or(|a| a.is_empty()) && rec["may_rest_on"].as_array().is_none_or(|a| a.is_empty()) && rec["says"].as_array().is_none_or(|a| a.is_empty());
        if empty {
            items.remove(item);
        } else {
            items.insert(item.to_string(), rec);
        }
        store.write(&items)?;
        Ok(true)
    }

    /// Add `traits` to `item`, set by `by`, from the items `from` (for a derivation). **Adding only**: no call of this removes or
    /// lowers anything. `confidential` brings `internal` with it. A trait the operator cleared is not put back by a path rule, a
    /// marking or a derivation while the item's content is what it was when it was cleared. Returns the traits that are new to
    /// the item.
    pub fn add(&self, item: &str, hash: Option<&str>, traits: &Set, by: &By, from: &[String]) -> Result<Set, String> {
        let want = closed(traits.clone());
        let mut new = Set::new();
        if want.is_empty() {
            return Ok(new);
        }
        let now = delphi_core::time::now_rfc3339();
        let who = by.as_string();
        self.change(item, |rec| {
            let had = set_of_record(rec);
            let mut changed = false;
            if let Some(h) = hash {
                if rec["hash"].as_str() != Some(h) {
                    rec["hash"] = json!(h);
                    changed = true;
                }
            }
            for t in &want {
                if by.held_back_by_a_clear() && cleared_holds(rec, *t, hash) {
                    continue;
                }
                let list = rec["traits"].as_array_mut().ok_or("the record is damaged")?;
                match list.iter_mut().find(|e| e["trait"] == t.as_str() && e["by"] == who.as_str()) {
                    Some(e) => {
                        // the same trait by the same hand: only a derivation's sources, or a path's standing, can grow
                        let mut ids = strings(&e["from"]);
                        let before = ids.len();
                        for f in from {
                            if !ids.contains(f) && ids.len() < MAX_FROM {
                                ids.push(f.clone());
                            }
                        }
                        if ids.len() != before {
                            e["from"] = json!(ids);
                            changed = true;
                        }
                        if matches!(by, By::Path { configured: true }) && e["configured"] != true {
                            e["configured"] = json!(true);
                            changed = true;
                        }
                    }
                    None => {
                        let mut e = json!({"trait": t.as_str(), "by": who, "at": now});
                        if let By::Path { configured } = by {
                            e["configured"] = json!(configured);
                        }
                        if !from.is_empty() {
                            e["from"] = json!(from.iter().take(MAX_FROM).collect::<Vec<_>>());
                        }
                        list.push(e);
                        changed = true;
                    }
                }
                if !had.contains(t) {
                    new.insert(*t);
                }
            }
            Ok(changed)
        })?;
        Ok(new)
    }

    /// The operator adds a trait (`delphi handling set`). It also lifts an earlier clear of the same trait.
    pub fn operator_set(&self, item: &str, t: Trait) -> Result<Set, String> {
        self.change(item, |rec| {
            let before = rec["cleared"].as_array().map(|a| a.len()).unwrap_or(0);
            if let Some(a) = rec["cleared"].as_array_mut() {
                a.retain(|c| c["trait"] != t.as_str() && !(t == Trait::Confidential && c["trait"] == "internal"));
            }
            Ok(rec["cleared"].as_array().map(|a| a.len()).unwrap_or(0) != before)
        })?;
        let new = self.add(item, None, &Set::from([t]), &By::Operator, &[])?;
        if let Some(s) = self.store(item) {
            s.log(SET_EVENT, json!({"item": item, "trait": t.as_str(), "by": "operator"}));
        }
        Ok(new)
    }

    /// The operator removes a trait from an item (declassifies it; `specs/121` §3 item 3). **The one caller is the operator's
    /// terminal command** (`delphi handling clear`); a test enumerates the callers, and no tool, host method, relay reply or
    /// handoff reaches it. `internal` cannot be removed from a confidential item until `confidential` is (§2 item 3). The clear is
    /// remembered, so the path rule or the derivation that set the trait does not put it back, and it writes
    /// [`CLEARED_EVENT`] with the item and the trait. Returns whether the item carried the trait.
    pub fn operator_clear(&self, item: &str, t: Trait) -> Result<bool, String> {
        let now = delphi_core::time::now_rfc3339();
        let mut carried = false;
        self.change(item, |rec| {
            let had = set_of_record(rec);
            if t == Trait::Internal && had.contains(&Trait::Confidential) {
                return Err("refused: the item is confidential, and confidential implies internal. Clear `confidential` first".into());
            }
            carried = had.contains(&t);
            if let Some(list) = rec["traits"].as_array_mut() {
                list.retain(|e| e["trait"] != t.as_str());
            }
            let hash = rec["hash"].clone();
            let mut cleared: Vec<Value> = rec["cleared"].as_array().cloned().unwrap_or_default();
            cleared.retain(|c| c["trait"] != t.as_str());
            cleared.push(json!({"trait": t.as_str(), "at": now, "hash": hash}));
            rec["cleared"] = json!(cleared);
            Ok(true)
        })?;
        if let Some(s) = self.store(item) {
            s.log(CLEARED_EVENT, json!({"item": item, "trait": t.as_str(), "by": "operator", "carried": carried}));
        }
        Ok(carried)
    }

    /// Note a marking seen in the content of an item on a path the operator did not configure (`specs/121` §3 item 2): it sets
    /// nothing, is shown beside the item (`says: confidential`) and is counted for review.
    pub fn note_says(&self, item: &str, says: &Set) -> Result<(), String> {
        if says.is_empty() {
            return Ok(());
        }
        self.change(item, |rec| {
            let mut have = set_from_names(&rec["says"]);
            let before = have.len();
            have.extend(says.iter().copied());
            rec["says"] = json!(names(&have));
            Ok(have.len() != before)
        })
        .map(|_| ())
    }

    /// The markings noted beside an item (never traits).
    pub fn says(&self, item: &str) -> Set {
        self.record(item).map(|r| set_from_names(&r["says"])).unwrap_or_default()
    }

    /// An item arrived (or is being reconciled): give it the traits of its arrival path, and read the marking in its text.
    /// The marking sets a trait only when a row of the operator's table matched the path; otherwise it is noted and sets nothing.
    /// Returns the item's whole set afterwards.
    pub fn arrive(&self, cfg: &HandlingConfig, item: &str, hash: Option<&str>, arrival: &Arrival, text: Option<&str>) -> Result<Set, String> {
        self.arrive_by(cfg, item, hash, std::slice::from_ref(arrival), text)
    }

    /// The same for an item that arrived by several paths at once (an imported page and the sources its bibliography lists): the
    /// union of what each path gives; the marking counts when any of them is a configured path.
    pub fn arrive_by(&self, cfg: &HandlingConfig, item: &str, hash: Option<&str>, arrivals: &[Arrival], text: Option<&str>) -> Result<Set, String> {
        let mut configured = false;
        for arrival in arrivals {
            let assigned = assign(cfg, arrival);
            configured |= assigned.configured;
            self.add(item, hash, &assigned.traits, &By::Path { configured: assigned.configured }, &[])?;
        }
        let marked = text.map(marking).unwrap_or_default();
        if !marked.is_empty() {
            if configured {
                self.add(item, hash, &marked, &By::Marking, &[])?;
            } else {
                self.note_says(item, &marked)?;
            }
        }
        if let Some(kind) = arrivals.first().map(|a| a.kind.clone()).filter(|k| !k.is_empty()) {
            self.change(item, |rec| {
                if rec["traits"].as_array().is_none_or(|a| a.is_empty()) && rec["says"].as_array().is_none_or(|a| a.is_empty()) || rec["kind"] == kind.as_str() {
                    return Ok(false);
                }
                rec["kind"] = json!(kind);
                Ok(true)
            })?;
        }
        Ok(self.set(item))
    }

    /// Propagation at a write (`specs/121` §4 items 1, 2 and 6). `item` was just written; `used` are the items it is anchored to,
    /// quotes, cites or lists as inputs; `given` are the labelled items its span was handed. The record takes the union of the
    /// traits of what it used, as `derived` with the ids they came from, and a `may_rest_on` note for what the span was given and
    /// did not use. The note changes no behaviour. Nothing a caller passes lowers the set: an item that is not labelled adds
    /// nothing, and nothing is removed here.
    pub fn derive(&self, item: &str, hash: Option<&str>, used: &[String], given: &[String]) -> Result<Derived, String> {
        let mut from: BTreeMap<Trait, Vec<String>> = BTreeMap::new();
        for u in used.iter().filter(|u| u.as_str() != item) {
            for t in self.set(u) {
                let ids = from.entry(t).or_default();
                if !ids.contains(u) {
                    ids.push(u.clone());
                }
            }
        }
        for (t, ids) in &from {
            self.add(item, hash, &Set::from([*t]), &By::Derived, ids)?;
        }
        let mut rest: Vec<(String, Set)> = Vec::new();
        for g in given.iter().filter(|g| g.as_str() != item && !used.contains(g)) {
            let set = self.set(g);
            if !set.is_empty() && !rest.iter().any(|(i, _)| i == g) && rest.len() < MAX_FROM {
                rest.push((g.clone(), set));
            }
        }
        if !rest.is_empty() {
            self.change(item, |rec| {
                let mut list: Vec<Value> = rec["may_rest_on"].as_array().cloned().unwrap_or_default();
                let before = list.len();
                for (g, set) in &rest {
                    if !list.iter().any(|e| e["item"] == g.as_str()) && list.len() < MAX_FROM {
                        list.push(json!({"item": g, "traits": names(set)}));
                    }
                }
                rec["may_rest_on"] = json!(list);
                if let Some(h) = hash {
                    rec["hash"] = json!(h);
                }
                Ok(list.len() != before)
            })?;
        }
        let rec = self.record(item);
        Ok(Derived { set: rec.as_ref().map(set_of_record).unwrap_or_default(), may_rest_on: rec.map(|r| r["may_rest_on"].as_array().into_iter().flatten().filter_map(|e| Some((e["item"].as_str()?.to_string(), set_from_names(&e["traits"])))).collect()).unwrap_or_default() })
    }

    /// `specs/121` §4 item 4, for the reconciliation only: a trait an item carries **only** as `derived` is dropped when none of
    /// the items it came from carries it any more (the operator cleared it there). A trait also set by a path, a marking, the
    /// operator, an agent or a host stays. Returns the traits dropped.
    pub(crate) fn settle_derived(&self, item: &str) -> Result<Set, String> {
        let Some(rec) = self.record(item) else { return Ok(Set::new()) };
        let mut gone = Set::new();
        let mut keep: Vec<Value> = Vec::new();
        for e in rec["traits"].as_array().into_iter().flatten() {
            let Some(t) = e["trait"].as_str().and_then(Trait::parse) else { continue };
            if e["by"] != "derived" {
                keep.push(e.clone());
                continue;
            }
            let live: Vec<String> = strings(&e["from"]).into_iter().filter(|f| self.set(f).contains(&t)).collect();
            if live.is_empty() {
                gone.insert(t);
            } else {
                let mut e = e.clone();
                e["from"] = json!(live);
                keep.push(e);
            }
        }
        let still: Set = keep.iter().filter_map(|e| e["trait"].as_str().and_then(Trait::parse)).collect();
        gone.retain(|t| !still.contains(t));
        // `confidential` implies `internal`: a derived internal that came with a confidential still carried stays.
        if still.contains(&Trait::Confidential) && !still.contains(&Trait::Internal) {
            if let Some(e) = rec["traits"].as_array().into_iter().flatten().find(|e| e["trait"] == "internal") {
                keep.push(e.clone());
                gone.remove(&Trait::Internal);
            }
        }
        if keep.len() != rec["traits"].as_array().map(|a| a.len()).unwrap_or(0) || keep.iter().zip(rec["traits"].as_array().into_iter().flatten()).any(|(a, b)| a != b) {
            self.change(item, |r| {
                r["traits"] = json!(keep);
                Ok(true)
            })?;
        }
        Ok(gone)
    }

    /// Write one line of a store's log (the reconciliation's and the views' events).
    pub(crate) fn log(&self, item: &str, event: &str, data: Value) {
        if let Some(s) = self.store(item) {
            s.log(event, data);
        }
    }
}

/// The most source ids one derived trait, and the most items one `may_rest_on` note, names.
pub const MAX_FROM: usize = 50;

/// Where the workspace's records live. `local/` is a path no role may read or write (`pathguard::never_readable`).
pub fn dir(ws_dir: &Path) -> PathBuf {
    ws_dir.join("local/handling")
}

/// The `[handling]` table of the workspace's `config.toml`. A file that is missing, does not parse or fails the check configures
/// nothing ([`HandlingConfig::read`]).
pub fn config_of(ws_dir: &Path) -> HandlingConfig {
    HandlingConfig::read(&std::fs::read_to_string(ws_dir.join("config.toml")).unwrap_or_default())
}

/// The item id of a file inside the workspace: its path relative to the workspace, `/`-separated. `None` outside it.
pub fn item_in_ws(ws_dir: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(ws_dir).ok()?;
    let item = normal(&rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/"));
    item_ok(&item).then_some(item)
}

/// Where the library's records live: inside the library, which no workspace zone reaches.
pub fn library_dir(home: &Path) -> PathBuf {
    home.join("library/handling")
}

// ---- assignment by arrival path (`specs/121` §3 item 1) -------------------------------------------------------------------

/// What is known of how an item arrived. Everything but the kind is what the connector or the import said; a field it did not
/// say is `None`, and the rules then label nothing they cannot tell (`specs/121` §9 item 5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Arrival {
    /// The connector or import kind: `email`, `dm`, `slack`, `wiki`, `private-doc`, `meeting`, `file`, `web`, ...
    pub kind: String,
    /// The folder, channel, space, address or URL the item came from, compared with a row's `under`.
    pub locator: String,
    /// A message: was it one to one?
    pub direct: Option<bool>,
    /// A message's sender and recipients, or a meeting's participants, as addresses, where known.
    pub addresses: Vec<String>,
    /// An email's recipients, where known.
    pub recipients: Option<Vec<String>>,
    /// A meeting's number of participants, where known.
    pub participants: Option<usize>,
    /// A message in the organisation's own workspace.
    pub org_workspace: Option<bool>,
}

impl Arrival {
    pub fn of(kind: &str, locator: &str) -> Arrival {
        Arrival { kind: kind.into(), locator: locator.into(), ..Default::default() }
    }
}

/// The traits a path gives, and whether a row of the operator's table gave them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Assigned {
    pub traits: Set,
    pub configured: bool,
}

fn domain_of(address: &str) -> Option<String> {
    let a = address.trim().trim_matches(|c| c == '<' || c == '>');
    a.rsplit_once('@').map(|(_, d)| d.to_ascii_lowercase()).filter(|d| !d.is_empty())
}

fn all_on_org(cfg: &HandlingConfig, addresses: &[String]) -> bool {
    !addresses.is_empty() && !cfg.org_domains.is_empty() && addresses.iter().all(|a| domain_of(a).is_some_and(|d| cfg.org_domains.iter().any(|o| d == o.to_ascii_lowercase())))
}

/// The traits of an arrival path. The most specific matching row of the operator's `[[handling.path]]` decides, and makes the path
/// configured; with none, the defaults of `specs/121` §3 item 1 apply. An item no rule knows is unlabelled: readable, and counted.
pub fn assign(cfg: &HandlingConfig, a: &Arrival) -> Assigned {
    let locator = a.locator.to_lowercase();
    let row = cfg.path.iter().filter(|p| p.kind == a.kind && p.under.as_deref().is_none_or(|u| locator.starts_with(&u.trim().to_lowercase()))).max_by_key(|p| p.under.as_deref().map(|u| u.trim().len()).unwrap_or(0));
    if let Some(row) = row {
        return Assigned { traits: closed(row.traits.iter().filter_map(|t| Trait::parse(t)).collect()), configured: true };
    }
    let mut set = Set::new();
    let direct_message = |set: &mut Set| {
        set.insert(Trait::Personal);
        if a.org_workspace == Some(true) || all_on_org(cfg, &a.addresses) {
            set.insert(Trait::Internal);
        }
    };
    match a.kind.as_str() {
        "email" => {
            let to = a.recipients.as_deref().unwrap_or(&[]);
            let to_a_list = to.iter().any(|r| cfg.list_addresses.iter().any(|l| l.eq_ignore_ascii_case(r.trim().trim_matches(|c| c == '<' || c == '>'))));
            if to_a_list || to.len() > cfg.company_wide_recipients {
                set.insert(Trait::Internal);
            } else {
                set.insert(Trait::Personal);
                let mut everyone = a.addresses.clone();
                everyone.extend(to.iter().cloned());
                if all_on_org(cfg, &everyone) {
                    set.insert(Trait::Internal);
                }
            }
        }
        "dm" => direct_message(&mut set),
        "slack" if a.direct == Some(true) => direct_message(&mut set),
        "slack" | "wiki" | "private-doc" => {
            set.insert(Trait::Internal);
        }
        "meeting" => match a.participants.or((!a.addresses.is_empty()).then_some(a.addresses.len())) {
            Some(1 | 2) => {
                set.insert(Trait::Personal);
            }
            Some(n) if n >= 3 && a.addresses.len() == n && all_on_org(cfg, &a.addresses) => {
                set.insert(Trait::Internal);
            }
            _ => {}
        },
        _ => {}
    }
    Assigned { traits: set, configured: false }
}

/// What a host's declared source kind adds (`specs/121` §11, narrowing 113 §2 item 4): `internal` adds `internal` and `person`
/// adds `personal`, set by `host`, with no weight. `private: true` is the library's flag and is not a trait.
pub fn host_kind(kind: &str) -> Set {
    match kind {
        "internal" => Set::from([Trait::Internal]),
        "person" => Set::from([Trait::Personal]),
        _ => Set::new(),
    }
}

// ---- markings in the content (`specs/121` §3 item 2) ------------------------------------------------------------------------

const HEAD_LINES: usize = 12;
const FOOT_LINES: usize = 6;
const MARK_LINE_MAX: usize = 90;
const MARK_KEYS: [&str; 5] = ["classification", "sensitivity", "confidentiality", "distribution", "marking"];

fn mark_of_line(line: &str) -> Set {
    let l: String = line.trim().trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    let mut set = Set::new();
    if l.is_empty() || l.chars().count() > MARK_LINE_MAX || l.contains("not confidential") || l.contains("non-confidential") || l.contains("not internal") || l.contains("unclassified") {
        return set;
    }
    let words: Vec<&str> = l.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    if words.contains(&"confidential") {
        set.insert(Trait::Confidential);
    }
    let internal_only = words.windows(2).any(|w| w == ["internal", "only"]) || words.windows(3).any(|w| w == ["internal", "use", "only"] || w == ["for", "internal", "use"]) || words == ["internal"];
    if internal_only {
        set.insert(Trait::Internal);
    }
    set
}

/// The marking a document carries in its header, footer, frontmatter or classification line: words such as "internal only" or
/// "confidential". This reads text and returns words; whether they set anything is [`Handling::arrive`]'s decision, by the path.
/// The frontmatter key `handling` is the daemon's own view and is not a marking.
pub fn marking(text: &str) -> Set {
    let mut set = Set::new();
    let mut body = text;
    if let Some(rest) = text.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n") {
            for line in rest[..end].lines() {
                if let Some((k, v)) = line.split_once(':') {
                    if MARK_KEYS.contains(&k.trim().to_ascii_lowercase().as_str()) {
                        set.extend(mark_of_line(v));
                        if v.trim().trim_matches(|c| c == '"' || c == '\'').eq_ignore_ascii_case("internal") {
                            set.insert(Trait::Internal);
                        }
                    }
                }
            }
            body = &rest[end + 5..];
        }
    }
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    for l in lines.iter().take(HEAD_LINES).chain(lines.iter().rev().take(FOOT_LINES.min(lines.len().saturating_sub(HEAD_LINES)))) {
        // a marking is a line of its own, or one that starts with the word: a sentence that mentions confidentiality is not one
        let t = l.trim().trim_start_matches(|c: char| !c.is_alphanumeric());
        let lower = t.to_lowercase();
        let short = t.split_whitespace().count() <= 8;
        if short || lower.starts_with("confidential") || lower.starts_with("internal only") || lower.starts_with("internal use only") || lower.starts_with("classification") {
            set.extend(mark_of_line(t));
        }
    }
    closed(set)
}

// ---- the view in an item's own file (`specs/121` §3 item 6) -----------------------------------------------------------------

/// The frontmatter key that shows an item's set.
pub const VIEW_KEY: &str = "handling";
/// The frontmatter key that shows the `may_rest_on` note, as item ids.
pub const REST_KEY: &str = "handling_may_rest_on";

/// What a Markdown file's frontmatter says its set is, or `None` when it says nothing. A view only: never authority.
pub fn view_of(text: &str) -> Option<Set> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    rest[..end].lines().find_map(|l| l.strip_prefix("handling:")).map(|v| v.trim().trim_start_matches('[').trim_end_matches(']').split(',').filter_map(Trait::parse).collect())
}

/// `text` with its frontmatter showing `set` (and `rest`, the `may_rest_on` ids): the lines are replaced, added before the closing
/// `---`, or removed when there is nothing to show. A file with no frontmatter is returned as it is.
pub fn with_view(text: &str, set: &Set, rest: &[String]) -> String {
    let Some(after) = text.strip_prefix("---\n") else { return text.to_string() };
    let Some(end) = after.find("\n---\n") else { return text.to_string() };
    let mut lines: Vec<String> = after[..end].lines().filter(|l| !l.starts_with("handling:") && !l.starts_with("handling_may_rest_on:")).map(String::from).collect();
    if !set.is_empty() {
        lines.push(format!("{VIEW_KEY}: [{}]", names(set).join(", ")));
    }
    if !rest.is_empty() {
        lines.push(format!("{REST_KEY}: [{}]", rest.iter().map(|r| format!("{:?}", r)).collect::<Vec<_>>().join(", ")));
    }
    format!("---\n{}\n---\n{}", lines.join("\n"), &after[end + 5..])
}

/// `text` (a Markdown file of the workspace at `path`) with its frontmatter showing the item's set as the record keeps it. A view
/// only: the record decides, and a view an editor changed is put back here. Text outside the workspace is returned as it is.
pub fn view_for(ws_dir: &Path, path: &Path, text: &str) -> String {
    let Some(item) = item_in_ws(ws_dir, path) else { return text.to_string() };
    let Some(rec) = Handling::workspace_only(ws_dir).record(&item) else { return with_view(text, &Set::new(), &[]) };
    let rest: Vec<String> = rec["may_rest_on"].as_array().into_iter().flatten().filter_map(|e| e["item"].as_str().map(String::from)).collect();
    with_view(text, &set_of_record(&rec), &rest)
}

/// The view for a JSON record (a claim): `{"traits": [...], "may_rest_on": [...]}`, or null when there is nothing to show.
pub fn json_view(d: &Derived) -> Value {
    if d.set.is_empty() && d.may_rest_on.is_empty() {
        return Value::Null;
    }
    json!({"traits": names(&d.set), "may_rest_on": d.may_rest_on.iter().map(|(i, _)| i).collect::<Vec<_>>()})
}

// ---- what a span was given (`specs/121` §4 item 2) --------------------------------------------------------------------------

fn given_file(ws_dir: &Path, key: &str) -> Option<PathBuf> {
    let ok = (1..=160).contains(&key.len()) && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '/' | '.')) && !key.contains("..") && key.split('/').all(|p| !p.is_empty());
    ok.then(|| dir(ws_dir).join("given").join(format!("{}.json", key.replace(['/', ':'], "__"))))
}

/// The key a span's given list is kept under: the mission and the span (a host session: its own id and `host`).
pub fn span_key(mission: &str, span: &str) -> String {
    format!("{mission}/{span}")
}

/// The most items one span's given list holds.
pub const MAX_GIVEN: usize = 500;

/// Record that the span `key` was handed `items` by a reader. Only the items that carry a trait are kept, by id: this is the list
/// a later write's `may_rest_on` note is computed from. It changes nothing about what the reader returns, sets no mark, and is not
/// read by any gate. Returns how many labelled items were new to the span.
pub fn note_given(h: &Handling, ws_dir: &Path, key: &str, items: &[String]) -> usize {
    let labelled: Vec<&String> = items.iter().filter(|i| !h.set(i).is_empty()).collect();
    let Some(path) = given_file(ws_dir, key).filter(|_| !labelled.is_empty()) else { return 0 };
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut have: Vec<String> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|v| strings(&v["items"])).unwrap_or_default();
    let before = have.len();
    for i in labelled {
        if !have.contains(i) && have.len() < MAX_GIVEN {
            have.push(i.clone());
        }
    }
    if have.len() == before {
        return 0;
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&path, json!({"key": key, "items": have}).to_string());
    have.len() - before
}

/// The labelled items the span `key` was handed so far.
pub fn given(ws_dir: &Path, key: &str) -> Vec<String> {
    given_file(ws_dir, key).and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|v| strings(&v["items"])).unwrap_or_default()
}

/// The items a text names (`specs/121` §4 item 1: "the items its citation markers name"): every labelled item whose id, or whose
/// library pointer (`@library/claim/<id>`, `@library/source/<id>`), the text carries. Only labelled items are looked for, so the
/// answer is the part of the used set that matters to a derivation.
pub fn named_in(h: &Handling, text: &str) -> Vec<String> {
    // The view's own lines name the `may_rest_on` items: they are not a use.
    let text = with_view(text, &Set::new(), &[]);
    let text = text.as_str();
    h.all()
        .into_iter()
        .filter(|(_, rec)| !set_of_record(rec).is_empty())
        .map(|(item, _)| item)
        .filter(|item| {
            let pointer = item.strip_prefix("library/claims/").map(|id| format!("@library/claim/{id}")).or_else(|| item.strip_prefix("library/sources/").map(|id| format!("@library/source/{id}")));
            let short = item.strip_prefix("local/").unwrap_or(item);
            text.contains(item.as_str()) || pointer.is_some_and(|p| text.contains(&p)) || (short.len() >= 12 && short.contains('/') && text.contains(short))
        })
        .collect()
}

/// A span wrote the record `item`: the whole of `specs/121` §4 items 1 and 2 for one write. `used` is what the record names; the
/// span's given list supplies the note. A failure to record is returned and never fails the write that called it.
pub fn written(h: &Handling, ws_dir: &Path, key: Option<&str>, item: &str, hash: Option<&str>, used: &[String]) -> Result<Derived, String> {
    let given = key.map(|k| given(ws_dir, k)).unwrap_or_default();
    h.derive(item, hash, used, &given)
}

/// A derived record was written as the Markdown file `file` of the workspace, by the span whose given list is kept under `key`:
/// derive its traits from what its text names and from `also_used` (ids the writer knows it used, such as a finding's source
/// evidence), note what it may rest on, and return the text with its view when the view changed (the caller writes it). With a
/// `home`, library items count too. A failure is logged and labels nothing; it never fails the write.
pub fn label_file(ws_dir: &Path, home: Option<&Path>, file: &Path, text: &str, key: Option<&str>, also_used: &[String]) -> Option<String> {
    let item = item_in_ws(ws_dir, file)?;
    let h = match home {
        Some(home) => Handling::of(ws_dir, home),
        None => Handling::workspace_only(ws_dir),
    };
    let mut used = named_in(&h, text);
    for u in also_used {
        if !used.contains(u) {
            used.push(u.clone());
        }
    }
    match written(&h, ws_dir, key, &item, None, &used) {
        Ok(d) => {
            let rest: Vec<String> = d.may_rest_on.iter().map(|(i, _)| i.clone()).collect();
            let shown = with_view(text, &d.set, &rest);
            (shown != text).then_some(shown)
        }
        Err(e) => {
            delphi_core::elog::warn("handling", "record_not_labelled", &e, json!({"item": item}));
            None
        }
    }
}

// ---- the write paths (`specs/121` §14: "one test that enumerates the write paths") -----------------------------------------

/// What a write path does about propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Propagation {
    /// The file calls into this module at the write: the record takes the traits of what it used.
    Derives,
    /// The path writes a derived record and does not propagate yet. Said plainly, with what keeps the exposure small today.
    NotYet { why: &'static str },
    /// The file names a writer and writes no record of its own that carries substance from an item.
    NoRecord { why: &'static str },
}

/// One file that writes a record made from other items. The file is relative to this crate's `src`.
pub struct WritePath {
    pub file: &'static str,
    pub what: &'static str,
    pub how: Propagation,
}

/// The calls that write a derived record (a claim, a finding, a note, a decision's brief). A line of this crate before its tests
/// that carries one of these puts its file in [`WRITES`], or the inventory test (`every_write_path_is_listed_with_what_it_does`)
/// fails: a new write path cannot ship without saying what it does about propagation.
pub const WRITERS: [&str; 7] = ["save_claim(", "finding_record::store(", "fr::store(", "fn record_finding(", "pub fn keep(", "write_merged(", "write_brief("];

/// Every file that writes a derived record, and what it does about propagation.
pub const WRITES: [WritePath; 11] = [
    WritePath { file: "library_claims.rs", what: "`save_claim`: every write of a library claim (added by the operator, a research run, a host's `record_research`, an anchor, a review)", how: Propagation::Derives },
    WritePath { file: "library_extract.rs", what: "saves the claims an accepted extraction made, through `save_claim`", how: Propagation::NoRecord { why: "it calls `save_claim`, which derives" } },
    WritePath { file: "library_private.rs", what: "marks a claim's anchors `text_removed`, through `save_claim`", how: Propagation::NoRecord { why: "it calls `save_claim`, which derives" } },
    WritePath { file: "library_review.rs", what: "records a review on a claim, through `save_claim`", how: Propagation::NoRecord { why: "it calls `save_claim`, which derives" } },
    WritePath { file: "library_pages.rs", what: "writes a page's claims back, through `save_claim`", how: Propagation::NoRecord { why: "it calls `save_claim`, which derives; the page itself is work output and is labelled by slice S2" } },
    WritePath { file: "finding_record.rs", what: "`record_finding`: an investigator's or a scout's finding (`label_stored`)", how: Propagation::Derives },
    WritePath { file: "service.rs", what: "a host's `record_finding`, stored through `finding_record::store` and labelled by `label_stored`", how: Propagation::Derives },
    WritePath { file: "provisional_notes.rs", what: "`keep` (an agent's or a host's note, a kept session summary) and `write_merged` (a merge the operator accepted)", how: Propagation::Derives },
    WritePath { file: "consolidate.rs", what: "an accepted merge, through `provisional_notes::write_merged`", how: Propagation::NoRecord { why: "it calls `write_merged`, which derives from the parts" } },
    WritePath { file: "decision_brief.rs", what: "`write_brief`: the brief of a decision", how: Propagation::NotYet { why: "a brief is written by a tool-less call from the decision's own payload and is shown to the operator only; no agent retrieves it" } },
    WritePath { file: "decision_run.rs", what: "asks for and stores a decision's brief (`write_brief`)", how: Propagation::NotYet { why: "as `decision_brief.rs`" } },
];

/// What `specs/121` §4 item 1 names and slice S1 does not label, said here so the gap is read, not found. Each is an event in the
/// mission log or a span's reply, not a file a search returns; S2 labels work output.
pub const NOT_LABELLED: [(&str, &str); 4] = [
    ("handoff", "a handoff is an envelope in the mission log; it is not retrieved as an item. The span it starts notes what its retrieval hands it, so what that span writes carries the note"),
    ("session summary", "a session's summary is an event; the one kept as a provisional note is labelled by `keep`. A host writing its own file is advised, not enforced (`specs/121` §9 item 1)"),
    ("Council evidence", "a Council's evidence is the seats' replies in the mission log"),
    ("advice", "a strategist's advice is a span's reply in the mission log"),
];

#[cfg(test)]
#[path = "handling_tests.rs"]
mod tests;
