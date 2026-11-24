//! `delphi handling status` and `delphi handling reconcile` (`specs/121` §3 items 5 and 7): apply the arrival rules to what the
//! workspace and the library already hold, then carry the traits to the records made from them (§4), and count what is left.
//!
//! What it scans, in order: the knowledge base's imported pages (`local/summaries/`, by the kinds and locators their bibliography
//! lists, and their own markings); the library's sources a host declared `internal` or `person`; then the records derived from
//! them: library claims (by their anchors), findings and provisional notes (by the items they name). Last, a trait a record carries
//! only as `derived` from an item that no longer carries it is dropped (§4 item 4), and every view (a file's frontmatter, a claim's
//! `handling` field) is brought back in line with the record.
//!
//! It never changes a `private` flag, never removes a trait an operator, an agent or a host set, and a second run changes nothing.
//! A dry run works on a copy of the two records in a scratch folder and writes no view, so it reports exactly what a real run would
//! do and changes nothing. No meeting records exist in a workspace yet: the meeting rules are tested in [`crate::handling`] and
//! wait for a connector that imports one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::handling::{self, Arrival, Handling, Set, Trait};

/// What a reconciliation found and did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub dry_run: bool,
    /// Items the arrival rules looked at.
    pub scanned: usize,
    /// Of those, the ones that carry a trait afterwards, by arrival kind.
    pub labelled_by_path: BTreeMap<String, usize>,
    /// The ones that carry none, by arrival kind: what a rule could be added for (§3 item 5).
    pub unlabelled_by_path: BTreeMap<String, usize>,
    /// Items whose marking was seen on a path the operator did not configure (`says: ...`).
    pub says: usize,
    /// Derived records (claims, findings, notes) that carry a trait afterwards.
    pub derived: usize,
    /// Records whose set changed in this run.
    pub changed: usize,
    /// Derived traits dropped because their source lost them (§4 item 4).
    pub dropped: usize,
    /// Views brought back in line with the record (a dry run counts the ones it would restore).
    pub views: usize,
    /// Every record's set afterwards, counted by trait.
    pub by_trait: BTreeMap<&'static str, usize>,
}

impl Report {
    pub fn to_json(&self) -> Value {
        json!({"dry_run": self.dry_run, "scanned": self.scanned, "labelled_by_path": self.labelled_by_path, "unlabelled_by_path": self.unlabelled_by_path, "says": self.says, "derived": self.derived, "changed": self.changed, "dropped": self.dropped, "views": self.views, "by_trait": self.by_trait})
    }

    /// The operator's lines.
    pub fn lines(&self) -> Vec<String> {
        let counts = |m: &BTreeMap<String, usize>| if m.is_empty() { "none".to_string() } else { m.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ") };
        let traits = if self.by_trait.is_empty() { "none".to_string() } else { self.by_trait.iter().map(|(k, v)| format!("{} {v}", Trait::parse(k).map(Trait::shown).unwrap_or(k))).collect::<Vec<_>>().join(", ") };
        let mut out = vec![
            format!("{}{} items looked at by their arrival path", if self.dry_run { "dry run: nothing was changed. " } else { "" }, self.scanned),
            format!("labelled, by path: {}", counts(&self.labelled_by_path)),
            format!("unlabelled, by path: {} (readable; add a [[handling.path]] row to label them)", counts(&self.unlabelled_by_path)),
            format!("markings on paths you did not configure (noted, set nothing): {}", self.says),
            format!("derived records labelled: {}", self.derived),
            format!("records {}: {}; derived traits dropped: {}; views {}: {}", if self.dry_run { "that would change" } else { "changed" }, self.changed, self.dropped, if self.dry_run { "to restore" } else { "restored" }, self.views),
            format!("by trait: {traits}"),
        ];
        if self.dry_run {
            out.push("run `delphi handling reconcile --apply` to write these".into());
        }
        out
    }
}

/// The imported pages of the knowledge base: the page file and the arrivals its bibliography lists.
fn imported_pages(ws_dir: &Path) -> Vec<(PathBuf, Vec<Arrival>)> {
    let dir = ws_dir.join(crate::kb_import::DIR);
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<(PathBuf, Vec<Arrival>)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md") && !p.to_string_lossy().ends_with(".bib.md") && std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file()))
        .map(|p| {
            let bib = p.with_extension("bib.md");
            let arrivals = std::fs::read_to_string(&bib).map(|t| bibliography_arrivals(&t)).unwrap_or_default();
            (p, arrivals)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The arrival kind of an imported page that lists no source.
pub const IMPORT_KIND: &str = "import";

/// The `- locator: ...` / `  kind: ...` records of a bibliography file, as arrivals. A page with none arrived as [`IMPORT_KIND`].
pub fn bibliography_arrivals(bib: &str) -> Vec<Arrival> {
    let mut out: Vec<Arrival> = Vec::new();
    for line in bib.lines() {
        if let Some(loc) = line.strip_prefix("- locator: ") {
            out.push(Arrival::of("", loc.trim()));
        } else if let (Some(k), Some(last)) = (line.strip_prefix("  kind: "), out.last_mut()) {
            last.kind = k.trim().to_string();
        }
    }
    out.retain(|a| !a.kind.is_empty());
    out
}

/// The arrivals of one imported page: its bibliography's, or [`IMPORT_KIND`] at the page's own name.
pub fn page_arrivals(page: &Path, listed: Vec<Arrival>) -> Vec<Arrival> {
    if listed.is_empty() {
        vec![Arrival::of(IMPORT_KIND, &page.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())]
    } else {
        listed
    }
}


/// The findings of every mission: `(file, text)`.
fn findings(ws_dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for (_, mdir) in delphi_core::sessions::dirs(ws_dir) {
        let Ok(rd) = std::fs::read_dir(mdir.join("findings")) else { continue };
        for p in rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "md")) {
            if std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()) {
                if let Ok(t) = std::fs::read_to_string(&p) {
                    out.push((p, t));
                }
            }
        }
    }
    out.sort();
    out
}

/// A dry run's scratch folder, removed when the run ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The two records copied into `scratch`, for a dry run: a reconciliation against the copy is the real one, minus the writes.
fn scratch_copy(ws_dir: &Path, home: &Path, scratch: &Path) -> Result<Handling, String> {
    let (ws_copy, lib_copy) = (scratch.join("ws"), scratch.join("lib"));
    for (from, to) in [(handling::dir(ws_dir), &ws_copy), (handling::library_dir(home), &lib_copy)] {
        std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
        if let Ok(b) = std::fs::read(from.join("records.json")) {
            std::fs::write(to.join("records.json"), b).map_err(|e| e.to_string())?;
        }
    }
    Ok(Handling::at(ws_copy, lib_copy))
}

/// Reconcile the workspace at `ws_dir` and the library under `home`. `dry_run`: report, change nothing.
pub fn reconcile(ws_dir: &Path, home: &Path, dry_run: bool) -> Result<Report, String> {
    let real = Handling::of(ws_dir, home);
    if let Some(bad) = real.unreadable().first() {
        return Err(format!("the handling record {} cannot be read; nothing was changed (move it aside and run again)", bad.display()));
    }
    // A dry run's fresh folder, never named by the clock (spec 125, R7): two dry runs at once each copy into their own.
    let scratch = if dry_run { Some(Scratch(delphi_core::tempname::create_dir(&std::env::temp_dir(), "delphi-handling-dry").map_err(|e| format!("cannot make a scratch folder for the dry run: {e}"))?)) } else { None };
    let h = match &scratch {
        Some(s) => scratch_copy(ws_dir, home, &s.0)?,
        None => real,
    };
    let before = h.all();
    let cfg = handling::config_of(ws_dir);
    let mut r = Report { dry_run, ..Report::default() };

    // 1. Arrival: the imported pages.
    for (page, listed) in imported_pages(ws_dir) {
        let Some(item) = handling::item_in_ws(ws_dir, &page) else { continue };
        let Ok(text) = std::fs::read_to_string(&page) else { continue };
        let arrivals = page_arrivals(&page, listed);
        let set = h.arrive_by(&cfg, &item, Some(&handling::content_hash(&text)), &arrivals, Some(&text))?;
        r.scanned += 1;
        let kind = arrivals.first().map(|a| a.kind.clone()).unwrap_or_default();
        *if set.is_empty() { r.unlabelled_by_path.entry(kind) } else { r.labelled_by_path.entry(kind) }.or_default() += 1;
    }
    // 2. Arrival: the library's sources a host declared (`internal`, `person`); any other source is counted by its kind.
    let lib = crate::library::Library::in_home(home);
    for (id, meta) in lib.list() {
        let kind = meta["source_kind"].as_str().unwrap_or("").to_string();
        let item = handling::source_item(&id);
        let declared = handling::host_kind(&kind);
        let host = meta["fetch_mode"] == "host_reported";
        if host && !declared.is_empty() {
            h.add(&item, None, &declared, &handling::By::Host, &[])?;
        }
        r.scanned += 1;
        let set = h.set(&item);
        let shown = if kind.is_empty() { meta["kind"].as_str().unwrap_or("source").to_string() } else { kind };
        *if set.is_empty() { r.unlabelled_by_path.entry(format!("library:{shown}")) } else { r.labelled_by_path.entry(format!("library:{shown}")) }.or_default() += 1;
    }
    // 3. Derived: claims by their anchors' sources, then findings and notes by what they name.
    for claim in lib.claims() {
        let Some(id) = claim["id"].as_str() else { continue };
        let used: Vec<String> = claim["anchors"].as_array().into_iter().flatten().filter_map(|a| a["source_id"].as_str()).map(handling::source_item).collect();
        h.derive(&handling::claim_item(id), None, &used, &[])?;
    }
    let mut written: Vec<(PathBuf, String, String)> = Vec::new();
    for (path, text) in findings(ws_dir) {
        let Some(item) = handling::item_in_ws(ws_dir, &path) else { continue };
        h.derive(&item, None, &handling::named_in(&h, &text), &[])?;
        written.push((path, item, text));
    }
    for n in crate::provisional_notes::list(ws_dir) {
        let Some(item) = handling::item_in_ws(ws_dir, &n.path) else { continue };
        let Ok(text) = std::fs::read_to_string(&n.path) else { continue };
        h.derive(&item, None, &handling::named_in(&h, &text), &[])?;
        written.push((n.path.clone(), item, text));
    }
    // 4. A derived trait whose source lost it (the operator cleared it there) goes (§4 item 4).
    for item in h.all().keys() {
        r.dropped += h.settle_derived(item)?.len();
    }
    // 5. Views: a file's frontmatter, a claim's `handling` field.
    for (path, item, text) in &written {
        let set = h.set(item);
        let rest: Vec<String> = h.record(item).map(|rec| rec["may_rest_on"].as_array().into_iter().flatten().filter_map(|e| e["item"].as_str().map(String::from)).collect()).unwrap_or_default();
        let shown = handling::with_view(text, &set, &rest);
        if shown != *text {
            r.views += 1;
            if !dry_run {
                std::fs::write(path, &shown).map_err(|e| format!("{}: {e}", path.display()))?;
                h.log(item, handling::RESTORED_EVENT, json!({"item": item, "traits": handling::names(&set)}));
            }
        }
    }
    for claim in lib.claims() {
        let Some(id) = claim["id"].as_str() else { continue };
        let item = handling::claim_item(id);
        let rec = h.record(&item);
        let d = handling::Derived {
            set: h.set(&item),
            may_rest_on: rec.map(|r| r["may_rest_on"].as_array().into_iter().flatten().filter_map(|e| Some((e["item"].as_str()?.to_string(), Set::new()))).collect()).unwrap_or_default(),
        };
        let view = handling::json_view(&d);
        if claim.get("handling").cloned().unwrap_or(Value::Null) != view {
            r.views += 1;
            if !dry_run {
                lib.set_handling_view(id, &view)?;
                h.log(&item, handling::RESTORED_EVENT, json!({"item": item, "traits": handling::names(&d.set)}));
            }
        }
    }
    // Counts.
    let after = h.all();
    r.changed = after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).count() + before.keys().filter(|k| !after.contains_key(*k)).count();
    for (item, rec) in &after {
        let set = handling::set_of(rec);
        if !rec["says"].as_array().is_none_or(|a| a.is_empty()) {
            r.says += 1;
        }
        let derived_kind = item.starts_with("library/claims/") || item.contains("/findings/") || item.contains("/provisional/");
        if derived_kind && !set.is_empty() {
            r.derived += 1;
        }
        for t in set {
            *r.by_trait.entry(t.as_str()).or_default() += 1;
        }
    }
    Ok(r)
}

/// What `delphi handling status` and the doctor line show: the reconciliation's dry run, plus the stores that cannot be read.
pub fn status(ws_dir: &Path, home: &Path) -> Result<Report, String> {
    reconcile(ws_dir, home, true)
}

/// The doctor's one line (§3 item 5): the unlabelled items by path and the markings seen on unconfigured paths, from the records
/// alone (no scan), so it is cheap. `None` when the workspace has no record yet.
pub fn doctor_line(ws_dir: &Path, home: &Path) -> Option<String> {
    let h = Handling::of(ws_dir, home);
    if let Some(bad) = h.unreadable().first() {
        return Some(format!("handling: the record {} cannot be read; `delphi handling reconcile` refuses until it is moved aside", bad.display()));
    }
    let all = h.all();
    if all.is_empty() && imported_pages(ws_dir).is_empty() {
        return None;
    }
    let mut by: BTreeMap<&'static str, usize> = BTreeMap::new();
    for rec in all.values() {
        for t in handling::set_of(rec) {
            *by.entry(t.shown()).or_default() += 1;
        }
    }
    let says = all.values().filter(|r| !r["says"].as_array().is_none_or(|a| a.is_empty())).count();
    let unlabelled = imported_pages(ws_dir).iter().filter_map(|(p, _)| handling::item_in_ws(ws_dir, p)).filter(|i| h.set(i).is_empty()).count();
    let traits = if by.is_empty() { "none labelled".to_string() } else { by.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ") };
    Some(format!("handling: {traits}; {unlabelled} imported page(s) unlabelled; {says} marking(s) on unconfigured paths (`delphi handling status` for the counts by path)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path().join("ws");
        let home = t.path().join("home");
        std::fs::create_dir_all(ws.join("local/sessions/m1/findings")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        (t, ws, home)
    }

    fn import(ws: &Path, text: &str) -> String {
        let r = crate::kb_import::import(ws, &crate::kb_import::Input { text, name_hint: "n.md", modified_secs: None, now_secs: 1_800_000_000, keep_id: None }, &|_| None).unwrap();
        handling::item_in_ws(ws, &r.page).unwrap()
    }

    /// AC-6: the dry run prints counts and changes nothing; a real run labels the fixture and its derived records; a second run
    /// changes nothing. And the bibliography's kinds decide an imported page's path.
    #[test]
    fn a_dry_run_changes_nothing_a_real_run_labels_and_a_second_run_changes_nothing() {
        let (_t, ws, home) = setup();
        let mail = import(&ws, "# Mail digest\n\nThe launch moves to May. Source: https://mail.example.com/thread/1\n");
        let wiki = import(&ws, "# Wiki page\n\nThe team structure. Source: https://wiki.example.com/team\n");
        let web = import(&ws, "# Web page\n\nCONFIDENTIAL\n\nA page that says so. Source: https://news.example.org/a\n");
        // The import itself already assigned by path.
        let h = Handling::of(&ws, &home);
        assert_eq!(h.set(&mail), Set::from([Trait::Personal]));
        assert_eq!(h.set(&wiki), Set::from([Trait::Internal]));
        assert!(h.set(&web).is_empty() && h.says(&web) == Set::from([Trait::Confidential, Trait::Internal]), "a web page's marking sets nothing and is noted");
        // A finding naming the mail page, a library source a host declared `person`, and a claim anchored to it.
        std::fs::write(ws.join("local/sessions/m1/findings/q.md"), format!("---\nkind: finding\nstatus: active\n---\n\nRests on {mail}.\n")).unwrap();
        let lib = crate::library::Library::in_home(&home);
        let sid = lib.add_private_pointer("person", "talk with the lab manager", "conversation", "2026-11-15", None).unwrap();
        // Forget what the writes did, as for a workspace that predates the slice.
        std::fs::remove_dir_all(handling::dir(&ws)).unwrap();
        let _ = std::fs::remove_dir_all(handling::library_dir(&home));
        let before_page = std::fs::read_to_string(ws.join(&mail)).unwrap();

        let dry = reconcile(&ws, &home, true).unwrap();
        assert!(dry.dry_run && dry.scanned == 4 && dry.changed > 0, "{dry:?}");
        assert_eq!(dry.labelled_by_path.get("email"), Some(&1));
        assert_eq!(dry.unlabelled_by_path.get("web"), Some(&1));
        assert_eq!(dry.says, 1);
        assert!(!handling::dir(&ws).exists() && !handling::library_dir(&home).join("records.json").exists(), "a dry run writes no record");
        assert_eq!(std::fs::read_to_string(ws.join(&mail)).unwrap(), before_page);
        assert!(!std::fs::read_to_string(ws.join("local/sessions/m1/findings/q.md")).unwrap().contains("handling:"), "a dry run writes no view");

        let real = reconcile(&ws, &home, false).unwrap();
        assert_eq!((real.scanned, real.changed, real.says, real.views), (dry.scanned, dry.changed, dry.says, dry.views), "the dry run said what the real run did");
        let h = Handling::of(&ws, &home);
        assert_eq!(h.set(&mail), Set::from([Trait::Personal]));
        assert_eq!(h.set(&handling::source_item(&sid)), Set::from([Trait::Personal]));
        let finding = "local/sessions/m1/findings/q.md";
        assert_eq!(h.set(finding), Set::from([Trait::Personal]), "the finding takes the trait of what it names");
        assert_eq!(h.record(finding).unwrap()["traits"][0]["by"], "derived");
        assert!(std::fs::read_to_string(ws.join(finding)).unwrap().contains("handling: [personal]"));

        let again = reconcile(&ws, &home, false).unwrap();
        assert_eq!((again.changed, again.views, again.dropped), (0, 0, 0), "{again:?}");

        // The operator clears the mail page's trait: the finding's derived copy goes at the next run, and the clear holds.
        assert!(h.operator_clear(&mail, Trait::Personal).unwrap());
        let after = reconcile(&ws, &home, false).unwrap();
        assert!(h.set(&mail).is_empty(), "the path rule does not put back what the operator cleared");
        assert!(h.set(finding).is_empty() && after.dropped == 1, "{after:?}");
        assert!(!std::fs::read_to_string(ws.join(finding)).unwrap().contains("handling:"), "the view follows the record");
    }

    #[test]
    fn an_unreadable_record_is_never_written_over() {
        let (_t, ws, home) = setup();
        std::fs::create_dir_all(handling::dir(&ws)).unwrap();
        std::fs::write(handling::dir(&ws).join("records.json"), "not json").unwrap();
        assert!(reconcile(&ws, &home, false).unwrap_err().contains("cannot be read"));
        assert_eq!(std::fs::read_to_string(handling::dir(&ws).join("records.json")).unwrap(), "not json");
        assert!(doctor_line(&ws, &home).unwrap().contains("cannot be read"));
    }

    #[test]
    fn bibliography_records_become_arrivals() {
        let bib = "---\nid: x.bib\n---\n\n## Sources\n\n- locator: https://mail.example.com/1\n  title: t\n  kind: email\n  retrieved: 2026-11-01\n- locator: https://wiki.example.com/a\n  title: w\n  kind: wiki\n  retrieved: 2026-11-01\n";
        let a = bibliography_arrivals(bib);
        assert_eq!(a.iter().map(|a| (a.kind.as_str(), a.locator.as_str())).collect::<Vec<_>>(), [("email", "https://mail.example.com/1"), ("wiki", "https://wiki.example.com/a")]);
        assert_eq!(page_arrivals(Path::new("/x/p.md"), Vec::new())[0].kind, IMPORT_KIND);
    }

    /// Spec 125 R7: dry runs at once in one process each copy into their own scratch folder and report the same thing. The
    /// folder used to be named by the process id and the clock (whole microseconds on macOS), so two dry runs started in one
    /// microsecond shared it, and the first to end removed the other's copy.
    #[test]
    fn dry_runs_at_once_each_get_their_own_scratch_folder() {
        let (_t, ws, home) = setup();
        import(&ws, "# Mail digest\n\nThe launch moves to May. Source: https://mail.example.com/thread/1\n");
        import(&ws, "# Wiki page\n\nThe team structure. Source: https://wiki.example.com/team\n");
        std::fs::remove_dir_all(handling::dir(&ws)).unwrap();
        let _ = std::fs::remove_dir_all(handling::library_dir(&home));
        let alone = reconcile(&ws, &home, true).unwrap();
        assert!(alone.changed > 0, "{alone:?}");
        let n = 8;
        for _round in 0..3 {
            let gate = std::sync::Arc::new(std::sync::Barrier::new(n));
            let handles: Vec<_> = (0..n)
                .map(|_| {
                    let (gate, ws, home) = (gate.clone(), ws.clone(), home.clone());
                    std::thread::spawn(move || {
                        gate.wait();
                        reconcile(&ws, &home, true)
                    })
                })
                .collect();
            for (i, h) in handles.into_iter().enumerate() {
                assert_eq!(h.join().unwrap().as_ref(), Ok(&alone), "dry run {i}");
            }
        }
        assert!(!handling::dir(&ws).exists(), "a dry run writes no record");
    }
}
