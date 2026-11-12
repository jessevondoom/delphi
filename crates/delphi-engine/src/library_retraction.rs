//! Marking a source retracted from the local Retraction Watch snapshot (`specs/101` §4, slice B).
//!
//! A source with a DOI is looked up in the snapshot at capture and at re-check, and when a new snapshot is imported. A hit writes a
//! `retraction` mark on the source's record: the kind (`retracted`, `corrected`, `expression_of_concern`), the snapshot's date and
//! version, and when the mark was made. The mark has three effects, all of them downgrades:
//!
//! * `retracted` and `expression_of_concern` stop the source counting as an origin (`cites_of` skips it, which is how a lost quote
//!   already reaches the count in `specs/098` §7.6); a `corrected` mark is a label and changes no count;
//! * a `retracted` source makes a claim that no other live source supports `contested`;
//! * the capture stays readable. **Nothing is deleted** (`specs/101` §12 item 9).
//!
//! Nothing here calls the network. With no policy (the default of [`Library::in_home`]) nothing is looked up and nothing is marked, so
//! the library behaves as spec 098 left it. A mark that is already on a source keeps its effect whether or not the policy is on: a
//! retraction does not un-happen because a switch changed.

use super::*;
use crate::retractions::{Hit, Kind, Policy};

/// What checking one source came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    /// No snapshot is in use (the switch is off, or none is imported), or the source is gone: nothing was looked up.
    NotChecked,
    /// The source has no DOI to match: not checked, and the report says so.
    NoIdentifier,
    /// Looked up, and the snapshot says nothing about it.
    Clear,
    /// The source already carries this snapshot's notice.
    Unchanged(Kind),
    /// A mark was written (or replaced by a stronger or newer one).
    Marked(Hit),
    /// An earlier mark from the snapshot is no longer in the snapshot (a reinstatement): it was lifted.
    Lifted,
}

/// The totals of checking every source.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Applied {
    pub checked: usize,
    pub no_identifier: usize,
    pub marked: usize,
    pub lifted: usize,
    pub unchanged: usize,
}

/// The DOI a source record names: its `DOI` field, else the DOI a `doi.org` address names. Never read from the page's text.
pub fn doi_of(meta: &Value) -> Option<String> {
    meta["DOI"].as_str().and_then(crate::doi::normalize).or_else(|| meta["canonical_url"].as_str().and_then(crate::doi::from_url))
}

/// Does this record's retraction mark take the source out of the origin count?
pub fn drops_origin(meta: &Value) -> bool {
    meta["retraction"]["kind"].as_str().and_then(Kind::parse).is_some_and(Kind::drops_origin)
}

impl Library {
    /// The same library checking sources against `p`.
    pub fn with_retractions(mut self, p: Policy) -> Library {
        self.retractions = p;
        self
    }

    pub fn retractions(&self) -> &Policy {
        &self.retractions
    }

    /// Check one source against the snapshot and write, replace or lift its mark. Idempotent.
    pub fn check_retraction(&self, id: &str) -> Check {
        let Some(index) = self.retractions.snapshot() else { return Check::NotChecked };
        let Some(mut meta) = self.read_meta(id) else { return Check::NotChecked };
        if !meta["purged"].is_null() {
            return Check::NotChecked;
        }
        let Some(doi) = doi_of(&meta) else { return Check::NoIdentifier };
        let hit = index.lookup(&doi);
        let prev: Option<Kind> = meta["retraction"]["kind"].as_str().and_then(Kind::parse);
        match hit {
            Some(h) if prev == Some(h.kind) && meta["retraction"]["snapshot"] == h.snapshot => Check::Unchanged(h.kind),
            Some(h) => {
                meta["retraction"] = json!({"kind": h.kind.as_str(), "date": h.date, "snapshot": h.snapshot, "doi": h.doi, "at": delphi_core::time::now_rfc3339(), "source": "retraction-watch"});
                if self.write_json(&self.source_dir(id).join("meta.json"), &meta).is_err() {
                    return Check::NotChecked;
                }
                self.log("source_retraction", json!({"id": id, "kind": h.kind.as_str(), "doi": h.doi, "date": h.date, "snapshot": h.snapshot}));
                self.source_retraction_changed(id, Some(h.kind));
                Check::Marked(h)
            }
            // the snapshot no longer lists it, and the mark came from a snapshot (never one made by hand): a reinstatement
            None if prev.is_some() && meta["retraction"]["source"] == "retraction-watch" => {
                if let Some(o) = meta.as_object_mut() {
                    o.remove("retraction");
                }
                if self.write_json(&self.source_dir(id).join("meta.json"), &meta).is_err() {
                    return Check::NotChecked;
                }
                self.log("retraction_lifted", json!({"id": id, "doi": doi}));
                self.source_retraction_changed(id, None);
                Check::Lifted
            }
            None => Check::Clear,
        }
    }

    /// Check every source (after a new snapshot is imported). Returns the totals; a source with no identifier is counted, not skipped
    /// silently.
    pub fn check_all_retractions(&self) -> Applied {
        let mut a = Applied::default();
        for (id, _) in self.list() {
            match self.check_retraction(&id) {
                Check::NotChecked => {}
                Check::NoIdentifier => {
                    a.checked += 1;
                    a.no_identifier += 1;
                }
                Check::Clear => a.checked += 1,
                Check::Unchanged(_) => {
                    a.checked += 1;
                    a.unchanged += 1;
                }
                Check::Marked(_) => {
                    a.checked += 1;
                    a.marked += 1;
                }
                Check::Lifted => {
                    a.checked += 1;
                    a.lifted += 1;
                }
            }
        }
        a
    }

    /// The sources that carry a mark, for the digest and `delphi sources`: id, title, kind, the snapshot's date of the notice.
    pub fn retracted_sources(&self) -> Vec<(String, String, Kind, String, String)> {
        self.list()
            .into_iter()
            .filter(|(_, m)| m["purged"].is_null())
            .filter_map(|(id, m)| {
                let kind = m["retraction"]["kind"].as_str().and_then(Kind::parse)?;
                Some((id, m["title"].as_str().unwrap_or("").to_string(), kind, m["retraction"]["date"].as_str().unwrap_or("").to_string(), m["retraction"]["snapshot"].as_str().unwrap_or("").to_string()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retractions::Index;
    use serde_json::json;

    const NOW: &str = "2026-11-11T12:00:00Z";

    fn snapshot(rows: &[&str]) -> Index {
        let csv = format!("Record ID,Title,OriginalPaperDOI,RetractionNature,RetractionDate\n{}\n", rows.join("\n"));
        Index::from_csv(csv.as_bytes(), NOW).unwrap()
    }

    fn lib_with(rows: &[&str]) -> (tempfile::TempDir, Library) {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path()).with_retractions(Policy::with_index(snapshot(rows)));
        (d, l)
    }

    fn page(l: &Library, dir: &Path, name: &str, doi: Option<&str>) -> String {
        let p = dir.join(name);
        std::fs::write(&p, format!("# {name}\n\nThe moon drives the tides.\n")).unwrap();
        let opts = AddOpts { licence: Some("CC0-1.0".into()), doi: doi.map(String::from), ..AddOpts::default() };
        l.add_file(&p, &opts, &Budgets::default()).unwrap().id
    }

    #[test]
    fn a_source_with_a_retracted_doi_is_marked_and_nothing_is_deleted() {
        let (d, l) = lib_with(&["1,t,10.1001/aaaa,Retraction,2021-03-04"]);
        let id = page(&l, d.path(), "a.md", Some("DOI:10.1001/AAAA"));
        // add_file already checked it at capture
        let m = l.read_meta(&id).unwrap();
        assert_eq!((m["retraction"]["kind"].as_str(), m["retraction"]["date"].as_str()), (Some("retracted"), Some("2021-03-04")));
        assert!(drops_origin(&m));
        assert!(l.source_dir(&id).join("source.md").is_file(), "the capture stays");
        assert_eq!(l.check_retraction(&id), Check::Unchanged(Kind::Retracted), "idempotent");
        assert_eq!(l.retracted_sources().len(), 1);
    }

    #[test]
    fn a_source_with_no_doi_is_not_checked_and_a_doi_org_address_names_one() {
        let (d, l) = lib_with(&["1,t,10.1001/bbbb,Retraction,2021-03-04"]);
        let id = page(&l, d.path(), "b.md", None);
        assert_eq!(l.check_retraction(&id), Check::NoIdentifier);
        assert!(l.read_meta(&id).unwrap()["retraction"].is_null());
        assert_eq!(doi_of(&json!({"canonical_url": "https://doi.org/10.1001/BBBB"})).as_deref(), Some("10.1001/bbbb"));
        assert_eq!(doi_of(&json!({"canonical_url": "https://example.com/10.1001/bbbb"})), None, "text of a page or a path is never read for one");
    }

    #[test]
    fn with_no_policy_nothing_is_looked_up() {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let id = page(&l, d.path(), "c.md", Some("10.1001/cccc"));
        assert_eq!(l.check_retraction(&id), Check::NotChecked);
        assert_eq!(Library::in_home(d.path()).with_retractions(Policy::load(d.path().join("library").as_path(), true)).check_retraction(&id), Check::NotChecked, "on, but no snapshot imported");
    }

    #[test]
    fn a_correction_labels_and_a_concern_drops_but_does_not_contest() {
        let (d, l) = lib_with(&["1,t,10.1001/dddd,Correction,2021-01-01", "2,t,10.1001/eeee,Expression of concern,2021-01-01"]);
        let (c, e) = (page(&l, d.path(), "d.md", Some("10.1001/dddd")), page(&l, d.path(), "e.md", Some("10.1001/eeee")));
        assert!(!drops_origin(&l.read_meta(&c).unwrap()) && l.read_meta(&c).unwrap()["retraction"]["kind"] == "corrected");
        assert!(drops_origin(&l.read_meta(&e).unwrap()));
    }

    #[test]
    fn a_new_snapshot_that_reinstates_lifts_the_mark_and_a_hand_made_one_is_left() {
        let (d, l) = lib_with(&["1,t,10.1001/ffff,Retraction,2021-03-04"]);
        let id = page(&l, d.path(), "f.md", Some("10.1001/ffff"));
        assert!(drops_origin(&l.read_meta(&id).unwrap()));
        let l2 = Library::in_home(d.path()).with_retractions(Policy::with_index(snapshot(&["1,t,10.1001/other,Retraction,2021-03-04"])));
        assert_eq!(l2.check_all_retractions(), Applied { checked: 1, lifted: 1, ..Applied::default() });
        assert!(l2.read_meta(&id).unwrap()["retraction"].is_null());
        // a mark whose source is not the snapshot is not lifted by it
        let mut m = l2.read_meta(&id).unwrap();
        m["retraction"] = json!({"kind": "retracted", "source": "operator"});
        l2.write_json(&l2.source_dir(&id).join("meta.json"), &m).unwrap();
        assert_eq!(l2.check_retraction(&id), Check::Clear);
        assert!(drops_origin(&l2.read_meta(&id).unwrap()));
    }

    #[test]
    fn a_new_version_keeps_the_mark_even_with_no_policy() {
        let (d, l) = lib_with(&["1,t,10.1001/gggg,Retraction,2021-03-04"]);
        let id = page(&l, d.path(), "g.md", Some("10.1001/gggg"));
        std::fs::write(d.path().join("g.md"), "# g\n\nChanged text.\n").unwrap();
        let bare = Library::in_home(d.path());
        let opts = AddOpts { licence: Some("CC0-1.0".into()), doi: Some("10.1001/gggg".into()), ..AddOpts::default() };
        bare.add_file(&d.path().join("g.md"), &opts, &Budgets::default()).unwrap();
        assert!(drops_origin(&bare.read_meta(&id).unwrap()), "a retraction does not un-happen because the next caller has no snapshot");
    }
}
