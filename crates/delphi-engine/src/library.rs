//! The central library, sources layer (`specs/055` §2, §3; slice SK1).
//!
//! One store per install, `<home>/library/` (`sources/`, and `claims/` and `pages/` for later slices). A source is
//! untrusted material kept whole, as excerpts or as a pointer, with CSL-JSON metadata and a licence record. Capture goes
//! through the V2a fetcher only (`fact_fetch`: its SSRF guard, size cap and cloaking check); nothing here opens a
//! network path of its own. The Markdown of a stored source is never edited: a change arrives as a new version.
//!
//! Licence logic is `delphi_bundle::sources::classify`, the same function that gates a bundle's `sources/`.

use crate::fact_fetch::{fetch_snapshot_checked, FetchPolicy, Refusal};
use crate::fact_sources::SourceList;
use delphi_bundle::sources::{classify, Licence};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[path = "library_bib.rs"]
pub mod bib;
#[path = "library_claims.rs"]
pub mod claims;
#[path = "library_excerpts.rs"]
pub mod excerpts;
#[path = "library_meta.rs"]
pub mod meta;
#[path = "library_recheck.rs"]
mod recheck;
pub use recheck::Recheck;

const MIB: u64 = 1024 * 1024;

/// The budgets of `specs/055` §3.6. Full capture only; excerpts have their own bounds (SK2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    /// Text of one source: 64 MiB.
    pub text: u64,
    /// Text and assets of one source: 512 MiB.
    pub source: u64,
    /// The whole library: 20 GiB.
    pub library: u64,
}

impl Default for Budgets {
    fn default() -> Self {
        Budgets { text: 64 * MIB, source: 512 * MIB, library: 20 * 1024 * MIB }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Pointer,
    Excerpts,
    Full,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Pointer => "pointer",
            Level::Excerpts => "excerpts",
            Level::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<Level> {
        match s {
            "pointer" => Some(Level::Pointer),
            "excerpts" => Some(Level::Excerpts),
            "full" => Some(Level::Full),
            _ => None,
        }
    }
}

/// What the operator chose for one add. An agent has no way to build one (`specs/055` §10).
#[derive(Debug, Clone, Default)]
pub struct AddOpts {
    pub licence: Option<String>,
    pub level: Option<Level>,
    /// Operator-flagged private archival: full capture of restricted material for the operator's own use (§3.3).
    pub private: bool,
    pub title: Option<String>,
    pub rights_holder: Option<String>,
    pub attribution_text: Option<String>,
    pub doi: Option<String>,
    /// Raise the text or per-source limit for this one source (§3.6).
    pub raise_text: Option<u64>,
    pub raise_source: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Added,
    NewVersion(u64),
    /// Same content as the stored capture: only `last_checked` moved.
    Unchanged,
    /// The stored capture holds more than the new one would; nothing was changed.
    KeptRicher,
}

#[derive(Debug, Clone)]
pub struct Stored {
    pub id: String,
    pub level: Level,
    pub outcome: Outcome,
    /// Why the level is lower than asked, or anything else the operator should be told.
    pub notes: Vec<String>,
}

/// Every way a source's body may leave the install (`specs/055` §10.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Bundle,
    Xenia,
    Export,
    DebugBundle,
    Log,
    Relay,
    Connector,
    Mail,
    WebSearch,
    Outbound,
    Telemetry,
}

impl Channel {
    pub const ALL: [Channel; 11] = [
        Channel::Bundle,
        Channel::Xenia,
        Channel::Export,
        Channel::DebugBundle,
        Channel::Log,
        Channel::Relay,
        Channel::Connector,
        Channel::Mail,
        Channel::WebSearch,
        Channel::Outbound,
        Channel::Telemetry,
    ];
}

pub fn is_private(meta: &Value) -> bool {
    meta["private"].as_bool() == Some(true) || meta["licence"].as_str().is_some_and(|l| l.trim().eq_ignore_ascii_case("private"))
}

/// May the body of this source go out through `channel`? A private source never does, and neither does a restricted
/// body, whatever the channel (`specs/055` §10.1). The citation is a separate question and is not decided here.
pub fn may_leave(meta: &Value, channel: Channel) -> Result<(), String> {
    if is_private(meta) {
        return Err(format!("a private source never leaves the install ({:?})", channel));
    }
    let licence = meta["licence"].as_str().unwrap_or("unknown");
    match classify(licence) {
        Licence::Open { .. } => Ok(()),
        Licence::Internal => Err(format!("an internal source leaves only through a confirmed publication ({:?})", channel)),
        Licence::Restricted(why) => Err(format!("{} ({:?})", why, channel)),
    }
}

pub struct Library {
    root: PathBuf,
}

/// A short stable id from the canonical URL or identifier, not from content (`specs/055` §3.2).
pub fn source_id(canonical: &str) -> String {
    let mut slug = String::new();
    let bare = canonical.split_once("://").map(|(_, r)| r).unwrap_or(canonical);
    for c in bare.to_ascii_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(40).collect();
    let slug = slug.trim_end_matches('-');
    let h = sha256_hex(canonical.as_bytes());
    if slug.is_empty() { format!("src-{}", &h[..8]) } else { format!("{}-{}", slug, &h[..8]) }
}

fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{:02x}", x)).collect()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.as_bytes()[0].is_ascii_alphanumeric() && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn dir_bytes(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_bytes(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let t = e.file_type()?;
        if t.is_symlink() {
            continue;
        }
        if t.is_dir() {
            copy_tree(&e.path(), &to.join(e.file_name()))?;
        } else {
            std::fs::copy(e.path(), to.join(e.file_name()))?;
        }
    }
    Ok(())
}

fn headings(text: &str) -> Vec<&str> {
    text.lines().filter(|l| l.starts_with('#')).map(str::trim).collect()
}

/// A short change note with no model: sizes, and the headings that came or went.
fn change_note(old: &str, new: &str) -> String {
    let (oh, nh) = (headings(old), headings(new));
    let added: Vec<&&str> = nh.iter().filter(|h| !oh.contains(h)).take(5).collect();
    let removed: Vec<&&str> = oh.iter().filter(|h| !nh.contains(h)).take(5).collect();
    let mut s = format!("size {} -> {} bytes", old.len(), new.len());
    if !added.is_empty() {
        s.push_str(&format!("; new sections: {}", added.iter().map(|h| h.trim_start_matches('#').trim()).collect::<Vec<_>>().join(", ")));
    }
    if !removed.is_empty() {
        s.push_str(&format!("; removed sections: {}", removed.iter().map(|h| h.trim_start_matches('#').trim()).collect::<Vec<_>>().join(", ")));
    }
    s
}

struct Prepared {
    id: String,
    meta: Map<String, Value>,
    text: Option<String>,
    assets: Option<PathBuf>,
    requested: Option<Level>,
    excerpts: Option<Value>,
    /// Adopted from an added bundle at `bundle add`: the operator's add is the act of trust, so the licence is not a ceiling.
    imported: bool,
}

impl Library {
    pub fn in_home(home: &Path) -> Library {
        Library { root: home.join("library") }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn sources_dir(&self) -> PathBuf {
        self.root.join("sources")
    }

    pub fn source_dir(&self, id: &str) -> PathBuf {
        self.sources_dir().join(id)
    }

    fn ensure(&self) -> std::io::Result<()> {
        for d in ["sources", "claims", "pages"] {
            std::fs::create_dir_all(self.root.join(d))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    /// Bytes the sources hold now.
    pub fn used_bytes(&self) -> u64 {
        dir_bytes(&self.sources_dir())
    }

    pub fn read_meta(&self, id: &str) -> Option<Value> {
        if !valid_id(id) {
            return None;
        }
        serde_json::from_slice(&std::fs::read(self.source_dir(id).join("meta.json")).ok()?).ok()
    }

    /// Every source's record, by id.
    pub fn list(&self) -> Vec<(String, Value)> {
        let Ok(rd) = std::fs::read_dir(self.sources_dir()) else { return Vec::new() };
        let mut out: Vec<(String, Value)> = rd.flatten().filter_map(|e| e.file_name().to_str().map(String::from)).filter_map(|id| self.read_meta(&id).map(|m| (id, m))).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// The full-text sources the knowledge index may read: not private, not purged, with text (`specs/055` §2). A private
    /// source stays out of the index, so no search, serve or export path can carry it.
    pub fn indexable(&self) -> Vec<(String, PathBuf)> {
        self.list()
            .into_iter()
            .filter(|(_, m)| !is_private(m) && m["purged"].is_null() && m["capture_level"] == "full" && (may_leave(m, Channel::Outbound).is_ok() || m["bundle_origin"]["revoked"].is_null() && m["bundle_origin"]["bundle"].is_string()))
            .map(|(id, _)| {
                let p = self.source_dir(&id).join("source.md");
                (id, p)
            })
            .filter(|(_, p)| p.is_file())
            .collect()
    }

    fn log(&self, event: &str, data: Value) {
        use std::io::Write;
        let line = json!({"ts": delphi_core::time::now_rfc3339(), "event": event, "data": data});
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.root.join("log.jsonl")) {
            let _ = writeln!(f, "{}", line);
        }
    }

    /// The level a capture ends at, and why it is not the one asked for. Licence first (`specs/055` §3.3), then the
    /// budgets (§3.6).
    fn decide(&self, meta: &Map<String, Value>, requested: Option<Level>, text: Option<&str>, asset_bytes: u64, imported: bool, b: &Budgets) -> Result<(Level, Vec<String>), String> {
        let licence = meta.get("licence").and_then(Value::as_str).unwrap_or("unknown");
        let private = is_private(&Value::Object(meta.clone()));
        let unknown = licence.trim().eq_ignore_ascii_case("unknown");
        let ceiling = if private || imported || matches!(classify(licence), Licence::Open { .. } | Licence::Internal) { Level::Full } else { Level::Excerpts };
        let default = if unknown { Level::Pointer } else { ceiling };
        let mut level = requested.unwrap_or(default);
        if level > ceiling {
            return Err(format!("licence `{}` allows at most excerpts; pass --private to keep a full copy for your own use (it never leaves the install)", licence));
        }
        let mut notes = Vec::new();
        if level == Level::Full && text.is_none() {
            level = Level::Pointer;
            notes.push("no text was captured; kept as a pointer".into());
        }
        if level == Level::Full {
            let tl = text.map(|t| t.len() as u64).unwrap_or(0);
            if tl > b.text {
                notes.push(format!("the text is {} bytes, over the {}-byte limit for one source's text; kept as excerpts (raise it for this source with --max-text-mib)", tl, b.text));
                level = Level::Excerpts;
            } else if tl + asset_bytes > b.source {
                notes.push(format!("text and assets are {} bytes, over the {}-byte limit for one source; kept as excerpts (raise it with --max-source-mib)", tl + asset_bytes, b.source));
                level = Level::Excerpts;
            } else if self.used_bytes() + tl + asset_bytes > b.library {
                notes.push(format!("the library budget of {} bytes would be exceeded; kept as a pointer (nothing is evicted)", b.library));
                level = Level::Pointer;
            }
        }
        Ok((level, notes))
    }

    fn store(&self, mut p: Prepared, b: &Budgets) -> Result<Stored, String> {
        if !valid_id(&p.id) {
            return Err(format!("`{}` is not a valid source id", p.id));
        }
        let licence = p.meta.get("licence").and_then(Value::as_str).unwrap_or("unknown").to_string();
        let asset_bytes = p.assets.as_deref().map(dir_bytes).unwrap_or(0);
        let (level, notes) = self.decide(&p.meta, p.requested, p.text.as_deref(), asset_bytes, p.imported, b)?;
        if let Some(t) = &p.text {
            p.meta.insert("work_bytes".into(), json!(t.len()));
        }
        if level != Level::Full {
            p.text = None;
            p.assets = None;
        }
        self.ensure().map_err(|e| e.to_string())?;
        let old = self.read_meta(&p.id);
        let old_level = old.as_ref().and_then(|m| m["capture_level"].as_str()).and_then(Level::parse);
        let old_text = std::fs::read_to_string(self.source_dir(&p.id).join("source.md")).ok();
        let now = delphi_core::time::now_rfc3339();
        if let Some(o) = &old {
            let purged = !o["purged"].is_null();
            // A pointer with no hash is the same capture when its citation and licence are.
            let hash = p.meta.get("content_hash").cloned().unwrap_or(Value::Null);
            let same = o["content_hash"] == hash && old_level == Some(level) && (hash.is_string() || (o["licence"] == p.meta["licence"] && o["title"] == p.meta["title"] && o["canonical_url"] == p.meta.get("canonical_url").cloned().unwrap_or(Value::Null)));
            if same && !purged {
                let mut m = o.clone();
                m["last_checked"] = json!(now);
                self.write_json(&self.source_dir(&p.id).join("meta.json"), &m)?;
                return Ok(Stored { id: p.id, level, outcome: Outcome::Unchanged, notes });
            }
            if !purged && old_level.is_some_and(|ol| ol > level) && old_text.is_some() {
                let mut notes = notes;
                notes.push(format!("the stored capture is {} and this one would be {}; nothing was changed (`delphi sources purge {}` first to replace it)", old_level.unwrap().as_str(), level.as_str(), p.id));
                return Ok(Stored { id: p.id, level, outcome: Outcome::KeptRicher, notes });
            }
        }
        let version = old.as_ref().and_then(|o| o["version"].as_str()).and_then(|v| v.parse::<u64>().ok()).map(|v| v + 1).unwrap_or(1);
        p.meta.insert("capture_level".into(), json!(level.as_str()));
        p.meta.insert("version".into(), json!(version.to_string()));
        p.meta.insert("last_checked".into(), json!(now));
        p.meta.entry("retrieved").or_insert(json!(now));
        p.meta.insert("licence".into(), json!(licence));
        p.meta.insert("status".into(), json!("not validated"));
        p.meta.entry("cited_by").or_insert(json!([]));
        if let Some(o) = &old {
            // `cited_by` is the daemon's record of who relied on the source; a new capture keeps it
            if let Some(c) = o.get("cited_by").filter(|c| c.as_array().is_some_and(|a| !a.is_empty())) {
                p.meta.insert("cited_by".into(), c.clone());
            }
            if !p.meta.contains_key("work_bytes") {
                if let Some(w) = o.get("work_bytes") {
                    p.meta.insert("work_bytes".into(), w.clone());
                }
            }
            if p.excerpts.is_none() && level != Level::Pointer {
                let old_ex = self.excerpts(&p.id);
                if !old_ex.is_empty() {
                    p.excerpts = Some(Value::Array(excerpts::carry_forward(&old_ex, p.text.as_deref(), version)));
                }
            }
        }
        if let (Some(ot), Some(nt)) = (&old_text, &p.text) {
            p.meta.insert("change_note".into(), json!(change_note(ot, nt)));
        }

        let stage = self.root.join(".staging").join(format!("{}-{}", p.id, std::process::id()));
        let _ = std::fs::remove_dir_all(&stage);
        let built = (|| -> Result<(), String> {
            std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
            self.write_json(&stage.join("meta.json"), &Value::Object(p.meta.clone()))?;
            if let Some(t) = &p.text {
                std::fs::write(stage.join("source.md"), t).map_err(|e| e.to_string())?;
            }
            if level != Level::Pointer {
                let empty = json!([]);
                if let Some(x) = p.excerpts.as_ref().or((level == Level::Excerpts).then_some(&empty)) {
                    self.write_json(&stage.join("excerpts.json"), x)?;
                }
            }
            if let Some(a) = &p.assets {
                if a.is_dir() {
                    copy_tree(a, &stage.join("assets")).map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        })();
        if let Err(e) = built {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(e);
        }
        let dest = self.source_dir(&p.id);
        let outcome = if old.is_some() {
            let purged = old.as_ref().is_some_and(|o| !o["purged"].is_null());
            let keep = dest.join("versions");
            let old_n = version - 1;
            for e in std::fs::read_dir(&dest).map_err(|e| e.to_string())?.flatten() {
                let name = e.file_name();
                if name == "versions" {
                    continue;
                }
                if purged {
                    let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
                } else {
                    let to = keep.join(old_n.to_string());
                    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
                    std::fs::rename(e.path(), to.join(&name)).map_err(|e| e.to_string())?;
                }
            }
            for e in std::fs::read_dir(&stage).map_err(|e| e.to_string())?.flatten() {
                std::fs::rename(e.path(), dest.join(e.file_name())).map_err(|e| e.to_string())?;
            }
            let _ = std::fs::remove_dir_all(&stage);
            Outcome::NewVersion(version)
        } else {
            std::fs::rename(&stage, &dest).map_err(|e| e.to_string())?;
            Outcome::Added
        };
        if old.is_some() {
            self.reanchor_claims(&p.id, p.text.as_deref(), version);
        }
        self.log("capture", json!({"id": p.id, "level": level.as_str(), "version": version, "content_hash": p.meta.get("content_hash")}));
        Ok(Stored { id: p.id, level, outcome, notes })
    }

    fn write_json(&self, path: &Path, v: &Value) -> Result<(), String> {
        std::fs::write(path, serde_json::to_vec_pretty(v).unwrap_or_default()).map_err(|e| format!("{}: {}", path.display(), e))
    }

    fn base_meta(opts: &AddOpts, kind: &str, title: &str) -> Map<String, Value> {
        let licence = opts.licence.clone().unwrap_or_else(|| "unknown".into());
        let mut m = Map::new();
        m.insert("type".into(), json!(kind));
        m.insert("title".into(), json!(opts.title.clone().unwrap_or_else(|| title.to_string())));
        m.insert("licence".into(), json!(licence.trim()));
        if let Some(r) = &opts.rights_holder {
            m.insert("rights_holder".into(), json!(r));
        }
        let share_alike = matches!(classify(&licence), Licence::Open { share_alike: true, .. });
        m.insert("share_alike".into(), json!(share_alike));
        if let Some(d) = &opts.doi {
            m.insert("DOI".into(), json!(d));
        }
        if opts.private {
            m.insert("private".into(), json!(true));
        }
        m.insert("fetch_mode".into(), json!("operator"));
        m
    }

    /// Fill `attribution_text` where the licence needs one, from the record alone.
    fn attribute(meta: &mut Map<String, Value>, given: Option<&String>, origin_hint: &str) {
        let licence = meta["licence"].as_str().unwrap_or("").to_string();
        let needs = matches!(classify(&licence), Licence::Open { attribution: true, .. });
        let text = match given {
            Some(t) => Some(t.clone()),
            None if needs => {
                let who = meta.get("rights_holder").and_then(Value::as_str).unwrap_or(origin_hint).to_string();
                let sa = if meta["share_alike"] == true { " Share-alike: derivatives carry the same licence." } else { "" };
                Some(format!("\"{}\", {}, licensed {}.{}", meta["title"].as_str().unwrap_or(""), who, licence, sa))
            }
            None => None,
        };
        if let Some(t) = text {
            meta.insert("attribution_text".into(), json!(t));
        }
    }

    /// Capture a URL through the V2a fetcher (`specs/055` §3.4).
    pub async fn add_url(&self, policy: &FetchPolicy, url: &str, opts: &AddOpts, budgets: &Budgets) -> Result<Stored, String> {
        let b = Budgets { text: opts.raise_text.unwrap_or(budgets.text), source: opts.raise_source.unwrap_or(budgets.source), ..*budgets };
        let canonical = delphi_adapter::reqwest::Url::parse(url).map_err(|_| "not a URL".to_string()).map(|mut u| {
            u.set_fragment(None);
            u.to_string()
        })?;
        let id = source_id(&canonical);
        let host = delphi_adapter::reqwest::Url::parse(&canonical).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
        let mut meta = Self::base_meta(opts, "webpage", &host);
        meta.insert("URL".into(), json!(canonical));
        meta.insert("canonical_url".into(), json!(canonical));
        let mut policy = policy.clone();
        policy.max_bytes = b.text as usize;
        match fetch_snapshot_checked(&policy, &canonical).await {
            Err(e) if e.refusal == Refusal::TooLarge => {
                let mut notes = vec![format!("the page is over the {}-byte text limit; kept as a pointer (raise it with --max-text-mib)", b.text)];
                let mut s = self.store(Prepared { id, meta, text: None, assets: None, requested: Some(Level::Pointer), excerpts: None, imported: false }, &b)?;
                s.notes.append(&mut notes);
                Ok(s)
            }
            Err(e) => Err(e.to_string()),
            Ok((snap, _evidence)) => {
                let title = snap.title.for_daemon().clone();
                if !title.is_empty() && opts.title.is_none() {
                    meta.insert("title".into(), json!(title));
                }
                meta.insert("retrieved".into(), json!(snap.retrieved));
                meta.insert("content_hash".into(), json!(format!("sha256:{}", snap.id)));
                meta.insert("origin".into(), json!(snap.origin));
                meta.insert("source_class".into(), json!(SourceList::default().classify(snap.final_url.for_daemon()).class.as_str()));
                meta.insert("tags".into(), json!([]));
                if let Some((_, l)) = snap.headers.for_daemon().iter().find(|(k, _)| k == "content-language") {
                    meta.insert("language".into(), json!(l.split(',').next().unwrap_or("").trim()));
                }
                let found = meta::read(&canonical, snap.headers.for_daemon(), &String::from_utf8_lossy(snap.body_for_daemon()), snap.text.for_daemon(), opts.doi.as_deref());
                for (k, v) in found.fields {
                    let set_by_operator = (k == "title" && opts.title.is_some()) || (k == "licence" && opts.licence.is_some()) || (k == "rights_holder" && opts.rights_holder.is_some());
                    if !set_by_operator {
                        meta.insert(k, v);
                    }
                }
                let licence = meta["licence"].as_str().unwrap_or("unknown").to_string();
                meta.insert("share_alike".into(), json!(matches!(classify(&licence), Licence::Open { share_alike: true, .. })));
                Self::attribute(&mut meta, opts.attribution_text.as_ref(), &snap.origin);
                let mut notes = found.notes;
                let mut requested = opts.level;
                if snap.cloaked {
                    meta.insert("cloaked".into(), json!(true));
                    notes.push("the page showed different text to a browser than to the daemon (cloaking); kept as a pointer".into());
                    requested = Some(Level::Pointer);
                }
                let (text, over) = if snap.truncated { (None, true) } else { (Some(format!("{}{}\n", if title.is_empty() { String::new() } else { format!("# {}\n\n", title) }, snap.text.for_daemon())), false) };
                if over {
                    notes.push(format!("the page is over the {}-byte text limit; kept as a pointer", b.text));
                    requested = Some(Level::Pointer);
                }
                let mut s = self.store(Prepared { id, meta, text, assets: None, requested, excerpts: None, imported: false }, &b)?;
                notes.append(&mut s.notes);
                s.notes = notes;
                Ok(s)
            }
        }
    }

    /// Capture a local text or Markdown file the operator names.
    pub fn add_file(&self, path: &Path, opts: &AddOpts, budgets: &Budgets) -> Result<Stored, String> {
        let b = Budgets { text: opts.raise_text.unwrap_or(budgets.text), source: opts.raise_source.unwrap_or(budgets.source), ..*budgets };
        let canon = path.canonicalize().map_err(|e| format!("{}: {}", path.display(), e))?;
        let len = std::fs::metadata(&canon).map_err(|e| e.to_string())?.len();
        let bytes = if len > b.text { Vec::new() } else { std::fs::read(&canon).map_err(|e| format!("{}: {}", canon.display(), e))? };
        let name = canon.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let (text, requested, mut notes) = if len > b.text {
            (None, Some(opts.level.map_or(Level::Excerpts, |l| l.min(Level::Excerpts))), vec![format!("the file is {} bytes, over the {}-byte limit for one source's text, so it was not read; raise it with --max-text-mib", len, b.text)])
        } else {
            let t = String::from_utf8(bytes.clone()).map_err(|_| "not a UTF-8 text or Markdown file".to_string())?;
            (Some(t), opts.level, Vec::new())
        };
        let id = source_id(&format!("file:{}", canon.display()));
        let mut meta = Self::base_meta(opts, "document", name);
        meta.insert("retrieved".into(), json!(delphi_core::time::now_rfc3339()));
        if len <= b.text {
            meta.insert("content_hash".into(), json!(format!("sha256:{}", sha256_hex(&bytes))));
        }
        meta.insert("tags".into(), json!([]));
        Self::attribute(&mut meta, opts.attribution_text.as_ref(), "operator");
        let mut s = self.store(Prepared { id, meta, text, assets: None, requested, excerpts: None, imported: false }, &b)?;
        notes.append(&mut s.notes);
        s.notes = notes;
        Ok(s)
    }

    /// Takedown (`specs/055` §3.7): delete the capture (text, assets, versions, excerpts), keep the citation and the hash.
    /// Returns the bytes removed, or `None` if the source is unknown. Logged.
    pub fn purge(&self, id: &str) -> Result<Option<u64>, String> {
        let Some(mut meta) = self.read_meta(id) else { return Ok(None) };
        let dir = self.source_dir(id);
        let before = dir_bytes(&dir);
        for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            if e.file_name() == "meta.json" {
                continue;
            }
            let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
        }
        meta["capture_level"] = json!("pointer");
        meta["purged"] = json!(delphi_core::time::now_rfc3339());
        self.write_json(&dir.join("meta.json"), &meta)?;
        let removed = before.saturating_sub(dir_bytes(&dir));
        self.source_purged(id);
        self.log("purge", json!({"id": id, "content_hash": meta["content_hash"], "bytes_removed": removed}));
        Ok(Some(removed))
    }

    /// Adopt a bundle's `sources/` at `bundle add` (`specs/024` §4.1a): each enters at the capture level the bundle
    /// shipped, with its licence record intact, and `bundle_origin = { bundle, hash, kid? }` recorded. Licence never
    /// refuses an adoption; a private source does. Whether the content reads as vouched is not stored here: it is derived
    /// from the lock entry (`vouched_by`), so a field in a bundle's own `meta.json` can give nothing. Indexing is by
    /// `indexable`. A re-add that finds the same content restores a vouching that had ended.
    pub fn import_bundle(&self, bundle_dir: &Path, label: &str, origin: &Origin, budgets: &Budgets) -> Vec<(String, Result<Stored, String>)> {
        let root = bundle_dir.join("sources");
        let Ok(rd) = std::fs::read_dir(&root) else { return Vec::new() };
        let mut ids: Vec<String> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).filter_map(|e| e.file_name().to_str().map(String::from)).collect();
        ids.sort();
        ids.into_iter()
            .map(|id| {
                let r = self.import_one(&root.join(&id), &id, label, origin, budgets);
                if let Ok(s) = &r {
                    if matches!(s.outcome, Outcome::Added | Outcome::NewVersion(_) | Outcome::Unchanged) {
                        self.record_origin(&id, origin, s.outcome != Outcome::Unchanged);
                    }
                }
                (id, r)
            })
            .collect()
    }

    fn import_one(&self, dir: &Path, id: &str, label: &str, _origin: &Origin, budgets: &Budgets) -> Result<Stored, String> {
        let mut meta = match serde_json::from_slice::<Value>(&std::fs::read(dir.join("meta.json")).map_err(|e| e.to_string())?) {
            Ok(Value::Object(m)) => m,
            _ => return Err("meta.json is not a JSON object".into()),
        };
        if is_private(&Value::Object(meta.clone())) {
            return Err("private sources are never adopted from a bundle".into());
        }
        // what a bundle's own file says about its status or origin is not read
        for k in ["status", "bundle_origin", "cited_by", "purged"] {
            meta.remove(k);
        }
        let text = std::fs::read_to_string(dir.join("source.md")).ok();
        let level = meta.get("capture_level").and_then(Value::as_str).and_then(Level::parse).unwrap_or(if text.is_some() { Level::Full } else { Level::Pointer });
        if !meta.get("content_hash").is_some_and(Value::is_string) {
            if let Some(t) = &text {
                meta.insert("content_hash".into(), json!(format!("sha256:{}", sha256_hex(t.as_bytes()))));
            }
        }
        meta.insert("imported_from".into(), json!(label));
        let assets = dir.join("assets");
        self.store(Prepared { id: id.to_string(), meta, text, assets: assets.is_dir().then_some(assets), requested: Some(level), excerpts: std::fs::read(dir.join("excerpts.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()), imported: true }, budgets)
    }

    fn excerpts_hash(&self, id: &str) -> String {
        sha256_hex(&std::fs::read(self.source_dir(id).join("excerpts.json")).unwrap_or_default())
    }

    /// `fresh`: the files on disk are what the bundle shipped, so their excerpts are the baseline; an unchanged source keeps
    /// the baseline it had, so an excerpt the operator added since still counts as theirs.
    fn record_origin(&self, id: &str, origin: &Origin, fresh: bool) {
        let Some(mut m) = self.read_meta(id) else { return };
        let was_revoked = m["bundle_origin"]["revoked"].is_string();
        let baseline = if fresh { json!(self.excerpts_hash(id)) } else { m["bundle_origin"]["excerpts_hash"].clone() };
        let mut o = json!({"bundle": origin.bundle, "hash": origin.hash, "excerpts_hash": baseline});
        if let Some(k) = &origin.kid {
            o["kid"] = json!(k);
        }
        m["bundle_origin"] = o;
        if self.write_json(&self.source_dir(id).join("meta.json"), &m).is_ok() && was_revoked {
            self.source_revouched(id);
        }
    }

    /// The org that vouches for this source now: the lock entry of the bundle it came in with, still at the hash and
    /// publisher key it came in with, and the vouching not ended. `None` for a source received any other way, and for
    /// every source whose bundle has left or changed (`specs/024` §4.1a, `specs/052` §3.1).
    pub fn vouched_by(&self, meta: &Value, entries: &[delphi_core::bundles::Entry]) -> Option<String> {
        let o = meta.get("bundle_origin").filter(|o| o.is_object() && o["revoked"].is_null())?;
        delphi_core::bundles::vouched_org(entries, o["bundle"].as_str()?, o["hash"].as_str()?, o["kid"].as_str()).map(String::from)
    }

    /// The vouching of `bundle` ends for every imported source that does not belong to the version now held
    /// (`keep` = `Some((hash, kid))` of the entry that stays, `None` when the bundle is gone). Sources the operator has not
    /// cited or excerpted are deleted; the others are kept and read *not validated · no longer vouched*, and every claim
    /// anchored in them is marked contested (`specs/024` §4.1a). Returns the ids deleted and the ids kept.
    pub fn revoke_bundle(&self, bundle: &str, keep: Option<(&str, Option<&str>)>) -> Revoked {
        let mut out = Revoked::default();
        for (id, mut m) in self.list() {
            let o = &m["bundle_origin"];
            if o["bundle"] != bundle || o["revoked"].is_string() {
                continue;
            }
            if keep.is_some_and(|(h, k)| o["hash"] == h && o["kid"].as_str() == k) {
                continue;
            }
            let cited = m["cited_by"].as_array().is_some_and(|a| !a.is_empty()) || !self.citing(&id).is_empty();
            let touched = o["excerpts_hash"].as_str().is_some_and(|h| h != self.excerpts_hash(&id));
            if !cited && !touched {
                if std::fs::remove_dir_all(self.source_dir(&id)).is_ok() {
                    self.log("unvouch_delete", json!({"id": id, "bundle": bundle}));
                    out.deleted.push(id);
                }
                continue;
            }
            m["bundle_origin"]["revoked"] = json!(delphi_core::time::now_rfc3339());
            if self.write_json(&self.source_dir(&id).join("meta.json"), &m).is_ok() {
                self.source_unvouched(&id);
                self.log("unvouch_keep", json!({"id": id, "bundle": bundle}));
                out.kept.push(id);
            }
        }
        out
    }
}

/// Where an imported source came from: the lock entry's bundle, hash and publisher key (`None` when unsigned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub bundle: String,
    pub hash: String,
    pub kid: Option<String>,
}

/// What a revocation did to the bundle's imported sources.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Revoked {
    pub deleted: Vec<String>,
    pub kept: Vec<String>,
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
