//! The capture record (`specs/142` §2.1; slice A).
//!
//! A capture is one version of one source. Beside its `source.md` it keeps `raw`, the bytes as they were fetched or handed over, and
//! `capture.json`, which says what the copy is a copy of and how it was made: both digests, the media type, who captured it, the
//! extractor's name and version, the rights it is kept under, and each later time the same thing was read again (a revisit).
//!
//! A version's three files are written once, by `Library::store`. The only later changes are made here: a revisit line added to
//! `capture.json`, and the marks eviction and purge leave when they remove the bodies. `raw` is kept only where the text is kept: a
//! pointer or an excerpts source records the digests of what was read and holds neither body. `raw` is never indexed, never put in a
//! prompt and never copied out; it exists for `verify` and for re-extraction.
//!
//! Sources captured before this record existed have no `capture.json`. They read as before. The next time the same bytes are read the
//! record is written for the version that is there (`adopted`), and from then on revisits are recorded in it; different bytes make a new
//! version, which has its record from the start. Nothing has to be run.

use super::*;

/// The most revisits a version lists; later ones are counted in `revisits_more`.
pub const REVISITS_MAX: usize = 100;
pub(super) const RAW: &str = "raw";
pub(super) const RECORD: &str = "capture.json";
pub(super) const TEXT: &str = "source.md";

/// What was read, as it came, on its way into `Library::store`.
#[derive(Debug, Clone)]
pub(super) struct Capture {
    /// The bytes as fetched or as handed over, unchanged.
    pub raw: Vec<u8>,
    pub media_type: String,
    /// `daemon`, `operator`, `ingest` or `host:<tool>/<model>`.
    pub captured_by: String,
    /// For a fetch: the final address and the status.
    pub fetched: Option<(String, u16)>,
    /// The name and version of what made the text from the bytes.
    pub extractor: (&'static str, Option<&'static str>),
}

/// The text as it is stored and hashed (`specs/142` §2.3 item 1): no byte-order mark, one kind of line ending, one Unicode normal
/// form (NFC). `text_sha256` is the hash of exactly this, and an anchor's positions count characters in it.
pub fn normalise(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let t = text.strip_prefix('\u{feff}').unwrap_or(text);
    t.replace("\r\n", "\n").replace('\r', "\n").nfc().collect()
}

/// The rights a capture is kept under: `private` (never leaves the install), `open` (a licence lets the library keep and share it, or
/// the operator adopted it from a bundle), else `held` (kept, or would be kept, with no such licence).
pub(super) fn rights_of(meta: &Map<String, Value>, imported: bool) -> &'static str {
    let licence = meta.get("licence").and_then(Value::as_str).unwrap_or("unknown");
    if is_private(&Value::Object(meta.clone())) {
        "private"
    } else if imported || matches!(classify(licence), Licence::Open { .. } | Licence::Internal) {
        "open"
    } else {
        "held"
    }
}

/// The record of one capture, as `capture.json` holds it.
pub(super) fn record(c: &Capture, text_sha256: Option<&str>, retrieved: &Value, rights: &str, version: u64) -> Value {
    let mut extractor = json!({"name": c.extractor.0});
    if let Some(v) = c.extractor.1 {
        extractor["version"] = json!(v);
    }
    let mut r = json!({
        "version": version.to_string(),
        "raw_sha256": sha256_hex(&c.raw),
        "text_sha256": text_sha256,
        "bytes": c.raw.len(),
        "media_type": c.media_type,
        "retrieved": retrieved,
        "captured_by": c.captured_by,
        "extractor": extractor,
        "rights": rights,
        "revisits": [],
    });
    if let Some((url, status)) = &c.fetched {
        r["final_url"] = json!(url);
        r["status"] = json!(status);
    }
    r
}

/// One finding of [`Library::verify`]: a kept file whose digest is not the one its record states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Differs {
    pub id: String,
    pub version: String,
    /// `raw` or `source.md`.
    pub file: &'static str,
    pub recorded: String,
    pub found: String,
}

/// What [`Library::verify`] saw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verified {
    /// Versions with a record whose kept files were hashed.
    pub checked: usize,
    /// Versions captured before the capture record existed: nothing to compare with.
    pub unrecorded: usize,
    pub differ: Vec<Differs>,
}

impl Library {
    /// The capture record of a source's current version, if it has one.
    pub fn capture_record(&self, id: &str) -> Option<Value> {
        if !valid_id(id) {
            return None;
        }
        serde_json::from_slice(&std::fs::read(self.source_dir(id).join(RECORD)).ok()?).ok()
    }

    /// The same thing was read again (`specs/142` §2.1 item 3): one line `{at, by}` on the current version's record, with
    /// `raw_changed` when the bytes differ and the text does not. At most [`REVISITS_MAX`] lines are kept, and then a count. A version
    /// captured before the record existed gains its record now, from what was just read (`adopt`).
    pub(super) fn revisit(&self, id: &str, at: &str, c: Option<&Capture>, text_sha256: Option<&str>, raw_changed: bool) -> Result<(), String> {
        let Some(c) = c else { return Ok(()) };
        let path = self.source_dir(id).join(RECORD);
        let mut rec = match self.capture_record(id) {
            Some(r) => r,
            None => self.adopt(id, c, text_sha256)?,
        };
        let mut line = json!({"at": at, "by": c.captured_by});
        if raw_changed {
            line["raw_changed"] = json!(true);
        }
        let mut all = rec["revisits"].as_array().cloned().unwrap_or_default();
        if all.len() < REVISITS_MAX {
            all.push(line);
            rec["revisits"] = Value::Array(all);
        } else {
            rec["revisits_more"] = json!(rec["revisits_more"].as_u64().unwrap_or(0) + 1);
        }
        self.write_json(&path, &rec)
    }

    /// The record of a version captured before the record existed, written when the same bytes are read again: the digests of what
    /// was read, and of the text on disk when the source is held whole (so the record states what is kept, whatever made it). `raw` is
    /// kept beside a text that is kept. Marked `adopted` with the date.
    fn adopt(&self, id: &str, c: &Capture, text_sha256: Option<&str>) -> Result<Value, String> {
        let dir = self.source_dir(id);
        let meta = self.read_meta(id).ok_or_else(|| format!("no source `{}`", id))?;
        let version = meta["version"].as_str().and_then(|v| v.parse::<u64>().ok()).unwrap_or(1);
        let on_disk = std::fs::read(dir.join(TEXT)).ok().map(|b| sha256_hex(&b));
        let rights = rights_of(meta.as_object().unwrap_or(&Map::new()), meta["imported_from"].is_string());
        let mut rec = record(c, on_disk.as_deref().or(text_sha256), &meta["retrieved"], rights, version);
        if on_disk.as_deref().is_some_and(|d| Some(d) != text_sha256) {
            rec["extractor"] = json!({"name": "unknown"});
        }
        rec["adopted"] = json!(delphi_core::time::now_rfc3339());
        if on_disk.is_some() && !dir.join(RAW).exists() {
            std::fs::write(dir.join(RAW), &c.raw).map_err(|e| format!("raw: {}", e))?;
        }
        Ok(rec)
    }

    /// The directories of a source's kept versions: the current one, then the earlier ones under `versions/`.
    pub(super) fn version_dirs(&self, id: &str) -> Vec<(String, PathBuf)> {
        let dir = self.source_dir(id);
        let current = self.read_meta(id).and_then(|m| m["version"].as_str().map(String::from)).unwrap_or_else(|| "1".into());
        let mut out = vec![(current, dir.clone())];
        let mut older: Vec<(u64, PathBuf)> = std::fs::read_dir(dir.join("versions")).into_iter().flatten().flatten().filter_map(|e| Some((e.file_name().to_str()?.parse::<u64>().ok()?, e.path()))).collect();
        older.sort_by_key(|a| std::cmp::Reverse(a.0));
        out.extend(older.into_iter().map(|(n, p)| (n.to_string(), p)));
        out
    }

    /// `delphi sources verify [<id>]` (`specs/142` §2.1 item 6): hash again every kept `raw` and `source.md` of every version that has a
    /// record, and report each that is not what its record states. It reads only; nothing is changed, repaired or logged.
    pub fn verify(&self, id: Option<&str>) -> Verified {
        let ids: Vec<String> = match id {
            Some(i) => vec![i.to_string()],
            None => self.list().into_iter().map(|(i, _)| i).collect(),
        };
        let mut out = Verified::default();
        for id in ids.iter().filter(|i| valid_id(i)) {
            for (version, dir) in self.version_dirs(id) {
                let Some(rec) = std::fs::read(dir.join(RECORD)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
                    if dir.join(TEXT).is_file() {
                        out.unrecorded += 1;
                    }
                    continue;
                };
                out.checked += 1;
                for (file, key) in [(RAW, "raw_sha256"), (TEXT, "text_sha256")] {
                    let Ok(bytes) = std::fs::read(dir.join(file)) else { continue };
                    let (found, recorded) = (sha256_hex(&bytes), rec[key].as_str().unwrap_or("").to_string());
                    if found != recorded {
                        out.differ.push(Differs { id: id.clone(), version: version.clone(), file, recorded, found });
                    }
                }
            }
        }
        out
    }
}

// ---- a capture a host handed over (`specs/142` §2.2; slice C) ----

/// The extractor named on text a host handed over as text (`specs/142` §2.1 item 4).
pub const HOST_EXTRACTOR: &str = "host";
/// Why a host capture can never be confirmed (§2.2 item 5): read under the private-read mark, or of a kind the daemon never fetches.
pub const CANNOT_CONFIRM: &str = "cannot be confirmed";
/// A host capture the daemon may still confirm by reading the address itself.
pub const NOT_CONFIRMED: &str = "not confirmed by the daemon";

/// Is this capture record a host's that the daemon has not confirmed?
pub fn unconfirmed_host(rec: &Value) -> bool {
    rec["captured_by"].as_str().is_some_and(|b| b.starts_with("host:")) && rec["confirmed_by_daemon"].is_null()
}

/// What a source's record says of a host capture (`specs/142` §2.2 items 3 and 5): `None` when its current version is not a host's or
/// the daemon confirmed it; else [`NOT_CONFIRMED`] or, for one the daemon will never read, [`CANNOT_CONFIRM`].
pub fn host_word(meta: &Value) -> Option<&'static str> {
    if !meta["host_capture"].is_object() || !meta["confirmed_by_daemon"].is_null() {
        return None;
    }
    Some(if meta["host_capture"]["confirmable"] == false { CANNOT_CONFIRM } else { NOT_CONFIRMED })
}

/// What a host handed over for one source, checked and normalised (`host_capture`), on its way into [`Library::add_handed`].
#[derive(Debug, Clone, Default)]
pub struct Handed {
    pub id: String,
    /// The canonical web address, when the locator is one.
    pub url: Option<String>,
    pub locator: String,
    /// The kind of 113 §2 item 1: `web`, `internal`, `file` or `person`.
    pub kind: String,
    pub title: String,
    /// RFC 3339.
    pub retrieved: String,
    /// The bytes as handed over: the file's, or the text's.
    pub raw: Vec<u8>,
    pub media_type: String,
    /// `file`: the text is made from `raw` by the library's extractor. Else `raw` is the text.
    pub from_file: bool,
    /// `host:<tool>/<model>`, or `operator` for `delphi sources capture`.
    pub captured_by: String,
    /// Library-private: read under the private-read mark, of a kind other than `web`, or carrying a private identifier.
    pub private: bool,
    /// The daemon will never read this address itself (§2.2 item 5).
    pub unconfirmable: bool,
    pub csl: Map<String, Value>,
    /// What the host said the licence is: recorded, shown, never acted on (§2.2 item 8).
    pub licence_claimed: Option<String>,
    pub note: Option<String>,
    pub topics: Vec<String>,
    /// A research topic or run the host names.
    pub research: Option<String>,
    /// `[library] unknown_licence`, as a word: what is kept of text no licence lets the library copy.
    pub keep: String,
    /// The bytes show a rights notice the code detects (`specs/142` §2.5 item 6): no text of it is kept unless a licence allows it.
    pub rights_notice: bool,
    /// Handling traits the host adds (`specs/142` §2.2 item 8); the caller writes them.
    pub labels: Vec<String>,
}

impl Handed {
    /// The text as the library stores it, and the title it found: the file's text by the library's own extractor, or the host's text.
    /// A title heads the text as it heads a page the daemon reads (`Library::capture_text`), so the daemon reading the same page and
    /// getting the same words gets the same `text_sha256`.
    pub fn text(&self) -> (String, String) {
        let (found, body) = if self.from_file { crate::fact_fetch::extract_text(&self.raw, &self.media_type) } else { (String::new(), String::from_utf8_lossy(&self.raw).trim().to_string()) };
        let title = if self.title.is_empty() { found } else { self.title.clone() };
        (normalise(&format!("{}{}\n", if title.is_empty() { String::new() } else { format!("# {}\n\n", title) }, body)), title)
    }
}

/// Did someone other than a host make this capture?
fn not_a_host(rec: &Value) -> bool {
    !rec["captured_by"].as_str().is_some_and(|b| b.starts_with("host:"))
}

impl Library {
    /// Keep what a host handed over (`specs/142` §2.2 items 2, 3, 5 and 8) as a capture: `captured_by` the host, its bytes as `raw`, the
    /// text it handed (or the text the library's extractor made of its file), and a record that says it is the host's word.
    ///
    /// * `rights` is the daemon's: a licence the daemon finds in an HTML file it read; else the text is held (`text_held`) or, under
    ///   the private-read mark or of a kind other than `web`, library-private. `licence_claimed` is recorded and never raises it.
    /// * Under `[library] unknown_licence = "pointer"` or `"excerpts"` no text of an unlicensed host capture is kept, as for a page the
    ///   daemon reads: the record keeps both digests.
    /// * A source the daemon already holds its own whole text of is not replaced: the host's text is refused unless it is that text
    ///   (a revisit). A daemon record's origin and class carry over, so the daemon's own anchors still count.
    ///
    /// With [`Library::previewing`] nothing is written and the record is left in the preview.
    pub fn add_handed(&self, h: &Handed, b: &Budgets) -> Result<Stored, String> {
        let (text, title) = h.text();
        let old = self.read_meta(&h.id);
        let old_rec = self.capture_record(&h.id);
        let text_sha = sha256_hex(text.as_bytes());
        if let (Some(o), Some(r)) = (&old, &old_rec) {
            let whole = o["capture_level"] == "full" && o["purged"].is_null() && r["evicted"]["text"].is_null();
            if not_a_host(r) && whole && r["text_sha256"] != text_sha.as_str() {
                return Err("the library holds Delphi's own copy of this address, and the text handed over differs from it; nothing was changed".into());
            }
        }
        let mut meta = Map::new();
        let kind = if h.kind.is_empty() { "web" } else { h.kind.as_str() };
        meta.insert("type".into(), json!(h.csl.get("type").and_then(Value::as_str).unwrap_or(if h.url.is_some() { "webpage" } else if kind == "file" { "document" } else { kind })));
        meta.insert("title".into(), json!(if title.is_empty() { "untitled".to_string() } else { title.clone() }));
        for (k, v) in &h.csl {
            if k != "type" && k != "title" {
                meta.insert(k.clone(), v.clone());
            }
        }
        // The licence is what the daemon reads in the bytes, never what the host says (§2.2 item 8).
        let found = if h.from_file && h.media_type == "text/html" { meta::read(h.url.as_deref().unwrap_or(""), &[], &String::from_utf8_lossy(&h.raw), &text, None).fields.get("licence").and_then(Value::as_str).map(String::from) } else { None };
        let licence = found.unwrap_or_else(|| "unknown".into());
        meta.insert("licence".into(), json!(licence));
        meta.insert("share_alike".into(), json!(matches!(classify(&licence), Licence::Open { share_alike: true, .. })));
        if let Some(c) = &h.licence_claimed {
            meta.insert("licence_claimed".into(), json!(c));
        }
        match &h.url {
            Some(u) => {
                meta.insert("URL".into(), json!(u));
                meta.insert("canonical_url".into(), json!(u));
            }
            None => {
                meta.insert("locator".into(), json!(h.locator));
            }
        }
        meta.insert("source_kind".into(), json!(kind));
        meta.insert("fetch_mode".into(), json!("host_handed"));
        meta.insert("retrieved".into(), json!(h.retrieved));
        meta.insert("content_hash".into(), json!(format!("sha256:{}", sha256_hex(&h.raw))));
        meta.insert("host_capture".into(), json!({"by": h.captured_by, "confirmable": !h.unconfirmable}));
        meta.insert("tags".into(), json!([]));
        if !h.topics.is_empty() {
            meta.insert("topics".into(), json!(h.topics));
        }
        if let Some(r) = &h.research {
            meta.insert("handed_for".into(), json!(r));
        }
        if let Some(n) = &h.note {
            meta.insert("host_note".into(), json!(n));
        }
        // What the daemon's own capture of this address knew carries over, so its anchors still count.
        if let Some(o) = old.as_ref().filter(|_| old_rec.as_ref().is_some_and(not_a_host)) {
            for k in ["origin", "source_class", "source_unfamiliar", "trusted_entry"] {
                if let Some(v) = o.get(k).filter(|v| !v.is_null()) {
                    meta.insert(k.into(), v.clone());
                }
            }
        }
        let open = matches!(classify(&licence), Licence::Open { .. } | Licence::Internal);
        let requested = if h.private {
            meta.insert("private".into(), json!(true));
            Some(Level::Full)
        } else if open {
            Some(Level::Full)
        } else if matches!(h.keep.as_str(), "pointer" | "excerpts") || h.rights_notice {
            Some(Level::Pointer)
        } else if h.keep == "private_text" {
            meta.insert("private".into(), json!(true));
            meta.insert("kept_privately".into(), json!("unknown_licence"));
            Some(Level::Full)
        } else {
            meta.insert("text_held".into(), json!(true));
            Some(Level::Full)
        };
        let capture = Capture { raw: h.raw.clone(), media_type: h.media_type.clone(), captured_by: h.captured_by.clone(), fetched: None, extractor: if h.from_file { (crate::fact_fetch::EXTRACTOR_NAME, Some(crate::fact_fetch::EXTRACTOR_VERSION)) } else { (HOST_EXTRACTOR, None) } };
        self.store(Prepared { id: h.id.clone(), meta, text: Some(text), assets: None, requested, excerpts: None, imported: false, capture: Some(capture) }, b)
    }

    /// The daemon read an address a host handed over and found the same text (`specs/142` §2.2 item 3): the capture gains
    /// `confirmed_by_daemon` with the date, and the record what the daemon's fetch knows of the page, so from now on it counts as any
    /// daemon capture does. `m` is the source's record about to be written.
    pub(super) fn confirm_host_capture(&self, id: &str, daemons: &Map<String, Value>, now: &str, m: &mut Value) -> Result<(), String> {
        let path = self.source_dir(id).join(RECORD);
        if let Some(mut rec) = self.capture_record(id) {
            rec["confirmed_by_daemon"] = json!(now);
            self.write_json(&path, &rec)?;
        }
        m["confirmed_by_daemon"] = json!(now);
        for k in ["origin", "source_class", "source_unfamiliar", "trusted_entry"] {
            if let Some(v) = daemons.get(k) {
                m[k] = v.clone();
            }
        }
        self.log("host_capture_confirmed", json!({"id": id}));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact_fetch::test_html_snapshot;
    use crate::library::claims::{AnchorSpec, ClaimOpts};

    const URL: &str = "https://example.org/tides";

    fn page(extra: &str, words: &str) -> crate::fact_fetch::Snapshot {
        test_html_snapshot(URL, &format!("<html><head><title>Tides</title></head><body{}><p>{}</p></body></html>", extra, words))
    }

    fn open() -> AddOpts {
        AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }
    }

    fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
        [RAW, TEXT, RECORD].iter().map(|f| (f.to_string(), std::fs::read(dir.join(f)).unwrap())).collect()
    }

    /// AC-1.
    #[test]
    fn a_captured_page_keeps_its_bytes_both_digests_the_extractor_and_who_captured_it() {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let snap = page("", "The tide turns twice a day.");
        let s = l.add_fetched(URL, &snap, &open(), &Budgets::default()).unwrap();
        assert_eq!((s.level, s.outcome.clone()), (Level::Full, Outcome::Added));
        let dir = l.source_dir(&s.id);
        let rec = l.capture_record(&s.id).unwrap();
        let raw = std::fs::read(dir.join(RAW)).unwrap();
        assert_eq!(raw, snap.body_for_daemon(), "raw is the bytes as fetched");
        assert_eq!(rec["raw_sha256"], sha256_hex(&raw));
        assert_eq!(rec["text_sha256"], sha256_hex(&std::fs::read(dir.join(TEXT)).unwrap()));
        assert_eq!(rec["extractor"], json!({"name": crate::fact_fetch::EXTRACTOR_NAME, "version": crate::fact_fetch::EXTRACTOR_VERSION}));
        assert_eq!((rec["captured_by"].as_str(), rec["rights"].as_str(), rec["media_type"].as_str()), (Some("daemon"), Some("open"), Some("text/html")));
        assert_eq!((rec["bytes"].as_u64(), rec["status"].as_u64(), rec["final_url"].as_str()), (Some(raw.len() as u64), Some(200), Some(URL)));
        assert_eq!(rec["revisits"], json!([]));
        assert_eq!(l.verify(None), Verified { checked: 1, ..Default::default() });
    }

    /// AC-2.
    #[test]
    fn the_same_bytes_are_a_revisit_the_same_text_is_a_revisit_marked_raw_changed_and_new_text_is_a_new_version() {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let first = page("", "The tide turns twice a day.");
        let s = l.add_fetched(URL, &first, &open(), &Budgets::default()).unwrap();
        let claim = l.add_claim("Tides turn twice a day", &ClaimOpts::default(), &[AnchorSpec { source_id: s.id.clone(), locator: Some("p1".into()), quote: "turns twice a day".into(), ..Default::default() }]).unwrap();
        let claim_id = claim["id"].as_str().unwrap().to_string();
        let anchors = l.claim(&claim_id).unwrap()["anchors"].clone();
        let dir = l.source_dir(&s.id);
        let kept = files(&dir);

        // the same bytes: a revisit, no version
        let again = l.add_fetched(URL, &first, &open(), &Budgets::default()).unwrap();
        assert_eq!(again.outcome, Outcome::Unchanged);
        let rec = l.capture_record(&s.id).unwrap();
        assert_eq!(rec["revisits"].as_array().unwrap().len(), 1);
        assert_eq!(rec["revisits"][0]["by"], "daemon");
        assert!(rec["revisits"][0]["raw_changed"].is_null() && rec["revisits"][0]["at"].is_string());

        // other bytes, the same words: a revisit marked raw_changed, no version, no anchor touched
        let markup = page(" class=\"night\"", "The tide turns twice a day.");
        assert_ne!(markup.id, first.id);
        let same_text = l.add_fetched(URL, &markup, &open(), &Budgets::default()).unwrap();
        assert_eq!(same_text.outcome, Outcome::Unchanged);
        let rec = l.capture_record(&s.id).unwrap();
        assert_eq!(rec["revisits"][1]["raw_changed"], true);
        assert_eq!(l.read_meta(&s.id).unwrap()["version"], "1");
        assert!(!dir.join("versions").exists());
        assert_eq!(l.claim(&claim_id).unwrap()["anchors"], anchors, "no anchor is touched");
        assert_eq!(files(&dir)[..2], kept[..2], "raw and source.md are what they were");

        // other words: version 2, and the first version's three files are byte for byte what they were
        let before = files(&dir);
        let changed = l.add_fetched(URL, &page("", "The tide turns twice a day. It is highest at the full moon."), &open(), &Budgets::default()).unwrap();
        assert_eq!(changed.outcome, Outcome::NewVersion(2));
        assert_eq!(files(&dir.join("versions").join("1")), before);
        let rec2 = l.capture_record(&s.id).unwrap();
        assert_eq!((rec2["version"].as_str(), rec2["revisits"].as_array().map(Vec::len)), (Some("2"), Some(0)));
        assert_ne!(rec2["text_sha256"], rec["text_sha256"]);
        assert_eq!(l.verify(Some(&s.id)), Verified { checked: 2, ..Default::default() });
    }

    #[test]
    fn a_version_lists_at_most_a_hundred_revisits_and_then_counts() {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let snap = page("", "The tide turns twice a day.");
        let s = l.add_fetched(URL, &snap, &open(), &Budgets::default()).unwrap();
        for _ in 0..REVISITS_MAX + 3 {
            l.add_fetched(URL, &snap, &open(), &Budgets::default()).unwrap();
        }
        let rec = l.capture_record(&s.id).unwrap();
        assert_eq!((rec["revisits"].as_array().unwrap().len(), rec["revisits_more"].as_u64()), (REVISITS_MAX, Some(3)));
    }

    /// A page no licence lets the library keep is a pointer: it records the digests of what was read and holds neither body, and
    /// the same words under other markup are still the same capture.
    #[test]
    fn a_pointer_records_both_digests_and_keeps_neither_body() {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let s = l.add_fetched(URL, &page("", "The tide turns twice a day."), &AddOpts::default(), &Budgets::default()).unwrap();
        assert_eq!(s.level, Level::Pointer);
        let dir = l.source_dir(&s.id);
        assert!(!dir.join(RAW).exists() && !dir.join(TEXT).exists());
        let rec = l.capture_record(&s.id).unwrap();
        assert!(rec["raw_sha256"].is_string() && rec["text_sha256"].is_string());
        assert_eq!(rec["rights"], "held");
        let again = l.add_fetched(URL, &page(" id=\"x\"", "The tide turns twice a day."), &AddOpts::default(), &Budgets::default()).unwrap();
        assert_eq!(again.outcome, Outcome::Unchanged);
        assert_eq!(l.capture_record(&s.id).unwrap()["revisits"][0]["raw_changed"], true);
        assert_eq!(l.read_meta(&s.id).unwrap()["version"], "1");
    }

    /// AC-3.
    #[test]
    fn verify_reports_a_version_edited_by_hand_and_changes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let s = l.add_fetched(URL, &page("", "The tide turns twice a day."), &open(), &Budgets::default()).unwrap();
        l.add_fetched(URL, &page("", "The tide turns twice a day, and more."), &open(), &Budgets::default()).unwrap();
        let old = l.source_dir(&s.id).join("versions").join("1");
        std::fs::write(old.join(TEXT), "# Tides\n\nThe tide never turns.\n").unwrap();
        let tree = |p: &Path| -> Vec<(PathBuf, Vec<u8>)> {
            fn walk(p: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
                for e in std::fs::read_dir(p).unwrap().flatten() {
                    if e.path().is_dir() {
                        walk(&e.path(), out);
                    } else {
                        out.push((e.path(), std::fs::read(e.path()).unwrap()));
                    }
                }
            }
            let mut out = Vec::new();
            walk(p, &mut out);
            out.sort();
            out
        };
        let before = tree(l.root());
        let v = l.verify(None);
        assert_eq!(v.checked, 2);
        assert_eq!(v.differ.len(), 1);
        assert_eq!((v.differ[0].id.as_str(), v.differ[0].version.as_str(), v.differ[0].file), (s.id.as_str(), "1", TEXT));
        assert_ne!(v.differ[0].recorded, v.differ[0].found);
        assert_eq!(tree(l.root()), before, "verify changes nothing");
    }

    /// The migration: a source captured before the record existed reads as before, is told apart by `verify`, and gains its record the
    /// next time the same bytes are read, once.
    #[test]
    fn a_source_from_before_the_record_keeps_working_and_gains_its_record_on_the_next_read() {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let snap = page("", "The tide turns twice a day.");
        let pointer = l.add_fetched(URL, &snap, &AddOpts::default(), &Budgets::default()).unwrap();
        let other = "https://example.org/moon";
        let moon = test_html_snapshot(other, "<html><body><p>The moon pulls the sea.</p></body></html>");
        let full = l.add_fetched(other, &moon, &open(), &Budgets::default()).unwrap();
        // as an earlier release left them: no record, no raw
        for id in [&pointer.id, &full.id] {
            std::fs::remove_file(l.source_dir(id).join(RECORD)).unwrap();
            let _ = std::fs::remove_file(l.source_dir(id).join(RAW));
        }
        assert_eq!(l.list().len(), 2);
        assert_eq!(l.indexable().len(), 1);
        assert_eq!(l.verify(None), Verified { checked: 0, unrecorded: 1, differ: vec![] });
        let meta_before = l.read_meta(&pointer.id).unwrap();

        for round in 1..=2 {
            assert_eq!(l.add_fetched(URL, &snap, &AddOpts::default(), &Budgets::default()).unwrap().outcome, Outcome::Unchanged);
            assert_eq!(l.add_fetched(other, &moon, &open(), &Budgets::default()).unwrap().outcome, Outcome::Unchanged);
            let (p, f) = (l.capture_record(&pointer.id).unwrap(), l.capture_record(&full.id).unwrap());
            assert!(p["adopted"].is_string() && f["adopted"].is_string());
            assert_eq!((p["revisits"].as_array().unwrap().len(), f["revisits"].as_array().unwrap().len()), (round, round), "adopted once, then revisited");
            assert_eq!(p["raw_sha256"], sha256_hex(snap.body_for_daemon()));
            assert!(!l.source_dir(&pointer.id).join(RAW).exists(), "a pointer gains no body");
            assert_eq!(std::fs::read(l.source_dir(&full.id).join(RAW)).unwrap(), moon.body_for_daemon());
        }
        let meta_after = l.read_meta(&pointer.id).unwrap();
        assert_eq!((meta_after["version"].clone(), meta_after["capture_level"].clone(), meta_after["content_hash"].clone()), (meta_before["version"].clone(), meta_before["capture_level"].clone(), meta_before["content_hash"].clone()));
        assert_eq!(l.verify(None), Verified { checked: 2, ..Default::default() });
    }

    /// AC-22, the part that is the capture's: a byte-order mark, other line endings and another normal form give one text and one hash.
    #[test]
    fn a_byte_order_mark_line_endings_and_normal_form_do_not_change_the_text_or_its_hash() {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let plain = "# Café\n\nThe café opens at dawn.\nIt closes at dusk.\n";
        let variants = [plain.to_string(), format!("\u{feff}{}", plain), plain.replace('\n', "\r\n"), plain.replace('é', "e\u{301}")];
        let mut seen = std::collections::BTreeSet::new();
        for (i, v) in variants.iter().enumerate() {
            let f = d.path().join(format!("cafe-{}.md", i));
            std::fs::write(&f, v).unwrap();
            let s = l.add_file(&f, &open(), &Budgets::default()).unwrap();
            assert_eq!(std::fs::read_to_string(l.source_dir(&s.id).join(TEXT)).unwrap(), plain);
            assert_eq!(std::fs::read(l.source_dir(&s.id).join(RAW)).unwrap(), v.as_bytes(), "raw is the bytes as handed over");
            let rec = l.capture_record(&s.id).unwrap();
            assert_eq!(rec["captured_by"], "operator");
            seen.insert(rec["text_sha256"].as_str().unwrap().to_string());
        }
        assert_eq!(seen.len(), 1);
    }

    /// A takedown removes the bodies and leaves the record with its digests and the date.
    #[test]
    fn a_purge_leaves_the_record_marked_purged() {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let s = l.add_fetched(URL, &page("", "The tide turns twice a day."), &open(), &Budgets::default()).unwrap();
        let before = l.capture_record(&s.id).unwrap();
        l.purge(&s.id).unwrap();
        let dir = l.source_dir(&s.id);
        assert!(!dir.join(RAW).exists() && !dir.join(TEXT).exists());
        let rec = l.capture_record(&s.id).unwrap();
        assert!(rec["purged"].is_string());
        assert_eq!((rec["raw_sha256"].clone(), rec["text_sha256"].clone()), (before["raw_sha256"].clone(), before["text_sha256"].clone()));
        assert_eq!(l.verify(None).differ, vec![]);
    }

    /// The fitness function of `specs/142` §6: a version's files are written by `store` and marked here, and nowhere else. It reads
    /// the crate's production text (each file up to its test module) for a write of `source.md`, `raw` or `capture.json`.
    #[test]
    fn only_store_writes_a_versions_files() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let allowed = ["library.rs", "library_capture.rs", "library_private.rs"];
        let names = ["\"source.md\"", "\"raw\"", "\"capture.json\"", "(RAW)", "(TEXT)", "(RECORD)"];
        let writes = ["fs::write(", "write_json(", "File::create(", "OpenOptions", "fs::rename(", "fs::copy("];
        let mut found = Vec::new();
        for e in std::fs::read_dir(&src).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".rs") || name.ends_with("_tests.rs") || allowed.contains(&name.as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(e.path()).unwrap();
            let production = text.split("#[cfg(test)]").next().unwrap_or("");
            for (n, line) in production.lines().enumerate() {
                // a path inside a library source, not another store's file of the same name
                if names.iter().any(|x| line.contains(x)) && writes.iter().any(|w| line.contains(w)) && (line.contains("source_dir(") || line.contains("stage")) {
                    found.push(format!("{}:{}: {}", name, n + 1, line.trim()));
                }
            }
        }
        assert!(found.is_empty(), "a version's files are written outside the library's store:\n{}", found.join("\n"));
    }
}
