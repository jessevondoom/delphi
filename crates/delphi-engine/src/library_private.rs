//! Text kept privately for retrieval only (`specs/119` §3 items 2, 4 and 5; slice B2).
//!
//! Under `[library] unknown_licence = "private_text"` a page no licence lets the library keep whole is kept whole as a **private**
//! source, marked `kept_privately`. Being private, it is refused by every channel that leaves the install (`egress::check`: an export,
//! a bundle, the federation, a promoted file), never indexed, never placed on an external page, and it counts as no origin. What this
//! file adds:
//!
//! * [`shares_run`]: whether a text carries a run of more than [`RUN_MAX`] characters of a private source's text. The page check uses
//!   it, so a page in the internal scope may quote a privately kept source only in passages of at most 300 characters.
//! * [`Library::private_texts`], [`Library::purge_private_text`]: what is held, and removing it. A purge removes the text and keeps
//!   the pointer and the claims; their anchors into it are marked `text_removed`.
//! * The text cache (`specs/142` §2.4): [`Library::cached`] is what is kept with no open licence (text research kept privately,
//!   every capture whose record says `rights: held`, every capture a host handed over); [`Library::note_use`] records a use (a search
//!   hit, a page gather, an open, a revisit) in one ledger, `library/cache-use.json`, written at most once a minute;
//!   [`Library::cap_text_cache`] holds the cache to `[library] text_cache_max_mib`, the least recently used first, their `raw` and then
//!   their text; [`Library::cache_admits_host`] is the rule that a host's capture cannot push out text in use.
//! * [`Library::remove_bodies`] (`specs/142` §2.1 item 2, §2.3 items 3 and 4): how a capture's bodies go, by eviction or by a purge of
//!   the cache. `raw` and the text are removed; the record stays with its digests and says `evicted` or `purged` with the date; the
//!   excerpts, the pinned excerpts, the claims and their anchors stay, the anchors as orphans of kind `text evicted` or `text purged`.
//!   This is not a takedown (`Library::purge`), which removes the excerpts and the quotes too.
//!
//! Sources the operator flagged private by hand (`delphi sources add --private`) are not `kept_privately` and are untouched here.

use super::*;
use std::collections::HashMap;

/// The longest run of a privately kept source's text one citation may carry, in characters (`specs/119` §3 item 2).
pub const RUN_MAX: usize = super::excerpts::PASSAGE_MAX;

/// Why a capture's bodies were removed (`specs/142` §2.1 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gone {
    /// The cache was over its cap.
    Evicted,
    /// The operator purged the cache.
    Purged,
}

/// One source kept privately whose text is held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateText {
    pub id: String,
    pub title: String,
    pub bytes: u64,
    /// When the text was last read or checked (`last_checked`, else `retrieved`), RFC 3339.
    pub touched: String,
}

fn collapse(s: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = !out.is_empty();
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    out
}

const BASE: u64 = 0x100_0000_01b3;

fn hashes(text: &[char], n: usize) -> impl Iterator<Item = (usize, u64)> + '_ {
    let top = (1..n).fold(1u64, |p, _| p.wrapping_mul(BASE));
    let mut h = 0u64;
    (0..text.len()).filter_map(move |i| {
        if i >= n {
            h = h.wrapping_sub((text[i - n] as u64).wrapping_mul(top));
        }
        h = h.wrapping_mul(BASE).wrapping_add(text[i] as u64);
        (i + 1 >= n).then(|| (i + 1 - n, h))
    })
}

/// Does `text` carry a run of more than `longer_than` characters of `private`? Whitespace is compared collapsed, so re-wrapping a
/// passage does not hide it. A rolling hash over every window of `text` is checked against every window of `private`, and each hit is
/// confirmed letter for letter.
pub fn shares_run(private: &str, text: &str, longer_than: usize) -> bool {
    let n = longer_than + 1;
    let (p, t) = (collapse(private), collapse(text));
    if p.len() < n || t.len() < n {
        return false;
    }
    let mut seen: HashMap<u64, Vec<usize>> = HashMap::new();
    for (at, h) in hashes(&t, n) {
        seen.entry(h).or_default().push(at);
    }
    hashes(&p, n).any(|(at, h)| seen.get(&h).is_some_and(|starts| starts.iter().any(|&s| t[s..s + n] == p[at..at + n])))
}

fn touched(meta: &Value) -> String {
    meta["last_checked"].as_str().or(meta["retrieved"].as_str()).unwrap_or("").to_string()
}

impl Library {
    /// The sources kept privately under `private_text` whose text is held, oldest first (`delphi sources private list`).
    pub fn private_texts(&self) -> Vec<PrivateText> {
        let mut out: Vec<PrivateText> = self
            .list()
            .into_iter()
            .filter(|(_, m)| m["kept_privately"].is_string() && m["purged"].is_null())
            .map(|(id, m)| PrivateText { bytes: self.held_text_bytes(&id), title: m["title"].as_str().unwrap_or("").chars().take(200).collect(), touched: touched(&m), id })
            .filter(|p| p.bytes > 0)
            .collect();
        out.sort_by(|a, b| a.touched.cmp(&b.touched).then(a.id.cmp(&b.id)));
        out
    }

    /// Remove the bodies of every kept version of a source (`specs/142` §2.1 item 2): `raw`, and with `text_too` the text. Each
    /// version's record stays and says what went and when (`evicted: {raw, text}`, or `purged`). When the text goes, the source reads as
    /// a pointer (or as excerpts, when it keeps some), its record is marked `text_evicted` or `text_purged`, and every anchor into it
    /// that still resolved becomes an orphan of that kind ([`Library::text_removed`]). The excerpts, the pinned excerpts, the claims and
    /// the citation are untouched. Returns the bytes removed.
    pub(super) fn remove_bodies(&self, id: &str, gone: Gone, text_too: bool) -> Result<u64, String> {
        let mut meta = self.read_meta(id).ok_or_else(|| format!("no source `{}`", id))?;
        let now = delphi_core::time::now_rfc3339();
        let mut removed = 0u64;
        let mut text_went = false;
        for (_, dir) in self.version_dirs(id) {
            let mut went: Vec<&str> = Vec::new();
            for (file, what) in [(capture::RAW, "raw"), (capture::TEXT, "text")] {
                if what == "text" && !text_too {
                    continue;
                }
                let Ok(m) = std::fs::metadata(dir.join(file)) else { continue };
                if std::fs::remove_file(dir.join(file)).is_ok() {
                    removed += m.len();
                    went.push(what);
                }
            }
            if went.is_empty() {
                continue;
            }
            text_went |= went.contains(&"text");
            if let Some(mut rec) = std::fs::read(dir.join(capture::RECORD)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
                match gone {
                    Gone::Evicted => {
                        for what in went {
                            rec["evicted"][what] = json!(now);
                        }
                    }
                    Gone::Purged => rec["purged"] = json!(now),
                }
                self.write_json(&dir.join(capture::RECORD), &rec)?;
            }
        }
        let (mark, why) = match gone {
            Gone::Evicted => ("text_evicted", claims::TEXT_EVICTED),
            Gone::Purged => ("text_purged", claims::TEXT_PURGED),
        };
        if text_went {
            if meta["capture_level"] == "full" {
                meta["capture_level"] = json!(if self.excerpts(id).is_empty() { Level::Pointer.as_str() } else { Level::Excerpts.as_str() });
            }
            meta[mark] = json!(now);
            self.write_json(&self.source_dir(id).join("meta.json"), &meta)?;
            self.text_removed(id, why, &now);
        }
        if removed > 0 {
            self.log("bodies_removed", json!({"id": id, "why": why, "text": text_went, "bytes": removed}));
        }
        Ok(removed)
    }

    /// The text of a source was removed by eviction or by a purge of the cache (`specs/142` §2.3 items 3 and 4): every anchor into it
    /// that still resolved becomes an orphan of that kind, with the date (and keeps the older `text_removed` mark). The claim is
    /// **not** contested. The anchor keeps its quote, its version and its state; what it then counts for is decided by whether its
    /// verified excerpt is kept ([`Library::counts`]). Each claim's support is recomputed, so an unbacked anchor leaves the count at once.
    fn text_removed(&self, source_id: &str, why: &'static str, now: &str) {
        for mut c in self.citing(source_id) {
            let mut touched = false;
            for a in c["anchors"].as_array_mut().into_iter().flatten() {
                if a["source_id"] == source_id && claims::active(a) && a["orphan"].is_null() {
                    a["orphan"] = json!({"why": why, "at": now});
                    a["text_removed"] = json!(now);
                    touched = true;
                }
            }
            if touched && self.save_claim(&c).is_ok() {
                self.log("claim_text_removed", json!({"id": c["id"], "source": source_id, "why": why}));
                if let Some(id) = c["id"].as_str() {
                    self.refresh_support(id);
                }
            }
        }
    }

    /// Remove the kept text of one privately kept source: its bodies go ([`Library::remove_bodies`]), the pointer, its excerpts and its
    /// claims stay, and every anchor into it is an orphan marked `text_removed` with the date. Returns the bytes removed. A source not
    /// kept privately is refused.
    fn remove_private_text(&self, id: &str, gone: Gone) -> Result<u64, String> {
        let meta = self.read_meta(id).ok_or_else(|| format!("no source `{}`", id))?;
        if !meta["kept_privately"].is_string() {
            return Err(format!("`{}` was not kept privately by research", id));
        }
        let removed = self.remove_bodies(id, gone, true)?;
        self.log("private_text_removed", json!({"id": id, "bytes": removed}));
        Ok(removed)
    }

    /// `delphi sources private purge [--older-than <days>]` (`specs/119` §3 item 4): remove the kept text of the sources kept privately,
    /// or only of those last read or checked more than `older_than_days` days before `now_unix`. Returns what was removed: id and bytes.
    pub fn purge_private_text(&self, older_than_days: Option<u64>, now_unix: i64) -> Result<Vec<(String, u64)>, String> {
        let mut out = Vec::new();
        for p in self.private_texts() {
            let age_days = delphi_core::time::parse_rfc3339(&p.touched).map(|t| (now_unix - t).max(0) as u64 / 86_400);
            if older_than_days.is_some_and(|d| age_days.is_some_and(|a| a < d)) {
                continue;
            }
            out.push((p.id.clone(), self.remove_private_text(&p.id, Gone::Purged)?));
        }
        Ok(out)
    }
}

// ---- the text cache, by use (`specs/142` §2.4; slice D1) ----

/// A capture used within this many days is in use: it is evicted only to make room for a capture the daemon itself made, never
/// for a host's (`specs/142` §2.4 item 3).
pub const IN_USE_DAYS: i64 = 7;
/// What a host capture is refused with when keeping it would mean evicting text in use (`specs/142` §2.2 item 7).
pub const CACHE_FULL: &str = "cache full: pin less or raise the cap";
/// The use ledger, in the library's root: `{"uses": {"<source id>": <unix seconds>}}`.
pub const LEDGER: &str = "cache-use.json";
/// The ledger file is written at most once in this many seconds; uses in between are held in memory.
pub const LEDGER_EVERY: i64 = 60;

/// One capture in the text cache: text kept with no open licence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cached {
    pub id: String,
    pub title: String,
    /// The bytes of `raw` and of the text, over every kept version.
    pub raw_bytes: u64,
    pub text_bytes: u64,
    /// When it was last used (a search hit, a page gather, an open, a revisit), Unix seconds. `None`: never.
    pub last_use: Option<i64>,
    /// When it was captured or last checked, Unix seconds: what orders captures that were never used.
    pub captured: i64,
    /// A host handed it over and the daemon has not confirmed it.
    pub host_unconfirmed: bool,
    /// Never evicted: the operator added it by hand, or pinned it (`specs/142` §2.4 item 4).
    pub kept_for_good: bool,
}

impl Cached {
    pub fn bytes(&self) -> u64 {
        self.raw_bytes + self.text_bytes
    }

    /// What least-recently-used orders by: the last use, or for a capture never used the time it was captured; among equals an
    /// unconfirmed host capture that was never used goes first.
    fn order(&self) -> (i64, bool, &str) {
        (self.last_use.unwrap_or(self.captured), !(self.host_unconfirmed && self.last_use.is_none()), self.id.as_str())
    }

    fn in_use(&self, now: i64) -> bool {
        self.last_use.is_some_and(|u| now - u <= IN_USE_DAYS * 86_400)
    }
}

/// Which body of a capture an eviction removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
    Raw,
    Text,
}

#[derive(Default)]
struct Pending {
    uses: std::collections::BTreeMap<String, i64>,
    /// When the ledger file was last written by this process.
    written: Option<i64>,
    /// How many times this process wrote the file.
    writes: u64,
}

fn pending() -> &'static std::sync::Mutex<HashMap<PathBuf, Pending>> {
    static P: std::sync::OnceLock<std::sync::Mutex<HashMap<PathBuf, Pending>>> = std::sync::OnceLock::new();
    P.get_or_init(Default::default)
}

fn read_ledger(path: &Path) -> std::collections::BTreeMap<String, i64> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v["uses"].as_object().map(|m| m.iter().filter_map(|(k, t)| Some((k.clone(), t.as_i64()?))).collect())).unwrap_or_default()
}

/// The source ids among index paths (`library/sources/<id>/...`), each once.
pub fn source_ids<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = paths.into_iter().filter_map(|p| p.strip_prefix("library/sources/")).filter_map(|r| r.split('/').next()).filter(|id| valid_id(id)).map(String::from).collect();
    out.sort();
    out.dedup();
    out
}

impl Library {
    /// A capture was used (`specs/142` §2.4 item 2): a search returned a passage of it, a page build gathered it, the operator opened
    /// it, or it was read again. The use is held in memory and the ledger file is written at most once a minute, on a thread of its
    /// own, so a search does not wait for a write. Returns whether a write was started.
    pub fn note_use<S: AsRef<str>>(&self, ids: &[S], now: i64) -> bool {
        if ids.is_empty() {
            return false;
        }
        let due = {
            let mut all = pending().lock().unwrap_or_else(|e| e.into_inner());
            let p = all.entry(self.root.clone()).or_default();
            for id in ids.iter().map(AsRef::as_ref).filter(|id| valid_id(id)) {
                let at = p.uses.entry(id.to_string()).or_insert(now);
                *at = (*at).max(now);
            }
            let due = p.written.is_none_or(|w| now - w >= LEDGER_EVERY);
            if due {
                p.written = Some(now);
            }
            due
        };
        if due {
            let lib = Library::in_home(self.root.parent().unwrap_or(&self.root));
            std::thread::spawn(move || lib.flush_uses());
        }
        due
    }

    /// Write the uses held in memory to the ledger now, merged with what the file holds (the later time wins) and without the
    /// sources that are gone. The daemon's writes are spaced by [`Library::note_use`]; a command the operator runs writes once.
    pub fn flush_uses(&self) {
        // One write at a time, each taking the uses as they are when it starts: a later write never puts back an earlier state.
        static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _one = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let held = {
            let mut all = pending().lock().unwrap_or_else(|e| e.into_inner());
            let p = all.entry(self.root.clone()).or_default();
            p.writes += 1;
            p.uses.clone()
        };
        if !self.root.is_dir() {
            return;
        }
        let path = self.root.join(LEDGER);
        let mut uses = read_ledger(&path);
        for (id, at) in held {
            let slot = uses.entry(id).or_insert(at);
            *slot = (*slot).max(at);
        }
        uses.retain(|id, _| self.source_dir(id).is_dir());
        let tmp = self.root.join(format!(".{}.{}", LEDGER, std::process::id()));
        if std::fs::write(&tmp, serde_json::to_vec_pretty(&json!({"uses": uses})).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// When each capture was last used: the ledger file and the uses still held in memory, the later of the two.
    pub fn last_uses(&self) -> std::collections::BTreeMap<String, i64> {
        let mut uses = read_ledger(&self.root.join(LEDGER));
        if let Some(p) = pending().lock().unwrap_or_else(|e| e.into_inner()).get(&self.root) {
            for (id, at) in &p.uses {
                let slot = uses.entry(id.clone()).or_insert(*at);
                *slot = (*slot).max(*at);
            }
        }
        uses
    }

    #[cfg(test)]
    fn ledger_writes(&self) -> u64 {
        pending().lock().unwrap().get(&self.root).map_or(0, |p| p.writes)
    }

    /// What the text cache holds (`specs/142` §2.4 item 1): every source whose bodies are kept with no open licence. That is text
    /// research kept privately (`kept_privately`), every capture whose record says `rights: held`, and every capture a host handed
    /// over. Text under an open licence is not in the cache. Least recently used first.
    pub fn cached(&self) -> Vec<Cached> {
        let uses = self.last_uses();
        let mut out: Vec<Cached> = Vec::new();
        for (id, m) in self.list() {
            if !m["purged"].is_null() {
                continue;
            }
            let rec = self.capture_record(&id).unwrap_or(Value::Null);
            let by = rec["captured_by"].as_str().unwrap_or("");
            let host = by.starts_with("host:");
            if !(m["kept_privately"].is_string() || rec["rights"] == "held" || host) {
                continue;
            }
            let (mut raw_bytes, mut text_bytes) = (0, 0);
            for (_, dir) in self.version_dirs(&id) {
                raw_bytes += std::fs::metadata(dir.join(capture::RAW)).map(|f| f.len()).unwrap_or(0);
                text_bytes += std::fs::metadata(dir.join(capture::TEXT)).map(|f| f.len()).unwrap_or(0);
            }
            if raw_bytes + text_bytes == 0 {
                continue;
            }
            out.push(Cached {
                title: m["title"].as_str().unwrap_or("").chars().take(200).collect(),
                raw_bytes,
                text_bytes,
                last_use: uses.get(&id).copied(),
                captured: delphi_core::time::parse_rfc3339(&touched(&m)).unwrap_or(0),
                host_unconfirmed: host && rec["confirmed_by_daemon"].is_null(),
                kept_for_good: by == "operator" || m["pinned"] == true,
                id,
            });
        }
        out.sort_by(|a, b| a.order().cmp(&b.order()));
        out
    }

    /// What must go for the cache to hold `incoming` more bytes under `max_bytes` (`specs/142` §2.4 item 3), least recently used
    /// first, in two passes: the `raw` files, then, if still over, the text. `keep` (the capture just kept) is not in either pass.
    /// Never a capture the operator added or pinned. For a host's capture (`for_host`) nothing used in the last [`IN_USE_DAYS`] days
    /// may go, and when that leaves the cache over its cap the answer is `Err(`[`CACHE_FULL`]`)`: the capture is refused and nothing
    /// is evicted. For the daemon's, whatever is still over after both passes is `keep`'s own to give up, last. Reads only.
    fn eviction_plan(&self, incoming: u64, keep: Option<&str>, for_host: bool, max_bytes: u64, now: i64) -> Result<Vec<(String, Body)>, String> {
        let all = self.cached();
        let mut total: u64 = all.iter().map(Cached::bytes).sum::<u64>() + incoming;
        let mut plan = Vec::new();
        let may_go: Vec<&Cached> = all.iter().filter(|c| !c.kept_for_good && Some(c.id.as_str()) != keep && !(for_host && c.in_use(now))).collect();
        for body in [Body::Raw, Body::Text] {
            for c in &may_go {
                if total <= max_bytes {
                    break;
                }
                let size = if body == Body::Raw { c.raw_bytes } else { c.text_bytes };
                if size > 0 {
                    total -= size;
                    plan.push((c.id.clone(), body));
                }
            }
        }
        if total > max_bytes {
            if for_host {
                return Err(CACHE_FULL.into());
            }
            if let Some(k) = keep.and_then(|k| all.iter().find(|c| c.id == k && !c.kept_for_good)) {
                for (body, size) in [(Body::Raw, k.raw_bytes), (Body::Text, k.text_bytes)] {
                    if total > max_bytes && size > 0 {
                        total -= size;
                        plan.push((k.id.clone(), body));
                    }
                }
            }
        }
        Ok(plan)
    }

    /// May a host hand over `incoming` more bytes (`specs/142` §2.2 item 7)? `Err(`[`CACHE_FULL`]`)` when keeping them would mean
    /// evicting a capture used in the last seven days. Nothing is evicted or written here: the caller stores the capture and then
    /// calls [`Library::cap_text_cache`].
    pub fn cache_admits_host(&self, incoming: u64, max_bytes: u64, now: i64) -> Result<(), String> {
        self.eviction_plan(incoming, None, true, max_bytes, now).map(|_| ())
    }

    /// Hold the cache to its cap after `keep` was kept (`specs/142` §2.4 item 3): while it is over `max_bytes`, evict the least
    /// recently used, their `raw` first and then their text, and `keep` itself last. When `keep` is a host's capture the daemon has
    /// not confirmed, nothing in use goes for it: it gives up its own bodies instead. An evicted source keeps its record with its
    /// digests and `evicted`, its excerpts, its claims and their anchors (orphans of kind `text evicted`). Returns what went.
    pub fn cap_text_cache(&self, max_bytes: u64, keep: &str, now: i64) -> Vec<(String, Body)> {
        let for_host = self.capture_record(keep).is_some_and(|r| r["captured_by"].as_str().is_some_and(|b| b.starts_with("host:")) && r["confirmed_by_daemon"].is_null());
        let plan = match self.eviction_plan(0, Some(keep), for_host, max_bytes, now) {
            Ok(p) => p,
            // a host's capture that does not fit beside text in use: it is the one that goes
            Err(_) => vec![(keep.to_string(), Body::Raw), (keep.to_string(), Body::Text)],
        };
        let mut done = Vec::new();
        for (id, body) in plan {
            if self.remove_bodies(&id, Gone::Evicted, body == Body::Text).is_ok_and(|n| n > 0) {
                done.push((id, body));
            }
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_longer_than_the_bound_is_found_however_it_is_wrapped_and_a_shorter_one_is_not() {
        let private: String = (0..200).map(|i| format!("word{} ", i)).collect();
        let run: String = private.chars().skip(37).take(301).collect();
        assert!(shares_run(&private, &format!("A page that quotes {} and goes on.", run), 300));
        let wrapped = run.replace(' ', "\n   ");
        assert!(shares_run(&private, &format!("Before.\n{}\nAfter.", wrapped), 300), "re-wrapping does not hide a run");
        let short: String = private.chars().skip(37).take(300).collect();
        assert!(!shares_run(&private, &format!("A page that quotes «{}» and goes on.", short), 300), "300 characters is allowed");
        assert!(!shares_run(&private, "nothing of it", 300));
        assert!(!shares_run("short", &private, 300));
    }

    // ---- the text cache, by use (`specs/142` §2.4; slice D1) ----

    const KIB: usize = 1024;
    /// A fixed clock: 2026-11-25T00:00:00Z.
    const NOW: i64 = 1_795_564_800;
    const DAY: i64 = 86_400;

    fn at(unix: i64) -> String {
        delphi_core::time::format_unix_millis(unix, 0)
    }

    /// A capture as it is kept with no open licence: the record, the bytes and the text, on disk. `by` is who captured it; `days_old`
    /// is when, before [`NOW`]. Until `specs/142` slice B keeps such text itself, a test writes it.
    fn kept(l: &Library, id: &str, raw: usize, text: &str, by: &str, days_old: i64) {
        let dir = l.source_dir(id);
        std::fs::create_dir_all(&dir).unwrap();
        let when = at(NOW - days_old * DAY);
        let raw = vec![b'r'; raw];
        if !raw.is_empty() {
            std::fs::write(dir.join("raw"), &raw).unwrap();
        }
        std::fs::write(dir.join("source.md"), text).unwrap();
        let meta = json!({"title": id, "type": "webpage", "licence": "unknown", "capture_level": "full", "version": "1", "retrieved": when, "last_checked": when, "status": "not validated", "cited_by": [], "canonical_url": format!("https://example.org/{id}"), "origin": "example.org", "content_hash": format!("sha256:{}", sha256_hex(&raw))});
        std::fs::write(dir.join("meta.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
        let rec = json!({"version": "1", "raw_sha256": sha256_hex(&raw), "text_sha256": sha256_hex(text.as_bytes()), "bytes": raw.len(), "media_type": "text/html", "retrieved": when, "captured_by": by, "extractor": {"name": "fact-fetch-text", "version": "1"}, "rights": "held", "revisits": []});
        std::fs::write(dir.join("capture.json"), serde_json::to_vec(&rec).unwrap()).unwrap();
    }

    fn lib() -> (tempfile::TempDir, Library) {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        std::fs::create_dir_all(l.sources_dir()).unwrap();
        (d, l)
    }

    fn has(l: &Library, id: &str) -> (bool, bool) {
        (l.source_dir(id).join("raw").exists(), l.source_dir(id).join("source.md").exists())
    }

    fn total(l: &Library) -> u64 {
        l.cached().iter().map(Cached::bytes).sum()
    }

    /// AC-11: with a 16 MiB cap and 20 MiB of held captures, the `raw` files of the least recently used go first, then their text;
    /// the capture just kept is kept; an evicted source keeps its record, digests, excerpts and claims; a capture a page build
    /// gathered today keeps its text while an unused one remains.
    #[test]
    fn past_the_cap_the_least_recently_used_lose_their_raw_first_and_then_their_text() {
        let (_d, l) = lib();
        let quote = "The tide turns twice a day in this bay";
        let text = |n: usize| format!("{quote}. {}", "x".repeat(1792 * KIB - quote.len() - 2 - n));
        for i in 0..10 {
            kept(&l, &format!("c{i}"), 256 * KIB, &text(i), "daemon", 30 - i as i64);
        }
        assert_eq!(total(&l), 10 * 2048 * KIB as u64 - 45, "20 MiB, less the few bytes that tell the texts apart");
        // a claim rests on the oldest, with its verified excerpt; and a page build gathered c1 today
        let claim = l.add_claim("The tide turns twice a day", &claims::ClaimOpts::default(), &[claims::AnchorSpec { source_id: "c0".into(), locator: Some("p1".into()), quote: quote.into(), ..Default::default() }]).unwrap();
        l.note_use(&["c1"], NOW);
        let record = l.capture_record("c0").unwrap();

        let went = l.cap_text_cache(16 * 1024 * KIB as u64, "c9", NOW);
        assert!(total(&l) <= 16 * 1024 * KIB as u64);
        // every raw but the kept capture's went, least recently used first, the gathered one last; then one text, the oldest's
        let order: Vec<(&str, Body)> = went.iter().map(|(id, b)| (id.as_str(), *b)).collect();
        assert_eq!(order, vec![("c0", Body::Raw), ("c2", Body::Raw), ("c3", Body::Raw), ("c4", Body::Raw), ("c5", Body::Raw), ("c6", Body::Raw), ("c7", Body::Raw), ("c8", Body::Raw), ("c1", Body::Raw), ("c0", Body::Text)]);
        assert_eq!(has(&l, "c9"), (true, true), "the capture just kept is kept");
        assert_eq!(has(&l, "c0"), (false, false));
        assert_eq!(has(&l, "c1"), (false, true), "gathered today: its text stays while an unused one remains");
        assert_eq!(has(&l, "c2"), (false, true));
        // what stays of the evicted source: the record with its digests and `evicted`, the pinned excerpt, the claim and its anchor
        let rec = l.capture_record("c0").unwrap();
        assert_eq!((rec["raw_sha256"].clone(), rec["text_sha256"].clone()), (record["raw_sha256"].clone(), record["text_sha256"].clone()));
        assert!(rec["evicted"]["raw"].is_string() && rec["evicted"]["text"].is_string());
        assert!(l.capture_record("c2").unwrap()["evicted"]["raw"].is_string() && l.capture_record("c2").unwrap()["evicted"]["text"].is_null());
        let meta = l.read_meta("c0").unwrap();
        assert!(meta["text_evicted"].is_string() && meta["purged"].is_null() && meta["capture_level"] == "pointer");
        assert_eq!(l.pinned("c0").len(), 1);
        let c = l.claim(claim["id"].as_str().unwrap()).unwrap();
        assert_eq!((c["anchors"][0]["quote"].as_str(), c["anchors"][0]["orphan"]["why"].as_str()), (Some(quote), Some(claims::TEXT_EVICTED)));
        assert_eq!(l.orphans_of("c0")[0].reason(), "text evicted, verified excerpt kept");
        assert_eq!(l.verify(None).differ, vec![], "what is left still matches its record");
    }

    /// AC-12: a capture returned by a search yesterday outlives one last used a month ago, whichever was captured first.
    #[test]
    fn a_capture_used_yesterday_outlives_one_used_a_month_ago() {
        let (_d, l) = lib();
        kept(&l, "older", 0, &"a".repeat(600 * KIB), "daemon", 400);
        kept(&l, "newer", 0, &"b".repeat(600 * KIB), "daemon", 200);
        kept(&l, "just", 0, &"c".repeat(600 * KIB), "daemon", 0);
        l.note_use(&["older"], NOW - DAY);
        l.note_use(&["newer"], NOW - 30 * DAY);
        assert_eq!(l.cached().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["newer", "older", "just"], "least recently used first");
        assert_eq!(l.cap_text_cache(1300 * KIB as u64, "just", NOW), vec![("newer".to_string(), Body::Text)]);
        assert_eq!((has(&l, "older").1, has(&l, "newer").1, has(&l, "just").1), (true, false, true));
    }

    /// The fitness function of `specs/142` §6: after every capture the cache is under its cap, whatever the sizes, and the capture
    /// just kept is the last to give anything up.
    #[test]
    fn the_cache_is_never_over_its_cap_after_a_capture() {
        let (_d, l) = lib();
        let cap = 64 * KIB as u64;
        for i in 0..40usize {
            let id = format!("s{i}");
            kept(&l, &id, (i * 7919) % (30 * KIB), &"t".repeat(1 + (i * 104_729) % (20 * KIB)), if i % 5 == 0 { "host:claude/x" } else { "daemon" }, 40 - i as i64);
            if i % 3 == 0 {
                l.note_use(&[format!("s{}", i / 2)], NOW - (i as i64) * 3600);
            }
            l.cap_text_cache(cap, &id, NOW);
            assert!(total(&l) <= cap, "after capture {i} the cache holds {} bytes", total(&l));
            if i % 5 != 0 {
                assert!(has(&l, &id).1, "a capture the daemon made keeps its text when it fits by itself");
            }
        }
        // one capture larger than the whole cap gives itself up, last
        kept(&l, "huge", 0, &"h".repeat(100 * KIB), "daemon", 0);
        l.cap_text_cache(cap, "huge", NOW);
        assert_eq!((total(&l), has(&l, "huge").1), (0, false));
    }

    /// AC-29: with the cache full of captures used this week, a host capture is refused with `cache full` and nothing is evicted,
    /// while a capture the daemon makes evicts the least recently used. The cap holds before any held text is kept by slice B: the
    /// captures here are the ones the library keeps today.
    #[test]
    fn a_host_capture_cannot_push_out_text_in_use_and_a_daemon_capture_can() {
        let (_d, l) = lib();
        let cap = 1024 * KIB as u64;
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            kept(&l, id, 0, &"x".repeat(340 * KIB), "daemon", 20);
            l.note_use(&[id], NOW - (3 - i as i64) * DAY);
        }
        let before = total(&l);
        assert_eq!(l.cache_admits_host(200 * KIB as u64, cap, NOW), Err(CACHE_FULL.to_string()));
        assert_eq!((total(&l), has(&l, "a"), has(&l, "b"), has(&l, "c")), (before, (false, true), (false, true), (false, true)), "nothing is evicted");
        assert_eq!(l.cache_admits_host(2 * KIB as u64, cap, NOW), Ok(()), "what fits under the cap is admitted");
        // eight days later the same text is no longer in use, and a host's capture may take its place
        assert_eq!(l.cache_admits_host(200 * KIB as u64, cap, NOW + 8 * DAY), Ok(()));

        // a host's capture that was stored all the same gives up its own bodies, not the text in use
        kept(&l, "handed", 10 * KIB, &"h".repeat(200 * KIB), "host:claude/x", 0);
        assert_eq!(l.cap_text_cache(cap, "handed", NOW), vec![("handed".to_string(), Body::Raw), ("handed".to_string(), Body::Text)]);
        assert_eq!((has(&l, "handed"), has(&l, "a").1, has(&l, "b").1, has(&l, "c").1), ((false, false), true, true, true));

        // the daemon's own capture evicts the least recently used, in use or not
        kept(&l, "fetched", 0, &"f".repeat(200 * KIB), "daemon", 0);
        assert_eq!(l.cap_text_cache(cap, "fetched", NOW), vec![("a".to_string(), Body::Text)]);
        assert_eq!((has(&l, "a").1, has(&l, "b").1, has(&l, "c").1, has(&l, "fetched").1), (false, true, true, true));
    }

    /// AC-29: an unconfirmed, never-used host capture is evicted before a daemon capture of the same age; a confirmed one, and one
    /// that was used, take their turn like any other.
    #[test]
    fn an_unconfirmed_never_used_host_capture_goes_before_a_daemon_capture_of_the_same_age() {
        let (_d, l) = lib();
        // ids chosen so that the name alone would order the daemon's first
        kept(&l, "a-daemon", 0, &"d".repeat(400 * KIB), "daemon", 10);
        kept(&l, "z-host", 0, &"h".repeat(400 * KIB), "host:claude/x", 10);
        kept(&l, "new", 0, &"n".repeat(400 * KIB), "daemon", 0);
        assert_eq!(l.cached().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["z-host", "a-daemon", "new"]);
        assert_eq!(l.cap_text_cache(900 * KIB as u64, "new", NOW), vec![("z-host".to_string(), Body::Text)]);

        // confirmed by the daemon: it counts as any daemon capture does, so the name decides between equals
        let (_d2, l2) = lib();
        kept(&l2, "a-daemon", 0, &"d".repeat(400 * KIB), "daemon", 10);
        kept(&l2, "z-host", 0, &"h".repeat(400 * KIB), "host:claude/x", 10);
        let mut rec = l2.capture_record("z-host").unwrap();
        rec["confirmed_by_daemon"] = json!(at(NOW));
        std::fs::write(l2.source_dir("z-host").join("capture.json"), serde_json::to_vec(&rec).unwrap()).unwrap();
        kept(&l2, "new", 0, &"n".repeat(400 * KIB), "daemon", 0);
        assert_eq!(l2.cap_text_cache(900 * KIB as u64, "new", NOW), vec![("a-daemon".to_string(), Body::Text)]);
    }

    /// `specs/142` §2.4 item 4: never evicted are text under an open licence (it is not in the cache), a source the operator added by
    /// hand, and a source the operator pinned.
    #[test]
    fn open_text_the_operators_own_and_pinned_sources_are_never_evicted() {
        let (d, l) = lib();
        kept(&l, "mine", 0, &"m".repeat(300 * KIB), "operator", 90);
        kept(&l, "pinned", 0, &"p".repeat(300 * KIB), "daemon", 80);
        let mut meta = l.read_meta("pinned").unwrap();
        meta["pinned"] = json!(true);
        std::fs::write(l.source_dir("pinned").join("meta.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
        let f = d.path().join("open.md");
        std::fs::write(&f, "o".repeat(300 * KIB)).unwrap();
        let open = l.add_file(&f, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
        kept(&l, "old", 0, &"o".repeat(300 * KIB), "daemon", 70);
        kept(&l, "new", 0, &"n".repeat(300 * KIB), "daemon", 0);
        assert!(!l.cached().iter().any(|c| c.id == open), "open text is not in this cache");
        // the cap is under what may never go: everything that may go goes, the kept capture last, and the rest stays
        let went = l.cap_text_cache(100 * KIB as u64, "new", NOW);
        assert_eq!(went, vec![("old".to_string(), Body::Text), ("new".to_string(), Body::Text)]);
        assert!(has(&l, "mine").1 && has(&l, "pinned").1 && has(&l, &open).1);
    }

    /// `specs/142` §2.4 item 2: uses are kept in one ledger file, written at most once a minute; a use is never lost between writes,
    /// and a source that is gone leaves the ledger.
    #[test]
    fn uses_go_to_one_ledger_written_at_most_once_a_minute() {
        let (_d, l) = lib();
        kept(&l, "a", 0, "text a", "daemon", 5);
        kept(&l, "b", 0, "text b", "daemon", 5);
        assert!(l.note_use(&["a"], NOW), "the first use starts a write");
        assert!(!l.note_use(&["b"], NOW + 30), "a use within the minute is held in memory");
        assert!(!l.note_use(&["a", "gone"], NOW + 59));
        assert_eq!(l.last_uses().get("b"), Some(&(NOW + 30)), "and is read all the same");
        assert!(l.note_use(&["b"], NOW + 60), "a minute later the next write starts");
        assert!(!l.note_use::<&str>(&[], NOW + 500));
        l.flush_uses();
        let on_disk: Value = serde_json::from_slice(&std::fs::read(l.root().join(LEDGER)).unwrap()).unwrap();
        assert_eq!(on_disk, json!({"uses": {"a": NOW + 59, "b": NOW + 60}}), "the later use of each, and nothing for a source that is not there");
        assert!(l.ledger_writes() >= 1);
        assert_eq!(source_ids(["library/sources/a/source.md", "library/sources/a/excerpts.md", "context/x.md", "library/sources/b/source.md", "library/pages/p.md", "library/sources/../x"]), vec!["a".to_string(), "b".to_string()]);
    }
}
